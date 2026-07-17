use super::*;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

pub(super) struct GitFixture {
    _temp: TempDir,
    pub(super) root: PathBuf,
    pub(super) project: ProjectRoot,
    pub(super) manager: Arc<WorktreeManager>,
}

pub(super) fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git starts");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(super) fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("fixture write");
}

impl GitFixture {
    pub(super) fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).expect("repo dir");
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "fixture@example.test"]);
        git(&root, &["config", "user.name", "Fixture"]);
        git(&root, &["branch", "-M", "main"]);
        write(&root.join("shared.txt"), "base\n");
        git(&root, &["add", "shared.txt"]);
        git(&root, &["commit", "-m", "base"]);
        let canonical = std::fs::canonicalize(&root).expect("canonical repo");
        let project = ProjectRoot::assume_canonical_for_test(canonical);
        let manager = Arc::new(WorktreeManager::with_storage_root(
            temp.path().join("managed-worktrees"),
        ));
        Self {
            _temp: temp,
            root,
            project,
            manager,
        }
    }

    pub(super) async fn assign(&self, agent_id: &str) -> PathBuf {
        self.manager
            .assign(&self.project, "team-1", agent_id)
            .await
            .expect("assign worktree");
        self.manager
            .assignment(self.project.as_path(), "team-1", agent_id)
            .await
            .expect("assignment record")
            .path
    }

    pub(super) fn commit_file(&self, worktree: &Path, path: &str, contents: &str) {
        write(&worktree.join(path), contents);
        git(worktree, &["add", path]);
        git(worktree, &["commit", "-m", &format!("change {path}")]);
    }
}

#[tokio::test]
async fn assignments_are_unique_and_leave_base_worktree_clean() {
    let fixture = GitFixture::new();
    let first = fixture.assign("worker-1").await;
    let second = fixture.assign("worker-2").await;
    assert_ne!(first, second);
    let storage = std::fs::canonicalize(&fixture.manager.storage_root).unwrap();
    assert!(first.starts_with(&storage));
    assert!(second.starts_with(&storage));
    let error = fixture
        .manager
        .assign(&fixture.project, "team-1", "worker-1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "worktree_already_assigned");
    assert!(git(&fixture.root, &["status", "--porcelain"]).is_empty());
}

#[tokio::test]
async fn concurrent_spawn_assignment_is_idempotent_for_the_same_member() {
    let fixture = GitFixture::new();
    let first_manager = fixture.manager.clone();
    let first_project = fixture.project.clone();
    let first = tokio::spawn(async move {
        first_manager
            .ensure_assigned(&first_project, "team-1", "worker-1")
            .await
    });
    let second_manager = fixture.manager.clone();
    let second_project = fixture.project.clone();
    let second = tokio::spawn(async move {
        second_manager
            .ensure_assigned(&second_project, "team-1", "worker-1")
            .await
    });
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    assert_eq!(snapshot.assignments.len(), 1);
    let (cwd, identity) = fixture
        .manager
        .spawn_target(&fixture.project, "team-1", "worker-1")
        .await
        .unwrap();
    assert_eq!(identity.canonical_root, cwd);
    assert!(git(&fixture.root, &["status", "--porcelain"]).is_empty());
}

#[tokio::test]
async fn repeated_snapshots_reuse_short_lived_git_details() {
    let fixture = GitFixture::new();
    fixture.assign("worker-1").await;
    fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    let first_capture = fixture
        .manager
        .detail_cache
        .lock()
        .await
        .values()
        .next()
        .unwrap()
        .captured_at;
    fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    let second_capture = fixture
        .manager
        .detail_cache
        .lock()
        .await
        .values()
        .next()
        .unwrap()
        .captured_at;
    assert_eq!(first_capture, second_capture);
}

#[tokio::test]
async fn concurrent_enqueue_and_integrate_are_serialized() {
    let fixture = GitFixture::new();
    let first_path = fixture.assign("worker-1").await;
    let second_path = fixture.assign("worker-2").await;
    fixture.commit_file(&first_path, "first.txt", "one\n");
    fixture.commit_file(&second_path, "second.txt", "two\n");

    let first_manager = fixture.manager.clone();
    let first_project = fixture.project.clone();
    let first_enqueue = tokio::spawn(async move {
        first_manager
            .enqueue(&first_project, "team-1", "worker-1", "tests pass".into())
            .await
            .unwrap()
    });
    let second_manager = fixture.manager.clone();
    let second_project = fixture.project.clone();
    let second_enqueue = tokio::spawn(async move {
        second_manager
            .enqueue(&second_project, "team-1", "worker-2", "tests pass".into())
            .await
            .unwrap()
    });
    let first_id = first_enqueue.await.unwrap();
    let second_id = second_enqueue.await.unwrap();
    fixture.manager.review(&first_id, true).await.unwrap();
    fixture.manager.review(&second_id, true).await.unwrap();

    let first_manager = fixture.manager.clone();
    let first_project = fixture.project.clone();
    let first_id_for_task = first_id.clone();
    let first_integrate = tokio::spawn(async move {
        first_manager
            .integrate(&first_project, &first_id_for_task)
            .await
    });
    let second_manager = fixture.manager.clone();
    let second_project = fixture.project.clone();
    let second_id_for_task = second_id.clone();
    let second_integrate = tokio::spawn(async move {
        second_manager
            .integrate(&second_project, &second_id_for_task)
            .await
    });
    first_integrate.await.unwrap().unwrap();
    second_integrate.await.unwrap().unwrap();

    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    assert_eq!(snapshot.candidates.len(), 2);
    assert!(snapshot
        .candidates
        .iter()
        .all(|candidate| candidate.status == MergeCandidateStatus::Integrated));
    assert_ne!(
        snapshot.candidates[0].queue_position,
        snapshot.candidates[1].queue_position
    );
    assert!(fixture.root.join("first.txt").exists());
    assert!(fixture.root.join("second.txt").exists());
    assert!(git(&fixture.root, &["status", "--porcelain"]).is_empty());
}

#[tokio::test]
async fn detects_conflict_against_updated_base_and_records_path() {
    let fixture = GitFixture::new();
    let worktree = fixture.assign("worker-1").await;
    fixture.commit_file(&worktree, "shared.txt", "worker\n");
    let candidate_id = fixture
        .manager
        .enqueue(&fixture.project, "team-1", "worker-1", "verified".into())
        .await
        .unwrap();
    fixture.manager.review(&candidate_id, true).await.unwrap();

    write(&fixture.root.join("shared.txt"), "base moved\n");
    git(&fixture.root, &["add", "shared.txt"]);
    git(&fixture.root, &["commit", "-m", "move base"]);
    let error = fixture
        .manager
        .integrate(&fixture.project, &candidate_id)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "merge_conflict");
    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    let candidate = &snapshot.candidates[0];
    assert_eq!(candidate.status, MergeCandidateStatus::Conflict);
    let conflict = candidate.conflict.as_ref().expect("conflict details");
    assert!(conflict.paths.iter().any(|path| path == "shared.txt"));
    assert_eq!(conflict.candidate_commit, candidate.commit);
    assert!(!conflict.base_commit.is_empty());
}

#[tokio::test]
async fn rejects_integration_without_review() {
    let fixture = GitFixture::new();
    let worktree = fixture.assign("worker-1").await;
    fixture.commit_file(&worktree, "candidate.txt", "candidate\n");
    let candidate_id = fixture
        .manager
        .enqueue(&fixture.project, "team-1", "worker-1", String::new())
        .await
        .unwrap();
    let error = fixture
        .manager
        .integrate(&fixture.project, &candidate_id)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "candidate_review_required");
    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    assert!(snapshot.review_required);
}

#[tokio::test]
async fn cleanup_requires_successful_integration_and_clean_matching_head() {
    let fixture = GitFixture::new();
    let worktree = fixture.assign("worker-1").await;
    fixture.commit_file(&worktree, "candidate.txt", "candidate\n");
    let candidate_id = fixture
        .manager
        .enqueue(&fixture.project, "team-1", "worker-1", "green".into())
        .await
        .unwrap();
    fixture.manager.review(&candidate_id, true).await.unwrap();
    fixture
        .manager
        .integrate(&fixture.project, &candidate_id)
        .await
        .unwrap();
    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    assert!(snapshot.assignments[0].cleanup_eligible);

    write(&worktree.join("candidate.txt"), "dirty\n");
    let error = fixture
        .manager
        .cleanup(&fixture.project, "team-1", "worker-1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "worktree_not_cleanup_eligible");

    git(&worktree, &["reset", "--hard", "HEAD"]);
    fixture
        .manager
        .cleanup(&fixture.project, "team-1", "worker-1")
        .await
        .unwrap();
    assert!(!worktree.exists());
}

#[tokio::test]
async fn rejects_path_traversal_ids_before_managed_path_creation() {
    let fixture = GitFixture::new();
    let error = fixture
        .manager
        .assign(&fixture.project, "../escape", "worker-1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "validation");
    let error = fixture
        .manager
        .assign(&fixture.project, "team-1", "..\\escape")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "validation");
    assert!(!fixture.manager.storage_root.join("escape").exists());
}

#[tokio::test]
async fn restart_restores_assignment_and_spawn_target() {
    let fixture = GitFixture::new();
    let assigned = fixture.assign("worker-1").await;
    let restored = WorktreeManager::with_storage_root(fixture.manager.storage_root.clone());
    restored
        .ensure_assigned(&fixture.project, "team-1", "worker-1")
        .await
        .expect("persisted assignment is reconciled");
    let (cwd, _) = restored
        .spawn_target(&fixture.project, "team-1", "worker-1")
        .await
        .expect("restored spawn target");
    assert_eq!(std::fs::canonicalize(cwd).unwrap(), assigned);
}

#[tokio::test]
async fn restart_adopts_registered_legacy_worktree_without_state_file() {
    let fixture = GitFixture::new();
    let assigned = fixture.assign("worker-1").await;
    std::fs::remove_file(fixture.manager.storage_root.join("assignments.json"))
        .expect("remove persisted state");
    let restored = WorktreeManager::with_storage_root(fixture.manager.storage_root.clone());
    let snapshot = restored
        .snapshot(&fixture.project, "team-1")
        .await
        .expect("startup reconciliation adopts the registered worktree");
    assert_eq!(snapshot.assignments.len(), 1);
    let record = restored
        .assignment(fixture.project.as_path(), "team-1", "worker-1")
        .await
        .unwrap();
    assert_eq!(record.path, assigned);
}

#[tokio::test]
async fn corrupt_assignment_state_is_quarantined_and_fails_open() {
    let fixture = GitFixture::new();
    std::fs::create_dir_all(&fixture.manager.storage_root).unwrap();
    let state_path = fixture.manager.storage_root.join("assignments.json");
    std::fs::write(&state_path, b"{ broken json").unwrap();
    fixture
        .manager
        .ensure_assigned(&fixture.project, "team-1", "worker-1")
        .await
        .expect("corrupt state must not block worktree creation");
    let backups = std::fs::read_dir(&fixture.manager.storage_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("assignments.json.bak.")
        })
        .count();
    assert_eq!(backups, 1);
    serde_json::from_slice::<serde_json::Value>(&std::fs::read(state_path).unwrap())
        .expect("state is replaced with valid JSON");
}

#[tokio::test]
async fn non_git_and_detached_projects_skip_optional_worktree_wiring() {
    let temp = tempfile::tempdir().unwrap();
    let plain_root = std::fs::canonicalize(temp.path()).unwrap();
    let plain_project = ProjectRoot::assume_canonical_for_test(plain_root);
    let manager = WorktreeManager::with_storage_root(temp.path().join("worktrees"));
    assert!(manager
        .optional_spawn_target(&plain_project, "team-1", "worker-1")
        .await
        .unwrap()
        .is_none());

    let fixture = GitFixture::new();
    git(&fixture.root, &["checkout", "--detach"]);
    assert!(fixture
        .manager
        .optional_spawn_target(&fixture.project, "team-1", "worker-1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn leader_can_cancel_candidate_without_owner_membership_dependency() {
    let fixture = GitFixture::new();
    let worktree = fixture.assign("departed-worker").await;
    fixture.commit_file(&worktree, "candidate.txt", "candidate\n");
    let candidate_id = fixture
        .manager
        .enqueue(
            &fixture.project,
            "team-1",
            "departed-worker",
            "verified".into(),
        )
        .await
        .unwrap();
    fixture.manager.cancel(&candidate_id).await.unwrap();
    let snapshot = fixture
        .manager
        .snapshot(&fixture.project, "team-1")
        .await
        .unwrap();
    assert_eq!(
        snapshot.candidates[0].status,
        MergeCandidateStatus::Cancelled
    );
}
