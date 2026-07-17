//! `team_create_leader` の補助 (lifecycle 遷移 / 失敗経路の後始末)。
//! create_leader.rs の 500 行 ratchet を守るため分離した。

use crate::team_hub::events::RecruitLifecycleState;
use crate::team_hub::TeamHub;

/// leader lifecycle の遷移 (失敗は無視: 状態機械側で不正遷移を reject する)。
pub(super) async fn leader_lifecycle(hub: &TeamHub, agent_id: &str, state: RecruitLifecycleState) {
    let _ = hub.transition_recruit_lifecycle(agent_id, state, None).await;
}

/// 失敗経路の共通後始末 (pending 即破棄 + lifecycle terminal + runtime 回収)。
/// create_leader の失敗は呼び出し元が既に Err を返しているため rescue 対象にしない:
/// grace を挟むと遅着 handshake が孤児 leader を復活させる (PR #34 レビュー)。
pub(super) async fn cancel_leader_recruit(
    hub: &TeamHub,
    team_id: &str,
    agent_id: &str,
    reason: &str,
) {
    // `team:recruit-cancelled` は finish_recruit_terminal が reason 付きで emit する。
    // 呼び出し側での手動 emit は二重通知になるため行わない (PR #34 レビュー)。
    hub.cancel_recruit_immediately(team_id, agent_id, reason).await;
}
