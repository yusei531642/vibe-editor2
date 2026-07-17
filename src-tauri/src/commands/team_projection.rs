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
const MAX_APPROVAL_REQUEST_ID_BYTES: usize = 256;
const MAX_SNAPSHOT_CURSORS: usize = 512;
const MAX_CURSOR_TIMESTAMP_BYTES: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRuntimeEventCursor {
    endpoint_id: String,
    epoch: u64,
    sequence: u64,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamProjectionSnapshotRequest {
    team_id: String,
    since_sequence: Vec<TeamRuntimeEventCursor>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamProjectionSnapshot {
    team_id: String,
    endpoints: Vec<crate::team_hub::runtime_endpoint::types::TeamRuntimeEndpointSnapshot>,
    runtime_events: Vec<RuntimeEventEnvelope>,
    retained_event_cursors: Vec<TeamRuntimeEventCursor>,
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
    RespondApproval {
        agent_id: String,
        request_id: String,
        decision: String,
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

fn validate_approval(request_id: &str, decision: &str) -> CommandResult<()> {
    if request_id.is_empty() || request_id.chars().any(char::is_control) {
        return Err(CommandError::validation(
            "request_id must be non-empty and contain no control characters",
        ));
    }
    crate::commands::validation::assert_max_size(request_id.len(), MAX_APPROVAL_REQUEST_ID_BYTES)?;
    if !matches!(
        decision,
        "accept" | "acceptForSession" | "decline" | "cancel"
    ) {
        return Err(CommandError::validation("unsupported approval decision"));
    }
    Ok(())
}

fn event_cursor(event: &RuntimeEventEnvelope) -> TeamRuntimeEventCursor {
    TeamRuntimeEventCursor {
        endpoint_id: event.endpoint_id.clone(),
        epoch: event.epoch,
        sequence: event.sequence,
        timestamp: event.timestamp.clone(),
    }
}

fn incremental_events(
    events: &[RuntimeEventEnvelope],
    since_sequence: &[TeamRuntimeEventCursor],
) -> Vec<RuntimeEventEnvelope> {
    let cursors = since_sequence
        .iter()
        .map(|cursor| (cursor.endpoint_id.as_str(), cursor))
        .collect::<std::collections::HashMap<_, _>>();
    let positions = cursors
        .iter()
        .filter_map(|(endpoint_id, cursor)| {
            events
                .iter()
                .rposition(|event| {
                    event.endpoint_id == **endpoint_id
                        && event.sequence == cursor.sequence
                        && event.epoch == cursor.epoch
                        && event.timestamp == cursor.timestamp
                })
                .map(|position| (*endpoint_id, position))
        })
        .collect::<std::collections::HashMap<_, _>>();
    events
        .iter()
        .enumerate()
        .filter(
            |(index, event)| match cursors.get(event.endpoint_id.as_str()) {
                None => true,
                Some(_) => positions
                    .get(event.endpoint_id.as_str())
                    .is_none_or(|position| index > position),
            },
        )
        .map(|(_, event)| event.clone())
        .collect()
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
    request: TeamProjectionSnapshotRequest,
) -> CommandResult<TeamProjectionSnapshot> {
    validate_id("team_id", &request.team_id)?;
    crate::commands::validation::assert_max_size(
        request.since_sequence.len(),
        MAX_SNAPSHOT_CURSORS,
    )?;
    for cursor in &request.since_sequence {
        validate_id("endpoint_id", &cursor.endpoint_id)?;
        if cursor.epoch == 0
            || cursor.sequence == 0
            || cursor.timestamp.is_empty()
            || cursor.timestamp.chars().any(char::is_control)
        {
            return Err(CommandError::validation(
                "snapshot cursor must contain a positive sequence and timestamp",
            ));
        }
        crate::commands::validation::assert_max_size(
            cursor.timestamp.len(),
            MAX_CURSOR_TIMESTAMP_BYTES,
        )?;
    }
    crate::commands::authz::assert_active_team(&state.team_hub, &request.team_id).await?;
    let endpoints = state
        .team_hub
        .runtime_bindings_snapshot(&request.team_id)
        .await;
    let endpoint_ids = endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let retained_events = state
        .runtime_manager
        .event_snapshot()
        .into_iter()
        .filter(|event| endpoint_ids.contains(event.endpoint_id.as_str()))
        .collect::<Vec<_>>();
    let runtime_events = incremental_events(&retained_events, &request.since_sequence);
    let retained_event_cursors = retained_events.iter().map(event_cursor).collect();
    Ok(TeamProjectionSnapshot {
        team_id: request.team_id,
        endpoints,
        runtime_events,
        retained_event_cursors,
        runtime_dropped_count: state.runtime_manager.dropped_event_count(),
    })
}

/// Phase 8 startup sync. The five fields intentionally match `team_projection_snapshot` so the
/// renderer can switch from durable replay to live polling without an intermediate shape.
#[tauri::command]
pub async fn session_restore_snapshot(
    state: State<'_, AppState>,
) -> CommandResult<Option<TeamProjectionSnapshot>> {
    let manager = state.runtime_manager.clone();
    let restored = tauri::async_runtime::spawn_blocking(move || manager.restore_latest())
        .await
        .map_err(|error| CommandError::coded("runtime_restore_task_failed", error.to_string()))?
        .map_err(|error| CommandError::coded("runtime_restore_failed", error))?;
    let Some(team_id) = restored.team_id else {
        return Ok(None);
    };
    for binding in &restored.bindings {
        let Some(resume_id) = binding.resume_id.clone() else {
            continue;
        };
        if binding.provider == "codex-native" {
            super::agent_runtime::record_known_thread_for_restore(
                &state.known_codex_threads,
                resume_id,
            );
        } else if binding.provider == "claude-native" {
            super::agent_runtime::record_known_thread_for_restore(
                &state.known_claude_sessions,
                resume_id,
            );
        }
    }
    let endpoints = restored
        .bindings
        .into_iter()
        .map(
            |binding| crate::team_hub::runtime_endpoint::types::TeamRuntimeEndpointSnapshot {
                team_id: binding.team_id,
                agent_id: binding.agent_id,
                endpoint_id: binding.endpoint_id,
                backend: if binding.provider == "pty" {
                    "pty"
                } else {
                    "native"
                }
                .to_string(),
                session_id: binding.resume_id,
                task_ids: Vec::new(),
                live: false,
                provider: binding.provider,
                restore_state: if binding.resumable {
                    "reconnectable".to_string()
                } else {
                    "terminated".to_string()
                },
            },
        )
        .collect();
    let retained_event_cursors = restored.events.iter().map(event_cursor).collect();
    Ok(Some(TeamProjectionSnapshot {
        team_id,
        endpoints,
        runtime_events: restored.events,
        retained_event_cursors,
        runtime_dropped_count: 0,
    }))
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
        TeamMemberCommand::RespondApproval {
            agent_id,
            request_id,
            decision,
        } => {
            authorize_member(&members, &agent_id)?;
            validate_approval(&request_id, &decision)?;
            let endpoint_id = state
                .team_hub
                .approval_runtime_endpoint(&request.team_id, &agent_id)
                .await
                .map_err(|error| CommandError::coded("team_approval_endpoint_failed", error))?;
            let manager = state.runtime_manager.clone();
            let operation_endpoint = endpoint_id.clone();
            let operation = tauri::async_runtime::spawn_blocking(move || {
                manager.respond_approval(&operation_endpoint, request_id, decision)
            })
            .await
            .map_err(|error| {
                CommandError::coded("runtime_blocking_task_failed", error.to_string())
            })?;
            emit_runtime_events(&app, &operation.events);
            operation
                .result
                .map_err(|error| CommandError::coded(error.code, error.message))?;
            Ok(TeamMemberCommandResult {
                action: "respondApproval".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::RuntimeEventPayload;

    fn event(endpoint_id: &str, sequence: u64, timestamp: &str) -> RuntimeEventEnvelope {
        let mut event = RuntimeEventEnvelope::new(
            endpoint_id.to_string(),
            if timestamp.starts_with("new") { 2 } else { 1 },
            sequence,
            RuntimeEventPayload::Diagnostic {
                message: format!("event-{sequence}"),
            },
        );
        event.timestamp = timestamp.to_string();
        event
    }

    #[test]
    fn snapshot_filter_returns_only_events_after_exact_endpoint_cursor() {
        let events = vec![
            event("endpoint-a", 1, "old-1"),
            event("endpoint-b", 1, "other-1"),
            event("endpoint-a", 2, "old-2"),
            event("endpoint-a", 1, "new-1"),
        ];
        let cursors = vec![TeamRuntimeEventCursor {
            endpoint_id: "endpoint-a".to_string(),
            epoch: 1,
            sequence: 2,
            timestamp: "old-2".to_string(),
        }];
        let filtered = incremental_events(&events, &cursors);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].endpoint_id, "endpoint-b");
        assert_eq!(filtered[1].timestamp, "new-1");
    }

    #[test]
    fn snapshot_filter_falls_back_to_full_endpoint_history_when_cursor_was_evicted() {
        let events = vec![event("endpoint-a", 3, "current-3")];
        let cursors = vec![TeamRuntimeEventCursor {
            endpoint_id: "endpoint-a".to_string(),
            epoch: 1,
            sequence: 2,
            timestamp: "evicted-2".to_string(),
        }];
        assert_eq!(incremental_events(&events, &cursors).len(), 1);
    }
}
