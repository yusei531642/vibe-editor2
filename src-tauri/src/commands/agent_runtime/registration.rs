use super::*;
use crate::agent_runtime::claude_agent::{
    ClaudeAgentRuntimeAdapter, ClaudeAgentRuntimeConfig, SidecarLaunchConfig,
};
#[cfg(unix)]
use crate::agent_runtime::codex::CodexRuntimeAdapter;

#[cfg(unix)]
pub(super) async fn register_codex_endpoint(
    app: &AppHandle,
    state: &State<'_, AppState>,
    request: RegisterCodexEndpointRequest,
) -> CommandResult<CodexRuntimeEndpointResult> {
    validate_endpoint_id(&request.endpoint_id)?;
    validate_runtime_option("model", request.model.as_deref())?;
    validate_runtime_permission(request.permission.as_deref())?;
    if let Some(team_id) = request.team_id.as_deref() {
        crate::commands::validation::validate_id_segment("team_id", team_id)?;
    }
    if let Some(agent_id) = request.agent_id.as_deref() {
        crate::commands::validation::validate_id_segment("agent_id", agent_id)?;
    }
    let runtime_team_agent = request.team_id.clone().zip(request.agent_id.clone());
    let restoring = matches!(&request.thread, CodexThreadAction::Resume { .. });
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
    let model = request.model.clone();
    let permission = request.permission.clone();
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
            finish_native_failure(
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
    let adapter_result = run_blocking(move || {
        CodexRuntimeAdapter::connect(socket_path, cwd, model, permission, sink)
    })
    .await?;
    let adapter = Arc::new(adapter_result.map_err(|error| {
        finish_native_failure(app, &state.runtime_manager, &endpoint_id, error)
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
    let thread_id = match adapter.thread_id() {
        Some(thread_id) => thread_id,
        None => {
            return Err(finish_native_failure(
                app,
                &state.runtime_manager,
                &endpoint_id,
                crate::agent_runtime::RuntimeAdapterError::new(
                    "runtime_thread_not_ready",
                    "Codex app-server did not return a thread id",
                    false,
                ),
            ));
        }
    };
    if let Some((team_id, agent_id)) = runtime_team_agent {
        if restoring {
            state
                .team_hub
                .authorize_restored_native_reconnect(&team_id, &agent_id, &endpoint_id)
                .await
                .map_err(CommandError::authz)?;
        }
        if let Err(error) = state
            .team_hub
            .bind_native_runtime_endpoint(
                &team_id,
                &agent_id,
                endpoint_id.clone(),
                Some(thread_id.clone()),
            )
            .await
        {
            return Err(finish_native_failure(
                app,
                &state.runtime_manager,
                &endpoint_id,
                crate::agent_runtime::RuntimeAdapterError::new(
                    "runtime_team_binding_failed",
                    error,
                    false,
                ),
            ));
        }
        let project_root = state.team_hub.team_project_root(&team_id).await;
        state
            .runtime_manager
            .persist_team_binding(crate::agent_runtime::RuntimeTeamBinding {
                project_root: project_root.as_deref(),
                team_id: &team_id,
                agent_id: &agent_id,
                endpoint_id: &endpoint_id,
                provider: "codex-native",
                resume_id: Some(thread_id.clone()),
                resumable: true,
            });
    }
    // start/resume/fork いずれも成功した thread を「観測済み」として記録し、
    // 以後の resume / fork を認可できるようにする。
    record_known_thread(&state.known_codex_threads, Some(thread_id.clone()));
    Ok(CodexRuntimeEndpointResult {
        endpoint_id,
        thread_id,
    })
}

pub(super) async fn register_claude_endpoint(
    app: &AppHandle,
    state: &State<'_, AppState>,
    request: RegisterClaudeEndpointRequest,
) -> CommandResult<ClaudeRuntimeEndpointResult> {
    let RegisterClaudeEndpointRequest {
        endpoint_id,
        team_id,
        agent_id,
        system_prompt,
        model,
        effort,
        permission,
        session,
    } = request;
    validate_endpoint_id(&endpoint_id)?;
    if team_id.is_some() != agent_id.is_some() {
        return Err(CommandError::validation(
            "teamId and agentId must be provided together",
        ));
    }
    let authorized_team_identity =
        if let Some((team_id, agent_id)) = team_id.as_deref().zip(agent_id.as_deref()) {
            let role = state
                .team_hub
                .authorized_team_agent_role(team_id, agent_id)
                .await?;
            Some((team_id.to_string(), agent_id.to_string(), role))
        } else {
            None
        };
    if let Some(prompt) = system_prompt.as_deref() {
        validate_bounded_no_nul("systemPrompt", prompt, 256 * 1024)?;
    }
    validate_runtime_option("model", model.as_deref())?;
    validate_runtime_option("effort", effort.as_deref())?;
    validate_runtime_permission(permission.as_deref())?;
    let resume_session = match &session {
        ClaudeSessionAction::Resume { session_id } | ClaudeSessionAction::Fork { session_id } => {
            crate::commands::validation::validate_id_segment("session_id", session_id)?;
            authorize_known_thread(&state.known_claude_sessions, session_id)?;
            Some(session_id.clone())
        }
        ClaudeSessionAction::Start => None,
    };
    let restoring = matches!(&session, ClaudeSessionAction::Resume { .. });
    let cwd = crate::state::current_project_root(&state.project_root);
    let settings = crate::commands::settings::settings_load().await?;
    let mut launch = SidecarLaunchConfig::production(settings.claude_command)
        .map_err(|error| CommandError::coded(error.code, error.message))?;
    // Claude Agent SDK は ~/.claude.json の mcpServers を自動では取り込まない。
    // team identity が認可済みの場合だけ Hub の接続情報を Rust 内で組み立て、renderer を
    // 経由せず sidecar へ渡す。token は sidecar error の redaction 対象にも追加する。
    let mcp_servers = if let Some((team_id, agent_id, role)) = &authorized_team_identity {
        let (socket, token, bridge_path) = state.team_hub.info().await;
        launch.secret_values.push(token.clone());
        Some(serde_json::json!({
            "vibe-team2": crate::mcp_config::team_bridge_desired(
                &socket,
                &token,
                &bridge_path,
                team_id,
                agent_id,
                role,
            )
        }))
    } else {
        None
    };
    let sink = claude_event_sink(
        app.clone(),
        state.runtime_manager.clone(),
        endpoint_id.clone(),
        state.known_claude_sessions.clone(),
    );
    let adapter_result = run_blocking(move || {
        ClaudeAgentRuntimeAdapter::connect(
            launch,
            ClaudeAgentRuntimeConfig {
                cwd,
                system_prompt,
                model,
                effort,
                permission,
                mcp_servers,
            },
            sink,
        )
    })
    .await?;
    let adapter = Arc::new(adapter_result.map_err(|error| {
        let operation = state
            .runtime_manager
            .fail_endpoint(&endpoint_id, error.clone());
        emit_events(app, &operation.events);
        CommandError::coded(error.code, error.message)
    })?);
    let manager = state.runtime_manager.clone();
    let operation_endpoint = endpoint_id.clone();
    let operation_adapter = adapter.clone();
    let operation = run_blocking(move || match session {
        ClaudeSessionAction::Start => {
            manager.register_endpoint(operation_endpoint, operation_adapter)
        }
        ClaudeSessionAction::Resume { session_id } => {
            manager.register_resumed_endpoint(operation_endpoint, operation_adapter, session_id)
        }
        ClaudeSessionAction::Fork { session_id } => {
            manager.register_forked_endpoint(operation_endpoint, operation_adapter, session_id)
        }
    })
    .await?;
    emit_events(app, &operation.events);
    operation
        .result
        .map_err(|error| CommandError::coded(error.code, error.message))?;
    let session_id = adapter.session_id().or(resume_session);
    if let Some((team_id, agent_id)) = team_id.zip(agent_id) {
        if restoring {
            state
                .team_hub
                .authorize_restored_native_reconnect(&team_id, &agent_id, &endpoint_id)
                .await
                .map_err(CommandError::authz)?;
        }
        state
            .team_hub
            .bind_native_runtime_endpoint(
                &team_id,
                &agent_id,
                endpoint_id.clone(),
                session_id.clone(),
            )
            .await
            .map_err(|message| {
                finish_native_failure(
                    app,
                    &state.runtime_manager,
                    &endpoint_id,
                    crate::agent_runtime::RuntimeAdapterError::new(
                        "runtime_team_binding_failed",
                        message,
                        false,
                    ),
                )
            })?;
        let project_root = state.team_hub.team_project_root(&team_id).await;
        state
            .runtime_manager
            .persist_team_binding(crate::agent_runtime::RuntimeTeamBinding {
                project_root: project_root.as_deref(),
                team_id: &team_id,
                agent_id: &agent_id,
                endpoint_id: &endpoint_id,
                provider: "claude-native",
                resume_id: session_id.clone(),
                resumable: session_id.is_some(),
            });
    }
    record_known_thread(&state.known_claude_sessions, session_id.clone());
    Ok(ClaudeRuntimeEndpointResult {
        endpoint_id,
        session_id,
    })
}
