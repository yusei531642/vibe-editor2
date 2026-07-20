use crate::agent_runtime::claude_agent::{
    ClaudeAdapterEvent, ClaudeAgentRuntimeAdapter, ClaudeAgentRuntimeConfig, SidecarLaunchConfig,
};
use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability, RuntimeManager,
    RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
use crate::pty::{InFlightTracker, SessionRegistry};
use crate::team_hub::TeamHub;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct CodexLeaderFixture;

impl AgentRuntimeAdapter for CodexLeaderFixture {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        vec![RuntimeCapability::StructuredEventStream]
    }

    fn spawn_session(
        &self,
        _request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn spawn_turn(&self, _request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn write(&self, _data: &str) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }
}

fn claude_reviewer(manager: Arc<RuntimeManager>) -> Arc<ClaudeAgentRuntimeAdapter> {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../src-sidecars/claude-agent/fixtures/fake-sidecar.mjs");
    let sink_manager = manager.clone();
    let sink = Arc::new(move |event| match event {
        ClaudeAdapterEvent::Session(_) => {}
        ClaudeAdapterEvent::Payload(payload) => {
            sink_manager.record_event("claude-reviewer", payload);
        }
        ClaudeAdapterEvent::Failure(error) => {
            sink_manager.fail_endpoint("claude-reviewer", error);
        }
    });
    Arc::new(
        ClaudeAgentRuntimeAdapter::connect(
            SidecarLaunchConfig {
                program: crate::agent_runtime::resolve_node_executable().expect("node fixture"),
                args: vec![fixture.to_string_lossy().into_owned(), "happy".to_string()],
                environment: Vec::new(),
                secret_values: Vec::new(),
                response_timeout: Duration::from_secs(2),
            },
            ClaudeAgentRuntimeConfig {
                system_prompt: Some("Review the leader output".to_string()),
                ..Default::default()
            },
            sink,
        )
        .expect("Claude fixture connects"),
    )
}

#[tokio::test]
async fn same_team_binds_codex_leader_and_claude_reviewer_simultaneously() {
    let manager = Arc::new(RuntimeManager::new());
    let hub = TeamHub::with_runtime(
        Arc::new(SessionRegistry::new()),
        manager.clone(),
        InFlightTracker::new(),
    );
    {
        let mut state = hub.state.lock().await;
        state.active_teams.insert("phase7-team".to_string());
        state.seed_role_binding("phase7-team", "codex-leader", "leader");
        state.seed_role_binding("phase7-team", "claude-reviewer", "reviewer");
    }
    hub.begin_recruit_lifecycle("phase7-team", "codex-leader", "leader")
        .await;
    hub.begin_recruit_lifecycle("phase7-team", "claude-reviewer", "reviewer")
        .await;

    assert!(manager
        .register_endpoint("codex-leader".into(), Arc::new(CodexLeaderFixture))
        .result
        .is_ok());
    let reviewer = claude_reviewer(manager.clone());
    assert!(manager
        .register_endpoint("claude-reviewer".into(), reviewer.clone())
        .result
        .is_ok());
    hub.bind_native_runtime_endpoint(
        "phase7-team",
        "codex-leader",
        "codex-leader".into(),
        Some("codex-session".into()),
    )
    .await
    .unwrap();
    hub.bind_native_runtime_endpoint(
        "phase7-team",
        "claude-reviewer",
        "claude-reviewer".into(),
        reviewer.session_id(),
    )
    .await
    .unwrap();

    let bindings = hub.runtime_bindings_snapshot("phase7-team").await;
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().all(|binding| binding.live));
    assert!(bindings.iter().any(|binding| {
        binding.agent_id == "codex-leader" && binding.endpoint_id == "codex-leader"
    }));
    assert!(bindings.iter().any(|binding| {
        binding.agent_id == "claude-reviewer" && binding.endpoint_id == "claude-reviewer"
    }));

    assert!(manager.dispose("claude-reviewer").result.is_ok());
    assert!(manager.dispose("codex-leader").result.is_ok());
}
