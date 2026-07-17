use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability, RuntimeManager,
    RuntimeSessionSpawnRequest,
};
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

/// PR #34 二次レビュー 🟡: native binding のみ (PTY session なし) の member で native
/// 配送が失敗したとき、PTY fallback へ落ちて `inject_no_session` に化けず、元の
/// native エラーが reason code として保たれること。
struct FailingNativeAdapter;

impl AgentRuntimeAdapter for FailingNativeAdapter {
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

    fn spawn_turn(
        &self,
        _request: &crate::agent_runtime::RuntimeTurnSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        self.write("")
    }

    fn write(&self, _data: &str) -> Result<(), RuntimeAdapterError> {
        Err(RuntimeAdapterError::new(
            "codex_turn_start_failed",
            "turn/start rejected by app-server",
            true,
        ))
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }
}

#[tokio::test]
async fn native_only_member_keeps_native_error_instead_of_pty_fallback() {
    let (hub, _registry, manager) = super::runtime_delivery::hub();
    hub.set_runtime_backend_for_test(BackendKind::Auto);
    let team_id = "team-native-only";
    let agent_id = "native-only-member";
    assert!(manager
        .register_endpoint("native-only-endpoint".into(), Arc::new(FailingNativeAdapter))
        .result
        .is_ok());
    super::runtime_delivery::seed_member(&hub, team_id, agent_id, "worker").await;
    super::runtime_delivery::seed_member(&hub, team_id, "leader-member", "leader").await;
    hub.bind_native_runtime_endpoint(team_id, agent_id, "native-only-endpoint".into(), None)
        .await
        .unwrap();

    let response = send(&hub, team_id, agent_id, "native failure").await;

    // fallback で PtyCompatAdapter が登録されて lifecycle.endpoint_id を上書きしないこと。
    let lifecycle = hub.recruit_lifecycle_for_test(agent_id).await.unwrap();
    assert_eq!(
        lifecycle.endpoint_id.as_deref(),
        Some("native-only-endpoint")
    );
    assert_reason_code(&response, agent_id, "inject_write_initial_failed");
    let message = response["deliveryStatus"][agent_id]["reason"]["message"]
        .as_str()
        .unwrap();
    assert!(message.contains("codex_turn_start_failed"), "{message}");
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
