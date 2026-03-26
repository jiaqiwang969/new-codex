use crate::git_info;
use crate::path_utils::write_atomically;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreePurpose {
    ForkedSession,
    SpawnedAgent,
}

impl WorktreePurpose {
    fn dir_component(self) -> &'static str {
        match self {
            WorktreePurpose::ForkedSession => "fork",
            WorktreePurpose::SpawnedAgent => "agent",
        }
    }

    fn branch_prefix(self) -> &'static str {
        match self {
            WorktreePurpose::ForkedSession => "codex/fork",
            WorktreePurpose::SpawnedAgent => "codex/agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorktree {
    pub id: Uuid,
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub purpose: WorktreePurpose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorktreeLease {
    pub version: u32,
    pub thread_id: String,
    pub parent_thread_id: Option<String>,
    pub pid: u32,
    pub repo_root: PathBuf,
    pub worktree_id: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub purpose: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn create_agent_worktree(
    parent_cwd: &Path,
    purpose: WorktreePurpose,
) -> Result<Option<AgentWorktree>> {
    let Some(repo_root) = git_info::resolve_root_git_project_for_trust(parent_cwd) else {
        return Ok(None);
    };

    let worktree_id = Uuid::new_v4();
    let worktree_id_str = worktree_id.to_string();
    let worktrees_dir = repo_root
        .join(".codex")
        .join("worktrees")
        .join(purpose.dir_component());
    tokio::fs::create_dir_all(&worktrees_dir)
        .await
        .with_context(|| format!("failed to create worktrees dir {}", worktrees_dir.display()))?;

    let worktree_path = worktrees_dir.join(&worktree_id_str);
    let branch = format!("{}/{worktree_id_str}", purpose.branch_prefix());
    let base_ref = git_stdout(["rev-parse", "HEAD"], parent_cwd)
        .await
        .context("failed to resolve git HEAD for worktree creation")?;

    let output = git_worktree_add(&repo_root, &branch, &worktree_path, &base_ref)
        .await
        .with_context(|| format!("failed to run git worktree add in {}", repo_root.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "git worktree add failed (branch={}, path={}, base={}): stdout: {} stderr: {}",
            branch,
            worktree_path.display(),
            base_ref,
            stdout.trim(),
            stderr.trim()
        );
    }

    Ok(Some(AgentWorktree {
        id: worktree_id,
        repo_root,
        path: worktree_path,
        branch,
        purpose,
    }))
}

pub async fn remove_agent_worktree(worktree: &AgentWorktree) -> Result<()> {
    let _ = git_worktree_remove(&worktree.repo_root, &worktree.path).await;
    let _ = git_output(
        ["branch", "-D", worktree.branch.as_str()],
        &worktree.repo_root,
    )
    .await;

    if tokio::fs::try_exists(&worktree.path).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(&worktree.path)
            .await
            .with_context(|| {
                format!("failed to remove worktree dir {}", worktree.path.display())
            })?;
    }

    Ok(())
}

pub fn build_lease(
    thread_id: &str,
    parent_thread_id: Option<String>,
    worktree: &AgentWorktree,
) -> AgentWorktreeLease {
    let now = unix_now();
    AgentWorktreeLease {
        version: 1,
        thread_id: thread_id.to_string(),
        parent_thread_id,
        pid: std::process::id(),
        repo_root: worktree.repo_root.clone(),
        worktree_id: worktree.id.to_string(),
        worktree_path: worktree.path.clone(),
        branch: worktree.branch.clone(),
        purpose: worktree.purpose.dir_component().to_string(),
        created_at: now,
        updated_at: now,
    }
}

pub fn write_lease(lease: &AgentWorktreeLease) -> Result<PathBuf> {
    let lease_path = lease
        .repo_root
        .join(".codex")
        .join("leases")
        .join(format!("{}.json", lease.thread_id));
    let contents =
        serde_json::to_string_pretty(lease).context("failed to serialize agent worktree lease")?;
    write_atomically(&lease_path, &contents)
        .with_context(|| format!("failed to write lease file {}", lease_path.display()))?;
    Ok(lease_path)
}

pub fn read_lease(repo_root: &Path, thread_id: &str) -> Result<Option<AgentWorktreeLease>> {
    let lease_path = repo_root
        .join(".codex")
        .join("leases")
        .join(format!("{thread_id}.json"));
    match std::fs::read_to_string(&lease_path) {
        Ok(contents) => {
            let lease =
                serde_json::from_str::<AgentWorktreeLease>(&contents).with_context(|| {
                    format!(
                        "failed to parse agent worktree lease {}",
                        lease_path.display()
                    )
                })?;
            Ok(Some(lease))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to read agent worktree lease {}",
                lease_path.display()
            )
        }),
    }
}

pub fn list_leases(repo_root: &Path) -> Result<Vec<AgentWorktreeLease>> {
    let leases_dir = repo_root.join(".codex").join("leases");
    let entries = match std::fs::read_dir(&leases_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read leases dir {}", leases_dir.display()));
        }
    };

    let mut leases = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        match path.extension() {
            Some(ext) if ext == OsStr::new("json") => {}
            _ => continue,
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read lease file {}", path.display()))?;
        let lease = serde_json::from_str::<AgentWorktreeLease>(&contents)
            .with_context(|| format!("failed to parse lease file {}", path.display()))?;
        leases.push(lease);
    }
    Ok(leases)
}

pub async fn ensure_worktree_for_thread(
    cwd: &Path,
    thread_id: &str,
) -> Result<Option<AgentWorktreeLease>> {
    let Some(repo_root) = git_info::resolve_root_git_project_for_trust(cwd) else {
        return Ok(None);
    };

    let Some(lease) = read_lease(&repo_root, thread_id)? else {
        return Ok(None);
    };

    ensure_worktree_for_lease(&lease).await?;
    Ok(Some(lease))
}

async fn ensure_worktree_for_lease(lease: &AgentWorktreeLease) -> Result<()> {
    if tokio::fs::try_exists(&lease.worktree_path)
        .await
        .unwrap_or(false)
    {
        return Ok(());
    }

    if let Some(parent) = lease.worktree_path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!("failed to create worktree parent dir {}", parent.display())
        })?;
    }

    let _ = git_output(["worktree", "prune"], &lease.repo_root).await;
    let output =
        git_worktree_add_existing_branch(&lease.repo_root, &lease.worktree_path, &lease.branch)
            .await
            .with_context(|| {
                format!(
                    "failed to run git worktree add for {}",
                    lease.repo_root.display()
                )
            })?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "git worktree add failed (branch={}, path={}): stdout: {} stderr: {}",
        lease.branch,
        lease.worktree_path.display(),
        stdout.trim(),
        stderr.trim()
    );
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn git_output<I, S>(args: I, cwd: &Path) -> std::io::Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .await
}

async fn git_worktree_add(
    repo_root: &Path,
    branch: &str,
    worktree_path: &Path,
    base_ref: &str,
) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch)
        .arg(worktree_path)
        .arg(base_ref)
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .await
}

async fn git_worktree_remove(
    repo_root: &Path,
    worktree_path: &Path,
) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .arg("worktree")
        .arg("remove")
        .arg("-f")
        .arg(worktree_path)
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .await
}

async fn git_worktree_add_existing_branch(
    repo_root: &Path,
    worktree_path: &Path,
    branch: &str,
) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(worktree_path)
        .arg(branch)
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .await
}

async fn git_stdout<I, S>(args: I, cwd: &Path) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(args, cwd)
        .await
        .with_context(|| format!("failed to run git in {}", cwd.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git command failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_info::current_branch_name;
    use core_test_support::skip_if_sandbox;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    async fn init_git_repo(root: &Path) -> Result<()> {
        let envs = vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ];
        let out = Command::new("git")
            .envs(envs.clone())
            .args(["init"])
            .current_dir(root)
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!("git init failed");
        }
        Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.name", "Test User"])
            .current_dir(root)
            .output()
            .await?;
        Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.email", "test@example.com"])
            .current_dir(root)
            .output()
            .await?;
        tokio::fs::write(root.join("README.md"), "hello\n").await?;
        Command::new("git")
            .envs(envs.clone())
            .args(["add", "."])
            .current_dir(root)
            .output()
            .await?;
        Command::new("git")
            .envs(envs.clone())
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .await?;
        Ok(())
    }

    fn sample_worktree(root: &Path) -> AgentWorktree {
        AgentWorktree {
            id: Uuid::new_v4(),
            repo_root: root.to_path_buf(),
            path: root.join("wt"),
            branch: "codex/agent/abc".to_string(),
            purpose: WorktreePurpose::SpawnedAgent,
        }
    }

    #[tokio::test]
    async fn create_and_remove_worktree_creates_branch_and_checkout() -> Result<()> {
        skip_if_sandbox!(Ok(()));
        let tmp = TempDir::new()?;
        let repo_root = tmp.path().join("repo");
        tokio::fs::create_dir_all(&repo_root).await?;
        init_git_repo(&repo_root).await?;

        let worktree = create_agent_worktree(&repo_root, WorktreePurpose::SpawnedAgent)
            .await?
            .context("expected worktree in git repo")?;
        assert!(worktree.path.is_dir());

        let branch = current_branch_name(&worktree.path)
            .await
            .context("failed to read branch from worktree")?;
        assert_eq!(branch, worktree.branch);

        remove_agent_worktree(&worktree).await?;
        assert!(!worktree.path.exists());
        Ok(())
    }

    #[test]
    fn lease_roundtrip_json() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        let worktree = sample_worktree(&root);
        let lease = build_lease("thread-1", Some("parent-1".to_string()), &worktree);
        let json = serde_json::to_string(&lease)?;
        let parsed = serde_json::from_str::<AgentWorktreeLease>(&json)?;
        assert_eq!(parsed, lease);
        Ok(())
    }

    #[test]
    fn write_and_read_lease_roundtrip() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        let worktree = sample_worktree(&root);
        let lease = build_lease("thread-1", Some("parent-1".to_string()), &worktree);

        let lease_path = write_lease(&lease)?;
        assert_eq!(
            lease_path,
            root.join(".codex").join("leases").join("thread-1.json")
        );

        let read_back = read_lease(&root, "thread-1")?;
        assert_eq!(read_back, Some(lease));
        Ok(())
    }

    #[test]
    fn list_leases_ignores_non_json_files() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        let worktree = sample_worktree(&root);
        let lease_one = build_lease("thread-1", Some("parent-1".to_string()), &worktree);
        let lease_two = build_lease("thread-2", None, &worktree);

        write_lease(&lease_one)?;
        write_lease(&lease_two)?;
        let leases_dir = root.join(".codex").join("leases");
        std::fs::write(leases_dir.join("README.txt"), "ignore me")?;

        let mut leases = list_leases(&root)?;
        leases.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
        let mut expected = vec![lease_one, lease_two];
        expected.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
        assert_eq!(leases, expected);
        Ok(())
    }

    #[tokio::test]
    async fn ensure_worktree_for_thread_returns_none_without_lease() -> Result<()> {
        let tmp = TempDir::new()?;
        let repo_root = tmp.path().join("repo");
        tokio::fs::create_dir_all(&repo_root).await?;
        init_git_repo(&repo_root).await?;

        let lease = ensure_worktree_for_thread(&repo_root, "thread-1").await?;
        assert_eq!(lease, None);
        Ok(())
    }
}
