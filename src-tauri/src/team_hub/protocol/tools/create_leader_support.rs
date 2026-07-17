//! `team_create_leader` の補助 (lifecycle 遷移 / 失敗経路の後始末)。
//! create_leader.rs の 500 行 ratchet を守るため分離した。

use crate::team_hub::events::RecruitLifecycleState;
use crate::team_hub::TeamHub;

/// leader lifecycle の遷移 (失敗は無視: 状態機械側で不正遷移を reject する)。
pub(super) async fn leader_lifecycle(hub: &TeamHub, agent_id: &str, state: RecruitLifecycleState) {
    let _ = hub.transition_recruit_lifecycle(agent_id, state, None).await;
}

/// 失敗経路の共通後始末 (lifecycle terminal + pending grace + runtime 回収)。
pub(super) async fn cancel_leader_recruit(hub: &TeamHub, team_id: &str, agent_id: &str) {
    hub.cancel_recruit_with_pending_grace(team_id, agent_id, "create_leader_cancelled")
        .await;
}
