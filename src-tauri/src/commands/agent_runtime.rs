// agent_runtime.* command — Issue #21 diagnostics / Issue #22 endpoint operations.

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
/// Phase 0 の system detector は PTY のみを報告し、native 要件不足時は必ず PTY へ戻す。
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEndpointResult {
    pub endpoint_id: String,
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
        assert_eq!(value["selectedBackend"], json!("pty"));
        assert_eq!(value["reason"], json!("autoPtyFallback"));
        assert_eq!(value["capabilities"], json!(["ptyExecution"]));
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
