//! `AgentRuntimeAdapter` implementation backed by one Claude Agent sidecar process.

use super::client::{ClientEvent, ClientEventSink, SidecarClient, SidecarLaunchConfig};
use crate::agent_runtime::{
    capabilities_for, AgentRuntimeAdapter, BackendKind, RuntimeAdapterError,
    RuntimeApprovalResponseRequest, RuntimeCapability, RuntimeEventPayload, RuntimeProvider,
    RuntimeSessionForkRequest, RuntimeSessionResumeRequest, RuntimeSessionSpawnRequest,
    RuntimeSteerRequest, RuntimeTurnSpawnRequest,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug)]
pub enum ClaudeAdapterEvent {
    Session(String),
    Payload(RuntimeEventPayload),
    Failure(RuntimeAdapterError),
}

pub type ClaudeAdapterEventSink = Arc<dyn Fn(ClaudeAdapterEvent) + Send + Sync>;

pub struct ClaudeAgentRuntimeAdapter {
    client: Arc<SidecarClient>,
    cwd: Option<String>,
    system_prompt: Option<String>,
    session_id: Arc<RwLock<Option<String>>>,
    disposed: AtomicBool,
}

impl ClaudeAgentRuntimeAdapter {
    pub fn connect(
        config: SidecarLaunchConfig,
        cwd: Option<String>,
        system_prompt: Option<String>,
        sink: ClaudeAdapterEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        let session_id = Arc::new(RwLock::new(None));
        let sink_session_id = session_id.clone();
        let client_sink: ClientEventSink = Arc::new(move |event| match event {
            ClientEvent::Session(value) => {
                *write_lock(&sink_session_id) = Some(value.clone());
                sink(ClaudeAdapterEvent::Session(value));
            }
            ClientEvent::Payload(payload) => sink(ClaudeAdapterEvent::Payload(payload)),
            ClientEvent::Failure(error) => sink(ClaudeAdapterEvent::Failure(error)),
        });
        let client = Arc::new(SidecarClient::spawn(config, client_sink)?);
        Ok(Self {
            client,
            cwd,
            system_prompt,
            session_id,
            disposed: AtomicBool::new(false),
        })
    }

    pub fn session_id(&self) -> Option<String> {
        read_lock(&self.session_id).clone()
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, RuntimeAdapterError> {
        if self.disposed.load(Ordering::Acquire) {
            return Err(RuntimeAdapterError::new(
                "runtime_endpoint_disposed",
                "Claude runtime endpoint has been disposed",
                false,
            ));
        }
        self.client.request(method, params)
    }

    fn update_session_id(&self, result: &Value) {
        if let Some(session_id) = result.get("sessionId").and_then(Value::as_str) {
            *write_lock(&self.session_id) = Some(session_id.to_string());
        }
    }

    fn submitted_input(&self, method: &str, input: &str) -> Result<(), RuntimeAdapterError> {
        let result = self.request(method, json!({ "input": input }))?;
        self.update_session_id(&result);
        Ok(())
    }
}

impl AgentRuntimeAdapter for ClaudeAgentRuntimeAdapter {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        capabilities_for(RuntimeProvider::ClaudeNative)
    }

    fn spawn_session(
        &self,
        request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        let result = self.request(
            "spawn",
            json!({
                "endpointId": request.endpoint_id,
                "cwd": self.cwd,
                "systemPrompt": self.system_prompt,
            }),
        )?;
        self.update_session_id(&result);
        Ok(())
    }

    fn resume_session(
        &self,
        request: &RuntimeSessionResumeRequest,
    ) -> Result<(), RuntimeAdapterError> {
        let result = self.request("resume", json!({ "sessionId": request.thread_id }))?;
        self.update_session_id(&result);
        Ok(())
    }

    fn fork_session(&self, request: &RuntimeSessionForkRequest) -> Result<(), RuntimeAdapterError> {
        let result = self.request("fork", json!({ "sessionId": request.thread_id }))?;
        self.update_session_id(&result);
        Ok(())
    }

    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        if !request.submit {
            return Err(RuntimeAdapterError::new(
                "runtime_native_draft_unsupported",
                "native Claude turns must be submitted",
                true,
            ));
        }
        self.submitted_input("turn", &request.input)
    }

    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.submitted_input("write", data)
    }

    fn inject(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.submitted_input("inject", data)
    }

    fn steer(&self, request: &RuntimeSteerRequest) -> Result<(), RuntimeAdapterError> {
        self.submitted_input("steer", &request.input)
    }

    fn interrupt(&self) -> Result<(), RuntimeAdapterError> {
        self.request("interrupt", json!({})).map(|_| ())
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
                "unsupported approval decision",
                true,
            ));
        }
        self.request(
            "respondApproval",
            json!({ "requestId": request.request_id, "decision": request.decision }),
        )
        .map(|_| ())
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        self.request("stop", json!({})).map(|_| ())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        if !self.disposed.swap(true, Ordering::AcqRel) {
            self.client.dispose();
        }
        Ok(())
    }
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
