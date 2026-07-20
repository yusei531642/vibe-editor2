use super::{CodexAdapterEvent, CodexRuntimeAdapter};
use crate::agent_runtime::{
    RuntimeEventPayload, RuntimeLifecycleState, RuntimeManager, RuntimeTurnSpawnRequest,
};
use crate::team_hub::app_server::error::AppServerError;
use crate::team_hub::app_server::wire::WsStream;
use serde_json::{json, Value};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};

#[derive(Clone, Copy)]
enum FixtureMode {
    Scripted,
    CrashAfterStart,
    VersionMismatch,
}

struct Fixture {
    socket_path: String,
    client_stream: Option<UnixStream>,
    transcript: mpsc::Receiver<String>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn spawn_fixture(mode: FixtureMode) -> Fixture {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = std::env::temp_dir().join(format!("vibe-runtime-{}.sock", &id[..8]));
    let _ = std::fs::remove_file(&path);
    let socket_path = path.to_string_lossy().into_owned();
    let (tx, transcript) = mpsc::channel();
    let client_stream = match UnixListener::bind(&path) {
        Ok(listener) => {
            tokio::spawn(async move {
                if let Ok((stream, _)) = listener.accept().await {
                    let _ = serve(stream, mode, tx).await;
                }
            });
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            let (client, server) = UnixStream::pair().expect("create fixture stream pair");
            tokio::spawn(async move {
                let _ = serve(server, mode, tx).await;
            });
            Some(client)
        }
        Err(error) => panic!("bind fixture socket: {error}"),
    };
    Fixture {
        socket_path,
        client_stream,
        transcript,
    }
}

async fn serve(
    stream: UnixStream,
    mode: FixtureMode,
    transcript: mpsc::Sender<String>,
) -> Result<(), AppServerError> {
    let mut ws = WsStream::new(stream, false);
    ws.server_handshake().await?;
    while let Some(text) = ws.read_text().await? {
        let message: Value = serde_json::from_str(&text)
            .map_err(|error| AppServerError::Protocol(error.to_string()))?;
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();
        if let Some(method) = method {
            let _ = transcript.send(method.to_string());
        }
        match (method, id) {
            (Some("initialize"), Some(id)) => {
                let version = if matches!(mode, FixtureMode::VersionMismatch) {
                    99
                } else {
                    1
                };
                send(
                    &mut ws,
                    json!({ "id": id, "result": { "protocolVersion": version } }),
                )
                .await?;
                if matches!(mode, FixtureMode::VersionMismatch) {
                    return Ok(());
                }
            }
            (Some("thread/start"), Some(id)) => {
                send(
                    &mut ws,
                    json!({ "id": id, "result": { "thread": { "id": "thread-new" } } }),
                )
                .await?;
                if matches!(mode, FixtureMode::CrashAfterStart) {
                    return Ok(());
                }
            }
            (Some("thread/resume"), Some(id)) => {
                let thread_id = message["params"]["threadId"].clone();
                send(
                    &mut ws,
                    json!({ "id": id, "result": { "thread": { "id": thread_id } } }),
                )
                .await?;
            }
            (Some("thread/fork"), Some(id)) => {
                send(
                    &mut ws,
                    json!({ "id": id, "result": { "thread": { "id": "thread-forked" } } }),
                )
                .await?;
            }
            (Some("turn/start"), Some(id)) => {
                let _ = transcript.send(format!(
                    "turn-options:{}:{}:{}:{}",
                    message["params"]["model"].as_str().unwrap_or_default(),
                    message["params"]["effort"].as_str().unwrap_or_default(),
                    message["params"]["sandboxPolicy"]["type"]
                        .as_str()
                        .unwrap_or_default(),
                    message["params"]["approvalPolicy"]
                        .as_str()
                        .unwrap_or_default()
                ));
                send(
                    &mut ws,
                    json!({ "id": id, "result": { "turn": { "id": "turn-active" } } }),
                )
                .await?;
                scripted_notifications(&mut ws).await?;
            }
            (Some("turn/steer"), Some(id)) => {
                let expected = message["params"]["expectedTurnId"]
                    .as_str()
                    .unwrap_or_default();
                let _ = transcript.send(format!("steer:{expected}"));
                send(&mut ws, json!({ "id": id, "result": {} })).await?;
            }
            (Some("turn/interrupt"), Some(id)) => {
                send(&mut ws, json!({ "id": id, "result": {} })).await?;
                send(
                    &mut ws,
                    json!({ "method": "turn/interrupted", "params": { "threadId": "thread-new", "turnId": "turn-active" } }),
                )
                .await?;
            }
            (None, Some(id)) if id == json!(900) => {
                let decision = message["result"]["decision"].as_str().unwrap_or_default();
                let _ = transcript.send(format!("approval:{decision}"));
            }
            (None, Some(id)) if id == json!(902) => {
                // 他 thread 宛 approval への自動応答 (decline を期待)。
                let decision = message["result"]["decision"].as_str().unwrap_or_default();
                let _ = transcript.send(format!("cross-thread-approval:{decision}"));
            }
            (None, Some(id)) if id == json!(901) => {
                let code = message["error"]["code"].as_i64().unwrap_or_default();
                let _ = transcript.send(format!("unknown:{code}"));
            }
            _ => {}
        }
    }
    Ok(())
}

async fn scripted_notifications(ws: &mut WsStream<UnixStream>) -> Result<(), AppServerError> {
    for notification in [
        json!({ "method": "item/agentMessage/delta", "params": { "threadId": "thread-other", "delta": "leak" } }),
        json!({ "method": "item/agentMessage/delta", "params": { "threadId": "thread-new", "delta": "hello" } }),
        json!({ "method": "item/completed", "params": { "item": { "type": "agentMessage", "text": "hello" } } }),
        json!({ "method": "item/started", "params": { "item": { "type": "commandExecution", "id": "call-1", "command": "git status" } } }),
        json!({ "method": "turn/diff/updated", "params": { "diff": "@@ -1 +1 @@" } }),
        json!({ "method": "thread/tokenUsage/updated", "params": { "usage": { "inputTokens": 4, "cachedInputTokens": 2, "outputTokens": 3 } } }),
        json!({ "id": 900, "method": "item/commandExecution/requestApproval", "params": { "reason": "test", "command": ["git", "status"], "cwd": "/tmp/project" } }),
        json!({ "id": 902, "method": "item/commandExecution/requestApproval", "params": { "threadId": "thread-other", "reason": "cross-thread", "command": ["rm", "-rf"], "cwd": "/tmp/other" } }),
        json!({ "id": 901, "method": "unknown/request", "params": { "threadId": "thread-new" } }),
    ] {
        send(ws, notification).await?;
    }
    Ok(())
}

async fn send(ws: &mut WsStream<UnixStream>, value: Value) -> Result<(), AppServerError> {
    ws.write_text(serde_json::to_string(&value).unwrap().as_bytes())
        .await
}

fn adapter_and_manager(
    fixture: &mut Fixture,
    endpoint_id: &str,
) -> (Arc<CodexRuntimeAdapter>, Arc<RuntimeManager>) {
    let manager = Arc::new(RuntimeManager::new());
    let manager_for_sink = manager.clone();
    let endpoint = endpoint_id.to_string();
    let sink = Arc::new(move |event| match event {
        CodexAdapterEvent::Payload(payload) => {
            manager_for_sink.record_event(&endpoint, payload);
        }
        CodexAdapterEvent::Failure(error) => {
            manager_for_sink.fail_endpoint(&endpoint, error);
        }
    });
    let cwd = Some("/tmp/project".to_string());
    let adapter = Arc::new(
        match fixture.client_stream.take() {
            Some(stream) => {
                CodexRuntimeAdapter::connect_stream(stream, cwd, None, None, false, sink)
            }
            None => CodexRuntimeAdapter::connect(
                fixture.socket_path.clone(),
                cwd,
                None,
                None,
                false,
                sink,
            ),
        }
        .expect("connect adapter"),
    );
    (adapter, manager)
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    for _ in 0..100 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition was not reached before timeout");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starts_resumes_and_forks_without_a_terminal() {
    let mut fixture = spawn_fixture(FixtureMode::Scripted);
    let (adapter, manager) = adapter_and_manager(&mut fixture, "native-start");
    let operation = manager.register_endpoint("native-start".into(), adapter.clone());
    assert!(operation.result.is_ok());
    assert_eq!(adapter.thread_id().as_deref(), Some("thread-new"));
    assert!(manager.dispose("native-start").result.is_ok());

    let mut fixture = spawn_fixture(FixtureMode::Scripted);
    let (adapter, manager) = adapter_and_manager(&mut fixture, "native-resume");
    let operation = manager.register_resumed_endpoint(
        "native-resume".into(),
        adapter.clone(),
        "thread-existing".into(),
    );
    assert!(operation.result.is_ok());
    assert_eq!(adapter.thread_id().as_deref(), Some("thread-existing"));
    assert!(manager.dispose("native-resume").result.is_ok());

    let mut fixture = spawn_fixture(FixtureMode::Scripted);
    let (adapter, manager) = adapter_and_manager(&mut fixture, "native-fork");
    let operation = manager.register_forked_endpoint(
        "native-fork".into(),
        adapter.clone(),
        "thread-existing".into(),
    );
    assert!(operation.result.is_ok());
    assert_eq!(adapter.thread_id().as_deref(), Some("thread-forked"));
    assert!(manager.dispose("native-fork").result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn projects_turn_steer_and_approval_as_envelopes() {
    let mut fixture = spawn_fixture(FixtureMode::Scripted);
    let (adapter, manager) = adapter_and_manager(&mut fixture, "native-events");
    assert!(manager
        .register_endpoint("native-events".into(), adapter.clone())
        .result
        .is_ok());
    assert!(manager
        .spawn_turn(
            "native-events",
            RuntimeTurnSpawnRequest {
                input: "hello".into(),
                submit: true,
                model: Some("gpt-fixture".into()),
                effort: Some("high".into()),
                permission: Some("workspace".into()),
            },
        )
        .result
        .is_ok());
    wait_until(|| {
        manager
            .event_snapshot()
            .iter()
            .any(|event| matches!(event.payload, RuntimeEventPayload::ApprovalRequest { .. }))
    })
    .await;
    assert!(manager.steer("native-events", "more".into()).result.is_ok());
    let request_id = manager
        .event_snapshot()
        .iter()
        .find_map(|event| match &event.payload {
            RuntimeEventPayload::ApprovalRequest { request_id, .. } => Some(request_id.clone()),
            _ => None,
        })
        .expect("approval request id");
    // pending でない requestId は業務エラーであり、actor loop / 接続を殺さない。
    let unknown = manager.respond_approval("native-events", "ghost-id".into(), "accept".into());
    assert!(unknown.result.is_err());
    assert!(manager
        .respond_approval("native-events", request_id, "accept".into())
        .result
        .is_ok());

    let events = manager.event_snapshot();
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::MessageDelta { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::ToolUse { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Diff { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Usage { .. })));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        RuntimeEventPayload::MessageDelta { delta } if delta.contains("leak")
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        RuntimeEventPayload::Diagnostic { message } if message.contains("unbound thread")
    )));
    assert_eq!(adapter.thread_id().as_deref(), Some("thread-new"));
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    // transcript は fixture 側 task が socket を読んでから流れる。respond_approval の
    // return は client 側 write 完了までしか保証しないため、非同期に到着を待つ。
    let mut transcript: Vec<String> = Vec::new();
    // 他 thread (thread-other) の approval は wire 上で自動 decline され、
    // renderer には bound thread の approval 1 件しか届かない (PR #33 六次レビュー)。
    let approval_requests = manager
        .event_snapshot()
        .iter()
        .filter(|event| matches!(event.payload, RuntimeEventPayload::ApprovalRequest { .. }))
        .count();
    assert_eq!(approval_requests, 1);
    for expected in [
        "turn-options:gpt-fixture:high:workspaceWrite:on-request",
        "steer:turn-active",
        "approval:accept",
        "cross-thread-approval:decline",
        "unknown:-32601",
    ] {
        wait_until(|| {
            transcript.extend(fixture.transcript.try_iter());
            transcript.iter().any(|line| line == expected)
        })
        .await;
    }

    let stopped = manager.stop("native-events");
    assert!(stopped.result.is_ok());
    assert!(!stopped.events.iter().any(|event| matches!(
        event.payload,
        RuntimeEventPayload::Lifecycle {
            state: RuntimeLifecycleState::Exited,
            ..
        }
    )));
    assert!(manager.registry().resolve("native-events").is_some());
    wait_until(|| {
        manager
            .interrupt("native-events")
            .result
            .is_err_and(|error| error.code == "runtime_turn_not_active")
    })
    .await;
    assert!(manager.registry().resolve("native-events").is_some());
    assert!(manager.dispose("native-events").result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_and_protocol_mismatch_are_explicit_failures() {
    let mut fixture = spawn_fixture(FixtureMode::CrashAfterStart);
    let (adapter, manager) = adapter_and_manager(&mut fixture, "native-crash");
    assert!(manager
        .register_endpoint("native-crash".into(), adapter)
        .result
        .is_ok());
    wait_until(|| manager.registry().is_empty()).await;
    let events = manager.event_snapshot();
    assert!(events.iter().any(|event| matches!(
        event.payload,
        RuntimeEventPayload::Lifecycle {
            state: RuntimeLifecycleState::Failed,
            ..
        }
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Error { .. })));

    let mut fixture = spawn_fixture(FixtureMode::VersionMismatch);
    let sink = Arc::new(|_: CodexAdapterEvent| {});
    let connection = match fixture.client_stream.take() {
        Some(stream) => CodexRuntimeAdapter::connect_stream(stream, None, None, None, false, sink),
        None => {
            CodexRuntimeAdapter::connect(fixture.socket_path.clone(), None, None, None, false, sink)
        }
    };
    let error = match connection {
        Ok(_) => panic!("version mismatch must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, "runtime_app_server_version_mismatch");
}
