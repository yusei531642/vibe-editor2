//! Recruit の明示状態機械と failure / cancellation rollback。

use crate::team_hub::events::{RecruitLifecyclePayload, RecruitLifecycleState};
use crate::team_hub::runtime_endpoint::RuntimeEndpoint;
use crate::team_hub::TeamHub;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::Emitter;

const TERMINAL_RECRUIT_RETENTION: Duration = Duration::from_secs(5 * 60);
static NEXT_RECRUIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn next_recruit_sequence() -> u64 {
    NEXT_RECRUIT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub(crate) struct RecruitLifecycle {
    pub team_id: String,
    pub agent_id: String,
    pub role_profile_id: String,
    pub sequence: u64,
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
            sequence: self.sequence,
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
            sequence: next_recruit_sequence(),
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
                return false;
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
            lifecycle.sequence = next_recruit_sequence();
            lifecycle.state = next;
            lifecycle.reason = reason;
            lifecycle.payload()
        };
        self.emit_recruit_lifecycle(payload).await;
        true
    }

    pub async fn fail_recruit(&self, agent_id: &str, reason: impl Into<String>) -> bool {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Failed, reason.into())
            .await
    }

    pub async fn cancel_recruit(&self, agent_id: &str, reason: impl Into<String>) -> bool {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Cancelled, reason.into())
            .await
    }

    pub async fn cancel_recruit_with_pending_grace(
        &self,
        team_id: &str,
        agent_id: &str,
        reason: impl Into<String>,
    ) {
        let transitioned = self.cancel_recruit(agent_id, reason).await;
        self.cancel_pending_recruit(agent_id).await;
        if !transitioned {
            self.cleanup_agent_runtime(team_id, agent_id).await;
        }
    }

    pub async fn discard_pending_recruit(&self, agent_id: &str) {
        self.state.lock().await.pending_recruits.remove(agent_id);
    }

    async fn finish_recruit_terminal(
        &self,
        agent_id: &str,
        terminal: RecruitLifecycleState,
        reason: String,
    ) -> bool {
        let cancel_payload = crate::team_hub::events::RecruitCancelledPayload {
            new_agent_id: agent_id.to_string(),
            reason: reason.clone(),
        };
        if !self
            .transition_recruit_lifecycle(agent_id, terminal, Some(reason))
            .await
        {
            return false;
        }
        let (team_id, sequence) = {
            let state = self.state.lock().await;
            let lifecycle = state
                .recruit_lifecycles
                .get(agent_id)
                .expect("transitioned recruit lifecycle must still exist");
            (lifecycle.team_id.clone(), lifecycle.sequence)
        };
        {
            let mut state = self.state.lock().await;
            state
                .agents
                .remove(&(team_id.clone(), agent_id.to_string()));
        }
        self.cleanup_agent_runtime(&team_id, agent_id).await;
        let _ = self
            .release_all_file_locks_for_agent(&team_id, agent_id)
            .await;
        if let Some(app) = self.app_handle.lock().await.clone() {
            let _ = app.emit("team:recruit-cancelled", cancel_payload);
        }
        self.schedule_terminal_recruit_cleanup(agent_id.to_string(), sequence);
        true
    }

    fn schedule_terminal_recruit_cleanup(&self, agent_id: String, sequence: u64) {
        let hub = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(TERMINAL_RECRUIT_RETENTION).await;
            let mut state = hub.state.lock().await;
            let should_remove = state
                .recruit_lifecycles
                .get(&agent_id)
                .is_some_and(|lifecycle| {
                    lifecycle.sequence == sequence
                        && matches!(
                            lifecycle.state,
                            RecruitLifecycleState::Failed | RecruitLifecycleState::Cancelled
                        )
                });
            if should_remove {
                state.recruit_lifecycles.remove(&agent_id);
            }
        });
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

    #[cfg(test)]
    pub(crate) fn lock_recruit_rescue_for_test() -> std::sync::MutexGuard<'static, ()> {
        super::recruit::RECRUIT_RESCUE_TEST_LOCK
            .lock()
            .expect("recruit rescue test mutex poisoned")
    }

    #[cfg(test)]
    pub(crate) fn take_recruit_rescued_events_for_test(&self) -> Vec<(String, u64)> {
        std::mem::take(
            &mut *super::recruit::RECRUIT_RESCUED_EVENTS_FOR_TEST
                .lock()
                .expect("recruit rescued test event mutex poisoned"),
        )
        .into_iter()
        .map(|payload| (payload.new_agent_id, payload.late_by_ms))
        .collect()
    }
}
