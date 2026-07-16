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
pub struct RegisterCodexEndpointRequest {
    pub endpoint_id: String,
    pub socket_path: Option<String>,
    pub codex_command: Option<String>,
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
    let operation = state
        .runtime_manager
        .register_endpoint(request.endpoint_id.clone(), adapter);
    finish_operation(&app, request.endpoint_id, operation)
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
            let operation = state.runtime_manager.dispose(&request.endpoint_id);
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
    if let Some(cwd) = request.cwd.as_deref() {
        if cwd.contains('\0') {
            return Err(CommandError::validation("cwd must not contain NUL"));
        }
        crate::commands::validation::assert_max_size(cwd.len(), 16 * 1024)?;
    }
    let endpoint_id = request.endpoint_id.clone();
    let socket_path = match request.socket_path {
        Some(path) if !path.trim().is_empty() => path,
        _ => crate::pty::codex_app_server::ensure_control_socket(
            request.codex_command.as_deref().unwrap_or("codex"),
        )
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
        })?,
    };
    let sink = codex_event_sink(
        app.clone(),
        state.runtime_manager.clone(),
        endpoint_id.clone(),
    );
    let adapter = Arc::new(
        CodexRuntimeAdapter::connect(socket_path, request.cwd, sink).map_err(|error| {
            finish_codex_failure(app, &state.runtime_manager, &endpoint_id, error)
        })?,
    );
    let operation = match request.thread {
        CodexThreadAction::Start => state
            .runtime_manager
            .register_endpoint(endpoint_id.clone(), adapter.clone()),
        CodexThreadAction::Resume { thread_id } => {
            crate::commands::validation::validate_id_segment("thread_id", &thread_id)?;
            state.runtime_manager.register_resumed_endpoint(
                endpoint_id.clone(),
                adapter.clone(),
                thread_id,
            )
        }
        CodexThreadAction::Fork { thread_id } => {
            crate::commands::validation::validate_id_segment("thread_id", &thread_id)?;
            state.runtime_manager.register_forked_endpoint(
                endpoint_id.clone(),
                adapter.clone(),
                thread_id,
            )
        }
    };
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
    let operation = state.runtime_manager.spawn_turn(
        &request.endpoint_id,
        RuntimeTurnSpawnRequest {
            input: request.input,
            submit: request.submit,
        },
    );
    finish_operation(&app, request.endpoint_id, operation)
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
    let operation = state.runtime_manager.write(&endpoint_id, &data);
    finish_operation(&app, endpoint_id, operation)
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
    let operation = state.runtime_manager.inject(&endpoint_id, &data);
    finish_operation(&app, endpoint_id, operation)
}

#[tauri::command]
pub async fn agent_runtime_steer(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RuntimeSteerCommandRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    validate_runtime_input(&request.input)?;
    let operation = state
        .runtime_manager
        .steer(&request.endpoint_id, request.input);
    finish_operation(&app, request.endpoint_id, operation)
}

#[tauri::command]
pub async fn agent_runtime_interrupt(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let operation = state.runtime_manager.interrupt(&endpoint_id);
    finish_operation(&app, endpoint_id, operation)
}

#[tauri::command]
pub async fn agent_runtime_respond_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RuntimeApprovalCommandRequest,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    crate::commands::validation::validate_id_segment("request_id", &request.request_id)?;
    let operation = state.runtime_manager.respond_approval(
        &request.endpoint_id,
        request.request_id,
        request.decision,
    );
    finish_operation(&app, request.endpoint_id, operation)
}

#[tauri::command]
pub async fn agent_runtime_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let operation = state.runtime_manager.stop(&endpoint_id);
    finish_operation(&app, endpoint_id, operation)
}

#[tauri::command]
pub async fn agent_runtime_dispose(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint_id: String,
) -> CommandResult<RuntimeEndpointResult> {
    validate_endpoint_id(&endpoint_id)?;
    let operation = state.runtime_manager.dispose(&endpoint_id);
    finish_operation(&app, endpoint_id, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn diagnostics_uses_camelcase_and_safe_auto_fallback() {
        let result = agent_runtime_diagnostics("auto".to_string()).await.unwrap();
        let value = serde_json::to_value(result).unwrap();

        assert_eq!(value["requestedBackend"], json!("auto"));
        #[cfg(unix)]
        {
            assert_eq!(value["selectedBackend"], json!("native"));
            assert_eq!(value["reason"], json!("autoNativeCapabilitiesAvailable"));
            assert!(value["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("approvalResponses")));
        }
        #[cfg(not(unix))]
        {
            assert_eq!(value["selectedBackend"], json!("pty"));
            assert_eq!(value["reason"], json!("autoPtyFallback"));
            assert_eq!(value["capabilities"], json!(["ptyExecution"]));
        }
    }

    #[tokio::test]
    async fn diagnostics_rejects_unknown_backend() {
        let error = agent_runtime_diagnostics("unknown".to_string())
            .await
            .unwrap_err();
        assert_eq!(error.code(), "validation");
        let serialized = serde_json::to_value(&error).unwrap();
        assert!(serialized["message"]
            .as_str()
            .unwrap()
            .contains("expected auto, native, or pty"));
    }
}
