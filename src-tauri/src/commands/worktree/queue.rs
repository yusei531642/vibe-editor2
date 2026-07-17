use super::{
    AssignmentRecord, CachedAssignmentDetails, MergeCandidateSnapshot, MergeCandidateStatus,
    MergeConflictSnapshot, WorktreeAssignmentSnapshot, WorktreeManager, WorktreeManagerSnapshot,
};
use crate::commands::error::{CommandError, CommandResult};
use std::path::Path;
use std::time::{Duration, Instant};

const DETAIL_CACHE_TTL: Duration = Duration::from_secs(15);

impl WorktreeManager {
    pub async fn snapshot(
        &self,
        project_root: &crate::commands::authz::ProjectRoot,
        team_id: &str,
    ) -> CommandResult<WorktreeManagerSnapshot> {
        // 非 git / detached HEAD / git 不在では毎 poll をエラーにせず「未対応」snapshot を返す
        // (PR #37 レビュー: terminal 側 fallback と同じ環境判定)。
        match super::git_ops::supports_worktree_project(project_root.as_path()).await {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                return Ok(WorktreeManagerSnapshot {
                    team_id: team_id.to_string(),
                    supported: false,
                    assignments: Vec::new(),
                    candidates: Vec::new(),
                    review_required: true,
                    integration_in_progress: false,
                });
            }
        }
        self.prepare_project(project_root).await?;
        let project_key = Self::project_key(project_root.as_path());
        let (assignments, mut candidates) = {
            let state = self.state.lock().await;
            let assignments = state
                .assignments
                .values()
                .filter(|record| {
                    record.key.project_key == project_key && record.key.team_id == team_id
                })
                .cloned()
                .collect::<Vec<_>>();
            let candidates = state
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.project_key == project_key && candidate.team_id == team_id
                })
                .cloned()
                .collect::<Vec<_>>();
            (assignments, candidates)
        };
        let mut views = Vec::with_capacity(assignments.len());
        for record in assignments {
            let cached = self.detail_cache.lock().await.get(&record.key).cloned();
            let details = if let Some(details) =
                cached.filter(|item| item.captured_at.elapsed() < DETAIL_CACHE_TTL)
            {
                details
            } else {
                let details = CachedAssignmentDetails {
                    captured_at: Instant::now(),
                    clean: super::git_ops::is_clean(&record.path)
                        .await
                        .unwrap_or(false),
                    head_commit: super::git_ops::rev_parse(&record.path, "HEAD")
                        .await
                        .unwrap_or_default(),
                };
                self.detail_cache
                    .lock()
                    .await
                    .insert(record.key.clone(), details.clone());
                details
            };
            let cleanup_eligible = details.clean
                && record.integrated_candidate_id.as_ref().is_some_and(|id| {
                    candidates.iter().any(|candidate| {
                        candidate.id == *id
                            && candidate.commit == details.head_commit
                            && candidate.status == MergeCandidateStatus::Integrated
                    })
                });
            views.push(WorktreeAssignmentSnapshot {
                team_id: record.key.team_id,
                agent_id: record.key.agent_id,
                branch_name: record.branch_name,
                base_branch: record.base_branch,
                base_commit: record.base_commit,
                head_commit: details.head_commit,
                clean: details.clean,
                cleanup_eligible,
            });
        }
        views.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        candidates.sort_by_key(|candidate| candidate.queue_position);
        Ok(WorktreeManagerSnapshot {
            team_id: team_id.to_string(),
            supported: true,
            assignments: views,
            candidates,
            review_required: true,
            integration_in_progress: self.integration_lock.try_lock().is_err(),
        })
    }

    pub async fn integrate(
        &self,
        project_root: &crate::commands::authz::ProjectRoot,
        candidate_id: &str,
    ) -> CommandResult<()> {
        self.prepare_project(project_root).await?;
        self.assert_approved(project_root.as_path(), candidate_id)
            .await?;
        let _integration = self.integration_lock.lock().await;
        let (candidate, assignment) = self
            .begin_integration(project_root.as_path(), candidate_id)
            .await?;
        if let Err(error) = self.persist().await {
            self.mark_failed(candidate_id).await;
            let _ = self.persist().await;
            return Err(error);
        }
        let result = self
            .integrate_locked(project_root.as_path(), &assignment, &candidate)
            .await;
        self.finish_integration(candidate_id, &assignment, result)
            .await
    }

    async fn assert_approved(&self, project_root: &Path, candidate_id: &str) -> CommandResult<()> {
        let state = self.state.lock().await;
        let candidate = state
            .candidates
            .iter()
            .find(|candidate| {
                candidate.id == candidate_id
                    && candidate.project_key == Self::project_key(project_root)
            })
            .ok_or_else(|| CommandError::not_found("merge candidate was not found"))?;
        if candidate.status != MergeCandidateStatus::Approved {
            return Err(CommandError::coded(
                "candidate_review_required",
                "merge candidate must be approved before integration",
            ));
        }
        Ok(())
    }

    async fn begin_integration(
        &self,
        project_root: &Path,
        candidate_id: &str,
    ) -> CommandResult<(MergeCandidateSnapshot, AssignmentRecord)> {
        let mut state = self.state.lock().await;
        let index = state
            .candidates
            .iter()
            .position(|candidate| {
                candidate.id == candidate_id
                    && candidate.project_key == Self::project_key(project_root)
            })
            .ok_or_else(|| CommandError::not_found("merge candidate was not found"))?;
        if state.candidates[index].status != MergeCandidateStatus::Approved {
            return Err(CommandError::coded(
                "candidate_review_required",
                "merge candidate must be approved before integration",
            ));
        }
        state.candidates[index].status = MergeCandidateStatus::Integrating;
        let candidate = state.candidates[index].clone();
        let key = Self::key(project_root, &candidate.team_id, &candidate.agent_id);
        let assignment = state.assignments.get(&key).cloned().ok_or_else(|| {
            state.candidates[index].status = MergeCandidateStatus::Failed;
            CommandError::not_found("candidate worktree assignment was not found")
        })?;
        Ok((candidate, assignment))
    }

    async fn mark_failed(&self, candidate_id: &str) {
        if let Some(candidate) = self
            .state
            .lock()
            .await
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id == candidate_id)
        {
            candidate.status = MergeCandidateStatus::Failed;
        }
    }

    async fn finish_integration(
        &self,
        candidate_id: &str,
        assignment: &AssignmentRecord,
        result: Result<(), IntegrationFailure>,
    ) -> CommandResult<()> {
        let response = {
            let mut state = self.state.lock().await;
            let Some(index) = state
                .candidates
                .iter()
                .position(|candidate| candidate.id == candidate_id)
            else {
                return Err(CommandError::internal(
                    "merge candidate disappeared while integration was in progress",
                ));
            };
            match result {
                Ok(()) => {
                    state.candidates[index].status = MergeCandidateStatus::Integrated;
                    state.candidates[index].conflict = None;
                    if let Some(record) = state.assignments.get_mut(&assignment.key) {
                        record.integrated_candidate_id = Some(candidate_id.to_string());
                    }
                    Ok(())
                }
                Err(IntegrationFailure::Conflict(conflict)) => {
                    state.candidates[index].status = MergeCandidateStatus::Conflict;
                    state.candidates[index].conflict = Some(conflict);
                    Err(CommandError::coded(
                        "merge_conflict",
                        "candidate conflicts with the updated base",
                    ))
                }
                Err(IntegrationFailure::Command(error)) => {
                    state.candidates[index].status = MergeCandidateStatus::Failed;
                    Err(error)
                }
            }
        };
        self.detail_cache.lock().await.remove(&assignment.key);
        self.persist().await?;
        response
    }

    async fn integrate_locked(
        &self,
        project_root: &Path,
        assignment: &AssignmentRecord,
        candidate: &MergeCandidateSnapshot,
    ) -> Result<(), IntegrationFailure> {
        if !super::git_ops::is_clean(project_root)
            .await
            .map_err(IntegrationFailure::Command)?
        {
            return Err(IntegrationFailure::Command(CommandError::coded(
                "base_worktree_dirty",
                "the base working tree must be clean before integration; untracked files count as dirty",
            )));
        }
        let current_branch = super::git_ops::current_branch(project_root)
            .await
            .map_err(IntegrationFailure::Command)?;
        if current_branch != assignment.base_branch {
            return Err(IntegrationFailure::Command(CommandError::coded(
                "base_branch_changed",
                "the base repository is no longer on the recorded branch",
            )));
        }
        let current_base = super::git_ops::rev_parse(project_root, "HEAD")
            .await
            .map_err(IntegrationFailure::Command)?;
        if let Some(paths) =
            super::git_ops::conflict_check(project_root, &current_base, &candidate.commit)
                .await
                .map_err(IntegrationFailure::Command)?
        {
            return Err(IntegrationFailure::Conflict(MergeConflictSnapshot {
                paths,
                base_commit: current_base,
                candidate_commit: candidate.commit.clone(),
            }));
        }
        super::git_ops::merge(project_root, &candidate.commit)
            .await
            .map_err(IntegrationFailure::Command)
    }

    pub async fn cleanup(
        &self,
        project_root: &crate::commands::authz::ProjectRoot,
        team_id: &str,
        agent_id: &str,
    ) -> CommandResult<()> {
        self.prepare_project(project_root).await?;
        let assignment = self
            .assignment(project_root.as_path(), team_id, agent_id)
            .await?;
        let clean = super::git_ops::is_clean(&assignment.path).await?;
        let head = super::git_ops::rev_parse(&assignment.path, "HEAD").await?;
        let integrated = {
            let state = self.state.lock().await;
            assignment
                .integrated_candidate_id
                .as_ref()
                .is_some_and(|id| {
                    state.candidates.iter().any(|candidate| {
                        candidate.id == *id
                            && candidate.commit == head
                            && candidate.status == MergeCandidateStatus::Integrated
                    })
                })
        };
        if !clean || !integrated {
            return Err(CommandError::coded(
                "worktree_not_cleanup_eligible",
                "only integrated, clean worktrees can be cleaned up",
            ));
        }
        super::git_ops::remove_worktree(&assignment.project_root, &assignment.path).await?;
        self.state.lock().await.assignments.remove(&assignment.key);
        self.detail_cache.lock().await.remove(&assignment.key);
        self.persist().await?;
        if let Err(error) =
            super::git_ops::delete_branch(&assignment.project_root, &assignment.branch_name).await
        {
            tracing::warn!(
                branch = %assignment.branch_name,
                "[worktree] worktree removed but branch cleanup failed: {error}"
            );
        }
        Ok(())
    }
}

enum IntegrationFailure {
    Conflict(MergeConflictSnapshot),
    Command(CommandError),
}
