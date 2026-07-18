//! AgentRuntimeAdapter backed by a dedicated connection to the shared Codex app-server daemon.

use super::client::{ClientEvent, ClientEventSink, ClientHandle};
use super::convert;
use crate::agent_runtime::{
    ensure_runtime_permission_not_escalated, AgentRuntimeAdapter, BackendKind, RuntimeAdapterError,
    RuntimeApprovalResponseRequest, RuntimeCapability, RuntimeEventPayload,
    RuntimeSessionForkRequest, RuntimeSessionResumeRequest, RuntimeSessionSpawnRequest,
    RuntimeSteerRequest, RuntimeTurnSpawnRequest,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, Debug)]
pub enum CodexAdapterEvent {
    Payload(RuntimeEventPayload),
    Failure(RuntimeAdapterError),
}

pub type CodexAdapterEventSink = Arc<dyn Fn(CodexAdapterEvent) + Send + Sync>;

#[derive(Default)]
struct SessionState {
    /// client actor と共有する thread 束縛 (approval の thread 照合にも使う)。
    thread_id: Arc<RwLock<Option<String>>>,
    active_turn_id: RwLock<Option<String>>,
}

pub struct CodexRuntimeAdapter {
    client: Mutex<Option<Arc<ClientHandle>>>,
    cwd: Option<String>,
    model: Option<String>,
    permission: Option<String>,
    state: Arc<SessionState>,
    disposed: AtomicBool,
}

impl CodexRuntimeAdapter {
    pub fn connect(
        socket_path: String,
        cwd: Option<String>,
        model: Option<String>,
        permission: Option<String>,
        sink: CodexAdapterEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        let state = Arc::new(SessionState::default());
        let client_sink = client_sink(state.clone(), sink);
        let client = Arc::new(ClientHandle::connect(
            socket_path,
            client_sink,
            state.thread_id.clone(),
        )?);
        Ok(Self {
            client: Mutex::new(Some(client)),
            cwd,
            model,
            permission,
            state,
            disposed: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub fn connect_stream(
        stream: tokio::net::UnixStream,
        cwd: Option<String>,
        model: Option<String>,
        permission: Option<String>,
        sink: CodexAdapterEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        let state = Arc::new(SessionState::default());
        let client_sink = client_sink(state.clone(), sink);
        let client = Arc::new(ClientHandle::connect_stream(
            stream,
            client_sink,
            state.thread_id.clone(),
        )?);
        Ok(Self {
            client: Mutex::new(Some(client)),
            cwd,
            model,
            permission,
            state,
            disposed: AtomicBool::new(false),
        })
    }

    pub fn thread_id(&self) -> Option<String> {
        read_lock(&self.state.thread_id).clone()
    }

    fn client(&self) -> Result<Arc<ClientHandle>, RuntimeAdapterError> {
        if self.disposed.load(Ordering::Acquire) {
            return Err(RuntimeAdapterError::new(
                "runtime_endpoint_disposed",
                "runtime endpoint has been disposed",
                false,
            ));
        }
        self.client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| {
                RuntimeAdapterError::new(
                    "runtime_app_server_disconnected",
                    "app-server connection is not available",
                    false,
                )
            })
    }

    fn set_thread_from_result(&self, result: &Value) -> Result<(), RuntimeAdapterError> {
        let thread_id = convert::thread_id(result).ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_app_server_protocol",
                "thread response did not include a thread id",
                false,
            )
        })?;
        *write_lock(&self.state.thread_id) = Some(thread_id);
        Ok(())
    }

    fn require_thread_id(&self) -> Result<String, RuntimeAdapterError> {
        self.thread_id().ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_thread_not_ready",
                "Codex thread is not ready",
                true,
            )
        })
    }

    fn active_turn_id(&self) -> Result<String, RuntimeAdapterError> {
        read_lock(&self.state.active_turn_id)
            .clone()
            .ok_or_else(|| {
                RuntimeAdapterError::new(
                    "runtime_turn_not_active",
                    "Codex thread has no active turn",
                    true,
                )
            })
    }

    fn start_turn(
        &self,
        input: &str,
        model: Option<&str>,
        effort: Option<&str>,
        permission: Option<&str>,
    ) -> Result<(), RuntimeAdapterError> {
        let thread_id = self.require_thread_id()?;
        let mut params = json!({ "threadId": thread_id, "input": [text_input(input)] });
        if let Some(model) = model {
            params["model"] = Value::String(model.to_string());
        }
        if let Some(effort) = effort {
            params["effort"] = Value::String(effort.to_string());
        }
        apply_turn_permission(&mut params, permission.or(self.permission.as_deref()));
        let result = self.client()?.request("turn/start", params)?;
        if let Some(turn_id) = convert::turn_id(&result) {
            *write_lock(&self.state.active_turn_id) = Some(turn_id);
        }
        Ok(())
    }
}

impl AgentRuntimeAdapter for CodexRuntimeAdapter {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        vec![
            RuntimeCapability::NativeProcessExecution,
            RuntimeCapability::StructuredEventStream,
            RuntimeCapability::CooperativeCancellation,
            RuntimeCapability::SessionResume,
            RuntimeCapability::SessionFork,
            RuntimeCapability::TurnSteering,
            RuntimeCapability::ApprovalResponses,
        ]
    }

    fn spawn_session(
        &self,
        _request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        let mut params = json!({});
        if let Some(cwd) = &self.cwd {
            params["cwd"] = Value::String(cwd.clone());
        }
        if let Some(model) = &self.model {
            params["model"] = Value::String(model.clone());
        }
        apply_thread_permission(&mut params, self.permission.as_deref());
        let result = self.client()?.request("thread/start", params)?;
        self.set_thread_from_result(&result)
    }

    fn resume_session(
        &self,
        request: &RuntimeSessionResumeRequest,
    ) -> Result<(), RuntimeAdapterError> {
        let result = self
            .client()?
            .request("thread/resume", json!({ "threadId": request.thread_id }))?;
        self.set_thread_from_result(&result)
    }

    fn fork_session(&self, request: &RuntimeSessionForkRequest) -> Result<(), RuntimeAdapterError> {
        let result = self
            .client()?
            .request("thread/fork", json!({ "threadId": request.thread_id }))?;
        self.set_thread_from_result(&result)
    }

    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        if !request.submit {
            return Err(RuntimeAdapterError::new(
                "runtime_native_draft_unsupported",
                "native Codex turns must be submitted",
                true,
            ));
        }
        ensure_runtime_permission_not_escalated(
            self.permission.as_deref(),
            request.permission.as_deref(),
        )?;
        self.start_turn(
            &request.input,
            request.model.as_deref(),
            request.effort.as_deref(),
            request.permission.as_deref(),
        )
    }

    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.start_turn(data, None, None, None)
    }

    fn inject(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.start_turn(data, None, None, None)
    }

    fn steer(&self, request: &RuntimeSteerRequest) -> Result<(), RuntimeAdapterError> {
        let thread_id = self.require_thread_id()?;
        let expected_turn_id = self.active_turn_id()?;
        self.client()?.request(
            "turn/steer",
            json!({
                "threadId": thread_id,
                "expectedTurnId": expected_turn_id,
                "input": [text_input(&request.input)],
            }),
        )?;
        Ok(())
    }

    fn interrupt(&self) -> Result<(), RuntimeAdapterError> {
        let thread_id = self.require_thread_id()?;
        let turn_id = self.active_turn_id()?;
        self.client()?.request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": turn_id }),
        )?;
        Ok(())
    }

    fn respond_approval(
        &self,
        request: &RuntimeApprovalResponseRequest,
    ) -> Result<(), RuntimeAdapterError> {
        if !matches!(
            request.decision.as_str(),
            "accept" | "acceptForSession" | "decline" | "cancel"
        ) {
            return Err(RuntimeAdapterError::new(
                "runtime_approval_decision_invalid",
                format!("unsupported approval decision '{}'", request.decision),
                true,
            ));
        }
        self.client()?
            .respond_approval(&request.request_id, &request.decision)
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        if read_lock(&self.state.active_turn_id).is_some() {
            self.interrupt()?;
        }
        Ok(())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Some(client) = self
            .client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            client.shutdown();
        }
        Ok(())
    }
}

fn client_sink(state: Arc<SessionState>, sink: CodexAdapterEventSink) -> ClientEventSink {
    Arc::new(move |event| match event {
        ClientEvent::Notification { method, params } => {
            if let Some(notification_thread_id) = convert::thread_id(&params) {
                let mut current = write_lock(&state.thread_id);
                if let Some(bound_thread_id) = current.as_deref() {
                    if bound_thread_id != notification_thread_id {
                        tracing::warn!(
                            bound_thread_id,
                            notification_thread_id,
                            method,
                            "[runtime] dropped app-server notification for another thread"
                        );
                        drop(current);
                        sink(CodexAdapterEvent::Payload(
                            RuntimeEventPayload::Diagnostic {
                                message: format!(
                                    "dropped app-server notification for unbound thread ({method})"
                                ),
                            },
                        ));
                        return;
                    }
                } else {
                    *current = Some(notification_thread_id);
                }
            }
            if method == "turn/started" {
                if let Some(turn_id) = convert::turn_id(&params) {
                    *write_lock(&state.active_turn_id) = Some(turn_id);
                }
            } else if matches!(method.as_str(), "turn/completed" | "turn/interrupted") {
                *write_lock(&state.active_turn_id) = None;
            }
            for payload in convert::notification(&method, &params) {
                sink(CodexAdapterEvent::Payload(payload));
            }
        }
        ClientEvent::ServerRequest {
            request_id,
            method,
            params,
        } => {
            sink(CodexAdapterEvent::Payload(convert::approval(
                request_id, method, &params,
            )));
        }
        ClientEvent::Failure(error) => sink(CodexAdapterEvent::Failure(error)),
    })
}

fn text_input(text: &str) -> Value {
    json!({ "type": "text", "text": text, "text_elements": [] })
}

fn apply_thread_permission(params: &mut Value, permission: Option<&str>) {
    match permission {
        Some("full") => {
            params["sandbox"] = Value::String("danger-full-access".to_string());
            params["approvalPolicy"] = Value::String("never".to_string());
        }
        Some("workspace") => {
            params["sandbox"] = Value::String("workspace-write".to_string());
            params["approvalPolicy"] = Value::String("on-request".to_string());
        }
        _ => {}
    }
}

fn apply_turn_permission(params: &mut Value, permission: Option<&str>) {
    match permission {
        Some("full") => {
            params["sandboxPolicy"] = json!({ "type": "dangerFullAccess" });
            params["approvalPolicy"] = Value::String("never".to_string());
        }
        Some("workspace") => {
            params["sandboxPolicy"] = json!({ "type": "workspaceWrite" });
            params["approvalPolicy"] = Value::String("on-request".to_string());
        }
        _ => {}
    }
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
