//! 長寿命 WebSocket-over-unix JSON-RPC client actor。

use crate::agent_runtime::RuntimeAdapterError;
use crate::team_hub::app_server::error::AppServerError;
use crate::team_hub::app_server::wire::WsStream;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const SUPPORTED_PROTOCOL_VERSION: &str = "1";

#[derive(Clone, Debug)]
pub enum ClientEvent {
    Notification {
        method: String,
        params: Value,
    },
    ServerRequest {
        request_id: String,
        method: String,
        params: Value,
    },
    Failure(RuntimeAdapterError),
}

pub type ClientEventSink = Arc<dyn Fn(ClientEvent) + Send + Sync>;

enum ClientCommand {
    Request {
        method: String,
        params: Value,
        deadline: Instant,
        response: std_mpsc::Sender<Result<Value, RuntimeAdapterError>>,
    },
    RespondApproval {
        request_id: String,
        decision: String,
        response: std_mpsc::Sender<Result<Value, RuntimeAdapterError>>,
    },
    Shutdown,
}

pub struct ClientHandle {
    commands: mpsc::UnboundedSender<ClientCommand>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    actor_thread_id: std::thread::ThreadId,
}

impl ClientHandle {
    pub fn connect(
        socket_path: String,
        sink: ClientEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        Self::connect_source(ClientSource::Path(socket_path), sink)
    }

    #[cfg(test)]
    pub fn connect_stream(
        stream: UnixStream,
        sink: ClientEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        Self::connect_source(ClientSource::Stream(stream), sink)
    }

    fn connect_source(
        source: ClientSource,
        sink: ClientEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("codex-runtime-app-server".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build codex app-server runtime");
                runtime.block_on(run(source, receiver, sink, ready_tx));
            })
            .map_err(|error| fatal("runtime_app_server_thread", error.to_string()))?;
        let actor_thread_id = thread.thread().id();
        match ready_rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                commands,
                thread: Mutex::new(Some(thread)),
                actor_thread_id,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(fatal(
                "runtime_app_server_timeout",
                "app-server initialization timed out",
            )),
        }
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, RuntimeAdapterError> {
        let (tx, rx) = std_mpsc::channel();
        self.commands
            .send(ClientCommand::Request {
                method: method.to_string(),
                params,
                deadline: Instant::now() + REQUEST_TIMEOUT,
                response: tx,
            })
            .map_err(|_| disconnected())?;
        rx.recv_timeout(REQUEST_TIMEOUT).map_err(|_| {
            RuntimeAdapterError::new(
                "runtime_app_server_timeout",
                format!("app-server request '{method}' timed out"),
                true,
            )
        })?
    }

    pub fn respond_approval(
        &self,
        request_id: &str,
        decision: &str,
    ) -> Result<(), RuntimeAdapterError> {
        let (tx, rx) = std_mpsc::channel();
        self.commands
            .send(ClientCommand::RespondApproval {
                request_id: request_id.to_string(),
                decision: decision.to_string(),
                response: tx,
            })
            .map_err(|_| disconnected())?;
        rx.recv_timeout(REQUEST_TIMEOUT).map_err(|_| {
            RuntimeAdapterError::new(
                "runtime_app_server_timeout",
                "approval response timed out",
                true,
            )
        })??;
        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.commands.send(ClientCommand::Shutdown);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if std::thread::current().id() != self.actor_thread_id {
            if let Some(thread) = thread {
                let _ = thread.join();
            }
        }
    }
}

async fn run(
    source: ClientSource,
    mut commands: mpsc::UnboundedReceiver<ClientCommand>,
    sink: ClientEventSink,
    ready: std_mpsc::SyncSender<Result<(), RuntimeAdapterError>>,
) {
    let mut ws = match tokio::time::timeout(REQUEST_TIMEOUT, connect_and_initialize(source)).await {
        Ok(Ok(ws)) => {
            let _ = ready.send(Ok(()));
            ws
        }
        Ok(Err(error)) => {
            let _ = ready.send(Err(error));
            return;
        }
        Err(_) => {
            let _ = ready.send(Err(fatal(
                "runtime_app_server_timeout",
                "app-server initialization timed out",
            )));
            return;
        }
    };
    let mut next_id = 2_i64;
    let mut pending = HashMap::new();
    let mut approvals: HashMap<String, Value> = HashMap::new();
    let mut pending_cleanup = tokio::time::interval(Duration::from_secs(1));

    let failure = loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ClientCommand::Shutdown) | None => return,
                Some(command) => if let Err(error) = handle_command(
                    command, &mut ws, &mut next_id, &mut pending, &mut approvals,
                ).await { break error; },
            },
            incoming = ws.read_text() => match incoming {
                Ok(Some(text)) => if let Err(error) = handle_incoming(
                    &text, &mut ws, &mut pending, &mut approvals, &sink,
                ).await { break error; },
                Ok(None) => break disconnected(),
                Err(error) => break map_wire_error(error),
            },
            _ = pending_cleanup.tick() => {
                expire_pending(&mut pending);
                // read 経路が積んだ PONG を返す (wire.rs の cancel-safe 化に伴い
                // PONG 送出は write 側の責務)。失敗は接続断として扱う。
                if let Err(error) = ws.flush_pending_pongs().await {
                    break map_wire_error(error);
                }
            },
        }
    };
    for (_, entry) in pending.drain() {
        let _ = entry.response.send(Err(failure.clone()));
    }
    sink(ClientEvent::Failure(failure));
}

struct PendingEntry {
    deadline: Instant,
    response: std_mpsc::Sender<Result<Value, RuntimeAdapterError>>,
}

type Pending = HashMap<String, PendingEntry>;

async fn handle_command(
    command: ClientCommand,
    ws: &mut WsStream<UnixStream>,
    next_id: &mut i64,
    pending: &mut Pending,
    approvals: &mut HashMap<String, Value>,
) -> Result<(), RuntimeAdapterError> {
    match command {
        ClientCommand::Request {
            method,
            params,
            deadline,
            response,
        } => {
            let id = *next_id;
            *next_id = next_id.saturating_add(1);
            if let Err(error) =
                write_json(ws, &json!({ "id": id, "method": method, "params": params })).await
            {
                let _ = response.send(Err(error.clone()));
                return Err(error);
            }
            pending.insert(format!("i:{id}"), PendingEntry { deadline, response });
        }
        ClientCommand::RespondApproval {
            request_id,
            decision,
            response,
        } => {
            // pending でない requestId (応答済み / 未知) は業務エラー: caller にだけ返し、
            // 接続と actor loop は維持する。loop を殺すのは実 I/O 失敗のみ。
            let Some(id) = approvals.remove(&request_id) else {
                let _ = response.send(Err(RuntimeAdapterError::new(
                    "runtime_approval_not_found",
                    format!("approval request '{request_id}' is not pending"),
                    true,
                )));
                return Ok(());
            };
            match write_json(ws, &json!({ "id": id, "result": { "decision": decision } })).await {
                Ok(()) => {
                    let _ = response.send(Ok(Value::Null));
                }
                Err(error) => {
                    let _ = response.send(Err(error.clone()));
                    return Err(error);
                }
            }
        }
        ClientCommand::Shutdown => {}
    }
    Ok(())
}

async fn handle_incoming(
    text: &str,
    ws: &mut WsStream<UnixStream>,
    pending: &mut Pending,
    approvals: &mut HashMap<String, Value>,
    sink: &ClientEventSink,
) -> Result<(), RuntimeAdapterError> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        fatal(
            "runtime_app_server_protocol",
            format!("invalid json: {error}"),
        )
    })?;
    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id");
    if method.is_none() {
        if let Some(entry) = id.and_then(id_key).and_then(|key| pending.remove(&key)) {
            let result = rpc_result(&value);
            let _ = entry.response.send(result);
        }
        return Ok(());
    }

    let method = method.unwrap_or_default().to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    if let Some(id) = id.cloned() {
        let request_id = id_key(&id).ok_or_else(|| {
            fatal(
                "runtime_app_server_protocol",
                "server request id is invalid",
            )
        })?;
        if is_supported_approval_method(&method) {
            approvals.insert(request_id.clone(), id);
            sink(ClientEvent::ServerRequest {
                request_id,
                method,
                params,
            });
        } else {
            write_json(
                ws,
                &json!({
                    "id": id,
                    "error": { "code": -32601, "message": format!("unsupported server request method '{method}'") }
                }),
            )
            .await?;
        }
    } else {
        sink(ClientEvent::Notification { method, params });
    }
    Ok(())
}

fn expire_pending(pending: &mut Pending) {
    let now = Instant::now();
    pending.retain(|_, entry| {
        if entry.deadline > now {
            return true;
        }
        let _ = entry.response.send(Err(RuntimeAdapterError::new(
            "runtime_app_server_timeout",
            "app-server request timed out",
            true,
        )));
        false
    });
}

fn is_supported_approval_method(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
    )
}

enum ClientSource {
    Path(String),
    #[cfg(test)]
    Stream(UnixStream),
}

async fn connect_and_initialize(
    source: ClientSource,
) -> Result<WsStream<UnixStream>, RuntimeAdapterError> {
    let stream = match source {
        ClientSource::Path(socket_path) => UnixStream::connect(socket_path)
            .await
            .map_err(|error| fatal("runtime_app_server_unreachable", error.to_string()))?,
        #[cfg(test)]
        ClientSource::Stream(stream) => stream,
    };
    let mut ws = WsStream::new(stream, true);
    ws.client_handshake().await.map_err(map_wire_error)?;
    let params = json!({
        "clientInfo": { "name": "vibe-editor2", "title": "vibe-editor 2", "version": env!("CARGO_PKG_VERSION") },
        "capabilities": { "experimentalApi": false, "requestAttestation": false }
    });
    write_json(
        &mut ws,
        &json!({ "id": 1, "method": "initialize", "params": params }),
    )
    .await?;
    let result = loop {
        let text = ws
            .read_text()
            .await
            .map_err(map_wire_error)?
            .ok_or_else(disconnected)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| fatal("runtime_app_server_protocol", error.to_string()))?;
        if value.get("id").and_then(Value::as_i64) == Some(1) && value.get("method").is_none() {
            break rpc_result(&value)?;
        }
    };
    if let Some(version) = result.get("protocolVersion") {
        let actual = version
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| version.to_string());
        if actual != SUPPORTED_PROTOCOL_VERSION {
            return Err(fatal(
                "runtime_app_server_version_mismatch",
                format!("unsupported app-server protocol version '{actual}', expected {SUPPORTED_PROTOCOL_VERSION}"),
            ));
        }
    }
    write_json(&mut ws, &json!({ "method": "initialized", "params": {} })).await?;
    Ok(ws)
}

async fn write_json(
    ws: &mut WsStream<UnixStream>,
    value: &Value,
) -> Result<(), RuntimeAdapterError> {
    let text = serde_json::to_string(value)
        .map_err(|error| fatal("runtime_app_server_protocol", error.to_string()))?;
    ws.write_text(text.as_bytes()).await.map_err(map_wire_error)
}

fn rpc_result(value: &Value) -> Result<Value, RuntimeAdapterError> {
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("app-server request failed");
        return Err(RuntimeAdapterError::new(
            "runtime_app_server_rpc_error",
            format!("app-server RPC error ({code}): {message}"),
            true,
        ));
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

/// JSON-RPC id を map キー化する。数値 `900` と文字列 `"900"` が同一キーへ潰れて
/// approval エントリを上書きし合わないよう、型タグ付きで区別する。
fn id_key(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|id| format!("s:{id}"))
        .or_else(|| value.as_i64().map(|id| format!("i:{id}")))
}

fn map_wire_error(error: AppServerError) -> RuntimeAdapterError {
    fatal("runtime_app_server_disconnected", error.to_string())
}

fn disconnected() -> RuntimeAdapterError {
    fatal(
        "runtime_app_server_disconnected",
        "app-server socket disconnected",
    )
}

fn fatal(code: &str, message: impl Into<String>) -> RuntimeAdapterError {
    RuntimeAdapterError::new(code, message, false)
}

#[cfg(test)]
mod tests {
    use super::is_supported_approval_method;

    #[test]
    fn approval_method_allowlist_is_exact() {
        assert!(is_supported_approval_method(
            "item/commandExecution/requestApproval"
        ));
        assert!(is_supported_approval_method(
            "item/fileChange/requestApproval"
        ));
        assert!(!is_supported_approval_method(
            "item/permissions/requestApproval"
        ));
        assert!(!is_supported_approval_method("unknownApproval"));
    }
}
