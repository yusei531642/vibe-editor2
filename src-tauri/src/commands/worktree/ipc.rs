use super::WorktreeManagerSnapshot;
use crate::commands::authz::ProjectRoot;
use crate::commands::error::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshotRequest {
    project_root: String,
    team_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCommandRequest {
    project_root: String,
    team_id: String,
    command: WorktreeCommand,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WorktreeCommand {
    Create {
        agent_id: String,
    },
    Resume {
        agent_id: String,
    },
    Enqueue {
        agent_id: String,
        evidence: String,
    },
    Review {
        candidate_id: String,
        decision: ReviewDecision,
    },
    Integrate {
        candidate_id: String,
    },
    Cleanup {
        agent_id: String,
    },
    Cancel {
        candidate_id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReviewDecision {
    Approve,
    RequestChanges,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCommandResult {
    action: String,
    snapshot: WorktreeManagerSnapshot,
}

async fn authorize_read_root(
    state: &crate::state::AppState,
    raw: &str,
) -> CommandResult<ProjectRoot> {
    crate::commands::authz::assert_readable_project_root(
        &state.project_root,
        &state.project_root_identity,
        raw,
    )
    .await
}

async fn authorize_write_root(
    state: &crate::state::AppState,
    raw: &str,
) -> CommandResult<ProjectRoot> {
    crate::commands::authz::assert_active_project_root(
        &state.project_root,
        &state.project_root_identity,
        raw,
    )
    .await
}

async fn authorize_member(
    state: &crate::state::AppState,
    team_id: &str,
    agent_id: &str,
) -> CommandResult<()> {
    state
        .team_hub
        .authorize_team_agent_binding(team_id, agent_id)
        .await
}

#[tauri::command]
pub async fn worktree_manager_snapshot(
    state: tauri::State<'_, crate::state::AppState>,
    request: WorktreeSnapshotRequest,
) -> CommandResult<WorktreeManagerSnapshot> {
    crate::commands::validation::validate_id_segment("team_id", &request.team_id)?;
    let root = authorize_read_root(&state, &request.project_root).await?;
    crate::commands::authz::assert_active_team(&state.team_hub, &request.team_id).await?;
    state
        .worktree_manager
        .snapshot(&root, &request.team_id)
        .await
}

#[tauri::command]
pub async fn worktree_manager_command(
    state: tauri::State<'_, crate::state::AppState>,
    request: WorktreeCommandRequest,
) -> CommandResult<WorktreeCommandResult> {
    crate::commands::validation::validate_id_segment("team_id", &request.team_id)?;
    let root = authorize_write_root(&state, &request.project_root).await?;
    crate::commands::authz::assert_active_team(&state.team_hub, &request.team_id).await?;
    let action = match request.command {
        WorktreeCommand::Create { agent_id } => {
            authorize_member(&state, &request.team_id, &agent_id).await?;
            state
                .worktree_manager
                .assign(&root, &request.team_id, &agent_id)
                .await?;
            "create"
        }
        WorktreeCommand::Resume { agent_id } => {
            authorize_member(&state, &request.team_id, &agent_id).await?;
            state
                .worktree_manager
                .resume(&root, &request.team_id, &agent_id)
                .await?;
            "resume"
        }
        WorktreeCommand::Enqueue { agent_id, evidence } => {
            authorize_member(&state, &request.team_id, &agent_id).await?;
            state
                .worktree_manager
                .enqueue(&root, &request.team_id, &agent_id, evidence)
                .await?;
            "enqueue"
        }
        WorktreeCommand::Review {
            candidate_id,
            decision,
        } => {
            let (team_id, agent_id) = state
                .worktree_manager
                .candidate_owner(&root, &candidate_id)
                .await?;
            assert_candidate_team(&request.team_id, &team_id)?;
            authorize_member(&state, &team_id, &agent_id).await?;
            state
                .worktree_manager
                .review(&candidate_id, matches!(decision, ReviewDecision::Approve))
                .await?;
            "review"
        }
        WorktreeCommand::Integrate { candidate_id } => {
            let (team_id, agent_id) = state
                .worktree_manager
                .candidate_owner(&root, &candidate_id)
                .await?;
            assert_candidate_team(&request.team_id, &team_id)?;
            authorize_member(&state, &team_id, &agent_id).await?;
            state
                .worktree_manager
                .integrate(&root, &candidate_id)
                .await?;
            "integrate"
        }
        WorktreeCommand::Cleanup { agent_id } => {
            authorize_member(&state, &request.team_id, &agent_id).await?;
            state
                .worktree_manager
                .cleanup(&root, &request.team_id, &agent_id)
                .await?;
            "cleanup"
        }
        WorktreeCommand::Cancel { candidate_id } => {
            let (team_id, _agent_id) = state
                .worktree_manager
                .candidate_owner(&root, &candidate_id)
                .await?;
            assert_candidate_team(&request.team_id, &team_id)?;
            // User-triggered active-team control: cancellation intentionally does not require the
            // candidate owner to remain a member, so departed workers cannot wedge the queue.
            state.worktree_manager.cancel(&candidate_id).await?;
            "cancel"
        }
    }
    .to_string();
    let snapshot = state
        .worktree_manager
        .snapshot(&root, &request.team_id)
        .await?;
    Ok(WorktreeCommandResult { action, snapshot })
}

fn assert_candidate_team(request_team_id: &str, candidate_team_id: &str) -> CommandResult<()> {
    if request_team_id == candidate_team_id {
        Ok(())
    } else {
        Err(CommandError::authz(
            "merge candidate does not belong to the active team",
        ))
    }
}
