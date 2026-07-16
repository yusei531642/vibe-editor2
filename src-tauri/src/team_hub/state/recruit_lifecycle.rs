//! Recruit の明示状態機械と failure / cancellation rollback。

use crate::team_hub::events::{RecruitLifecyclePayload, RecruitLifecycleState};
use crate::team_hub::runtime_endpoint::RuntimeEndpoint;
use crate::team_hub::TeamHub;
use tauri::Emitter;

#[derive(Clone, Debug)]
pub(crate) struct RecruitLifecycle {
    pub team_id: String,
    pub agent_id: String,
    pub role_profile_id: String,
    pub state: RecruitLifecycleState,
    pub endpoint_id: Option<String>,
    pub session_id: Option<String>,
    pub task_ids: Vec<u32>,
    pub reason: Option<String>,
}

impl RecruitLifecycle {
    fn payload(&self) -> RecruitLifecyclePayload {
        RecruitLifecyclePayload {
            team_id: self.team_id.clone(),
            agent_id: self.agent_id.clone(),
            role_profile_id: self.role_profile_id.clone(),
            state: self.state,
            endpoint_id: self.endpoint_id.clone(),
            session_id: self.session_id.clone(),
            task_ids: self.task_ids.clone(),
            reason: self.reason.clone(),
        }
    }
}

fn can_transition(from: RecruitLifecycleState, to: RecruitLifecycleState) -> bool {
    use RecruitLifecycleState::{Cancelled, Failed, Handshaking, Ready, Requested, Spawning};
    matches!(
        (from, to),
        (Requested, Spawning)
            | (Spawning, Handshaking)
            | (Handshaking, Ready)
            | (
                Requested | Spawning | Handshaking | Ready,
                Failed | Cancelled
            )
    )
}

impl super::hub_state::HubState {
    pub(crate) fn attach_runtime_to_recruit(
        &mut self,
        team_id: &str,
        agent_id: &str,
        endpoint: &RuntimeEndpoint,
    ) {
        if let Some(lifecycle) = self.recruit_lifecycles.get_mut(agent_id) {
            if lifecycle.team_id == team_id {
                lifecycle.endpoint_id = Some(endpoint.endpoint_id.clone());
                lifecycle.session_id = endpoint.session_id.clone();
            }
        }
    }

    pub(crate) fn attach_task_to_recruit(&mut self, team_id: &str, agent_id: &str, task_id: u32) {
        if let Some(lifecycle) = self.recruit_lifecycles.get_mut(agent_id) {
            if lifecycle.team_id == team_id && !lifecycle.task_ids.contains(&task_id) {
                lifecycle.task_ids.push(task_id);
            }
        }
    }
}

impl TeamHub {
    pub async fn begin_recruit_lifecycle(
        &self,
        team_id: &str,
        agent_id: &str,
        role_profile_id: &str,
    ) {
        let lifecycle = RecruitLifecycle {
            team_id: team_id.to_string(),
            agent_id: agent_id.to_string(),
            role_profile_id: role_profile_id.to_string(),
            state: RecruitLifecycleState::Requested,
            endpoint_id: None,
            session_id: None,
            task_ids: Vec::new(),
            reason: None,
        };
        let payload = lifecycle.payload();
        self.state
            .lock()
            .await
            .recruit_lifecycles
            .insert(agent_id.to_string(), lifecycle);
        self.emit_recruit_lifecycle(payload).await;
    }

    pub async fn transition_recruit_lifecycle(
        &self,
        agent_id: &str,
        next: RecruitLifecycleState,
        reason: Option<String>,
    ) -> bool {
        let payload = {
            let mut state = self.state.lock().await;
            let Some(lifecycle) = state.recruit_lifecycles.get_mut(agent_id) else {
                return false;
            };
            if lifecycle.state == next {
                return true;
            }
            if !can_transition(lifecycle.state, next) {
                tracing::warn!(
                    agent_id,
                    from = ?lifecycle.state,
                    to = ?next,
                    "[teamhub] rejected recruit lifecycle transition"
                );
                return false;
            }
            lifecycle.state = next;
            lifecycle.reason = reason;
            lifecycle.payload()
        };
        self.emit_recruit_lifecycle(payload).await;
        true
    }

    pub async fn fail_recruit(&self, agent_id: &str, reason: impl Into<String>) {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Failed, reason.into())
            .await;
    }

    pub async fn cancel_recruit(&self, agent_id: &str, reason: impl Into<String>) {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Cancelled, reason.into())
            .await;
    }

    async fn finish_recruit_terminal(
        &self,
        agent_id: &str,
        terminal: RecruitLifecycleState,
        reason: String,
    ) {
        let cancel_payload = crate::team_hub::events::RecruitCancelledPayload {
            new_agent_id: agent_id.to_string(),
            reason: reason.clone(),
        };
        let team_id = {
            let state = self.state.lock().await;
            state
                .recruit_lifecycles
                .get(agent_id)
                .map(|lifecycle| lifecycle.team_id.clone())
        };
        let _ = self
            .transition_recruit_lifecycle(agent_id, terminal, Some(reason))
            .await;
        {
            let mut state = self.state.lock().await;
            state.pending_recruits.remove(agent_id);
            if let Some(team_id) = team_id.as_deref() {
                state
                    .agents
                    .remove(&(team_id.to_string(), agent_id.to_string()));
            }
        }
        if let Some(team_id) = team_id {
            self.cleanup_agent_runtime(&team_id, agent_id).await;
            let _ = self
                .release_all_file_locks_for_agent(&team_id, agent_id)
                .await;
        }
        if let Some(app) = self.app_handle.lock().await.clone() {
            let _ = app.emit("team:recruit-cancelled", cancel_payload);
        }
    }

    async fn emit_recruit_lifecycle(&self, payload: RecruitLifecyclePayload) {
        let app = self.app_handle.lock().await.clone();
        if let Some(app) = app {
            if let Err(error) = app.emit("team:recruit-lifecycle", payload) {
                tracing::warn!("[teamhub] failed to emit recruit lifecycle: {error}");
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn recruit_lifecycle_for_test(
        &self,
        agent_id: &str,
    ) -> Option<RecruitLifecycle> {
        self.state
            .lock()
            .await
            .recruit_lifecycles
            .get(agent_id)
            .cloned()
    }
}
