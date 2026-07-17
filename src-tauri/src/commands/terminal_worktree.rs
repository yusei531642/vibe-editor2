//! Team worker PTY の managed worktree 解決 (terminal.rs の baseline ratchet 対応で分離)。
//! 失敗は `?` で Err にせず `WorktreeResolution::Fail` (ok:false result) で返す —
//! spawn slot を消費したままの Err return は DoS ガード窓を塞ぐ (PR #37 レビュー)。

use super::terminal::TerminalCreateResult;
use crate::state::AppState;

pub(super) enum WorktreeResolution {
    /// managed worktree を cwd に使う (spawn 直前に identity 再照合すること)。
    Managed(String, crate::commands::project_authority::ProjectRootIdentity),
    /// worktree 未対応環境 (非 git / detached HEAD / git 不在): 従来 cwd を使う。
    PlainCwd,
    /// 認可・割当失敗: この result をそのまま返して spawn を中止する。
    Fail(TerminalCreateResult),
}

fn fail(message: String) -> WorktreeResolution {
    WorktreeResolution::Fail(TerminalCreateResult {
        ok: false,
        error: Some(message),
        ..Default::default()
    })
}

pub(super) async fn resolve_worker_worktree(
    state: &tauri::State<'_, AppState>,
    role: Option<&str>,
    team_id: &str,
    agent_id: &str,
) -> WorktreeResolution {
    if !super::terminal::uses_managed_worker_worktree(role) {
        return WorktreeResolution::PlainCwd;
    }
    let Some(active_root) = crate::state::current_project_root(&state.project_root) else {
        return fail("no active project root".to_string());
    };
    let project_root = match crate::commands::authz::assert_active_project_root(
        &state.project_root,
        &state.project_root_identity,
        &active_root,
    )
    .await
    {
        Ok(root) => root,
        Err(error) => return fail(format!("project authorization failed: {error}")),
    };
    match state
        .worktree_manager
        .optional_spawn_target(&project_root, team_id, agent_id)
        .await
    {
        Ok(Some((managed_cwd, identity))) => WorktreeResolution::Managed(managed_cwd, identity),
        Ok(None) => {
            tracing::warn!(
                team_id,
                agent_id,
                "[terminal] project is not a git repository or is on detached HEAD; using plain cwd"
            );
            WorktreeResolution::PlainCwd
        }
        Err(error) => fail(format!("worktree assignment failed: {error}")),
    }
}
