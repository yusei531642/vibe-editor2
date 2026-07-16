use crate::agent_runtime::{BackendKind, RuntimeManager};
use crate::pty::session::test_support::failing_recording_handle;
use crate::pty::{InFlightTracker, SessionRegistry};
use crate::team_hub::protocol::tools::team_send;
use crate::team_hub::{CallContext, TeamHub};
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

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

async fn seed_members(hub: &TeamHub, team_id: &str, agent_id: &str) {
    let mut state = hub.state.lock().await;
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

fn assert_reason_code(response: &Value, agent_id: &str, expected: &str) {
    assert_eq!(
        response["deliveryStatus"][agent_id]["reason"]["code"],
        expected
    );
    assert_eq!(response["failedRecipients"][0]["reason"]["code"], expected);
}

#[tokio::test]
async fn pty_delivery_preserves_inject_no_session_reason_code() {
    let (hub, _registry) = hub();
    let team_id = "team-no-session";
    let agent_id = "missing-member";
    seed_members(&hub, team_id, agent_id).await;

    let response = send(&hub, team_id, agent_id, "missing session").await;

    assert_reason_code(&response, agent_id, "inject_no_session");
}

#[tokio::test]
async fn pty_delivery_preserves_inject_write_partial_reason_code() {
    let (hub, registry) = hub();
    let team_id = "team-partial";
    let agent_id = "partial-member";
    let (handle, writes) =
        failing_recording_handle(agent_id, team_id, Arc::new(AtomicUsize::new(0)), 1);
    assert!(registry
        .insert_if_absent("partial-session".into(), handle)
        .is_ok());
    seed_members(&hub, team_id, agent_id).await;

    let response = send(&hub, team_id, agent_id, &"x".repeat(256)).await;

    assert!(!writes.lock().unwrap().is_empty());
    assert_reason_code(&response, agent_id, "inject_write_partial");
}
