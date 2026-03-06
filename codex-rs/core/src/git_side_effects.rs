use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::git_info::get_git_repo_root;
use crate::git_info::run_git_command_with_timeout;
use crate::protocol::EventMsg;
use crate::protocol::FileSystemMutatedEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitState {
    repo_root: PathBuf,
    head_hash: Option<String>,
    uncommitted_files: BTreeMap<PathBuf, Option<SystemTime>>,
}

pub(crate) async fn track_tool_side_effects<T, F, Fut>(
    cwd: &Path,
    call_id: String,
    session: &Session,
    turn: &TurnContext,
    f: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let before_state = capture_git_state(cwd).await;
    let result = f().await;
    let after_state = capture_git_state(cwd).await;

    if let (Some(before_state), Some(after_state)) = (before_state, after_state) {
        let changed_files = compute_git_side_effects(&before_state, &after_state).await;
        if !changed_files.is_empty() {
            let files = changed_files
                .iter()
                .map(|path| {
                    path.strip_prefix(&before_state.repo_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string()
                })
                .collect::<Vec<_>>();

            {
                let mut guard = turn.side_effects_files.lock().await;
                guard.extend(files.iter().cloned());
            }

            session
                .send_event(
                    turn,
                    EventMsg::FileSystemMutated(FileSystemMutatedEvent { call_id, files }),
                )
                .await;
        }
    }

    result
}

async fn capture_git_state(cwd: &Path) -> Option<GitState> {
    let repo_root = get_git_repo_root(cwd)?;
    let head_output = run_git_command_with_timeout(&["rev-parse", "HEAD"], &repo_root).await;
    let head_hash = head_output.and_then(|out| {
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
    });

    let status_output =
        run_git_command_with_timeout(&["status", "--porcelain", "-z"], &repo_root).await;
    let mut uncommitted_files = BTreeMap::new();
    if let Some(out) = status_output
        && out.status.success()
    {
        let status = String::from_utf8_lossy(&out.stdout);
        let parts = status.split('\0').collect::<Vec<_>>();
        let mut index = 0;
        while index < parts.len() {
            let part = parts[index];
            if part.is_empty() {
                index += 1;
                continue;
            }
            let status_code = &part[..2];
            let path_str = &part[3..];
            let abs_path = repo_root.join(path_str);
            let modified_at = std::fs::metadata(&abs_path).and_then(|m| m.modified()).ok();
            uncommitted_files.insert(abs_path, modified_at);
            if status_code.starts_with('R') || status_code.starts_with('C') {
                index += 1;
            }
            index += 1;
        }
    }

    Some(GitState {
        repo_root,
        head_hash,
        uncommitted_files,
    })
}

async fn compute_git_side_effects(before: &GitState, after: &GitState) -> Vec<PathBuf> {
    if before.repo_root != after.repo_root {
        return Vec::new();
    }

    let mut changed_files = BTreeSet::new();
    for (path, after_modified_at) in &after.uncommitted_files {
        match before.uncommitted_files.get(path) {
            Some(before_modified_at) if before_modified_at == after_modified_at => {}
            _ => {
                changed_files.insert(path.clone());
            }
        }
    }

    if before.head_hash != after.head_hash {
        if let (Some(old_head), Some(new_head)) = (&before.head_hash, &after.head_hash) {
            let diff_output = run_git_command_with_timeout(
                &["diff", "--name-only", "-z", old_head, new_head],
                &before.repo_root,
            )
            .await;
            if let Some(out) = diff_output
                && out.status.success()
            {
                let diff = String::from_utf8_lossy(&out.stdout);
                for path_str in diff.split('\0').filter(|path| !path.is_empty()) {
                    changed_files.insert(before.repo_root.join(path_str));
                }
            }
        } else if before.head_hash.is_none()
            && let Some(new_head) = &after.head_hash
        {
            let show_output = run_git_command_with_timeout(
                &["show", "--name-only", "--format=", "-z", new_head],
                &before.repo_root,
            )
            .await;
            if let Some(out) = show_output
                && out.status.success()
            {
                let diff = String::from_utf8_lossy(&out.stdout);
                for path_str in diff.split('\0').filter(|path| !path.is_empty()) {
                    changed_files.insert(before.repo_root.join(path_str));
                }
            }
        }
    }

    changed_files.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::make_session_and_context_with_rx;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use tempfile::tempdir;

    async fn init_git_repo() -> tempfile::TempDir {
        let repo = tempdir().expect("tempdir");
        for cmd in [
            &["git", "init", "-q"][..],
            &["git", "config", "user.name", "Test User"][..],
            &["git", "config", "user.email", "test@example.com"][..],
        ] {
            let status = tokio::process::Command::new(cmd[0])
                .args(&cmd[1..])
                .current_dir(repo.path())
                .status()
                .await
                .expect("run git command");
            assert!(status.success(), "git command should succeed: {cmd:?}");
        }
        repo
    }

    #[tokio::test]
    async fn compute_git_side_effects_detects_new_uncommitted_files() {
        let repo = init_git_repo().await;
        let before = GitState {
            repo_root: repo.path().to_path_buf(),
            head_hash: Some("abc".to_string()),
            uncommitted_files: BTreeMap::new(),
        };
        let after = GitState {
            repo_root: repo.path().to_path_buf(),
            head_hash: Some("abc".to_string()),
            uncommitted_files: BTreeMap::from([(repo.path().join("src/main.rs"), None)]),
        };

        let changed = compute_git_side_effects(&before, &after).await;
        assert_eq!(changed, vec![repo.path().join("src/main.rs")]);
    }

    #[tokio::test]
    async fn track_tool_side_effects_records_relative_paths_and_emits_event() {
        let repo = init_git_repo().await;
        std::fs::write(repo.path().join("tracked.txt"), "before\n").expect("write tracked file");
        let add_status = tokio::process::Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(repo.path())
            .status()
            .await
            .expect("git add");
        assert!(add_status.success());
        let commit_status = tokio::process::Command::new("git")
            .args(["commit", "-qm", "initial"])
            .current_dir(repo.path())
            .status()
            .await
            .expect("git commit");
        assert!(commit_status.success());

        let (session, turn, rx) = make_session_and_context_with_rx().await;
        track_tool_side_effects(
            repo.path(),
            "call-1".to_string(),
            session.as_ref(),
            turn.as_ref(),
            || async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                std::fs::write(repo.path().join("tracked.txt"), "after\n")
                    .expect("modify tracked file");
            },
        )
        .await;

        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if let EventMsg::FileSystemMutated(event) = event.msg {
                    break event;
                }
            }
        })
        .await
        .expect("timed out waiting for file mutation event");

        assert_eq!(event.call_id, "call-1");
        assert_eq!(event.files, vec!["tracked.txt".to_string()]);

        let side_effects = turn.side_effects_files.lock().await.clone();
        assert_eq!(
            side_effects.into_iter().collect::<Vec<_>>(),
            vec!["tracked.txt".to_string()]
        );
    }
}
