//! Blocking request client + background JSONL event reader for the Node sidecar.

use super::sidecar_protocol::{
    ConvertedSidecarEvent, Redactor, SidecarIncoming, SidecarRequest, SIDECAR_PROTOCOL,
    SIDECAR_PROTOCOL_VERSION,
};
use crate::agent_runtime::{RuntimeAdapterError, RuntimeEventPayload};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

pub enum ClientEvent {
    Session(String),
    Payload(RuntimeEventPayload),
    Failure(RuntimeAdapterError),
}

pub type ClientEventSink = Arc<dyn Fn(ClientEvent) + Send + Sync>;

pub struct SidecarLaunchConfig {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub secret_values: Vec<String>,
    pub response_timeout: Duration,
}

impl SidecarLaunchConfig {
    pub fn production(claude_command: String) -> Result<Self, RuntimeAdapterError> {
        let program = crate::agent_runtime::resolve_node_executable().ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_claude_node_unavailable",
                "Node.js is unavailable for the Claude Agent sidecar",
                false,
            )
        })?;
        let entrypoint = crate::agent_runtime::resolve_sidecar_entrypoint().ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_claude_sidecar_unavailable",
                "Claude Agent sidecar entrypoint is unavailable",
                false,
            )
        })?;
        let mut environment = safe_parent_environment();
        environment.push(("VIBE_CLAUDE_COMMAND".to_string(), claude_command));
        let mut secret_values = Vec::new();
        for name in credential_environment_names() {
            if let Ok(value) = std::env::var(name) {
                if !value.is_empty() {
                    secret_values.push(value.clone());
                    environment.push((name.to_string(), value));
                }
            }
        }
        Ok(Self {
            program,
            args: vec![entrypoint.to_string_lossy().into_owned()],
            environment,
            secret_values,
            response_timeout: DEFAULT_RESPONSE_TIMEOUT,
        })
    }
}

type PendingResponse = Result<Value, RuntimeAdapterError>;
type PendingMap = Arc<Mutex<HashMap<String, mpsc::Sender<PendingResponse>>>>;

pub struct SidecarClient {
    stdin: Mutex<Option<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    pending: PendingMap,
    next_id: AtomicU64,
    expected_exit: Arc<AtomicBool>,
    response_timeout: Duration,
    redactor: Redactor,
}

impl SidecarClient {
    pub fn spawn(
        config: SidecarLaunchConfig,
        sink: ClientEventSink,
    ) -> Result<Self, RuntimeAdapterError> {
        let redactor = Redactor::new(config.secret_values);
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .env_clear()
            .envs(config.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            RuntimeAdapterError::new(
                "runtime_claude_sidecar_spawn_failed",
                format!("failed to spawn Claude Agent sidecar: {error}"),
                false,
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| pipe_error("stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| pipe_error("stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| pipe_error("stderr"))?;
        let child = Arc::new(Mutex::new(child));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let expected_exit = Arc::new(AtomicBool::new(false));
        let failure_sent = Arc::new(AtomicBool::new(false));
        let (hello_tx, hello_rx) = mpsc::channel();

        spawn_stdout_reader(ReaderContext {
            stdout: Some(stdout),
            child: child.clone(),
            pending: pending.clone(),
            expected_exit: expected_exit.clone(),
            failure_sent: failure_sent.clone(),
            redactor: redactor.clone(),
            sink: sink.clone(),
            hello: Some(hello_tx),
        });
        spawn_stderr_reader(stderr, redactor.clone());

        match hello_rx.recv_timeout(HELLO_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stdin: Mutex::new(Some(stdin)),
                child,
                pending,
                next_id: AtomicU64::new(1),
                expected_exit,
                response_timeout: config.response_timeout,
                redactor,
            }),
            Ok(Err(error)) => {
                terminate_child(&child);
                Err(error)
            }
            Err(_) => {
                terminate_child(&child);
                Err(RuntimeAdapterError::new(
                    "runtime_claude_sidecar_handshake_timeout",
                    "Claude Agent sidecar handshake timed out",
                    false,
                ))
            }
        }
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, RuntimeAdapterError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let request = SidecarRequest {
            message_type: "request",
            version: SIDECAR_PROTOCOL_VERSION,
            id: &id,
            method,
            params,
        };
        let mut line = serde_json::to_vec(&request).map_err(|error| {
            RuntimeAdapterError::new("runtime_claude_protocol", error.to_string(), false)
        })?;
        line.push(b'\n');
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id.clone(), tx);
        let write_result = {
            let mut guard = lock(&self.stdin);
            match guard.as_mut() {
                Some(stdin) => stdin.write_all(&line).and_then(|()| stdin.flush()),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "sidecar stdin is closed",
                )),
            }
        };
        if let Err(error) = write_result {
            lock(&self.pending).remove(&id);
            return Err(RuntimeAdapterError::new(
                "runtime_claude_sidecar_disconnected",
                self.redactor.redact(&error.to_string()),
                false,
            ));
        }
        match rx.recv_timeout(self.response_timeout) {
            Ok(response) => response,
            Err(_) => {
                lock(&self.pending).remove(&id);
                let error = RuntimeAdapterError::new(
                    "runtime_claude_sidecar_response_timeout",
                    format!("Claude Agent sidecar did not respond to {method}"),
                    false,
                );
                Err(error)
            }
        }
    }

    pub fn dispose(&self) {
        self.expected_exit.store(true, Ordering::Release);
        let _ = self.request("dispose", serde_json::json!({}));
        *lock(&self.stdin) = None;
        terminate_child(&self.child);
    }
}

impl Drop for SidecarClient {
    fn drop(&mut self) {
        self.expected_exit.store(true, Ordering::Release);
        terminate_child(&self.child);
    }
}

struct ReaderContext {
    stdout: Option<std::process::ChildStdout>,
    child: Arc<Mutex<Child>>,
    pending: PendingMap,
    expected_exit: Arc<AtomicBool>,
    failure_sent: Arc<AtomicBool>,
    redactor: Redactor,
    sink: ClientEventSink,
    hello: Option<mpsc::Sender<Result<(), RuntimeAdapterError>>>,
}

fn spawn_stdout_reader(mut context: ReaderContext) {
    thread::spawn(move || {
        let mut hello = context.hello.take();
        let stdout = context.stdout.take().expect("sidecar stdout is present");
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    notify_failure(&context, &mut hello, protocol_error(error.to_string()));
                    return;
                }
            };
            let incoming: SidecarIncoming = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => {
                    notify_failure(
                        &context,
                        &mut hello,
                        protocol_error("sidecar emitted invalid JSON"),
                    );
                    return;
                }
            };
            if incoming.version() != SIDECAR_PROTOCOL_VERSION {
                notify_failure(
                    &context,
                    &mut hello,
                    protocol_error("sidecar protocol version mismatch"),
                );
                return;
            }
            match incoming {
                SidecarIncoming::Hello {
                    protocol,
                    capabilities,
                    ..
                } => {
                    if protocol != SIDECAR_PROTOCOL {
                        notify_failure(
                            &context,
                            &mut hello,
                            protocol_error("sidecar protocol name mismatch"),
                        );
                        return;
                    }
                    let _ = capabilities;
                    if let Some(sender) = hello.take() {
                        let _ = sender.send(Ok(()));
                    }
                }
                SidecarIncoming::Response {
                    id,
                    ok,
                    result,
                    error,
                    ..
                } => {
                    let Some(sender) = lock(&context.pending).remove(&id) else {
                        continue;
                    };
                    let response = if ok {
                        Ok(result)
                    } else {
                        Err(error.map_or_else(
                            || protocol_error("sidecar error response omitted error"),
                            |error| error.into_runtime_error(&context.redactor),
                        ))
                    };
                    let _ = sender.send(response);
                }
                SidecarIncoming::Event { event, .. } => match event.convert(&context.redactor) {
                    ConvertedSidecarEvent::Session(session_id) => {
                        (context.sink)(ClientEvent::Session(session_id));
                    }
                    ConvertedSidecarEvent::Payload(payload) => {
                        (context.sink)(ClientEvent::Payload(payload));
                    }
                    ConvertedSidecarEvent::Failure(error) => {
                        notify_failure(&context, &mut hello, error);
                        return;
                    }
                },
            }
        }
        if context.expected_exit.load(Ordering::Acquire) {
            return;
        }
        let detail = lock(&context.child)
            .try_wait()
            .ok()
            .flatten()
            .map_or_else(|| "unexpected EOF".to_string(), |status| status.to_string());
        notify_failure(
            &context,
            &mut hello,
            RuntimeAdapterError::new(
                "runtime_claude_sidecar_crashed",
                format!("Claude Agent sidecar exited: {detail}"),
                false,
            ),
        );
    });
}

fn notify_failure(
    context: &ReaderContext,
    hello: &mut Option<mpsc::Sender<Result<(), RuntimeAdapterError>>>,
    error: RuntimeAdapterError,
) {
    let was_handshaking = hello.is_some();
    if let Some(sender) = hello.take() {
        let _ = sender.send(Err(error.clone()));
    }
    for (_, pending) in lock(&context.pending).drain() {
        let _ = pending.send(Err(error.clone()));
    }
    if !was_handshaking && !context.failure_sent.swap(true, Ordering::AcqRel) {
        (context.sink)(ClientEvent::Failure(error));
    }
}

fn spawn_stderr_reader(stderr: std::process::ChildStderr, redactor: Redactor) {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::warn!("[claude-agent-sidecar] {}", redactor.redact(&line));
        }
    });
}

fn safe_parent_environment() -> Vec<(String, String)> {
    const NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "LC_ALL",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
    ];
    NAMES
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| ((*name).to_string(), value))
        })
        .collect()
}

fn credential_environment_names() -> &'static [&'static str] {
    &[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ]
}

fn terminate_child(child: &Arc<Mutex<Child>>) {
    let mut child = lock(child);
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn pipe_error(name: &str) -> RuntimeAdapterError {
    RuntimeAdapterError::new(
        "runtime_claude_sidecar_pipe_failed",
        format!("Claude Agent sidecar {name} pipe is unavailable"),
        false,
    )
}

fn protocol_error(message: impl Into<String>) -> RuntimeAdapterError {
    RuntimeAdapterError::new("runtime_claude_sidecar_protocol", message, false)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
