use super::{
    ClaudeAdapterEvent, ClaudeAgentRuntimeAdapter, ClaudeAgentRuntimeConfig, SidecarLaunchConfig,
};
use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeApprovalResponseRequest,
    RuntimeCapability, RuntimeDeliveryRequest, RuntimeEventPayload, RuntimeLifecycleState,
    RuntimeManager, RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn fixture_config(scenario: &str, secret: Option<&str>) -> SidecarLaunchConfig {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../src-sidecars/claude-agent/fixtures/fake-sidecar.mjs");
    let mut environment = Vec::new();
    let mut secret_values = Vec::new();
    if let Some(secret) = secret {
        environment.push(("TEST_SECRET".to_string(), secret.to_string()));
        secret_values.push(secret.to_string());
    }
    SidecarLaunchConfig {
        program: crate::agent_runtime::resolve_node_executable().expect("node fixture runtime"),
        args: vec![fixture.to_string_lossy().into_owned(), scenario.to_string()],
        environment,
        secret_values,
        response_timeout: Duration::from_secs(2),
    }
}

#[test]
fn production_launch_rejects_settings_supplied_claude_wrappers() {
    let error = match SidecarLaunchConfig::production("/tmp/untrusted-claude-wrapper".into()) {
        Ok(_) => panic!("custom command unexpectedly entered native provider"),
        Err(error) => error,
    };
    assert_eq!(error.code, "runtime_claude_custom_command_requires_pty");
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for sidecar event"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn managed_adapter(
    manager: Arc<RuntimeManager>,
    endpoint_id: &str,
    scenario: &str,
    secret: Option<&str>,
) -> Arc<ClaudeAgentRuntimeAdapter> {
    let sink_manager = manager.clone();
    let sink_endpoint = endpoint_id.to_string();
    let sink = Arc::new(move |event| match event {
        ClaudeAdapterEvent::Session(_) => {}
        ClaudeAdapterEvent::Payload(payload) => {
            sink_manager.record_event(&sink_endpoint, payload);
        }
        ClaudeAdapterEvent::Failure(error) => {
            sink_manager.fail_endpoint(&sink_endpoint, error);
        }
    });
    Arc::new(
        ClaudeAgentRuntimeAdapter::connect(
            fixture_config(scenario, secret),
            ClaudeAgentRuntimeConfig::default(),
            sink,
        )
        .expect("fixture sidecar connects"),
    )
}

#[test]
fn fixture_projects_message_tool_approval_diff_usage_and_lifecycle() {
    let manager = Arc::new(RuntimeManager::new());
    let adapter = managed_adapter(manager.clone(), "claude-reviewer", "happy", None);
    let registration = manager.register_endpoint("claude-reviewer".into(), adapter.clone());
    assert!(registration.result.is_ok());
    assert!(manager
        .spawn_turn(
            "claude-reviewer",
            RuntimeTurnSpawnRequest {
                input: "review".into(),
                submit: true,
                model: None,
                effort: None,
                permission: None,
            },
        )
        .result
        .is_ok());
    wait_until(|| adapter.session_id().as_deref() == Some("claude-fixture-session"));
    wait_until(|| {
        manager
            .event_snapshot()
            .iter()
            .any(|event| matches!(event.payload, RuntimeEventPayload::ApprovalRequest { .. }))
    });
    assert!(manager
        .respond_approval("claude-reviewer", "approval-1".into(), "accept".into(),)
        .result
        .is_ok());
    wait_until(|| {
        manager
            .event_snapshot()
            .iter()
            .any(|event| matches!(event.payload, RuntimeEventPayload::Usage { .. }))
    });
    let events = manager.event_snapshot();
    assert!(events.iter().any(|event| matches!(
        event.payload,
        RuntimeEventPayload::Lifecycle {
            state: RuntimeLifecycleState::Ready,
            ..
        }
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::MessageDelta { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::MessageComplete { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::ToolUse { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Diagnostic { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Diff { .. })));
}

struct FakeCodexAdapter;

impl AgentRuntimeAdapter for FakeCodexAdapter {
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

#[test]
fn same_manager_runs_codex_leader_and_claude_reviewer_simultaneously() {
    let manager = Arc::new(RuntimeManager::new());
    let reviewer = managed_adapter(manager.clone(), "claude-reviewer", "happy", None);
    assert!(manager
        .register_endpoint("codex-leader".into(), Arc::new(FakeCodexAdapter))
        .result
        .is_ok());
    assert!(manager
        .register_endpoint("claude-reviewer".into(), reviewer)
        .result
        .is_ok());
    assert!(manager.registry().resolve("codex-leader").is_some());
    assert!(manager.registry().resolve("claude-reviewer").is_some());
}

#[test]
fn protocol_mismatch_is_rejected_before_registration() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let result = ClaudeAgentRuntimeAdapter::connect(
        fixture_config("protocol-mismatch", None),
        ClaudeAgentRuntimeConfig::default(),
        Arc::new(move |event| captured.lock().unwrap().push(event)),
    );
    let error = match result {
        Ok(_) => panic!("protocol mismatch unexpectedly connected"),
        Err(error) => error,
    };
    assert_eq!(error.code, "runtime_claude_sidecar_protocol");
    assert!(!error.recoverable);
}

#[test]
fn crash_emits_error_then_failed_and_detaches_endpoint() {
    let manager = Arc::new(RuntimeManager::new());
    let adapter = managed_adapter(manager.clone(), "crashing", "crash", None);
    assert!(manager
        .register_endpoint("crashing".into(), adapter)
        .result
        .is_ok());
    let _ = manager.spawn_turn(
        "crashing",
        RuntimeTurnSpawnRequest {
            input: "crash".into(),
            submit: true,
            model: None,
            effort: None,
            permission: None,
        },
    );
    wait_until(|| manager.registry().resolve("crashing").is_none());
    let events = manager.event_snapshot();
    let error_index = events.iter().position(|event| {
        matches!(
            &event.payload,
            RuntimeEventPayload::Error { code, .. } if code == "runtime_claude_sidecar_crashed"
        )
    });
    let failed_index = events.iter().position(|event| {
        matches!(
            event.payload,
            RuntimeEventPayload::Lifecycle {
                state: RuntimeLifecycleState::Failed,
                ..
            }
        )
    });
    assert!(error_index.is_some_and(|index| failed_index.is_some_and(|failed| index < failed)));
}

#[test]
fn dispose_after_reader_failure_skips_response_timeout() {
    let failed = Arc::new(AtomicBool::new(false));
    let captured = failed.clone();
    let adapter = ClaudeAgentRuntimeAdapter::connect(
        fixture_config("invalid-json", None),
        ClaudeAgentRuntimeConfig::default(),
        Arc::new(move |event| {
            if matches!(event, ClaudeAdapterEvent::Failure(_)) {
                captured.store(true, Ordering::Release);
            }
        }),
    )
    .expect("fixture sidecar connects");
    adapter
        .spawn_session(&RuntimeSessionSpawnRequest {
            endpoint_id: "invalid-json".into(),
        })
        .expect("fixture session starts");
    adapter
        .spawn_turn(&RuntimeTurnSpawnRequest {
            input: "trigger protocol failure".into(),
            submit: true,
            model: None,
            effort: None,
            permission: None,
        })
        .expect("turn response arrives before invalid JSON");
    wait_until(|| failed.load(Ordering::Acquire));

    let started = Instant::now();
    adapter.dispose().expect("dispose succeeds");

    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn team_mcp_config_reaches_claude_sidecar_without_renderer_round_trip() {
    let manager = Arc::new(RuntimeManager::new());
    let sink_manager = manager.clone();
    let adapter = Arc::new(
        ClaudeAgentRuntimeAdapter::connect(
            fixture_config("mcp-options", None),
            ClaudeAgentRuntimeConfig {
                mcp_servers: Some(serde_json::json!({
                    "vibe-team2": {
                        "type": "stdio",
                        "command": "node",
                        "args": ["/tmp/team-bridge.js"],
                        "env": {
                            "VIBE_TEAM_SOCKET": "/tmp/team.sock",
                            "VIBE_TEAM_TOKEN": "secret",
                            "VIBE_TEAM_ID": "team-1",
                            "VIBE_AGENT_ID": "leader-team-1",
                            "VIBE_TEAM_ROLE": "leader"
                        }
                    }
                })),
                ..Default::default()
            },
            Arc::new(move |event| {
                if let ClaudeAdapterEvent::Payload(payload) = event {
                    sink_manager.record_event("mcp-options", payload);
                }
            }),
        )
        .expect("fixture sidecar connects"),
    );
    assert!(manager
        .register_endpoint("mcp-options".into(), adapter)
        .result
        .is_ok());
    wait_until(|| {
        manager.event_snapshot().iter().any(|event| {
            matches!(
                &event.payload,
                RuntimeEventPayload::Diagnostic { message }
                    if message == "mcp:stdio:node:true:true:team-1:leader-team-1:leader"
            )
        })
    });
}

#[test]
fn team_delivery_steers_an_active_claude_turn() {
    let manager = Arc::new(RuntimeManager::new());
    let sink_manager = manager.clone();
    let adapter = Arc::new(
        ClaudeAgentRuntimeAdapter::connect(
            fixture_config("team-delivery", None),
            ClaudeAgentRuntimeConfig::default(),
            Arc::new(move |event| {
                if let ClaudeAdapterEvent::Payload(payload) = event {
                    sink_manager.record_event("team-delivery", payload);
                }
            }),
        )
        .expect("fixture sidecar connects"),
    );
    assert!(manager
        .register_endpoint("team-delivery".into(), adapter)
        .result
        .is_ok());
    assert!(manager
        .deliver_team_message_blocking(
            "team-delivery",
            RuntimeDeliveryRequest {
                from_role: "leader".into(),
                data: "calculate 1+1".into(),
            },
        )
        .result
        .is_ok());
    wait_until(|| {
        manager.event_snapshot().iter().any(|event| {
            matches!(
                &event.payload,
                RuntimeEventPayload::Diagnostic { message }
                    if message == "delivery:steer:[Team ← leader] calculate 1+1"
            )
        })
    });
}

#[test]
fn credentials_are_redacted_from_renderer_and_buffer_payloads() {
    const SECRET: &str = "sk-ant-fixture-secret-value";
    let manager = Arc::new(RuntimeManager::new());
    let adapter = managed_adapter(manager.clone(), "secret", "secret", Some(SECRET));
    assert!(manager
        .register_endpoint("secret".into(), adapter)
        .result
        .is_ok());
    assert!(manager
        .spawn_turn(
            "secret",
            RuntimeTurnSpawnRequest {
                input: "echo".into(),
                submit: true,
                model: None,
                effort: None,
                permission: None,
            },
        )
        .result
        .is_ok());
    wait_until(|| {
        manager
            .event_snapshot()
            .iter()
            .any(|event| matches!(event.payload, RuntimeEventPayload::MessageComplete { .. }))
    });
    let serialized = serde_json::to_string(&manager.event_snapshot()).unwrap();
    assert!(!serialized.contains(SECRET));
    assert!(serialized.contains("<redacted>"));
}

#[test]
fn selected_model_effort_and_permission_reach_claude_sidecar() {
    let manager = Arc::new(RuntimeManager::new());
    let adapter = managed_adapter(manager.clone(), "options", "options", None);
    assert!(manager
        .register_endpoint("options".into(), adapter)
        .result
        .is_ok());
    assert!(manager
        .spawn_turn(
            "options",
            RuntimeTurnSpawnRequest {
                input: "verify options".into(),
                submit: true,
                model: Some("fable".into()),
                effort: Some("max".into()),
                permission: Some("full".into()),
            },
        )
        .result
        .is_ok());
    wait_until(|| {
        manager.event_snapshot().iter().any(|event| {
            matches!(
                &event.payload,
                RuntimeEventPayload::Diagnostic { message }
                    if message == "options:fable:max:full"
            )
        })
    });
    assert!(manager.event_snapshot().iter().any(|event| matches!(
        event.payload,
        RuntimeEventPayload::TurnComplete { interrupted: false }
    )));
}

#[test]
fn approval_decisions_are_validated_before_sidecar_delivery() {
    let manager = Arc::new(RuntimeManager::new());
    let adapter = managed_adapter(manager, "approval", "happy", None);
    let error = adapter
        .respond_approval(&RuntimeApprovalResponseRequest {
            request_id: "approval-1".into(),
            decision: "always".into(),
        })
        .unwrap_err();
    assert_eq!(error.code, "runtime_approval_decision_invalid");
}
