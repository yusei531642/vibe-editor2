use crate::commands::error::{CommandError, CommandResult};
use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

const MIN_GIT_VERSION: (u64, u64) = (2, 38);
static GIT_VERSION: tokio::sync::OnceCell<Result<(u64, u64), String>> =
    tokio::sync::OnceCell::const_new();

struct GitOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn command() -> Command {
    let mut command = Command::new("git");
    command.args([
        "-c",
        "core.fsmonitor=",
        "-c",
        "core.hooksPath=",
        "-c",
        "core.editor=:",
        "-c",
        "core.askpass=:",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "tag.gpgsign=false",
        "-c",
        "gpg.program=:",
        "-c",
        "protocol.version=2",
    ]);
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}

async fn run<I, S>(cwd: &Path, args: I) -> CommandResult<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = command()
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| {
            tracing::error!(%error, "[worktree] failed to spawn git");
            CommandError::coded("git_spawn_failed", "git could not be started")
        })?;
    Ok(GitOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn require(output: GitOutput, code: &str, message: &str) -> CommandResult<String> {
    if output.success {
        return Ok(output.stdout.trim().to_string());
    }
    tracing::warn!(stderr = %output.stderr, "[worktree] git operation failed: {code}");
    Err(CommandError::coded(code, message))
}

fn parse_git_version(raw: &str) -> Option<(u64, u64)> {
    let version = raw
        .trim()
        .strip_prefix("git version ")?
        .split_whitespace()
        .next()?;
    let mut parts = version.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

pub(super) async fn ensure_supported_version(cwd: &Path) -> CommandResult<()> {
    let result = GIT_VERSION
        .get_or_init(|| async {
            let output = run(cwd, ["--version"])
                .await
                .map_err(|error| error.to_string())?;
            if !output.success {
                return Err("git --version failed".to_string());
            }
            parse_git_version(&output.stdout)
                .ok_or_else(|| "git returned an unrecognized version string".to_string())
        })
        .await;
    match result {
        Ok(version) => validate_git_version(*version),
        Err(reason) => Err(CommandError::coded(
            "git_version_unavailable",
            reason.clone(),
        )),
    }
}

fn validate_git_version((major, minor): (u64, u64)) -> CommandResult<()> {
    if (major, minor) >= MIN_GIT_VERSION {
        Ok(())
    } else {
        Err(CommandError::coded(
            "git_version_unsupported",
            format!("git >= 2.38 is required; detected {major}.{minor}"),
        ))
    }
}

/// PTY wiring is optional: non-git projects and detached HEAD keep their plain cwd.
/// snapshot が 3 秒ポーリングで呼ぶため、git サブプロセス 2 本の起動を短期 TTL で
/// キャッシュする (PR #37 レビュー)。git 化 / detached 解消は数十秒での反映で十分。
const SUPPORTS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);
static SUPPORTS_CACHE: std::sync::Mutex<
    Option<(std::path::PathBuf, std::time::Instant, bool)>,
> = std::sync::Mutex::new(None);

pub(super) async fn supports_worktree_project(cwd: &Path) -> CommandResult<bool> {
    {
        let cache = SUPPORTS_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached_path, at, value)) = cache.as_ref() {
            if cached_path == cwd && at.elapsed() < SUPPORTS_CACHE_TTL {
                return Ok(*value);
            }
        }
    }
    let value = supports_worktree_project_uncached(cwd).await?;
    *SUPPORTS_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((cwd.to_path_buf(), std::time::Instant::now(), value));
    Ok(value)
}

async fn supports_worktree_project_uncached(cwd: &Path) -> CommandResult<bool> {
    let repository = run(cwd, ["rev-parse", "--is-inside-work-tree"]).await?;
    if !repository.success || repository.stdout.trim() != "true" {
        return Ok(false);
    }
    let branch = run(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"]).await?;
    if !branch.success || branch.stdout.trim().is_empty() {
        return Ok(false);
    }
    ensure_supported_version(cwd).await?;
    Ok(true)
}

pub(super) async fn current_branch(cwd: &Path) -> CommandResult<String> {
    require(
        run(cwd, ["symbolic-ref", "--short", "HEAD"]).await?,
        "base_branch_unavailable",
        "the base repository must be on a branch",
    )
}

pub(super) async fn rev_parse(cwd: &Path, revision: &str) -> CommandResult<String> {
    require(
        run(cwd, ["rev-parse", "--verify", revision]).await?,
        "git_revision_failed",
        "a required git revision could not be resolved",
    )
}

pub(super) async fn add_worktree(
    cwd: &Path,
    path: &Path,
    branch: &str,
    base: &str,
) -> CommandResult<()> {
    let output = run(
        cwd,
        [
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(branch),
            path.as_os_str(),
            OsStr::new(base),
        ],
    )
    .await?;
    require(
        output,
        "worktree_create_failed",
        "git could not create the managed worktree",
    )
    .map(|_| ())
}

pub(super) async fn remove_worktree(cwd: &Path, path: &Path) -> CommandResult<()> {
    let output = run(
        cwd,
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            path.as_os_str(),
        ],
    )
    .await?;
    require(
        output,
        "worktree_remove_failed",
        "git could not remove the managed worktree",
    )
    .map(|_| ())
}

pub(super) async fn delete_branch(cwd: &Path, branch: &str) -> CommandResult<()> {
    let output = run(cwd, ["branch", "-D", branch]).await?;
    require(
        output,
        "worktree_branch_remove_failed",
        "git could not remove the integrated worktree branch",
    )
    .map(|_| ())
}

pub(super) async fn ensure_worktree(path: &Path) -> CommandResult<()> {
    if !path.is_dir() {
        return Err(CommandError::not_found(
            "the assigned worktree no longer exists",
        ));
    }
    rev_parse(path, "HEAD").await.map(|_| ())
}

pub(super) struct WorktreeMetadata {
    pub path: std::path::PathBuf,
    pub head: String,
    pub branch: String,
}

pub(super) async fn managed_worktree_metadata(
    project_root: &Path,
    expected_path: &Path,
) -> CommandResult<Option<WorktreeMetadata>> {
    let expected = match tokio::fs::canonicalize(expected_path).await {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    Ok(list_worktree_metadata(project_root)
        .await?
        .into_iter()
        .find(|metadata| metadata.path == expected))
}

pub(super) async fn list_worktree_metadata(
    project_root: &Path,
) -> CommandResult<Vec<WorktreeMetadata>> {
    let stdout = require(
        run(project_root, ["worktree", "list", "--porcelain"]).await?,
        "worktree_list_failed",
        "git could not list registered worktrees",
    )?;
    let mut worktrees = Vec::new();
    for block in stdout.split("\n\n") {
        let mut path = None;
        let mut head = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(std::path::PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                branch = Some(value.to_string());
            }
        }
        let Some(raw_path) = path else { continue };
        let Ok(canonical) = tokio::fs::canonicalize(raw_path).await else {
            continue;
        };
        if let (Some(head), Some(branch)) = (head, branch) {
            worktrees.push(WorktreeMetadata {
                path: canonical,
                head,
                branch,
            });
        }
    }
    Ok(worktrees)
}

pub(super) async fn merge_base(cwd: &Path, left: &str, right: &str) -> CommandResult<String> {
    require(
        run(cwd, ["merge-base", left, right]).await?,
        "worktree_base_recovery_failed",
        "git could not recover the managed worktree base commit",
    )
}

pub(super) async fn is_clean(cwd: &Path) -> CommandResult<bool> {
    let output = run(cwd, ["status", "--porcelain=v1", "--untracked-files=all"]).await?;
    require(
        output,
        "worktree_status_failed",
        "git could not inspect worktree status",
    )
    .map(|stdout| stdout.is_empty())
}

pub(super) async fn ensure_descendant(cwd: &Path, base: &str, commit: &str) -> CommandResult<()> {
    let output = run(cwd, ["merge-base", "--is-ancestor", base, commit]).await?;
    if output.success {
        Ok(())
    } else {
        Err(CommandError::coded(
            "candidate_not_descendant",
            "candidate commit is not descended from its recorded base",
        ))
    }
}

pub(super) async fn changed_paths(
    cwd: &Path,
    base: &str,
    commit: &str,
) -> CommandResult<Vec<String>> {
    let output = run(
        cwd,
        [
            "diff",
            "--name-only",
            "-z",
            &format!("{base}..{commit}"),
            "--",
        ],
    )
    .await?;
    let stdout = require(
        output,
        "candidate_diff_failed",
        "git could not collect candidate paths",
    )?;
    Ok(stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

pub(super) async fn conflict_check(
    cwd: &Path,
    base: &str,
    candidate: &str,
) -> CommandResult<Option<Vec<String>>> {
    let output = run(
        cwd,
        ["merge-tree", "--write-tree", "--name-only", base, candidate],
    )
    .await?;
    if output.success {
        return Ok(None);
    }
    let paths = output
        .stdout
        .lines()
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        tracing::warn!(stderr = %output.stderr, "[worktree] merge-tree failed without conflict paths");
        return Err(CommandError::coded(
            "merge_tree_failed",
            "git could not validate the candidate against the updated base",
        ));
    }
    Ok(Some(paths))
}

pub(super) async fn merge(cwd: &Path, candidate: &str) -> CommandResult<()> {
    let output = run(cwd, ["merge", "--no-ff", "--no-edit", candidate]).await?;
    if output.success {
        return Ok(());
    }
    tracing::warn!(stderr = %output.stderr, "[worktree] merge failed after conflict preflight");
    let _ = run(cwd, ["merge", "--abort"]).await;
    Err(CommandError::coded(
        "merge_failed",
        "git could not integrate the reviewed candidate",
    ))
}

#[cfg(test)]
mod version_tests {
    use super::{parse_git_version, validate_git_version};

    #[test]
    fn parses_platform_git_version_suffixes() {
        assert_eq!(parse_git_version("git version 2.38.0\n"), Some((2, 38)));
        assert_eq!(
            parse_git_version("git version 2.45.2.windows.1"),
            Some((2, 45))
        );
        assert_eq!(parse_git_version("unexpected"), None);
    }

    #[test]
    fn rejects_git_older_than_merge_tree_requirement() {
        assert_eq!(
            validate_git_version((2, 37)).unwrap_err().code(),
            "git_version_unsupported"
        );
        validate_git_version((2, 38)).unwrap();
    }
}
