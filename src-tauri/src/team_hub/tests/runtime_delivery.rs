use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability, RuntimeManager,
    RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
use crate::pty::session::test_support::recording_handle;
use crate::pty::{InFlightTracker, SessionRegistry};
use crate::team_hub::protocol::tools::team_send;
use crate::team_hub::{CallContext, TeamHub};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(super) struct RecordingNativeAdapter {
    pub(super) writes: Arc<Mutex<Vec<String>>>,
    pub(super) stops: Arc<AtomicUsize>,
    pub(super) disposes: Arc<AtomicUsize>,
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

pub(super) struct NativeFixture {
    pub(super) adapter: Arc<RecordingNativeAdapter>,
    pub(super) writes: Arc<Mutex<Vec<String>>>,
    pub(super) stops: Arc<AtomicUsize>,
    pub(super) disposes: Arc<AtomicUsize>,
}

pub(super) fn native_adapter() -> NativeFixture {
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

