//! endpoint への操作系 IPC (turn / write / steer / interrupt / approval / stop / dispose)。
//! agent_runtime.rs の 500 行 ratchet を守るため操作コマンドだけを分離した。

use super::*;

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
