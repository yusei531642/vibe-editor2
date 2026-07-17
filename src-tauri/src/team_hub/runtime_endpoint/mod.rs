//! TeamHub が所有する agentId -> runtime endpoint 対応と統合配送。

pub(crate) mod types;
#[cfg(test)]
mod test_support;

use types::*;
use crate::agent_runtime::{
    BackendKind, PtyCompatAdapter, RuntimeDeliveryRequest, RuntimeEventEnvelope, RuntimeManager,
};
use crate::pty::SessionRegistry;
use crate::team_hub::inject::InjectError;
use crate::team_hub::state::HubState;
use crate::team_hub::TeamHub;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;


#[derive(Clone)]
pub(crate) struct RuntimeRouting {
    pub manager: Arc<RuntimeManager>,
    pub backend_override: Arc<std::sync::RwLock<Option<BackendKind>>>,
    pub pty_binding_lock: Arc<Mutex<()>>,
    #[cfg(test)]
    pub codex_delivery_override:
        Arc<std::sync::RwLock<Option<crate::team_hub::codex_delivery::CodexDelivery>>>,
    #[cfg(test)]
    pub legacy_app_server_override: Arc<std::sync::RwLock<Option<bool>>>,
    #[cfg(test)]
    pub legacy_app_server_deliveries: Arc<std::sync::Mutex<Vec<LegacyAppServerDelivery>>>,
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
                pty_binding_lock: Arc::new(Mutex::new(())),
                #[cfg(test)]
                codex_delivery_override: Arc::new(std::sync::RwLock::new(None)),
                #[cfg(test)]
                legacy_app_server_override: Arc::new(std::sync::RwLock::new(None)),
                #[cfg(test)]
                legacy_app_server_deliveries: Arc::new(std::sync::Mutex::new(Vec::new())),
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
        let _binding_guard = self.runtime.pty_binding_lock.lock().await;
        // 認可 (PR #34 レビュー): terminal_create 経由の (team_id, agent_id) も renderer 由来。
        // native bind と同一の fail-closed 検証を通す。
        self.authorize_team_agent_binding(team_id, agent_id).await?;
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

    /// (team_id, agent_id) が「active team の実在メンバー (handshake 済み agents /
    /// PTY session / 非 terminal recruit lifecycle のいずれか)」であることを検証する。
    /// renderer 由来の binding 系入力に共通の fail-closed 認可 (PR #34 レビュー)。
    async fn authorize_team_agent_binding(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<(), String> {
        let state = self.state.lock().await;
        if !state.active_teams.contains(team_id) {
            return Err(format!("team '{team_id}' is not active"));
        }
        let recruited_here = state
            .recruit_lifecycles
            .get(agent_id)
            .is_some_and(|lifecycle| {
                lifecycle.team_id == team_id
                    && !matches!(
                        lifecycle.state,
                        crate::team_hub::events::RecruitLifecycleState::Failed
                            | crate::team_hub::events::RecruitLifecycleState::Cancelled
                    )
            });
        let is_member = state
            .agents
            .contains_key(&(team_id.to_string(), agent_id.to_string()))
            || self
                .registry
                .list_team_members(team_id)
                .iter()
                .any(|(member_agent_id, _)| member_agent_id == agent_id);
        if !recruited_here && !is_member {
            return Err(format!(
                "agent '{agent_id}' is not a member of team '{team_id}'"
            ));
        }
        Ok(())
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
        // 認可 (PR #34 一次レビュー 🟡7): renderer 由来の (team_id, agent_id) は信頼境界外。
        // active な team の実在メンバーであることを fail-closed に検証し、live な native
        // binding の上書き (既存 worker の配送乗っ取り) を拒否する。
        self.authorize_team_agent_binding(team_id, agent_id).await?;
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
        if let Some(existing) = &binding.native {
            if existing.endpoint_id != endpoint.endpoint_id
                && self
                    .runtime
                    .manager
                    .registry()
                    .resolve(&existing.endpoint_id)
                    .is_some()
            {
                return Err(format!(
                    "agent '{agent_id}' already has a live native endpoint '{}'",
                    existing.endpoint_id
                ));
            }
        }
        // live PTY 稼働中への後付け native bind は配送乗っ取りに使えるため拒否 (PR #34)。
        if let Some(existing_pty) = &binding.pty {
            // liveness は runtime_endpoint_is_live と同一基準 (phantom endpoint 対策、PR #34)。
            if self
                .runtime
                .manager
                .registry()
                .resolve(&existing_pty.endpoint_id)
                .is_some()
                && self.registry.get_by_agent(agent_id).is_some()
            {
                return Err(format!(
                    "agent '{agent_id}' is already running on a live PTY endpoint '{}'",
                    existing_pty.endpoint_id
                ));
            }
        }
        binding.native = Some(endpoint.clone());
        state.attach_runtime_to_recruit(team_id, agent_id, &endpoint);
        Ok(())
    }

    pub(crate) fn selected_runtime_backend(&self) -> BackendKind {
        self.runtime
            .backend_override
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unwrap_or_else(crate::agent_runtime::requested_backend)
    }


    pub(crate) fn prefers_legacy_codex_pty(&self) -> bool {
        #[cfg(test)]
        if let Some(delivery) = *self
            .runtime
            .codex_delivery_override
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            return delivery == crate::team_hub::codex_delivery::CodexDelivery::Pty;
        }
        crate::team_hub::codex_delivery::prefers_pty()
    }

    pub(crate) async fn try_deliver_native_message(
        &self,
        team_id: &str,
        agent_id: &str,
        from_role: &str,
        data: &str,
        backend: BackendKind,
    ) -> Option<Result<(), InjectError>> {
        if backend == BackendKind::Pty {
            return None;
        }
        let endpoint = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&key(team_id, agent_id))
                .and_then(|binding| binding.native.clone())
        }?;
        if self
            .runtime
            .manager
            .registry()
            .resolve(&endpoint.endpoint_id)
            .is_none()
        {
            self.prune_native_runtime_endpoint(team_id, agent_id, &endpoint.endpoint_id)
                .await;
            return None;
        }
        let result = self
            .deliver_to_runtime_endpoint(
                &endpoint.endpoint_id,
                RuntimeEndpointBackend::Native,
                from_role,
                data,
            )
            .await;
        if result.is_err()
            && self
                .runtime
                .manager
                .registry()
                .resolve(&endpoint.endpoint_id)
                .is_none()
        {
            self.prune_native_runtime_endpoint(team_id, agent_id, &endpoint.endpoint_id)
                .await;
        }
        Some(result)
    }

    pub(crate) async fn deliver_pty_message(
        &self,
        team_id: &str,
        agent_id: &str,
        from_role: &str,
        data: &str,
    ) -> Result<(), InjectError> {
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
        self.deliver_to_runtime_endpoint(&endpoint_id, RuntimeEndpointBackend::Pty, from_role, data)
            .await
    }

    async fn deliver_to_runtime_endpoint(
        &self,
        endpoint_id: &str,
        backend: RuntimeEndpointBackend,
        from_role: &str,
        data: &str,
    ) -> Result<(), InjectError> {
        let request = RuntimeDeliveryRequest {
            data: data.to_string(),
            from_role: from_role.to_string(),
        };
        let operation = match backend {
            RuntimeEndpointBackend::Native => {
                let manager = self.runtime.manager.clone();
                let endpoint_id = endpoint_id.to_string();
                tauri::async_runtime::spawn_blocking(move || {
                    manager.deliver_team_message_blocking(&endpoint_id, request)
                })
                .await
                .map_err(|error| InjectError::TaskJoinFailed {
                    phase: "native_delivery",
                    source: error.to_string(),
                })?
            }
            RuntimeEndpointBackend::Pty => {
                self.runtime
                    .manager
                    .deliver_team_message(endpoint_id, request)
                    .await
            }
        };
        self.emit_runtime_events(&operation.events).await;
        operation
            .result
            .map_err(crate::team_hub::runtime_cleanup::restore_inject_error)
    }

    async fn prune_native_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
        endpoint_id: &str,
    ) {
        let mut state = self.state.lock().await;
        if let Some(binding) = state.runtime_endpoints.get_mut(&key(team_id, agent_id)) {
            if binding
                .native
                .as_ref()
                .is_some_and(|endpoint| endpoint.endpoint_id == endpoint_id)
            {
                binding.native = None;
            }
        }
    }

    pub(super) async fn emit_runtime_events(&self, events: &[RuntimeEventEnvelope]) {
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
            // binding が無い agent に空 entry を作らない。作ると team teardown の
            // cleanup_agent_runtime 走査対象になり、bind していない PTY session まで
            // kill されてしまう (PR #34 一次レビュー 🟡4)。
            if let Some(binding) = state.runtime_endpoints.get_mut(&key(team_id, agent_id)) {
                if !binding.task_ids.contains(&task_id) {
                    binding.task_ids.push(task_id);
                }
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
}
