//! TeamHub が所有する agentId -> runtime endpoint 対応と統合配送。

use crate::agent_runtime::{
    BackendKind, PtyCompatAdapter, RuntimeDeliveryRequest, RuntimeEventEnvelope, RuntimeManager,
};
use crate::pty::session::TerminationReason;
use crate::pty::SessionRegistry;
use crate::team_hub::inject::InjectError;
use crate::team_hub::state::HubState;
use crate::team_hub::TeamHub;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeEndpointBackend {
    Native,
    Pty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeEndpoint {
    pub endpoint_id: String,
    pub backend: RuntimeEndpointBackend,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentRuntimeBinding {
    pub native: Option<RuntimeEndpoint>,
    pub pty: Option<RuntimeEndpoint>,
    pub task_ids: Vec<u32>,
}

pub(crate) type RuntimeEndpointMap = HashMap<(String, String), AgentRuntimeBinding>;

#[derive(Clone)]
pub(crate) struct RuntimeRouting {
    pub manager: Arc<RuntimeManager>,
    pub backend_override: Arc<std::sync::RwLock<Option<BackendKind>>>,
}

fn key(team_id: &str, agent_id: &str) -> (String, String) {
    (team_id.to_string(), agent_id.to_string())
}

fn pty_endpoint_id(agent_id: &str) -> String {
    format!("team-pty-{agent_id}")
}

impl TeamHub {
    /// テスト専用コンストラクタ。production は in-flight tracker を共有する
    /// `with_inflight` を使う (`AppState::new` 経由)。Issue #801: caller は
    /// `#[cfg(test)]` モジュールのみのため test build 限定にし dead_code 警告を解消する。
    #[cfg(test)]
    pub fn new(registry: Arc<SessionRegistry>) -> Self {
        Self::with_runtime(
            registry,
            Arc::new(RuntimeManager::new()),
            crate::pty::InFlightTracker::new(),
        )
    }

    /// Issue #630: AppState 側で生成した in-flight tracker を共有する用。
    /// `AppState::new()` から呼ばれる。
    #[allow(dead_code)]
    pub fn with_inflight(
        registry: Arc<SessionRegistry>,
        inflight: Arc<crate::pty::InFlightTracker>,
    ) -> Self {
        Self::with_runtime(registry, Arc::new(RuntimeManager::new()), inflight)
    }

    pub fn with_runtime(
        registry: Arc<SessionRegistry>,
        runtime_manager: Arc<RuntimeManager>,
        inflight: Arc<crate::pty::InFlightTracker>,
    ) -> Self {
        Self {
            registry,
            runtime: RuntimeRouting {
                manager: runtime_manager,
                backend_override: Arc::new(std::sync::RwLock::new(None)),
            },
            state: Arc::new(Mutex::new(HubState {
                teams: HashMap::new(),
                active_teams: HashSet::new(),
                endpoint: String::new(),
                token: String::new(),
                bridge_path: PathBuf::new(),
                pending_recruits: HashMap::new(),
                recruit_lifecycles: HashMap::new(),
                agents: HashMap::new(),
                runtime_endpoints: HashMap::new(),
                role_profile_summary: Vec::new(),
                dynamic_roles: HashMap::new(),
                file_locks: HashMap::new(),
                recruit_semaphores: HashMap::new(),
                message_flusher: crate::team_hub::state::MessageFlusher::default(),
            })),
            app_handle: Arc::new(Mutex::new(None)),
            inflight,
        }
    }

    pub async fn bind_pty_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
        session_id: Option<String>,
    ) -> Result<String, String> {
        let endpoint_id = pty_endpoint_id(agent_id);
        let already_bound = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&key(team_id, agent_id))
                .and_then(|binding| binding.pty.as_ref())
                .is_some_and(|endpoint| {
                    endpoint.endpoint_id == endpoint_id
                        && self
                            .runtime
                            .manager
                            .registry()
                            .resolve(&endpoint_id)
                            .is_some()
                })
        };
        if !already_bound {
            if self
                .runtime
                .manager
                .registry()
                .resolve(&endpoint_id)
                .is_some()
            {
                let operation = self.runtime.manager.dispose(&endpoint_id);
                self.emit_runtime_events(&operation.events).await;
            }
            let adapter = Arc::new(PtyCompatAdapter::for_team_agent(
                self.registry.clone(),
                agent_id,
            ));
            let operation = self.runtime.manager.register_endpoint(endpoint_id.clone(), adapter);
            self.emit_runtime_events(&operation.events).await;
            operation
                .result
                .map_err(|error| format!("{}: {}", error.code, error.message))?;
        }

        let endpoint = RuntimeEndpoint {
            endpoint_id: endpoint_id.clone(),
            backend: RuntimeEndpointBackend::Pty,
            session_id,
        };
        let mut state = self.state.lock().await;
        let binding = state
            .runtime_endpoints
            .entry(key(team_id, agent_id))
            .or_default();
        binding.pty = Some(endpoint.clone());
        state.attach_runtime_to_recruit(team_id, agent_id, &endpoint);
        Ok(endpoint_id)
    }

    pub async fn bind_native_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
        endpoint_id: String,
        session_id: Option<String>,
    ) -> Result<(), String> {
        if self
            .runtime
            .manager
            .registry()
            .resolve(&endpoint_id)
            .is_none()
        {
            return Err(format!(
                "runtime endpoint '{endpoint_id}' is not registered"
            ));
        }
        let endpoint = RuntimeEndpoint {
            endpoint_id,
            backend: RuntimeEndpointBackend::Native,
            session_id,
        };
        let mut state = self.state.lock().await;
        let binding = state
            .runtime_endpoints
            .entry(key(team_id, agent_id))
            .or_default();
        binding.native = Some(endpoint.clone());
        state.attach_runtime_to_recruit(team_id, agent_id, &endpoint);
        Ok(())
    }

    pub async fn deliver_agent_message(
        &self,
        team_id: &str,
        agent_id: &str,
        from_role: &str,
        data: &str,
    ) -> Result<(), InjectError> {
        let backend = self
            .runtime
            .backend_override
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unwrap_or_else(crate::agent_runtime::requested_backend);
        self.deliver_agent_message_with_backend(team_id, agent_id, from_role, data, backend)
            .await
    }

    #[cfg(test)]
    pub(crate) fn set_runtime_backend_for_test(&self, backend: BackendKind) {
        *self
            .runtime
            .backend_override
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(backend);
    }

    pub(crate) async fn deliver_agent_message_with_backend(
        &self,
        team_id: &str,
        agent_id: &str,
        from_role: &str,
        data: &str,
        backend: BackendKind,
    ) -> Result<(), InjectError> {
        if backend != BackendKind::Pty {
            let native = {
                let state = self.state.lock().await;
                state
                    .runtime_endpoints
                    .get(&key(team_id, agent_id))
                    .and_then(|binding| binding.native.clone())
            };
            if let Some(endpoint) = native {
                if self
                    .runtime
                    .manager
                    .registry()
                    .resolve(&endpoint.endpoint_id)
                    .is_some()
                {
                    match self
                        .deliver_to_runtime_endpoint(&endpoint.endpoint_id, from_role, data)
                        .await
                    {
                        Ok(()) => return Ok(()),
                        Err(error) => tracing::warn!(
                            agent_id,
                            endpoint_id = endpoint.endpoint_id,
                            code = error.code(),
                            "[teamhub] native delivery failed; falling back to PTY"
                        ),
                    }
                }
            }
        }

        let session_id = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&key(team_id, agent_id))
                .and_then(|binding| binding.pty.as_ref())
                .and_then(|endpoint| endpoint.session_id.clone())
        };
        let endpoint_id = self
            .bind_pty_runtime_endpoint(team_id, agent_id, session_id)
            .await
            .map_err(|message| {
                InjectError::WriteInitialFailed(format!(
                    "runtime_pty_endpoint_registration_failed: {message}"
                ))
            })?;
        self.deliver_to_runtime_endpoint(&endpoint_id, from_role, data)
            .await
    }

    async fn deliver_to_runtime_endpoint(
        &self,
        endpoint_id: &str,
        from_role: &str,
        data: &str,
    ) -> Result<(), InjectError> {
        let operation = self
            .runtime
            .manager
            .deliver_team_message(
                endpoint_id,
                RuntimeDeliveryRequest {
                    data: data.to_string(),
                    from_role: from_role.to_string(),
                },
            )
            .await;
        self.emit_runtime_events(&operation.events).await;
        operation
            .result
            .map_err(|error| {
                InjectError::WriteInitialFailed(format!("{}: {}", error.code, error.message))
            })
    }

    async fn emit_runtime_events(&self, events: &[RuntimeEventEnvelope]) {
        let app = self.app_handle.lock().await.clone();
        let Some(app) = app else { return };
        for event in events {
            let event_name = format!("runtime:event:{}", event.endpoint_id);
            if let Err(error) = app.emit(&event_name, event) {
                tracing::warn!("[teamhub] failed to emit runtime event: {error}");
            }
        }
    }

    pub async fn team_members(&self, team_id: &str) -> Vec<(String, String)> {
        let mut members = {
            let state = self.state.lock().await;
            state.team_member_roles(team_id)
        };
        for member in self.registry.list_team_members(team_id) {
            if !members.iter().any(|(agent_id, _)| agent_id == &member.0) {
                members.push(member);
            }
        }
        members
    }

    pub async fn associate_task_runtime(
        &self,
        team_id: &str,
        targets: &[(String, String)],
        task_id: u32,
    ) {
        let mut state = self.state.lock().await;
        for (agent_id, _) in targets {
            let binding = state
                .runtime_endpoints
                .entry(key(team_id, agent_id))
                .or_default();
            if !binding.task_ids.contains(&task_id) {
                binding.task_ids.push(task_id);
            }
            state.attach_task_to_recruit(team_id, agent_id, task_id);
        }
    }

    pub async fn runtime_endpoint_is_live(&self, team_id: &str, agent_id: &str) -> bool {
        let binding = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&key(team_id, agent_id))
                .cloned()
        };
        let Some(binding) = binding else { return false };
        binding
            .native
            .iter()
            .chain(binding.pty.iter())
            .any(|endpoint| {
                self.runtime
                    .manager
                    .registry()
                    .resolve(&endpoint.endpoint_id)
                    .is_some()
                    && (endpoint.backend == RuntimeEndpointBackend::Native
                        || self.registry.get_by_agent(agent_id).is_some())
            })
    }

    pub async fn cleanup_agent_runtime(&self, team_id: &str, agent_id: &str) {
        let binding = {
            let mut state = self.state.lock().await;
            state.runtime_endpoints.remove(&key(team_id, agent_id))
        };
        if let Some(binding) = binding {
            for endpoint in binding.native.into_iter().chain(binding.pty) {
                let stop = self.runtime.manager.stop(&endpoint.endpoint_id);
                self.emit_runtime_events(&stop.events).await;
                if self
                    .runtime
                    .manager
                    .registry()
                    .resolve(&endpoint.endpoint_id)
                    .is_some()
                {
                    let dispose = self.runtime.manager.dispose(&endpoint.endpoint_id);
                    self.emit_runtime_events(&dispose.events).await;
                }
            }
        }
        if let Some(session) = self.registry.get_by_agent(agent_id) {
            if let Err(error) = session.kill(TerminationReason::UserClose) {
                tracing::warn!(agent_id, "[teamhub] fallback PTY cleanup failed: {error}");
            }
        }
    }

    pub async fn cleanup_team_runtimes(&self, team_id: &str) {
        let agent_ids: Vec<String> = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .keys()
                .filter(|(candidate, _)| candidate == team_id)
                .map(|(_, agent_id)| agent_id.clone())
                .collect()
        };
        for agent_id in agent_ids {
            self.cleanup_agent_runtime(team_id, &agent_id).await;
        }
    }
}
