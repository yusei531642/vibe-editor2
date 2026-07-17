//! recruit の grace rescue / dismiss 復活防止の統合テスト。
//! runtime_delivery.rs の 500 行 ratchet を守るため分離した。

use super::runtime_delivery::{hub, seed_member};
use crate::team_hub::events::RecruitLifecycleState;
use crate::team_hub::TeamHub;

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn production_timeout_cancellation_keeps_grace_rescue_reachable() {
    let _rescue_guard = TeamHub::lock_recruit_rescue_for_test();
    let (hub, _registry, _manager) = hub();
    let team_id = "team-grace-integration";
    let agent_id = "grace-agent";
    let _ = hub.take_recruit_rescued_events_for_test();
    let _channels = hub
        .try_register_pending_recruit(
            agent_id.into(),
            team_id.into(),
            "programmer".into(),
            "leader".into(),
            false,
            &[],
        )
        .await
        .unwrap();
    hub.begin_recruit_lifecycle(team_id, agent_id, "programmer")
        .await;
    let _ = hub
        .transition_recruit_lifecycle(agent_id, RecruitLifecycleState::Spawning, None)
        .await;

    hub.cancel_recruit_with_pending_grace(team_id, agent_id, "ack_timeout")
        .await;
    assert!(hub
        .state
        .lock()
        .await
        .pending_recruits
        .contains_key(agent_id));

    hub.resolve_recruit_ack(
        agent_id,
        team_id,
        crate::team_hub::RecruitAckOutcome {
            ok: true,
            reason: None,
            phase: None,
        },
    )
    .await
    .unwrap();
    let rescued = hub.take_recruit_rescued_events_for_test();
    assert_eq!(rescued.len(), 1);
    assert_eq!(rescued[0].0, agent_id);

    // rescue 後の handshake 成功で lifecycle が Ready まで解決すること
    // (PR #34 二次レビュー: spawning のまま取り残されない)。
    assert!(
        hub.resolve_pending_recruit(agent_id, team_id, "programmer")
            .await
    );
    let lifecycle_state = hub
        .state
        .lock()
        .await
        .recruit_lifecycles
        .get(agent_id)
        .map(|l| l.state);
    assert_eq!(lifecycle_state, Some(RecruitLifecycleState::Ready));
}

/// PR #34 レビュー: dismiss は grace/rescue の対象外。handshake 窓での dismiss 後に
/// 遅着 handshake が来ても member として復活しない。
#[tokio::test]
async fn dismiss_during_handshake_window_cannot_be_resurrected() {
    let (hub, _registry, _manager) = hub();
    let team_id = "team-dismiss-window";
    let agent_id = "window-agent";
    let _channels = hub
        .try_register_pending_recruit(
            agent_id.into(),
            team_id.into(),
            "programmer".into(),
            "leader".into(),
            false,
            &[],
        )
        .await
        .unwrap();
    hub.begin_recruit_lifecycle(team_id, agent_id, "programmer").await;
    seed_member(&hub, team_id, agent_id, "programmer").await;

    hub.cancel_recruit_immediately(team_id, agent_id, "dismissed").await;

    // 遅着 handshake は pending が既に無いため拒否される (復活しない)。
    assert!(!hub.resolve_pending_recruit(agent_id, team_id, "programmer").await);
    let state = hub.state.lock().await;
    assert!(!state.pending_recruits.contains_key(agent_id));
    let lifecycle = state.recruit_lifecycles.get(agent_id).map(|l| l.state);
    assert_eq!(lifecycle, Some(RecruitLifecycleState::Cancelled));
}
