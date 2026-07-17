//! Recruit の明示状態機械と failure / cancellation rollback。

use super::recruit_grace::{recruit_grace_from_env, PendingCancelOutcome};
use crate::team_hub::events::{RecruitLifecyclePayload, RecruitLifecycleState};
use crate::team_hub::runtime_endpoint::types::RuntimeEndpoint;
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
        self.transition_recruit_lifecycle_with_payload(agent_id, next, reason)
            .await
            .is_some()
    }

    /// 遷移に成功したら emit 済み payload を返す。遷移と同一 lock 内で snapshot を取るため、
    /// 呼び出し側は再 lock で entry を引き直す必要がない (別 task の retain と競合しても
    /// panic しない — PR #34 一次レビュー 🔴1)。
    async fn transition_recruit_lifecycle_with_payload(
        &self,
        agent_id: &str,
        next: RecruitLifecycleState,
        reason: Option<String>,
    ) -> Option<RecruitLifecyclePayload> {
        let payload = {
            let mut state = self.state.lock().await;
            let lifecycle = state.recruit_lifecycles.get_mut(agent_id)?;
            if lifecycle.state == next {
                return None;
            }
            if !can_transition(lifecycle.state, next) {
                tracing::warn!(
                    agent_id,
                    from = ?lifecycle.state,
                    to = ?next,
                    "[teamhub] rejected recruit lifecycle transition"
                );
                return None;
            }
            lifecycle.sequence = next_recruit_sequence();
            lifecycle.state = next;
            lifecycle.reason = reason;
            lifecycle.payload()
        };
        self.emit_recruit_lifecycle(payload.clone()).await;
        Some(payload)
    }

    /// handshake 完了済み member の lifecycle を Ready まで進める (冪等)。
    ///
    /// 通常経路では `verify_recruit_liveness` が Ready を打つが、Issue #577 の遅着 ack rescue
    /// では `team_recruit` が既に timeout で return しているため到達しない。rescue 後の
    /// handshake 成功時にここで前進させ、placeholder が spawning のまま解決しない事態を防ぐ
    /// (PR #34 二次レビュー)。既に Ready / terminal の場合は各遷移が拒否されて no-op。
    pub async fn advance_recruit_to_ready(&self, agent_id: &str) {
        for next in [
            RecruitLifecycleState::Spawning,
            RecruitLifecycleState::Handshaking,
            RecruitLifecycleState::Ready,
        ] {
            let _ = self.transition_recruit_lifecycle(agent_id, next, None).await;
        }
    }

    pub async fn fail_recruit(&self, agent_id: &str, reason: impl Into<String>) -> bool {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Failed, reason.into())
            .await
    }

    pub async fn cancel_recruit(&self, agent_id: &str, reason: impl Into<String>) -> bool {
        self.finish_recruit_terminal(agent_id, RecruitLifecycleState::Cancelled, reason.into())
            .await
    }

    /// grace 付き terminal cancel (PR #34 一次レビュー 🔴2)。
    ///
    /// terminal cleanup を先に確定させると、Issue #577 の遅着 ack rescue が
    /// 「endpoint / AgentEntry 破棄済みの zombie member」を復活させてしまう。
    /// そのため pending の grace 解決を待ってから terminal 遷移を finalize する:
    /// - grace 中に rescue された場合は terminal 処理を行わない (handshake が続行する)
    /// - grace 満了 (rescue 無し) / pending 不在 / grace=0 の場合に finalize する
    /// - 既に grace 中の pending への再要求 (例: grace 中の dismiss) は escalation として
    ///   pending を即破棄し finalize する (rescue によるユーザー意図の巻き戻しを防ぐ)
    pub async fn cancel_recruit_with_pending_grace(
        &self,
        team_id: &str,
        agent_id: &str,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        match self.cancel_pending_recruit_deferring(agent_id).await {
            PendingCancelOutcome::GraceScheduled { timed_out_at } => {
                let hub = self.clone();
                let team_id = team_id.to_string();
                let agent_id = agent_id.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(recruit_grace_from_env()).await;
                    let rescued = {
                        let mut s = hub.state.lock().await;
                        match s
                            .pending_recruits
                            .get(&agent_id)
                            .and_then(|p| p.timed_out_at)
                        {
                            // 同一 timeout 起点の pending が残っている = rescue されなかった。
                            Some(ts) if ts == timed_out_at => {
                                s.pending_recruits.remove(&agent_id);
                                false
                            }
                            // pending が消えている / 別 timeout 起点 = rescue 済みか escalation 済み。
                            _ => true,
                        }
                    };
                    if !rescued {
                        hub.finalize_recruit_cancel(&team_id, &agent_id, &reason).await;
                    }
                });
            }
            PendingCancelOutcome::Finalize => {
                self.finalize_recruit_cancel(team_id, agent_id, &reason).await;
            }
        }
    }

    /// grace を一切挟まない即時 terminal cancel。dismiss などユーザー意図の除去は
    /// 遅着 handshake / rescue によって巻き戻されてはならない (PR #34 レビュー):
    /// pending を即破棄してから terminal を確定する。
    pub async fn cancel_recruit_immediately(
        &self,
        team_id: &str,
        agent_id: &str,
        reason: impl Into<String>,
    ) {
        self.discard_pending_recruit(agent_id).await;
        self.finalize_recruit_cancel(team_id, agent_id, &reason.into()).await;
    }

    async fn finalize_recruit_cancel(&self, team_id: &str, agent_id: &str, reason: &str) {
        if !self.cancel_recruit(agent_id, reason.to_string()).await {
            // lifecycle 不在 (再起動後など) でも binding / process は残り得るため回収する。
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
        // 遷移と同一 lock 内で snapshot した payload を使う。遷移直後に別 task
        // (clear_team の retain / TTL 掃除) が entry を消しても panic しない。
        let Some(payload) = self
            .transition_recruit_lifecycle_with_payload(agent_id, terminal, Some(reason))
            .await
        else {
            return false;
        };
        let (team_id, sequence) = (payload.team_id.clone(), payload.sequence);
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
