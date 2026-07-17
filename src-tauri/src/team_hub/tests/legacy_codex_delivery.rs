use crate::agent_runtime::{BackendKind, RuntimeManager};
use crate::pty::session::test_support::recording_handle;
use crate::pty::{codex_app_server, InFlightTracker, SessionRegistry};
use crate::team_hub::codex_delivery::CodexDelivery;
use crate::team_hub::protocol::tools::team_send;
use crate::team_hub::{CallContext, TeamHub};
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

fn hub() -> (TeamHub, Arc<SessionRegistry>) {
    let registry = Arc::new(SessionRegistry::new());
    let hub = TeamHub::with_runtime(
        registry.clone(),
        Arc::new(RuntimeManager::new()),
        InFlightTracker::new(),
    );
    hub.set_runtime_backend_for_test(BackendKind::Pty);
    (hub, registry)
}

fn codex_handle(
    agent_id: &str,
    team_id: &str,
    socket: String,
) -> (crate::pty::session::SessionHandle, Arc<Mutex<Vec<u8>>>) {
    let (mut handle, writes) = recording_handle(agent_id, team_id, Arc::new(AtomicUsize::new(0)));
    handle.is_codex = true;
    codex_app_server::set_socket(&handle, socket);
    codex_app_server::set_thread_id(&handle, "legacy-thread");
    (handle, writes)
}

async fn seed_members(hub: &TeamHub, team_id: &str, agent_id: &str) {
    let mut state = hub.state.lock().await;
    // bind_pty_runtime_endpoint の authz (active team + membership) を満たす。
    state.active_teams.insert(team_id.to_string());
    state.seed_role_binding(team_id, "leader-member", "leader");
    state.seed_role_binding(team_id, agent_id, "worker");
}

async fn send(hub: &TeamHub, team_id: &str, agent_id: &str, message: &str) -> Value {
    team_send(
        hub,
        &CallContext {
            team_id: team_id.into(),
            role: "leader".into(),
            agent_id: "leader-member".into(),
        },
        &json!({"to": agent_id, "message": message}),
    )
    .await
    .unwrap()
}

fn expected_pty_delivery(message: &str) -> Vec<u8> {
    let mut expected = crate::team_hub::inject::build_chunks("[Team ← leader] ", message)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    expected.push(b'\r');
    expected
}

#[tokio::test]
async fn pty_runtime_backend_retains_legacy_codex_app_server_precedence() {
    let (hub, registry) = hub();
    hub.set_codex_delivery_for_test(CodexDelivery::Backend);
    let team_id = "team-legacy-backend";
    let agent_id = "codex-member";
    hub.set_legacy_app_server_result_for_test(true);
    let (handle, pty_writes) = codex_handle(agent_id, team_id, "/tmp/codex.sock".into());
    assert!(registry
        .insert_if_absent("codex-session".into(), handle)
        .is_ok());
    seed_members(&hub, team_id, agent_id).await;

    let response = send(&hub, team_id, agent_id, "legacy app-server").await;

    assert_eq!(response["deliveryStatus"][agent_id]["state"], "delivered");
    assert_eq!(
        hub.take_legacy_app_server_deliveries_for_test(),
        vec![(
            agent_id.into(),
            "/tmp/codex.sock".into(),
            "legacy-thread".into(),
            "legacy app-server".into(),
        )]
    );
    assert!(pty_writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn codex_delivery_pty_setting_preserves_exact_legacy_inject_trace() {
    let (hub, registry) = hub();
    hub.set_codex_delivery_for_test(CodexDelivery::Pty);
    let team_id = "team-legacy-pty";
    let agent_id = "codex-pty-member";
    let missing_socket = std::env::temp_dir()
        .join(format!("missing-{}.sock", uuid::Uuid::new_v4().simple()))
        .to_string_lossy()
        .into_owned();
    let (handle, pty_writes) = codex_handle(agent_id, team_id, missing_socket);
    assert!(registry
        .insert_if_absent("codex-pty-session".into(), handle)
        .is_ok());
    seed_members(&hub, team_id, agent_id).await;

    let response = send(&hub, team_id, agent_id, "legacy pty bytes").await;

    assert_eq!(response["deliveryStatus"][agent_id]["state"], "delivered");
    assert!(hub.take_legacy_app_server_deliveries_for_test().is_empty());
    assert_eq!(
        *pty_writes.lock().unwrap(),
        expected_pty_delivery("legacy pty bytes")
    );
}
