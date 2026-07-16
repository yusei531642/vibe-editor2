use super::*;
use crate::agent_runtime::codex::CodexRuntimeAdapter;

pub(super) async fn register_codex_endpoint(
    app: &AppHandle,
    state: &State<'_, AppState>,
    request: RegisterCodexEndpointRequest,
) -> CommandResult<CodexRuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    let runtime_team_agent = request.team_id.clone().zip(request.agent_id.clone());
    if let Some(cwd) = request.cwd.as_deref() {
        validate_bounded_no_nul("cwd", cwd, 16 * 1024)?;
    }
    if let Some(command) = request.codex_command.as_deref() {
        validate_bounded_no_nul("codexCommand", command, MAX_RUNTIME_PATH_BYTES)?;
    }
    if let Some(socket_path) = request.socket_path.as_deref() {
        validate_bounded_no_nul("socketPath", socket_path, MAX_RUNTIME_PATH_BYTES)?;
    }
    match &request.thread {
        CodexThreadAction::Resume { thread_id } | CodexThreadAction::Fork { thread_id } => {
            crate::commands::validation::validate_id_segment("thread_id", thread_id)?;
        }
        CodexThreadAction::Start => {}
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
    let cwd = request.cwd;
    let adapter_result =
        run_blocking(move || CodexRuntimeAdapter::connect(socket_path, cwd, sink)).await?;
    let adapter = Arc::new(
        adapter_result.map_err(|error| {
            finish_codex_failure(app, &state.runtime_manager, &endpoint_id, error)
        })?,
    );
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
    if let Some((team_id, agent_id)) = runtime_team_agent {
        state
            .team_hub
            .bind_native_runtime_endpoint(
                &team_id,
                &agent_id,
                endpoint_id.clone(),
                Some(thread_id.clone()),
            )
            .await
            .map_err(|error| CommandError::coded("runtime_team_binding_failed", error))?;
    }
    Ok(CodexRuntimeEndpointResult {
        endpoint_id,
        thread_id,
    })
}
