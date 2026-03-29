use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use crate::codex::TurnContext;
use codex_git_utils::get_git_repo_root;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitState {
    pub repo_root: PathBuf,
    pub head_hash: Option<String>,
    pub uncommitted_files: BTreeMap<PathBuf, Option<SystemTime>>,
}

pub(crate) async fn track_tool_side_effects<T, F, Fut>(cwd: &Path, turn: &TurnContext, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let before_state = capture_git_state(cwd).await;
    let result = f().await;
    let after_state = capture_git_state(cwd).await;

    if let (Some(b), Some(a)) = (before_state, after_state) {
        let changed_files = compute_git_side_effects(&b, &a).await;
        if !changed_files.is_empty() {
            let mut guard = turn.side_effects_files.lock().await;
            for file in &changed_files {
                guard.insert(file.to_string_lossy().to_string());
            }
        }
    }
    result
}

pub async fn capture_git_state(cwd: &Path) -> Option<GitState> {
    let repo_root = get_git_repo_root(cwd)?;

    // Get HEAD hash
    let head_output = run_git_command_with_timeout(&["rev-parse", "HEAD"], &repo_root).await;
    let head_hash = head_output.and_then(|out| {
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    });

    // Get uncommitted files
    let status_output =
        run_git_command_with_timeout(&["status", "--porcelain", "-z"], &repo_root).await;
    let mut uncommitted_files = BTreeMap::new();
    if let Some(out) = status_output
        && out.status.success()
    {
        let status_str = String::from_utf8_lossy(&out.stdout);
        let parts: Vec<&str> = status_str.split('\0').collect();
        let mut i = 0;
        while i < parts.len() {
            let part = parts[i];
            if part.is_empty() {
                i += 1;
                continue;
            }
            let status = &part[0..2];
            let path_str = &part[3..];
            let abs_path = repo_root.join(path_str);

            let mtime = std::fs::metadata(&abs_path).and_then(|m| m.modified()).ok();

            uncommitted_files.insert(abs_path, mtime);

            if status.starts_with('R') || status.starts_with('C') {
                i += 1;
            }
            i += 1;
        }
    }

    Some(GitState {
        repo_root,
        head_hash,
        uncommitted_files,
    })
}

pub async fn compute_git_side_effects(before: &GitState, after: &GitState) -> Vec<PathBuf> {
    if before.repo_root != after.repo_root {
        return Vec::new();
    }

    let mut changed_files = BTreeSet::new();

    // 1. Uncommitted files that changed mtime or are new.
    for (path, after_mtime) in &after.uncommitted_files {
        if let Some(before_mtime) = before.uncommitted_files.get(path) {
            if before_mtime != after_mtime {
                changed_files.insert(path.clone());
            }
        } else {
            // It's a new uncommitted file
            changed_files.insert(path.clone());
        }
    }

    // 2. If HEAD changed, diff the two commits.
    if before.head_hash != after.head_hash {
        if let (Some(old_head), Some(new_head)) = (&before.head_hash, &after.head_hash) {
            let diff_out = run_git_command_with_timeout(
                &["diff", "--name-only", "-z", old_head, new_head],
                &before.repo_root,
            )
            .await;
            if let Some(out) = diff_out
                && out.status.success()
            {
                let diff_str = String::from_utf8_lossy(&out.stdout);
                for path_str in diff_str.split('\0') {
                    if !path_str.is_empty() {
                        changed_files.insert(before.repo_root.join(path_str));
                    }
                }
            }
        } else if before.head_hash.is_none()
            && let Some(new_head) = after.head_hash.as_ref()
        {
            // First commit
            let diff_out = run_git_command_with_timeout(
                &["show", "--name-only", "--format=", "-z", new_head],
                &before.repo_root,
            )
            .await;
            if let Some(out) = diff_out
                && out.status.success()
            {
                let diff_str = String::from_utf8_lossy(&out.stdout);
                for path_str in diff_str.split('\0') {
                    if !path_str.is_empty() {
                        changed_files.insert(before.repo_root.join(path_str));
                    }
                }
            }
        }
    }

    changed_files.into_iter().collect()
}

async fn run_git_command_with_timeout(args: &[&str], cwd: &Path) -> Option<std::process::Output> {
    let mut command = Command::new("git");
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .current_dir(cwd)
        .kill_on_drop(true);
    let result = timeout(Duration::from_secs(5), command.output()).await;

    match result {
        Ok(Ok(output)) => Some(output),
        _ => None,
    }
}
