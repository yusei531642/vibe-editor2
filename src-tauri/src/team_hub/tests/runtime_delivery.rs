use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability, RuntimeManager,
    RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
use crate::pty::session::test_support::recording_handle;
use crate::pty::{InFlightTracker, SessionRegistry};
use crate::team_hub::events::RecruitLifecycleState;
use crate::team_hub::protocol::tools::team_send;
use crate::team_hub::{CallContext, TeamHub};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct RecordingNativeAdapter {
    writes: Arc<Mutex<Vec<String>>>,
    stops: Arc<AtomicUsize>,
    disposes: Arc<AtomicUsize>,
}

impl AgentRuntimeAdapter for RecordingNativeAdapter {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        vec![RuntimeCapability::NativeProcessExecution]
    }

    fn spawn_session(
        &self,
        _request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        self.write(&request.input)
    }

    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(data.to_string());
        Ok(())
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        self.disposes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct NativeFixture {
    adapter: Arc<RecordingNativeAdapter>,
    writes: Arc<Mutex<Vec<String>>>,
    stops: Arc<AtomicUsize>,
    disposes: Arc<AtomicUsize>,
}

fn native_adapter() -> NativeFixture {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let stops = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    NativeFixture {
        adapter: Arc::new(RecordingNativeAdapter {
            writes: writes.clone(),
            stops: stops.clone(),
            disposes: disposes.clone(),
        }),
        writes,
        stops,
        disposes,
    }
}

pub(super) fn hub() -> (TeamHub, Arc<SessionRegistry>, Arc<RuntimeManager>) {
    let registry = Arc::new(SessionRegistry::new());
    let manager = Arc::new(RuntimeManager::new());
    let hub = TeamHub::with_runtime(registry.clone(), manager.clone(), InFlightTracker::new());
    (hub, registry, manager)
}

pub(super) async fn seed_member(hub: &TeamHub, team_id: &str, agent_id: &str, role: &str) {
    {
        let mut state = hub.state.lock().await;
        state.active_teams.insert(team_id.to_string());
        state.seed_role_binding(team_id, agent_id, role);
    }
    // native bind の spawn-phase gate を満たすため、spawn 中の lifecycle も登録する。
    hub.begin_recruit_lifecycle(team_id, agent_id, role).await;
    let _ = hub
        .transition_recruit_lifecycle(
            agent_id,
            crate::team_hub::events::RecruitLifecycleState::Spawning,
            None,
        )
        .await;
}


fn expected_pty_delivery(from_role: &str, message: &str) -> Vec<u8> {
    let banner = format!("[Team ← {from_role}] ");
    let mut expected = crate::team_hub::inject::build_chunks(&banner, message)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    expected.push(b'\r');
    expected
}

#[tokio::test]
async fn team_send_delivers_to_native_and_pty_members_through_agent_mapping() {
    let (hub, registry, manager) = hub();
    hub.set_runtime_backend_for_test(BackendKind::Auto);
    let team_id = "team-mixed";
    let native_id = "native-member";
    let pty_id = "pty-member";
    let native = native_adapter();
    assert!(manager
        .register_endpoint("native-endpoint".into(), native.adapter)
        .result
        .is_ok());
    seed_member(&hub, team_id, native_id, "worker").await;
    hub.bind_native_runtime_endpoint(
        team_id,
        native_id,
        "native-endpoint".into(),
        Some("thread-native".into()),
    )
    .await
    .unwrap();

    seed_member(&hub, team_id, pty_id, "reviewer").await;
    let kills = Arc::new(AtomicUsize::new(0));
    let (handle, pty_writes) = recording_handle(pty_id, team_id, kills);
    assert!(registry
        .insert_if_absent("pty-session".into(), handle)
        .is_ok());
    hub.bind_pty_runtime_endpoint(team_id, pty_id, Some("pty-session".into()))
        .await
        .unwrap();

    seed_member(&hub, team_id, "leader-member", "leader").await;
    seed_member(&hub, team_id, native_id, "programmer").await;
    let response = team_send(
        &hub,
        &CallContext {
            team_id: team_id.into(),
            role: "leader".into(),
            agent_id: "leader-member".into(),
        },
        &json!({"to": "all", "message": "mixed delivery"}),
    )
    .await
    .unwrap();

    assert_eq!(response["delivered"].as_array().unwrap().len(), 2);
    assert_eq!(
        *native
            .writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec!["[Team ← leader] mixed delivery".to_string()]
    );
    assert_eq!(
        *pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        expected_pty_delivery("leader", "mixed delivery")
    );
}

#[tokio::test]
async fn live_team_members_excludes_disposed_native_endpoint_but_roster_keeps_it() {
    let (hub, _registry, manager) = hub();
    let team_id = "team-native-members";
    for (agent_id, endpoint_id) in [
        ("live-native", "live-native-endpoint"),
        ("dead-native", "dead-native-endpoint"),
    ] {
        let native = native_adapter();
        assert!(manager
            .register_endpoint(endpoint_id.into(), native.adapter)
            .result
            .is_ok());
        seed_member(&hub, team_id, agent_id, "worker").await;
        hub.bind_native_runtime_endpoint(team_id, agent_id, endpoint_id.into(), None)
            .await
            .unwrap();
    }

    assert!(manager.dispose("dead-native-endpoint").result.is_ok());

    let mut roster = hub.team_members(team_id).await;
    roster.sort();
    assert_eq!(
        roster,
        vec![
            ("dead-native".into(), "worker".into()),
            ("live-native".into(), "worker".into()),
        ]
    );
    let members = hub.live_team_members(team_id).await;
    assert_eq!(members, vec![("live-native".into(), "worker".into())]);
}

#[tokio::test]
async fn pty_binding_rejects_session_created_before_member_authorization() {
    let (hub, registry, _manager) = hub();
    let team_id = "team-pty-authz";
    let agent_id = "untrusted-member";
    hub.state.lock().await.active_teams.insert(team_id.into());
    let (handle, _writes) =
        recording_handle(agent_id, team_id, Arc::new(AtomicUsize::new(0)));
    assert!(registry
        .insert_if_absent("untrusted-session".into(), handle)
        .is_ok());

    let error = hub
        .bind_pty_runtime_endpoint(team_id, agent_id, Some("untrusted-session".into()))
        .await
        .unwrap_err();

    assert!(error.contains("not a member"), "{error}");
}

#[tokio::test]
async fn explicit_pty_backend_preserves_legacy_inject_trace_and_skips_native() {
    let (hub, registry, manager) = hub();
    hub.set_runtime_backend_for_test(BackendKind::Pty);
    let team_id = "team-pty-regression";
    let agent_id = "dual-member";
    let native = native_adapter();
    assert!(manager
        .register_endpoint("native-dual".into(), native.adapter)
        .result
        .is_ok());
    seed_member(&hub, team_id, agent_id, "worker").await;
    hub.bind_native_runtime_endpoint(
        team_id,
        agent_id,
        "native-dual".into(),
        Some("thread-dual".into()),
    )
    .await
    .unwrap();
    let (handle, pty_writes) = recording_handle(agent_id, team_id, Arc::new(AtomicUsize::new(0)));
    assert!(registry.insert_if_absent("pty-dual".into(), handle).is_ok());
    // renderer 経由の bind は live native がいる member へは拒否される (乗っ取り防止)。
    assert!(hub
        .bind_pty_runtime_endpoint(team_id, agent_id, Some("pty-dual".into()))
        .await
        .is_err());
    // 配送 fallback (信頼済み Rust 経路) は backend=pty 強制時に PTY を成立させる。
    hub.bind_pty_runtime_endpoint_for_delivery(team_id, agent_id, Some("pty-dual".into()))
        .await
        .unwrap();
    seed_member(&hub, team_id, "leader-pty", "leader").await;
    seed_member(&hub, team_id, agent_id, "worker").await;

    team_send(
        &hub,
        &CallContext {
            team_id: team_id.into(),
            role: "leader".into(),
            agent_id: "leader-pty".into(),
        },
        &json!({"to": agent_id, "message": "legacy bytes"}),
    )
    .await
    .unwrap();

    assert!(native
        .writes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_empty());
    assert_eq!(
        *pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        expected_pty_delivery("leader", "legacy bytes")
    );
}

#[tokio::test]
async fn concurrent_pty_binding_registers_one_live_endpoint() {
    let (hub, _registry, manager) = hub();
    seed_member(&hub, "team-race", "race-member", "worker").await;
    let first = hub.bind_pty_runtime_endpoint("team-race", "race-member", None);
    let second = hub.bind_pty_runtime_endpoint("team-race", "race-member", None);

    let (first, second) = tokio::join!(first, second);

    assert_eq!(first.as_deref(), Ok("team-pty-race-member"));
    assert_eq!(second.as_deref(), Ok("team-pty-race-member"));
    assert!(manager.registry().resolve("team-pty-race-member").is_some());
}

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
