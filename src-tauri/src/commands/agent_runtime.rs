// agent_runtime.* command — Issue #21 diagnostics / Issue #22 endpoint operations.
#[cfg(unix)]
use crate::agent_runtime::codex::{CodexAdapterEvent, CodexAdapterEventSink, CodexRuntimeAdapter};
use crate::agent_runtime::{
    select_backend, BackendKind, PtyCompatAdapter, RuntimeCapability, RuntimeEventEnvelope,
    RuntimeOperation, RuntimeTurnSpawnRequest, SelectionReason, SystemCapabilityDetector,
};
use crate::commands::error::{CommandError, CommandResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

const MAX_RUNTIME_INPUT_BYTES: usize = 64 * 1024;
const MAX_APPROVAL_REQUEST_ID_BYTES: usize = 256;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeDiagnostics {
    pub requested_backend: BackendKind,
    pub selected_backend: BackendKind,
    pub reason: SelectionReason,
    pub capabilities: Vec<RuntimeCapability>,
}
/// Renderer の未保存 draft も診断できるよう backend を引数で受ける。
/// system detector は Unix で native adapter、Windows で PTY fallback を報告する。
/// ただし Unix でも `codex` が PATH にない場合は PTY fallback reason を報告する。
#[tauri::command]
pub async fn agent_runtime_diagnostics(backend: String) -> CommandResult<AgentRuntimeDiagnostics> {
    let requested_backend =
        BackendKind::try_from(backend.as_str()).map_err(CommandError::validation)?;
    let selection = select_backend(requested_backend, &SystemCapabilityDetector);
    Ok(AgentRuntimeDiagnostics {
        requested_backend: selection.requested_backend,
        selected_backend: selection.selected_backend,
        reason: selection.reason,
        capabilities: selection.capabilities,
    })
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPtyEndpointRequest {
    pub endpoint_id: String,
    pub session_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTurnRequest {
    pub endpoint_id: String,
    pub input: String,
    pub submit: bool,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CodexThreadAction {
    Start,
    Resume { thread_id: String },
    Fork { thread_id: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// DESIGN.md "Runtime boundary": renderer からは endpoint 意図のみを受け、
/// 実行バイナリ (codex command) は settings.json、control socket は Rust 側の
/// daemon 検出を正本とする。raw path / argv を renderer から受けない。
pub struct RegisterCodexEndpointRequest {
    pub endpoint_id: String,
    pub cwd: Option<String>,
    pub thread: CodexThreadAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSteerCommandRequest {
    pub endpoint_id: String,
    pub input: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeApprovalCommandRequest {
    pub endpoint_id: String,
    pub request_id: String,
    pub decision: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEndpointResult {
    pub endpoint_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeEndpointResult {
    pub endpoint_id: String,
    pub thread_id: String,
}

fn validate_endpoint_id(endpoint_id: &str) -> CommandResult<()> {
    crate::commands::validation::validate_id_segment("endpoint_id", endpoint_id).map(|_| ())
}

fn validate_runtime_input(input: &str) -> CommandResult<()> {
    crate::commands::validation::assert_max_size(input.len(), MAX_RUNTIME_INPUT_BYTES)
}

#[cfg(unix)]
fn validate_bounded_no_nul(name: &str, value: &str, max: usize) -> CommandResult<()> {
    if value.contains('\0') {
        return Err(CommandError::validation(format!(
            "{name} must not contain NUL"
        )));
    }
    crate::commands::validation::assert_max_size(value.len(), max)
}

fn validate_approval_request_id(request_id: &str) -> CommandResult<()> {
    if request_id.is_empty() || request_id.chars().any(char::is_control) {
        return Err(CommandError::validation(
            "request_id must be non-empty and contain no control characters",
        ));
    }
    crate::commands::validation::assert_max_size(request_id.len(), MAX_APPROVAL_REQUEST_ID_BYTES)
}

/// resume / fork 対象の thread id が「この process が開始/観測した thread」の集合に
/// 含まれることを要求する (project authority 迂回の防止、Issue #23 三次レビュー)。
fn authorize_known_thread(
    known: &std::sync::Mutex<std::collections::HashSet<String>>,
    thread_id: &str,
) -> CommandResult<()> {
    let guard = known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.contains(thread_id) {
        Ok(())
    } else {
        Err(CommandError::authz(format!(
            "thread '{thread_id}' was not started by this session; resume/fork is not authorized"
        )))
    }
}

// unix の登録経路と (両 OS でコンパイルされる) unit test の双方から使うため cfg を付けない。
fn record_known_thread(
    known: &std::sync::Mutex<std::collections::HashSet<String>>,
    thread_id: Option<String>,
) {
    if let Some(thread_id) = thread_id {
        known
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(thread_id);
    }
}

fn emit_events(app: &AppHandle, events: &[RuntimeEventEnvelope]) {
    for event in events {
        let event_name = format!("runtime:event:{}", event.endpoint_id);
        if let Err(error) = app.emit(&event_name, event) {
            tracing::warn!(
                endpoint_id = %event.endpoint_id,
                sequence = event.sequence,
                "[runtime] failed to emit normalized event: {error}"
            );
        }
    }
}

#[cfg(unix)]
fn codex_event_sink(
    app: AppHandle,
    manager: Arc<crate::agent_runtime::RuntimeManager>,
    endpoint_id: String,
) -> CodexAdapterEventSink {
    Arc::new(move |event| match event {
        CodexAdapterEvent::Payload(payload) => {
            let event = manager.record_event(&endpoint_id, payload);
            emit_events(&app, std::slice::from_ref(&event));
        }
        CodexAdapterEvent::Failure(error) => {
            let operation = manager.fail_endpoint(&endpoint_id, error);
            emit_events(&app, &operation.events);
        }
    })
}

#[cfg(unix)]
fn finish_codex_failure(
    app: &AppHandle,
    manager: &crate::agent_runtime::RuntimeManager,
    endpoint_id: &str,
    error: crate::agent_runtime::RuntimeAdapterError,
) -> CommandError {
    let operation = manager.fail_endpoint(endpoint_id, error.clone());
    emit_events(app, &operation.events);
    CommandError::coded(error.code, error.message)
}

fn finish_operation(
    app: &AppHandle,
    endpoint_id: String,
    operation: RuntimeOperation,
) -> CommandResult<RuntimeEndpointResult> {
    emit_events(app, &operation.events);
    operation
        .result
        .map_err(|error| CommandError::coded(error.code, error.message))?;
    Ok(RuntimeEndpointResult { endpoint_id })
}

async fn run_blocking<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError::coded("runtime_blocking_task_failed", error.to_string()))
}

async fn finish_blocking_operation<F>(
    app: &AppHandle,
    endpoint_id: String,
    operation: F,
) -> CommandResult<RuntimeEndpointResult>
where
    F: FnOnce() -> RuntimeOperation + Send + 'static,
{
    finish_operation(app, endpoint_id, run_blocking(operation).await?)
}

#[tauri::command]
pub async fn agent_runtime_register_pty_endpoint(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RegisterPtyEndpointRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    crate::commands::validation::validate_id_segment("session_id", &request.session_id)?;
    let adapter = Arc::new(PtyCompatAdapter::new(
        state.pty_registry.clone(),
        request.session_id,
    ));
    let manager = state.runtime_manager.clone();
    let endpoint_id = request.endpoint_id;
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.register_endpoint(operation_endpoint, adapter)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_register_codex_endpoint(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RegisterCodexEndpointRequest,
) -> CommandResult<CodexRuntimeEndpointResult> {
    #[cfg(not(unix))]
    {
        let _ = (app, state, request);
        return Err(CommandError::coded(
            "runtime_native_unsupported",
            "Codex app-server runtime is only available on Unix",
        ));
    }
    #[cfg(unix)]
    register_codex_endpoint(&app, &state, request).await
}

#[tauri::command]
pub async fn agent_runtime_reconnect_codex(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RegisterCodexEndpointRequest,
) -> CommandResult<CodexRuntimeEndpointResult> {
    #[cfg(not(unix))]
    {
        let _ = (app, state, request);
        return Err(CommandError::coded(
            "runtime_native_unsupported",
            "Codex app-server runtime is only available on Unix",
        ));
    }
    #[cfg(unix)]
    {
        validate_endpoint_id(&request.endpoint_id)?;
        if state
            .runtime_manager
            .registry()
            .resolve(&request.endpoint_id)
            .is_some()
        {
            let manager = state.runtime_manager.clone();
            let endpoint_id = request.endpoint_id.clone();
            let operation = run_blocking(move || manager.dispose(&endpoint_id)).await?;
            emit_events(&app, &operation.events);
        }
        register_codex_endpoint(&app, &state, request).await
    }
}

#[cfg(unix)]
async fn register_codex_endpoint(
    app: &AppHandle,
    state: &State<'_, AppState>,
    request: RegisterCodexEndpointRequest,
) -> CommandResult<CodexRuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    // cwd は active project root / native picker 由来 grant への authority 照合を必須とする
    // (renderer 指定の任意パスで thread を開かせない)。省略時は authority 照合済みの
    // active project root を使う。
    let cwd = match request.cwd.as_deref() {
        Some(given) => {
            validate_bounded_no_nul("cwd", given, 16 * 1024)?;
            let authorized = crate::commands::authz::assert_readable_project_root(
                &state.project_root,
                &state.project_root_identity,
                given,
            )
            .await?;
            Some(authorized.as_str().to_string())
        }
        None => state.project_root.load().as_deref().map(|p| p.to_string()),
    };
    match &request.thread {
        CodexThreadAction::Resume { thread_id } | CodexThreadAction::Fork { thread_id } => {
            crate::commands::validation::validate_id_segment("thread_id", thread_id)?;
            // 認可: この process が自ら開始/観測した thread のみ resume/fork できる。
            // 任意 threadId の指定で authority 外プロジェクトの thread を開かせない。
            authorize_known_thread(&state.known_codex_threads, thread_id)?;
        }
        CodexThreadAction::Start => {}
    }
    let endpoint_id = request.endpoint_id.clone();
    // codex 実行コマンドは settings.json (Rust 正本) から解決し、renderer 入力を使わない。
    let codex_command = crate::commands::settings::settings_load()
        .await
        .map(|settings| settings.codex_command)
        .unwrap_or_else(|_| "codex".to_string());
    // control socket は常に Rust 側の daemon 検出で解決する (renderer 指定の socket へ
    // ユーザー入力や approval を流させない)。
    let socket_path = crate::pty::codex_app_server::ensure_control_socket(&codex_command)
        .await
        .ok_or_else(|| {
            finish_codex_failure(
                app,
                &state.runtime_manager,
                &endpoint_id,
                crate::agent_runtime::RuntimeAdapterError::new(
                    "runtime_app_server_unavailable",
                    "Codex app-server control socket is unavailable",
                    false,
                ),
            )
        })?;
    let sink = codex_event_sink(
        app.clone(),
        state.runtime_manager.clone(),
        endpoint_id.clone(),
    );
    let adapter_result =
        run_blocking(move || CodexRuntimeAdapter::connect(socket_path, cwd, sink)).await?;
    let adapter =
        Arc::new(adapter_result.map_err(|error| {
            finish_codex_failure(app, &state.runtime_manager, &endpoint_id, error)
        })?);
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    let operation_adapter = adapter.clone();
    let operation = run_blocking(move || match request.thread {
        CodexThreadAction::Start => {
            manager.register_endpoint(operation_endpoint, operation_adapter)
        }
        CodexThreadAction::Resume { thread_id } => {
            manager.register_resumed_endpoint(operation_endpoint, operation_adapter, thread_id)
        }
        CodexThreadAction::Fork { thread_id } => {
            manager.register_forked_endpoint(operation_endpoint, operation_adapter, thread_id)
        }
    })
    .await?;
    emit_events(app, &operation.events);
    operation
        .result
        .map_err(|error| CommandError::coded(error.code, error.message))?;
    let thread_id = adapter.thread_id().ok_or_else(|| {
        CommandError::coded(
            "runtime_thread_not_ready",
            "Codex app-server did not return a thread id",
        )
    })?;
    // start/resume/fork いずれも成功した thread を「観測済み」として記録し、
    // 以後の resume / fork を認可できるようにする。
    record_known_thread(&state.known_codex_threads, Some(thread_id.clone()));
    Ok(CodexRuntimeEndpointResult {
        endpoint_id,
        thread_id,
    })
}

#[tauri::command]
pub async fn agent_runtime_spawn_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RuntimeTurnRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    validate_runtime_input(&request.input)?;
    let manager = state.runtime_manager.clone();
    let endpoint_id = request.endpoint_id;
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.spawn_turn(
            &operation_endpoint,
            RuntimeTurnSpawnRequest {
                input: request.input,
                submit: request.submit,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_write(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
    data: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    validate_runtime_input(&data)?;
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.write(&operation_endpoint, &data)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_inject(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
    data: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    validate_runtime_input(&data)?;
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.inject(&operation_endpoint, &data)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_steer(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RuntimeSteerCommandRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    validate_runtime_input(&request.input)?;
    let manager = state.runtime_manager.clone();
    let endpoint_id = request.endpoint_id;
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.steer(&operation_endpoint, request.input)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_interrupt(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.interrupt(&operation_endpoint)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_respond_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RuntimeApprovalCommandRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    validate_approval_request_id(&request.request_id)?;
    let manager = state.runtime_manager.clone();
    let endpoint_id = request.endpoint_id;
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.respond_approval(&operation_endpoint, request.request_id, request.decision)
    })
    .await
}

#[tauri::command]
pub async fn agent_runtime_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || manager.stop(&operation_endpoint)).await
}

#[tauri::command]
pub async fn agent_runtime_dispose(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    finish_blocking_operation(&app, endpoint_id, move || {
        manager.dispose(&operation_endpoint)
    })
    .await
}

#[cfg(test)]
mod tests;
