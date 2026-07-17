//! Issue #26: Team Card / Inspector / Approval Center の認可済み IPC 境界。

use crate::agent_runtime::RuntimeEventEnvelope;
use crate::commands::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::team_hub::protocol::tools::call_renderer_tool;
use crate::team_hub::CallContext;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

const MAX_TEAM_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamProjectionSnapshot {
    team_id: String,
    endpoints: Vec<crate::team_hub::TeamRuntimeEndpointSnapshot>,
    runtime_events: Vec<RuntimeEventEnvelope>,
    runtime_dropped_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberCommandRequest {
    team_id: String,
    command: TeamMemberCommand,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum TeamMemberCommand {
    Send {
        agent_id: Option<String>,
        message: String,
    },
    Interrupt {
        agent_id: String,
    },
    Stop {
        agent_id: String,
    },
    Dismiss {
        agent_id: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberCommandResult {
    action: String,
    affected_agent_ids: Vec<String>,
}

fn validate_id(name: &str, value: &str) -> CommandResult<()> {
    crate::commands::validation::validate_id_segment(name, value).map(|_| ())
}

async fn authorized_leader_context(
    state: &AppState,
    team_id: &str,
) -> CommandResult<(CallContext, Vec<(String, String)>)> {
    crate::commands::authz::assert_active_team(&state.team_hub, team_id).await?;
    let leader_id = {
        let hub_state = state.team_hub.state.lock().await;
        hub_state
            .teams
            .get(team_id)
            .and_then(|team| team.active_leader_agent_id.clone())
    }
    .ok_or_else(|| CommandError::authz("team has no active leader"))?;
    let members = state.team_hub.team_members(team_id).await;
    if !members.iter().any(|(agent_id, _)| agent_id == &leader_id) {
        return Err(CommandError::authz(
            "active leader is not an authorized team member",
        ));
    }
    Ok((
        CallContext {
            team_id: team_id.to_string(),
            role: "leader".to_string(),
            agent_id: leader_id,
        },
        members,
    ))
}

fn authorize_member(members: &[(String, String)], agent_id: &str) -> CommandResult<()> {
    validate_id("agent_id", agent_id)?;
    if members.iter().any(|(member_id, _)| member_id == agent_id) {
        Ok(())
    } else {
        Err(CommandError::authz(
            "agent is not an active member of this team",
        ))
    }
}

fn emit_runtime_events(app: &AppHandle, events: &[RuntimeEventEnvelope]) {
    for event in events {
        if let Err(error) = app.emit(&format!("runtime:event:{}", event.endpoint_id), event) {
            tracing::warn!("[team-projection] failed to emit runtime event: {error}");
        }
    }
}

#[tauri::command]
pub async fn team_projection_snapshot(
    state: State<'_, AppState>,
    team_id: String,
) -> CommandResult<TeamProjectionSnapshot> {
    validate_id("team_id", &team_id)?;
    crate::commands::authz::assert_active_team(&state.team_hub, &team_id).await?;
    let endpoints = state.team_hub.runtime_bindings_snapshot(&team_id).await;
    let endpoint_ids = endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let runtime_events = state
        .runtime_manager
        .event_snapshot()
        .into_iter()
        .filter(|event| endpoint_ids.contains(event.endpoint_id.as_str()))
        .collect();
    Ok(TeamProjectionSnapshot {
        team_id,
        endpoints,
        runtime_events,
        runtime_dropped_count: state.runtime_manager.dropped_event_count(),
    })
}

#[tauri::command]
pub async fn team_member_command(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TeamMemberCommandRequest,
) -> CommandResult<TeamMemberCommandResult> {
    validate_id("team_id", &request.team_id)?;
    let (ctx, members) = authorized_leader_context(&state, &request.team_id).await?;
    match request.command {
        TeamMemberCommand::Send { agent_id, message } => {
            if message.trim().is_empty() || message.contains('\0') {
                return Err(CommandError::validation(
                    "message must be non-empty and contain no NUL",
                ));
            }
            crate::commands::validation::assert_max_size(message.len(), MAX_TEAM_MESSAGE_BYTES)?;
            if let Some(agent_id) = agent_id.as_deref() {
                authorize_member(&members, agent_id)?;
            }
            let to = agent_id.as_deref().unwrap_or("all");
            call_renderer_tool(
                &state.team_hub,
                &ctx,
                "team_send",
                &json!({ "to": to, "message": message, "kind": "advisory" }),
            )
            .await
            .map_err(|error| CommandError::coded("team_send_failed", error))?;
            let affected_agent_ids = match agent_id {
                Some(agent_id) => vec![agent_id],
                None => members
                    .into_iter()
                    .map(|(agent_id, _)| agent_id)
                    .filter(|agent_id| agent_id != &ctx.agent_id)
                    .collect(),
            };
            Ok(TeamMemberCommandResult {
                action: "send".to_string(),
                affected_agent_ids,
            })
        }
        TeamMemberCommand::Interrupt { agent_id } => {
            authorize_member(&members, &agent_id)?;
            let (_, operation) = state
                .team_hub
                .control_pty_runtime(&request.team_id, &agent_id, "interrupt")
                .await
                .map_err(|error| CommandError::coded("team_pty_control_failed", error))?;
            emit_runtime_events(&app, &operation.events);
            operation
                .result
                .map_err(|error| CommandError::coded(error.code, error.message))?;
            Ok(TeamMemberCommandResult {
                action: "interrupt".to_string(),
                affected_agent_ids: vec![agent_id],
            })
        }
        TeamMemberCommand::Stop { agent_id } => {
            authorize_member(&members, &agent_id)?;
            let (_, operation) = state
                .team_hub
                .control_pty_runtime(&request.team_id, &agent_id, "stop")
                .await
                .map_err(|error| CommandError::coded("team_pty_control_failed", error))?;
            emit_runtime_events(&app, &operation.events);
            operation
                .result
                .map_err(|error| CommandError::coded(error.code, error.message))?;
            Ok(TeamMemberCommandResult {
                action: "stop".to_string(),
                affected_agent_ids: vec![agent_id],
            })
        }
        TeamMemberCommand::Dismiss { agent_id } => {
            authorize_member(&members, &agent_id)?;
            call_renderer_tool(
                &state.team_hub,
                &ctx,
                "team_dismiss",
                &json!({ "agent_id": agent_id }),
            )
            .await
            .map_err(|error| CommandError::coded("team_dismiss_failed", error))?;
            Ok(TeamMemberCommandResult {
                action: "dismiss".to_string(),
                affected_agent_ids: vec![agent_id],
            })
        }
    }
}
