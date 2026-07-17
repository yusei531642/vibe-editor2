//! Issue #27: Rust-owned worktree assignments and reviewed merge queue IPC.

mod git_ops;
pub(crate) mod ipc;
mod persistence;
mod queue;

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

use crate::commands::authz::ProjectRoot;
use crate::commands::error::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::sync::Mutex;

const MAX_EVIDENCE_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssignmentKey {
    project_key: String,
    team_id: String,
    agent_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssignmentRecord {
    key: AssignmentKey,
    project_root: PathBuf,
    path: PathBuf,
    branch_name: String,
    base_branch: String,
    base_commit: String,
    integrated_candidate_id: Option<String>,
}

#[derive(Default)]
struct ManagerState {
    assignments: HashMap<AssignmentKey, AssignmentRecord>,
    candidates: Vec<MergeCandidateSnapshot>,
    next_queue_position: u64,
}

#[derive(Clone)]
struct CachedAssignmentDetails {
    captured_at: Instant,
    head_commit: String,
    clean: bool,
}

pub struct WorktreeManager {
    storage_root: PathBuf,
    state: Mutex<ManagerState>,
    assignment_lock: Mutex<()>,
    integration_lock: Mutex<()>,
    persistence_lock: Mutex<()>,
    loaded: Mutex<bool>,
    reconciled_projects: Mutex<HashSet<String>>,
    detail_cache: Mutex<HashMap<AssignmentKey, CachedAssignmentDetails>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeAssignmentSnapshot {
    pub team_id: String,
    pub agent_id: String,
    pub branch_name: String,
    pub base_branch: String,
    pub base_commit: String,
    pub head_commit: String,
    pub clean: bool,
    pub cleanup_eligible: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeCandidateStatus {
    PendingReview,
    Approved,
    ChangesRequested,
    Integrating,
    Integrated,
    Conflict,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeConflictSnapshot {
    pub paths: Vec<String>,
    pub base_commit: String,
    pub candidate_commit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCandidateSnapshot {
    #[serde(skip)]
    project_key: String,
    pub id: String,
    pub team_id: String,
    pub agent_id: String,
    pub commit: String,
    pub evidence: String,
    pub base_commit: String,
    pub changed_paths: Vec<String>,
    pub queue_position: u64,
    pub status: MergeCandidateStatus,
    pub conflict: Option<MergeConflictSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeManagerSnapshot {
    pub team_id: String,
    /// 非 git プロジェクト / detached HEAD / git 不在では false。renderer は
    /// poll を止めて「worktree 未対応」表示に切り替える (PR #37 レビュー)。
    pub supported: bool,
    pub assignments: Vec<WorktreeAssignmentSnapshot>,
    pub candidates: Vec<MergeCandidateSnapshot>,
    pub review_required: bool,
    pub integration_in_progress: bool,
}

impl WorktreeManager {
    pub fn new() -> Self {
        Self::with_storage_root(crate::util::config_paths::vibe_root().join("worktrees"))
    }

    fn with_storage_root(storage_root: PathBuf) -> Self {
        Self {
            storage_root,
            state: Mutex::new(ManagerState::default()),
            assignment_lock: Mutex::new(()),
            integration_lock: Mutex::new(()),
            persistence_lock: Mutex::new(()),
            loaded: Mutex::new(false),
            reconciled_projects: Mutex::new(HashSet::new()),
            detail_cache: Mutex::new(HashMap::new()),
        }
    }

    fn project_key(project_root: &Path) -> String {
        let mut digest = Sha256::new();
        digest.update(project_root.to_string_lossy().replace('\\', "/").as_bytes());
        digest
            .finalize()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn key(project_root: &Path, team_id: &str, agent_id: &str) -> AssignmentKey {
        AssignmentKey {
            project_key: Self::project_key(project_root),
            team_id: team_id.to_string(),
            agent_id: agent_id.to_string(),
        }
    }

    fn validate_ids(team_id: &str, agent_id: &str) -> CommandResult<()> {
        crate::commands::validation::validate_id_segment("team_id", team_id)?;
        crate::commands::validation::validate_id_segment("agent_id", agent_id)?;
        Ok(())
    }

    async fn managed_path(&self, key: &AssignmentKey) -> CommandResult<PathBuf> {
        crate::commands::validation::validate_id_segment("team_id", &key.team_id)?;
        crate::commands::validation::validate_id_segment("agent_id", &key.agent_id)?;
        tokio::fs::create_dir_all(&self.storage_root).await?;
        let root = tokio::fs::canonicalize(&self.storage_root).await?;
        let parent = root.join(&key.project_key).join(&key.team_id);
        tokio::fs::create_dir_all(&parent).await?;
        let parent = tokio::fs::canonicalize(parent).await?;
        if !parent.starts_with(&root) {
            return Err(CommandError::authz(
                "managed worktree parent escaped storage root",
            ));
        }
        let path = parent.join(&key.agent_id);
        if let Ok(existing) = tokio::fs::canonicalize(&path).await {
            if !existing.starts_with(&root) {
                return Err(CommandError::authz(
                    "managed worktree path escaped storage root",
                ));
            }
        }
        Ok(path)
    }

    pub async fn assign(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<()> {
        Self::validate_ids(team_id, agent_id)?;
        self.prepare_project(project_root).await?;
        let key = Self::key(project_root.as_path(), team_id, agent_id);
        let _assignment = self.assignment_lock.lock().await;
        if self.state.lock().await.assignments.contains_key(&key) {
            return Err(CommandError::coded(
                "worktree_already_assigned",
                "this team member already has a worktree",
            ));
        }
        let path = self.managed_path(&key).await?;
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            return Err(CommandError::coded(
                "worktree_path_occupied",
                "the managed worktree location is already occupied",
            ));
        }
        self.create_assignment(project_root, key, path).await
    }

    /// Team PTY spawn 用。並行 spawn でも assignment creation を 1 回に直列化する。
    pub async fn ensure_assigned(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<()> {
        Self::validate_ids(team_id, agent_id)?;
        self.prepare_project(project_root).await?;
        let key = Self::key(project_root.as_path(), team_id, agent_id);
        let _assignment = self.assignment_lock.lock().await;
        let existing = { self.state.lock().await.assignments.get(&key).cloned() };
        if let Some(record) = existing {
            if record.path.is_dir() {
                return git_ops::ensure_worktree(&record.path).await;
            }
            // worker が managed directory を削除しても assignment と branch は正本に残る。
            // missing registration を prune し、同じ branch を再 attach して復旧する。
            let expected_path = self.managed_path(&key).await?;
            if record.path != expected_path {
                return Err(CommandError::authz(
                    "stored worktree path does not match the managed assignment path",
                ));
            }
            git_ops::prune_worktrees(project_root.as_path()).await?;
            git_ops::add_existing_worktree(
                project_root.as_path(),
                &record.path,
                &record.branch_name,
            )
            .await?;
            return git_ops::ensure_worktree(&record.path).await;
        }
        let path = self.managed_path(&key).await?;
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            if self
                .adopt_managed_assignment(project_root, key.clone(), path.clone())
                .await?
            {
                return Ok(());
            }
            return Err(CommandError::coded(
                "worktree_path_occupied",
                "the managed worktree location is occupied by an unrecognized worktree",
            ));
        }
        self.create_assignment(project_root, key, path).await
    }

    async fn create_assignment(
        &self,
        project_root: &ProjectRoot,
        key: AssignmentKey,
        path: PathBuf,
    ) -> CommandResult<()> {
        let team_id = &key.team_id;
        let agent_id = &key.agent_id;
        let base_branch = git_ops::current_branch(project_root.as_path()).await?;
        let base_commit = git_ops::rev_parse(project_root.as_path(), "HEAD").await?;
        let branch_name = format!(
            "vibe/{team_id}/{agent_id}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        git_ops::add_worktree(project_root.as_path(), &path, &branch_name, &base_commit).await?;
        let canonical_path = tokio::fs::canonicalize(&path).await?;
        let canonical_storage = tokio::fs::canonicalize(&self.storage_root).await?;
        if !canonical_path.starts_with(&canonical_storage) {
            let _ = git_ops::remove_worktree(project_root.as_path(), &path).await;
            return Err(CommandError::authz("created worktree escaped storage root"));
        }
        let record = AssignmentRecord {
            key: key.clone(),
            project_root: project_root.as_path().to_path_buf(),
            path: canonical_path,
            branch_name,
            base_branch,
            base_commit,
            integrated_candidate_id: None,
        };
        self.state.lock().await.assignments.insert(key, record);
        self.persist().await
    }

    /// Renderer に path を返さず、spawn 境界だけへ managed cwd + filesystem identity を渡す。
    pub async fn spawn_target(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<(
        String,
        crate::commands::project_authority::ProjectRootIdentity,
    )> {
        let record = self
            .assignment(project_root.as_path(), team_id, agent_id)
            .await?;
        let canonical = tokio::fs::canonicalize(&record.path).await?;
        let storage = tokio::fs::canonicalize(&self.storage_root).await?;
        if canonical != record.path || !canonical.starts_with(storage) {
            return Err(CommandError::authz(
                "managed worktree identity is no longer valid",
            ));
        }
        let identity = crate::commands::project_authority::capture_identity(&canonical).await?;
        Ok((crate::pty::path_norm::display_path(&canonical), identity))
    }

    pub async fn resume(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<()> {
        Self::validate_ids(team_id, agent_id)?;
        self.prepare_project(project_root).await?;
        let key = Self::key(project_root.as_path(), team_id, agent_id);
        let record = self
            .state
            .lock()
            .await
            .assignments
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                CommandError::not_found("no Rust-owned worktree assignment exists for this member")
            })?;
        git_ops::ensure_worktree(&record.path).await
    }

    async fn assignment(
        &self,
        project_root: &Path,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<AssignmentRecord> {
        self.ensure_loaded().await?;
        let key = Self::key(project_root, team_id, agent_id);
        self.state
            .lock()
            .await
            .assignments
            .get(&key)
            .cloned()
            .ok_or_else(|| CommandError::not_found("no worktree is assigned to this team member"))
    }

    pub async fn enqueue(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
        evidence: String,
    ) -> CommandResult<String> {
        Self::validate_ids(team_id, agent_id)?;
        self.prepare_project(project_root).await?;
        crate::commands::validation::assert_max_size(evidence.len(), MAX_EVIDENCE_BYTES)?;
        let assignment = self
            .assignment(project_root.as_path(), team_id, agent_id)
            .await?;
        if !git_ops::is_clean(&assignment.path).await? {
            return Err(CommandError::coded(
                "worktree_dirty",
                "commit or discard worktree changes before collecting a merge candidate",
            ));
        }
        let commit = git_ops::rev_parse(&assignment.path, "HEAD").await?;
        git_ops::ensure_descendant(&assignment.path, &assignment.base_commit, &commit).await?;
        let changed_paths =
            git_ops::changed_paths(&assignment.path, &assignment.base_commit, &commit).await?;
        if changed_paths.is_empty() {
            return Err(CommandError::coded(
                "candidate_empty",
                "the worktree has no committed changes from its base commit",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut state = self.state.lock().await;
        state.next_queue_position += 1;
        let queue_position = state.next_queue_position;
        state.candidates.push(MergeCandidateSnapshot {
            project_key: assignment.key.project_key.clone(),
            id: id.clone(),
            team_id: team_id.to_string(),
            agent_id: agent_id.to_string(),
            commit,
            evidence,
            base_commit: assignment.base_commit,
            changed_paths,
            queue_position,
            status: MergeCandidateStatus::PendingReview,
            conflict: None,
        });
        drop(state);
        self.persist().await?;
        Ok(id)
    }

    pub async fn candidate_owner(
        &self,
        project_root: &ProjectRoot,
        candidate_id: &str,
    ) -> CommandResult<(String, String)> {
        self.prepare_project(project_root).await?;
        let project_key = Self::project_key(project_root.as_path());
        let state = self.state.lock().await;
        state
            .candidates
            .iter()
            .find(|candidate| candidate.id == candidate_id && candidate.project_key == project_key)
            .map(|candidate| (candidate.team_id.clone(), candidate.agent_id.clone()))
            .ok_or_else(|| CommandError::not_found("merge candidate was not found"))
    }

    pub async fn review(&self, candidate_id: &str, approve: bool) -> CommandResult<()> {
        self.ensure_loaded().await?;
        let mut state = self.state.lock().await;
        let candidate = state
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id == candidate_id)
            .ok_or_else(|| CommandError::not_found("merge candidate was not found"))?;
        if !matches!(
            candidate.status,
            MergeCandidateStatus::PendingReview
                | MergeCandidateStatus::ChangesRequested
                | MergeCandidateStatus::Conflict
        ) {
            return Err(CommandError::coded(
                "candidate_not_reviewable",
                "merge candidate cannot be reviewed in its current state",
            ));
        }
        candidate.status = if approve {
            MergeCandidateStatus::Approved
        } else {
            MergeCandidateStatus::ChangesRequested
        };
        candidate.conflict = None;
        drop(state);
        self.persist().await
    }

    pub async fn cancel(&self, candidate_id: &str) -> CommandResult<()> {
        self.ensure_loaded().await?;
        let mut state = self.state.lock().await;
        let candidate = state
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id == candidate_id)
            .ok_or_else(|| CommandError::not_found("merge candidate was not found"))?;
        if matches!(
            candidate.status,
            MergeCandidateStatus::Integrating
                | MergeCandidateStatus::Integrated
                | MergeCandidateStatus::Cancelled
        ) {
            return Err(CommandError::coded(
                "candidate_not_cancellable",
                "merge candidate cannot be cancelled in its current state",
            ));
        }
        candidate.status = MergeCandidateStatus::Cancelled;
        candidate.conflict = None;
        drop(state);
        self.persist().await
    }
}

impl Default for WorktreeManager {
    fn default() -> Self {
        Self::new()
    }
}
