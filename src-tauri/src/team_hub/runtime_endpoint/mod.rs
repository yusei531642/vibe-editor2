//! TeamHub が所有する agentId -> runtime endpoint 対応と統合配送。

mod binding;
pub(crate) mod types;
mod snapshot;
#[cfg(test)]
mod test_support;

use crate::agent_runtime::{
    BackendKind, RuntimeDeliveryRequest, RuntimeEventEnvelope, RuntimeManager,
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
use types::*;

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
                initial_native_admissions: HashSet::new(),
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
            .bind_pty_runtime_endpoint_for_delivery(team_id, agent_id, session_id)
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
                // reconnect の has_prior_native 判定用に endpoint id を履歴として残す。
                binding.prior_native_endpoint = Some(endpoint_id.to_string());
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

    pub(crate) async fn live_team_members(&self, team_id: &str) -> Vec<(String, String)> {
        let mut members: Vec<(String, String)> = {
            let state = self.state.lock().await;
            state
                .team_member_roles(team_id)
                .into_iter()
                .filter(|(agent_id, _)| {
                    let Some(binding) = state.runtime_endpoints.get(&key(team_id, agent_id)) else {
                        // binding 未確立の active member は従来どおり配送対象に残し、
                        // inject_no_session などの構造化失敗を返せるようにする。
                        return true;
                    };
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
                        })
                })
                .collect()
        };
        for member in self.registry.list_team_members(team_id) {
            if !members.iter().any(|(agent_id, _)| agent_id == &member.0) {
                members.push(member);
            }
        }
        members
    }

    /// Issue #27: renderer-supplied pair を active team + current member binding へ fail-closed に限定する。
    ///
    /// spawn 前の pre-check (terminal_create / worktree assign) から呼ばれるため、まだ PTY も
    /// hub handshake も無い「recruit 進行中」の agent も許容する。判定基準は bind 側の
    /// `authorize_runtime_endpoint_binding` (binding.rs) と同一に保つ — ここだけ厳しくすると
    /// 新規 recruit の初回 spawn が authz で落ちる (PR #37 レビュー 🟡)。
    pub async fn authorize_team_agent_binding(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> crate::commands::error::CommandResult<()> {
        self.authorized_team_agent_role(team_id, agent_id)
            .await
            .map(|_| ())
    }

    /// Issue #27: worker isolation policy must use the Hub-owned role, never renderer input.
    pub async fn authorized_team_agent_role(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> crate::commands::error::CommandResult<String> {
        crate::commands::validation::validate_id_segment("team_id", team_id)?;
        crate::commands::validation::validate_id_segment("agent_id", agent_id)?;
        crate::commands::authz::assert_active_team(self, team_id).await?;
        let role = {
            let state = self.state.lock().await;
            state.bound_role(team_id, agent_id).or_else(|| {
                state
                    .recruit_lifecycles
                    .get(agent_id)
                    .filter(|lifecycle| {
                        lifecycle.team_id == team_id
                            && !matches!(
                                lifecycle.state,
                                crate::team_hub::events::RecruitLifecycleState::Failed
                                    | crate::team_hub::events::RecruitLifecycleState::Cancelled
                            )
                    })
                    .map(|lifecycle| lifecycle.role_profile_id.clone())
            })
        };
        role.ok_or_else(|| {
            crate::commands::error::CommandError::authz(
                "agent is not an active member of this team",
            )
        })
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

    /// recruit 直後の liveness 検証用。binding 未確立の member (bind 失敗を warn で続行した
    /// PTY worker / socket を持たない pull 型 virtual member) は team_members() と同じく
    /// 「配送時に構造化失敗を返せる正当な状態」として許容し、endpoint を記録した binding が
    /// あるのに全 endpoint が dead の場合だけ false を返す (PR #34 レビュー)。
    pub async fn runtime_binding_absent_or_live(&self, team_id: &str, agent_id: &str) -> bool {
        let has_recorded_endpoint = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&key(team_id, agent_id))
                .is_some_and(|binding| binding.native.is_some() || binding.pty.is_some())
        };
        !has_recorded_endpoint || self.runtime_endpoint_is_live(team_id, agent_id).await
    }
}
