//! (teamId, agentId) と runtime endpoint の binding 確立 (PTY / native)。
//! 認可 (fail-closed) と乗っ取り防止の中心。mod.rs の 500 行 ratchet を守るため分離した。

use super::types::*;
use super::{key, pty_endpoint_id};
use crate::agent_runtime::{BackendKind, PtyCompatAdapter};
use crate::team_hub::TeamHub;
use std::sync::Arc;

impl TeamHub {
    /// Durable resume tokens are admitted only after normal active-team/member authorization.
    /// Marking the prior endpoint lets the existing reconnect gate distinguish this path from a
    /// renderer attempt to attach a new native runtime to an already-ready member.
    pub async fn authorize_restored_native_reconnect(
        &self,
        team_id: &str,
        agent_id: &str,
        endpoint_id: &str,
    ) -> Result<(), String> {
        self.authorize_runtime_endpoint_binding(team_id, agent_id)
            .await?;
        let mut state = self.state.lock().await;
        state
            .runtime_endpoints
            .entry(key(team_id, agent_id))
            .or_default()
            .prior_native_endpoint = Some(endpoint_id.to_string());
        Ok(())
    }

    /// renderer 起点 (terminal_create) の PTY binding。live native member への上書きは
    /// 乗っ取り防止のため拒否する (PR #34 レビュー)。Rust 内部の配送 fallback は
    /// `bind_pty_runtime_endpoint_for_delivery` を使う。
    pub async fn bind_pty_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
        session_id: Option<String>,
    ) -> Result<String, String> {
        self.bind_pty_runtime_endpoint_inner(team_id, agent_id, session_id, true)
            .await
    }

    /// 配送 fallback (deliver_pty_message) 用: `agentRuntimeBackend=pty` 強制時など、
    /// live native binding が残っていても PTY 配送を成立させる必要がある信頼済み経路。
    pub(crate) async fn bind_pty_runtime_endpoint_for_delivery(
        &self,
        team_id: &str,
        agent_id: &str,
        session_id: Option<String>,
    ) -> Result<String, String> {
        self.bind_pty_runtime_endpoint_inner(team_id, agent_id, session_id, false)
            .await
    }

    async fn bind_pty_runtime_endpoint_inner(
        &self,
        team_id: &str,
        agent_id: &str,
        session_id: Option<String>,
        reject_live_native: bool,
    ) -> Result<String, String> {
        let _binding_guard = self.runtime.pty_binding_lock.lock().await;
        // 認可 (PR #34 レビュー): terminal_create 経由の (team_id, agent_id) も renderer 由来。
        // native bind と同一の fail-closed 検証を通す。
        self.authorize_runtime_endpoint_binding(team_id, agent_id)
            .await?;
        // native bind 側と対称の乗っ取り防止: live な native endpoint を持つ member への
        // PTY bind (terminal_create 経由の上書き) は拒否する (PR #34 レビュー)。
        if reject_live_native {
            let state = self.state.lock().await;
            if let Some(binding) = state.runtime_endpoints.get(&key(team_id, agent_id)) {
                if let Some(native) = &binding.native {
                    if self
                        .runtime
                        .manager
                        .registry()
                        .resolve(&native.endpoint_id)
                        .is_some()
                    {
                        return Err(format!(
                            "agent '{agent_id}' is already running on a live native endpoint '{}'",
                            native.endpoint_id
                        ));
                    }
                }
            }
        }
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
        drop(state);
        let project_root = self.team_project_root(team_id).await;
        self.runtime.manager.persist_team_binding(
            crate::agent_runtime::RuntimeTeamBinding {
                project_root: project_root.as_deref(),
                team_id,
                agent_id,
                endpoint_id: &endpoint_id,
                provider: "pty",
                resume_id: endpoint.session_id.clone(),
                resumable: false,
            },
        );
        Ok(endpoint_id)
    }

    /// (team_id, agent_id) が active team の既存 active member または非 terminal recruit
    /// lifecycle であることを検証する。renderer が直前に作った PTY session は認可根拠にしない。
    async fn authorize_runtime_endpoint_binding(
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
        let is_member = state.bound_role(team_id, agent_id).is_some();
        if !recruited_here && !is_member {
            return Err(format!(
                "agent '{agent_id}' is not a member of team '{team_id}'"
            ));
        }
        Ok(())
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub async fn bind_native_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
        endpoint_id: String,
        session_id: Option<String>,
    ) -> Result<(), String> {
        // PTY / native の相互 live 判定は同一 lock 下で行う。並行 invoke で互いの
        // 書き込みをすり抜ける TOCTOU を防ぐ (PR #34 レビュー)。
        let _binding_guard = self.runtime.pty_binding_lock.lock().await;
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
        self.authorize_runtime_endpoint_binding(team_id, agent_id)
            .await?;
        let endpoint = RuntimeEndpoint {
            endpoint_id,
            backend: RuntimeEndpointBackend::Native,
            session_id,
        };
        let mut state = self.state.lock().await;
        // 初回 native bind は spawn 中 (非 Ready の recruit lifecycle) に限定する。Ready 済み
        // member への後付けは既存 native binding の再接続のみ許可 (PTY を持たない API/virtual
        // member の乗っ取り防止、PR #34)。binding の可変借用前に判定する。
        let has_prior_native = state
            .runtime_endpoints
            .get(&key(team_id, agent_id))
            .is_some_and(|binding| {
                binding.native.is_some() || binding.prior_native_endpoint.is_some()
            });
        if !has_prior_native {
            let spawning = state.recruit_lifecycles.get(agent_id).is_some_and(|l| {
                l.team_id == team_id
                    && matches!(
                        l.state,
                        crate::team_hub::events::RecruitLifecycleState::Requested
                            | crate::team_hub::events::RecruitLifecycleState::Spawning
                            | crate::team_hub::events::RecruitLifecycleState::Handshaking
                    )
            });
            if !spawning {
                return Err(format!(
                    "agent '{agent_id}' has no active recruit; native binding is only \
                     established during spawn or by reconnecting an existing binding"
                ));
            }
        }
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
}
