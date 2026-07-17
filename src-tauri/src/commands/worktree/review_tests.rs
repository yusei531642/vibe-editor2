use super::tests::{write, GitFixture};
use super::WorktreeManager;

/// PR #37 三次レビュー 🟡: `git worktree list` はディレクトリが消えた prunable
/// worktree も列挙し続ける。ENOENT を list 全体のエラーにすると、以降の reconcile /
/// assign がプロジェクト全体で恒久失敗するため、該当エントリのみ skip する。
#[tokio::test]
async fn deleted_worktree_directory_does_not_poison_listing_or_new_assignments() {
    let fixture = GitFixture::new();
    let doomed = fixture.assign("worker-1").await;
    fixture.commit_file(&doomed, "survives.txt", "preserved branch content\n");
    tokio::fs::remove_dir_all(&doomed).await.expect("simulate worker deleting its worktree");

    let listed = super::git_ops::list_worktree_metadata(fixture.project.as_path())
        .await
        .expect("prunable エントリは skip して list は成功する");
    assert!(!listed.iter().any(|metadata| metadata.path == doomed));

    fixture
        .manager
        .ensure_assigned(&fixture.project, "team-1", "worker-1")
        .await
        .expect("missing registration を prune して同じ branch を再 attach する");
    assert_eq!(
        std::fs::read_to_string(doomed.join("survives.txt")).unwrap(),
        "preserved branch content\n"
    );

    // 消えた worktree が居ても新規 member の割当は成立する
    let fresh = fixture.assign("worker-2").await;
    assert!(fresh.exists());
}

#[tokio::test]
async fn base_dirty_error_explicitly_mentions_untracked_files() {
    let fixture = GitFixture::new();
    let worktree = fixture.assign("worker-1").await;
    fixture.commit_file(&worktree, "candidate.txt", "candidate\n");
    let candidate_id = fixture
        .manager
        .enqueue(&fixture.project, "team-1", "worker-1", "green".into())
        .await
        .unwrap();
    fixture.manager.review(&candidate_id, true).await.unwrap();
    write(&fixture.root.join("untracked.txt"), "dirty\n");
    let error = fixture
        .manager
        .integrate(&fixture.project, &candidate_id)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "base_worktree_dirty");
    assert!(error.to_string().contains("untracked files count as dirty"));
}

#[tokio::test]
async fn branch_delete_failure_after_worktree_removal_does_not_stick_assignment() {
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
    let key = WorktreeManager::key(fixture.project.as_path(), "team-1", "worker-1");
    fixture
        .manager
        .state
        .lock()
        .await
        .assignments
        .get_mut(&key)
        .unwrap()
        .branch_name = "vibe/team-1/missing-branch".into();
    fixture
        .manager
        .cleanup(&fixture.project, "team-1", "worker-1")
        .await
        .expect("branch cleanup failure is warning-only");
    assert!(fixture
        .manager
        .assignment(fixture.project.as_path(), "team-1", "worker-1")
        .await
        .is_err());
}
