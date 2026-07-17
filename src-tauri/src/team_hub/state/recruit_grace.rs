//! Issue #577: recruit ack timeout 後の grace window (遅着 ack rescue) と、
//! PR #34 レビューで導入した「terminal finalize を grace 解決後まで遅延する」経路。
//! recruit.rs の file-size ratchet を守るため grace 関連だけを分離した。

use super::super::TeamHub;
use super::recruit::{RECRUIT_GRACE_DEFAULT_MS, RECRUIT_GRACE_MAX_MS};
use std::time::Duration;
use std::time::Instant;

/// `cancel_pending_recruit_deferring` の結果。GraceScheduled の場合、呼び出し側が
/// grace 満了後に「同一 timeout 起点の pending が残っていれば除去して finalize」する責務を負う。
#[derive(Clone, Copy, Debug)]
pub(crate) enum PendingCancelOutcome {
    GraceScheduled { timed_out_at: Instant },
    Finalize,
}

/// Issue #577: timeout 後に遅着 ack を rescue する grace window。
/// `VIBE_TEAM_RECRUIT_GRACE_MS=0` は旧挙動互換、`>10000` / parse 失敗 / 未設定は default。
pub(crate) fn recruit_grace_from_env() -> Duration {
    let ms = std::env::var("VIBE_TEAM_RECRUIT_GRACE_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|&n| n <= RECRUIT_GRACE_MAX_MS)
        .unwrap_or(RECRUIT_GRACE_DEFAULT_MS);
    Duration::from_millis(ms)
}

impl TeamHub {
    /// timeout 等でキャンセル: ack channel は即時 close しつつ、短い grace window 中は
    /// pending を残して renderer からの遅着 ack を rescue できるようにする (Issue #577)。
    pub async fn cancel_pending_recruit(&self, agent_id: &str) {
        self.cancel_pending_recruit_with_grace(agent_id, recruit_grace_from_env())
            .await;
    }

    /// terminal cancel を伴う pending キャンセル (PR #34 一次レビュー 🔴2)。
    ///
    /// `cancel_pending_recruit_with_grace` と異なり、grace 満了時の pending 除去と
    /// terminal finalize を **呼び出し側 (recruit_lifecycle) の task** が行うため、
    /// ここでは除去 task を spawn しない。戻り値で grace の要否を通知する。
    pub(crate) async fn cancel_pending_recruit_deferring(
        &self,
        agent_id: &str,
    ) -> PendingCancelOutcome {
        let grace = recruit_grace_from_env();
        let timed_out_at = Instant::now();
        let mut s = self.state.lock().await;
        let Some(pending) = s.pending_recruits.get_mut(agent_id) else {
            return PendingCancelOutcome::Finalize;
        };
        if pending.timed_out_at.is_some() {
            // 既に grace 中への再要求 (例: grace 中の dismiss) は escalation:
            // rescue によるユーザー意図の巻き戻しを防ぐため即確定する。
            s.pending_recruits.remove(agent_id);
            return PendingCancelOutcome::Finalize;
        }
        let _ = pending.ack_tx.take();
        if grace.is_zero() {
            s.pending_recruits.remove(agent_id);
            return PendingCancelOutcome::Finalize;
        }
        pending.timed_out_at = Some(timed_out_at);
        PendingCancelOutcome::GraceScheduled { timed_out_at }
    }

    pub(super) async fn cancel_pending_recruit_with_grace(&self, agent_id: &str, grace: Duration) {
        let timed_out_at = Instant::now();
        let should_schedule_cleanup = {
            let mut s = self.state.lock().await;
            let Some(pending) = s.pending_recruits.get_mut(agent_id) else {
                return;
            };

            // 既に timeout 済みなら idempotent に扱う。重複 cleanup task を増やさない。
            if pending.timed_out_at.is_some() {
                return;
            }

            // ack waiter には従来どおり Err を返すため、ack_tx は timeout 時点で close する。
            let _ = pending.ack_tx.take();

            if grace.is_zero() {
                // VIBE_TEAM_RECRUIT_GRACE_MS=0 は旧挙動互換: 即時に pending を破棄する。
                s.pending_recruits.remove(agent_id);
                false
            } else {
                pending.timed_out_at = Some(timed_out_at);
                true
            }
        };

        if should_schedule_cleanup {
            let hub = self.clone();
            let agent_id = agent_id.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(grace).await;
                let mut s = hub.state.lock().await;
                let should_remove = s
                    .pending_recruits
                    .get(&agent_id)
                    .and_then(|p| p.timed_out_at)
                    .is_some_and(|ts| ts == timed_out_at);
                if should_remove {
                    s.pending_recruits.remove(&agent_id);
                }
            });
        }
    }
}
