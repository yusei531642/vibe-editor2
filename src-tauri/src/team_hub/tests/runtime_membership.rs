//! recruit lifecycle / membership 認可まわりのテスト。runtime_delivery.rs から
//! file-size ratchet (新規 500 行) 対応で分離した。fixture は runtime_delivery 側を共有する。
use super::runtime_delivery::{hub, native_adapter, seed_member};
use crate::pty::session::test_support::recording_handle;
use crate::team_hub::events::RecruitLifecycleState;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn recruit_failure_rolls_back_placeholder_process_endpoint_and_sequence() {
    let (hub, _registry, manager) = hub();
    let team_id = "team-failure";
    let agent_id = "failed-recruit";
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
    let native = native_adapter();
    assert!(manager
        .register_endpoint("failed-endpoint".into(), native.adapter)
        .result
        .is_ok());
    seed_member(&hub, team_id, agent_id, "worker").await;
    hub.bind_native_runtime_endpoint(
        team_id,
        agent_id,
        "failed-endpoint".into(),
        Some("failed-session".into()),
    )
    .await
    .unwrap();

    let _ = hub.fail_recruit(agent_id, "spawn_failed").await;
    hub.discard_pending_recruit(agent_id).await;

    let lifecycle = hub.recruit_lifecycle_for_test(agent_id).await.unwrap();
    assert_eq!(lifecycle.state, RecruitLifecycleState::Failed);
    assert_eq!(lifecycle.endpoint_id.as_deref(), Some("failed-endpoint"));
    let state = hub.state.lock().await;
    assert!(!state.pending_recruits.contains_key(agent_id));
    assert!(!state
        .agents
        .contains_key(&(team_id.to_string(), agent_id.to_string())));
    assert!(!state
        .runtime_endpoints
        .contains_key(&(team_id.to_string(), agent_id.to_string())));
    drop(state);
    assert!(manager.registry().resolve("failed-endpoint").is_none());
    assert_eq!(manager.tracked_sequence_count(), 0);
    assert_eq!(native.stops.load(Ordering::SeqCst), 1);
    assert_eq!(native.disposes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn recruit_transitions_preserve_runtime_session_and_task_association() {
    let (hub, _registry, manager) = hub();
    let native = native_adapter();
    assert!(manager
        .register_endpoint("associated-endpoint".into(), native.adapter)
        .result
        .is_ok());
    hub.begin_recruit_lifecycle("team-associated", "associated-agent", "reviewer")
        .await;
    let _ = hub
        .transition_recruit_lifecycle("associated-agent", RecruitLifecycleState::Spawning, None)
        .await;
    seed_member(&hub, "team-associated", "associated-agent", "worker").await;
    hub.bind_native_runtime_endpoint(
        "team-associated",
        "associated-agent",
        "associated-endpoint".into(),
        Some("associated-session".into()),
    )
    .await
    .unwrap();
    hub.associate_task_runtime(
        "team-associated",
        &[("associated-agent".to_string(), "worker".to_string())],
        24,
    )
    .await;
    let _ = hub
        .transition_recruit_lifecycle("associated-agent", RecruitLifecycleState::Handshaking, None)
        .await;
    let _ = hub
        .transition_recruit_lifecycle("associated-agent", RecruitLifecycleState::Ready, None)
        .await;

    let lifecycle = hub
        .recruit_lifecycle_for_test("associated-agent")
        .await
        .unwrap();
    assert_eq!(lifecycle.state, RecruitLifecycleState::Ready);
    assert_eq!(
        lifecycle.endpoint_id.as_deref(),
        Some("associated-endpoint")
    );
    assert_eq!(lifecycle.session_id.as_deref(), Some("associated-session"));
    assert_eq!(lifecycle.task_ids, vec![24]);
}

#[tokio::test]
async fn recruit_sequence_is_monotonic_and_rejected_terminal_has_no_state() {
    let (hub, _registry, _manager) = hub();
    assert!(!hub.cancel_recruit("absent-agent", "dismissed").await);
    assert!(hub
        .recruit_lifecycle_for_test("absent-agent")
        .await
        .is_none());

    hub.begin_recruit_lifecycle("team-sequence", "sequence-agent", "worker")
        .await;
    let requested = hub
        .recruit_lifecycle_for_test("sequence-agent")
        .await
        .unwrap()
        .sequence;
    assert!(hub
        .transition_recruit_lifecycle(
            "sequence-agent",
            RecruitLifecycleState::Spawning,
            None,
        )
        .await);
    let spawning = hub
        .recruit_lifecycle_for_test("sequence-agent")
        .await
        .unwrap()
        .sequence;
    hub.begin_recruit_lifecycle("team-sequence", "sequence-agent", "worker")
        .await;
    let rerequested = hub
        .recruit_lifecycle_for_test("sequence-agent")
        .await
        .unwrap()
        .sequence;

    assert!(requested < spawning);
    assert!(spawning < rerequested);
}

/// PR #37 レビュー 🟡: spawn 前 pre-check (terminal_create / worktree assign) は
/// PTY も hub handshake も無い「recruit 進行中」の agent を許容する。判定基準は
/// bind 側 authorize_runtime_endpoint_binding と同一。
#[tokio::test]
async fn authorize_team_agent_binding_allows_recruit_in_progress_member() {
    let (hub, registry, _manager) = hub();
    {
        let mut state = hub.state.lock().await;
        state.active_teams.insert("team-precheck".into());
    }
    hub.begin_recruit_lifecycle("team-precheck", "fresh-recruit", "programmer")
        .await;

    hub.authorize_team_agent_binding("team-precheck", "fresh-recruit")
        .await
        .expect("recruit 進行中の agent は spawn 前 pre-check を通過する");
    // 非メンバー・非 recruit は従来どおり拒否
    assert!(hub
        .authorize_team_agent_binding("team-precheck", "stranger")
        .await
        .is_err());
    let (handle, _writes) = recording_handle(
        "session-only",
        "team-precheck",
        Arc::new(AtomicUsize::new(0)),
    );
    assert!(registry
        .insert_if_absent("session-only".into(), handle)
        .is_ok());
    assert!(hub
        .authorize_team_agent_binding("team-precheck", "session-only")
        .await
        .is_err());
    // terminal 状態 (Cancelled) の recruit は許容しない
    assert!(hub.cancel_recruit("fresh-recruit", "dismissed").await);
    assert!(hub
        .authorize_team_agent_binding("team-precheck", "fresh-recruit")
        .await
        .is_err());
}

/// PR #34 一次レビュー 🟡7: bind_native_runtime_endpoint は renderer 由来の
/// (team_id, agent_id) を fail-closed に検証する。
#[tokio::test]
async fn bind_native_endpoint_rejects_unauthorized_team_or_agent() {
    let (hub, _registry, manager) = hub();
    let native = native_adapter();
    assert!(manager
        .register_endpoint("endpoint-a".into(), native.adapter)
        .result
        .is_ok());

    // inactive team は拒否
    let err = hub
        .bind_native_runtime_endpoint("ghost-team", "agent-a", "endpoint-a".into(), None)
        .await
        .unwrap_err();
    assert!(err.contains("not active"), "{err}");

    // active team でも非メンバーは拒否
    seed_member(&hub, "team-authz", "member-a", "worker").await;
    let err = hub
        .bind_native_runtime_endpoint("team-authz", "intruder", "endpoint-a".into(), None)
        .await
        .unwrap_err();
    assert!(err.contains("not a member"), "{err}");

    // メンバーへの bind は成功し、live binding の乗っ取りは拒否
    hub.bind_native_runtime_endpoint("team-authz", "member-a", "endpoint-a".into(), None)
        .await
        .unwrap();
    let other = native_adapter();
    assert!(manager
        .register_endpoint("endpoint-b".into(), other.adapter)
        .result
        .is_ok());
    let err = hub
        .bind_native_runtime_endpoint("team-authz", "member-a", "endpoint-b".into(), None)
        .await
        .unwrap_err();
    assert!(err.contains("already has a live native endpoint"), "{err}");
}
