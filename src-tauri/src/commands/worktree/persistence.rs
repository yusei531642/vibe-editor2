use super::{
    AssignmentKey, AssignmentRecord, ManagerState, MergeCandidateSnapshot, MergeCandidateStatus,
    WorktreeManager,
};
use crate::commands::authz::ProjectRoot;
use crate::commands::error::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const STATE_VERSION: u32 = 1;
const STATE_FILE: &str = "assignments.json";

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedManagerState {
    version: u32,
    assignments: Vec<AssignmentRecord>,
    candidates: Vec<PersistedCandidate>,
    next_queue_position: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCandidate {
    project_key: String,
    id: String,
    team_id: String,
    agent_id: String,
    commit: String,
    evidence: String,
    base_commit: String,
    changed_paths: Vec<String>,
    queue_position: u64,
    status: MergeCandidateStatus,
    conflict: Option<super::MergeConflictSnapshot>,
}

impl From<&MergeCandidateSnapshot> for PersistedCandidate {
    fn from(value: &MergeCandidateSnapshot) -> Self {
        Self {
            project_key: value.project_key.clone(),
            id: value.id.clone(),
            team_id: value.team_id.clone(),
            agent_id: value.agent_id.clone(),
            commit: value.commit.clone(),
            evidence: value.evidence.clone(),
            base_commit: value.base_commit.clone(),
            changed_paths: value.changed_paths.clone(),
            queue_position: value.queue_position,
            status: value.status.clone(),
            conflict: value.conflict.clone(),
        }
    }
}

impl From<PersistedCandidate> for MergeCandidateSnapshot {
    fn from(value: PersistedCandidate) -> Self {
        Self {
            project_key: value.project_key,
            id: value.id,
            team_id: value.team_id,
            agent_id: value.agent_id,
            commit: value.commit,
            evidence: value.evidence,
            base_commit: value.base_commit,
            changed_paths: value.changed_paths,
            queue_position: value.queue_position,
            status: if value.status == MergeCandidateStatus::Integrating {
                // A process cannot still own the integration mutex after restart.
                MergeCandidateStatus::Failed
            } else {
                value.status
            },
            conflict: value.conflict,
        }
    }
}

impl WorktreeManager {
    fn state_path(&self) -> PathBuf {
        self.storage_root.join(STATE_FILE)
    }

    pub(super) async fn ensure_loaded(&self) -> CommandResult<()> {
        let mut loaded = self.loaded.lock().await;
        if *loaded {
            return Ok(());
        }
        let loaded_state = match crate::commands::safe_load::safe_load_or_quarantine::<
            PersistedManagerState,
        >(&self.state_path(), Some(0o600))
        .await
        {
            crate::commands::safe_load::LoadOutcome::Loaded(value)
                if value.version == STATE_VERSION =>
            {
                value
            }
            crate::commands::safe_load::LoadOutcome::Loaded(value) => {
                tracing::warn!(
                    version = value.version,
                    "[worktree] unsupported assignment state version; starting empty"
                );
                PersistedManagerState::default()
            }
            crate::commands::safe_load::LoadOutcome::Absent
            | crate::commands::safe_load::LoadOutcome::Corrupted => {
                PersistedManagerState::default()
            }
        };
        let assignments = loaded_state
            .assignments
            .into_iter()
            .map(|record| (record.key.clone(), record))
            .collect::<HashMap<_, _>>();
        let candidates = loaded_state
            .candidates
            .into_iter()
            .map(MergeCandidateSnapshot::from)
            .collect();
        *self.state.lock().await = ManagerState {
            assignments,
            candidates,
            next_queue_position: loaded_state.next_queue_position,
        };
        *loaded = true;
        Ok(())
    }

    pub(super) async fn persist(&self) -> CommandResult<()> {
        let _write = self.persistence_lock.lock().await;
        let persisted = {
            let state = self.state.lock().await;
            PersistedManagerState {
                version: STATE_VERSION,
                assignments: state.assignments.values().cloned().collect(),
                candidates: state
                    .candidates
                    .iter()
                    .map(PersistedCandidate::from)
                    .collect(),
                next_queue_position: state.next_queue_position,
            }
        };
        let bytes = serde_json::to_vec_pretty(&persisted)?;
        crate::commands::atomic_write::atomic_write_with_mode(
            &self.state_path(),
            &bytes,
            Some(0o600),
        )
        .await
        .map_err(CommandError::from)
    }

    pub(super) async fn prepare_project(&self, project_root: &ProjectRoot) -> CommandResult<()> {
        self.ensure_loaded().await?;
        super::git_ops::ensure_supported_version(project_root.as_path()).await?;
        let project_key = Self::project_key(project_root.as_path());
        if self.reconciled_projects.lock().await.contains(&project_key) {
            return Ok(());
        }
        let _assignment = self.assignment_lock.lock().await;
        if self.reconciled_projects.lock().await.contains(&project_key) {
            return Ok(());
        }
        // Requiring a named branch here keeps explicit IPC operations strict. PTY wiring probes
        // repository support before reaching this method and falls back for detached HEAD.
        let base_branch = super::git_ops::current_branch(project_root.as_path()).await?;
        let current_base = super::git_ops::rev_parse(project_root.as_path(), "HEAD").await?;
        tokio::fs::create_dir_all(&self.storage_root).await?;
        let storage = tokio::fs::canonicalize(&self.storage_root).await?;
        let registered = super::git_ops::list_worktree_metadata(project_root.as_path()).await?;
        let records = {
            let state = self.state.lock().await;
            state
                .assignments
                .values()
                .filter(|record| record.key.project_key == project_key)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut valid = HashMap::new();
        for mut record in records {
            if !record_matches_project(&record, project_root.as_path(), &storage).await {
                tracing::warn!(
                    team_id = %record.key.team_id,
                    agent_id = %record.key.agent_id,
                    "[worktree] dropping unsafe or stale persisted assignment"
                );
                continue;
            }
            let Some(metadata) = registered
                .iter()
                .find(|metadata| metadata.path == record.path)
            else {
                tracing::warn!(
                    team_id = %record.key.team_id,
                    agent_id = %record.key.agent_id,
                    "[worktree] persisted assignment is not registered by git; dropping it"
                );
                continue;
            };
            let expected_prefix = format!(
                "vibe/{}/{agent}-",
                record.key.team_id,
                agent = record.key.agent_id
            );
            if !metadata.branch.starts_with(&expected_prefix) {
                tracing::warn!(branch = %metadata.branch, "[worktree] refusing mismatched managed branch");
                continue;
            }
            record.project_root = project_root.as_path().to_path_buf();
            record.path = metadata.path.clone();
            record.branch_name = metadata.branch.clone();
            valid.insert(record.key.clone(), record);
        }
        let project_storage = storage.join(&project_key);
        for metadata in registered {
            let Some((team_id, agent_id)) = managed_owner(&metadata.path, &project_storage) else {
                continue;
            };
            let key = Self::key(project_root.as_path(), &team_id, &agent_id);
            if valid.contains_key(&key) {
                continue;
            }
            let expected_prefix = format!("vibe/{team_id}/{agent_id}-");
            if !metadata.branch.starts_with(&expected_prefix) {
                continue;
            }
            let base_commit =
                super::git_ops::merge_base(project_root.as_path(), &current_base, &metadata.head)
                    .await?;
            tracing::info!(
                team_id,
                agent_id,
                branch = %metadata.branch,
                "[worktree] adopting registered managed worktree missing from persisted state"
            );
            valid.insert(
                key.clone(),
                AssignmentRecord {
                    key,
                    project_root: project_root.as_path().to_path_buf(),
                    path: metadata.path,
                    branch_name: metadata.branch,
                    base_branch: base_branch.clone(),
                    base_commit,
                    integrated_candidate_id: None,
                },
            );
        }
        {
            let mut state = self.state.lock().await;
            state
                .assignments
                .retain(|key, _| key.project_key != project_key);
            state.assignments.extend(valid);
        }
        self.detail_cache
            .lock()
            .await
            .retain(|key, _| key.project_key != project_key);
        self.reconciled_projects.lock().await.insert(project_key);
        self.persist().await
    }

    pub(super) async fn adopt_managed_assignment(
        &self,
        project_root: &ProjectRoot,
        key: AssignmentKey,
        path: PathBuf,
    ) -> CommandResult<bool> {
        let Some(metadata) =
            super::git_ops::managed_worktree_metadata(project_root.as_path(), &path).await?
        else {
            return Ok(false);
        };
        let expected_prefix = format!("vibe/{}/{agent}-", key.team_id, agent = key.agent_id);
        if !metadata.branch.starts_with(&expected_prefix) {
            return Ok(false);
        }
        let base_branch = super::git_ops::current_branch(project_root.as_path()).await?;
        let current_base = super::git_ops::rev_parse(project_root.as_path(), "HEAD").await?;
        let base_commit =
            super::git_ops::merge_base(project_root.as_path(), &current_base, &metadata.head)
                .await?;
        self.state.lock().await.assignments.insert(
            key.clone(),
            AssignmentRecord {
                key,
                project_root: project_root.as_path().to_path_buf(),
                path: metadata.path,
                branch_name: metadata.branch,
                base_branch,
                base_commit,
                integrated_candidate_id: None,
            },
        );
        self.persist().await?;
        Ok(true)
    }

    pub async fn optional_spawn_target(
        &self,
        project_root: &ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<
        Option<(
            String,
            crate::commands::project_authority::ProjectRootIdentity,
        )>,
    > {
        if !super::git_ops::supports_worktree_project(project_root.as_path()).await? {
            return Ok(None);
        }
        self.ensure_assigned(project_root, team_id, agent_id)
            .await?;
        self.spawn_target(project_root, team_id, agent_id)
            .await
            .map(Some)
    }
}

async fn record_matches_project(
    record: &AssignmentRecord,
    project_root: &Path,
    storage_root: &Path,
) -> bool {
    if super::WorktreeManager::validate_ids(&record.key.team_id, &record.key.agent_id).is_err() {
        return false;
    }
    let Ok(record_project) = tokio::fs::canonicalize(&record.project_root).await else {
        return false;
    };
    let Ok(record_path) = tokio::fs::canonicalize(&record.path).await else {
        return false;
    };
    record_project == project_root && record_path.starts_with(storage_root)
}

fn managed_owner(path: &Path, project_storage: &Path) -> Option<(String, String)> {
    let relative = path.strip_prefix(project_storage).ok()?;
    let mut components = relative.components();
    let team_id = components.next()?.as_os_str().to_str()?.to_string();
    let agent_id = components.next()?.as_os_str().to_str()?.to_string();
    if components.next().is_some() || WorktreeManager::validate_ids(&team_id, &agent_id).is_err() {
        return None;
    }
    Some((team_id, agent_id))
}
