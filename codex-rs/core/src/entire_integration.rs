use anyhow::Result;

use crate::codex::Session;
use crate::codex::TurnContext;
use chrono::DateTime;
use chrono::Utc;
use codex_hooks::EntireSummary;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntireCheckpoint {
    pub checkpoint_id: String,
    pub commit_hash: String,
    pub timestamp: DateTime<Utc>,
    pub prompt_summary: String,
    pub files_changed: Vec<String>,
    pub model_used: String,
    pub ai_summary: Option<EntireSummary>,
}

pub async fn get_recent_entire_checkpoints_with_summaries(
    cwd: &Path,
    limit: usize,
) -> Result<Vec<EntireCheckpoint>> {
    let mut checkpoints = get_recent_entire_checkpoints(cwd, limit).await?;
    for checkpoint in &mut checkpoints {
        checkpoint.ai_summary = codex_hooks::load_summary(cwd, &checkpoint.checkpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }
    Ok(checkpoints)
}

pub(crate) fn backfill_recent_entire_summaries(
    session: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    limit: usize,
) {
    let session = Arc::clone(session);
    let turn_context = Arc::clone(turn_context);
    tokio::spawn(async move {
        if let Err(err) =
            backfill_recent_entire_summaries_inner(&session, &turn_context, limit).await
        {
            warn!(
                turn_id = %turn_context.sub_id,
                error = %err,
                "failed to backfill Entire summaries"
            );
        }
    });
}

async fn backfill_recent_entire_summaries_inner(
    session: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    limit: usize,
) -> Result<()> {
    for checkpoint in get_recent_entire_checkpoints(turn_context.cwd.as_path(), limit).await? {
        if !should_backfill_summary(&checkpoint) {
            continue;
        }

        if codex_hooks::load_summary(&turn_context.cwd, &checkpoint.checkpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .is_some()
        {
            continue;
        }

        let summary_input = codex_hooks::EntireSummaryInput {
            thread_id: session.conversation_id.to_string(),
            turn_id: checkpoint.checkpoint_id.clone(),
            user_prompt: checkpoint.prompt_summary.clone(),
            ai_response: format!("Modified {} files", checkpoint.files_changed.len()),
            files_changed: checkpoint.files_changed.clone(),
        };

        match crate::entire_summary_generator::generate_entire_summary(
            session,
            turn_context,
            &summary_input,
        )
        .await
        {
            Ok(summary) => {
                if let Err(err) = codex_hooks::save_summary(
                    &turn_context.cwd,
                    &checkpoint.checkpoint_id,
                    &summary,
                )
                .await
                {
                    warn!(
                        checkpoint_id = %checkpoint.checkpoint_id,
                        error = %err,
                        "failed to save backfilled Entire summary"
                    );
                }
            }
            Err(err) => {
                warn!(
                    checkpoint_id = %checkpoint.checkpoint_id,
                    error = %err,
                    "failed to generate backfilled Entire summary"
                );
            }
        }
    }

    Ok(())
}

fn should_backfill_summary(checkpoint: &EntireCheckpoint) -> bool {
    if !checkpoint.files_changed.is_empty() {
        return true;
    }

    let normalized_prompt = checkpoint.prompt_summary.trim().to_ascii_lowercase();
    if normalized_prompt.is_empty() {
        return false;
    }
    if normalized_prompt.chars().count() >= 10 {
        return true;
    }

    !matches!(
        normalized_prompt.as_str(),
        "hi" | "hello" | "hey" | "ok" | "okay" | "thanks" | "thank you" | "yo"
    )
}

pub async fn get_recent_entire_checkpoints(
    cwd: &Path,
    limit: usize,
) -> Result<Vec<EntireCheckpoint>> {
    if !is_git_repo(cwd) {
        return Ok(Vec::new());
    }

    let output = Command::new("git")
        .arg("log")
        .arg("--grep=Entire-Checkpoint")
        .arg(format!("-{}", limit * 2))
        .arg("--format=%H|%ct|%(trailers:key=Entire-Checkpoint,valueonly)")
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let log_output = String::from_utf8_lossy(&output.stdout);
    let mut checkpoints = Vec::new();

    for line in log_output.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 3 {
            continue;
        }

        let commit_hash = parts[0].to_string();
        let Ok(timestamp) = parts[1].parse::<i64>() else {
            continue;
        };
        let checkpoint_id = parts[2].trim().to_string();
        if checkpoint_id.is_empty() {
            continue;
        }

        match read_checkpoint_details(cwd, &checkpoint_id, &commit_hash, timestamp).await {
            Ok(checkpoint) => checkpoints.push(checkpoint),
            Err(err) => {
                warn!("Failed to read checkpoint {checkpoint_id}: {err}");
            }
        }

        if checkpoints.len() >= limit {
            break;
        }
    }

    Ok(checkpoints)
}

async fn read_checkpoint_details(
    cwd: &Path,
    checkpoint_id: &str,
    commit_hash: &str,
    timestamp: i64,
) -> Result<EntireCheckpoint> {
    let prompt_summary = get_commit_message_summary(cwd, commit_hash)?;
    let files_changed = get_checkpoint_files(cwd, commit_hash)?;
    let model_used =
        get_checkpoint_model(cwd, checkpoint_id).unwrap_or_else(|_| "unknown".to_string());

    Ok(EntireCheckpoint {
        checkpoint_id: checkpoint_id.to_string(),
        commit_hash: commit_hash.to_string(),
        timestamp: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now),
        prompt_summary,
        files_changed,
        model_used,
        ai_summary: None,
    })
}

fn is_git_repo(cwd: &Path) -> bool {
    Command::new("git")
        .arg("rev-parse")
        .arg("--git-dir")
        .current_dir(cwd)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn get_commit_message_summary(cwd: &Path, commit_hash: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("log")
        .arg("--format=%s")
        .arg("-n")
        .arg("1")
        .arg(commit_hash)
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        return Ok("(no summary available)".to_string());
    }

    let message = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let summary = message.chars().take(200).collect::<String>();
    Ok(if message.len() > 200 {
        format!("{summary}...")
    } else {
        summary
    })
}

fn get_checkpoint_files(cwd: &Path, commit_hash: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("diff-tree")
        .arg("--no-commit-id")
        .arg("--name-only")
        .arg("-r")
        .arg(commit_hash)
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(std::string::ToString::to_string)
        .collect())
}

fn get_checkpoint_model(cwd: &Path, checkpoint_id: &str) -> Result<String> {
    let branch_name = format!("entire/{checkpoint_id}");
    let output = Command::new("git")
        .arg("show")
        .arg(format!("{branch_name}:session.json"))
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Branch not found");
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(json["model_slug"].as_str().unwrap_or("unknown").to_string())
}

pub fn format_checkpoints_summary(checkpoints: &[EntireCheckpoint]) -> String {
    if checkpoints.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    for checkpoint in checkpoints {
        let summary_text = if let Some(ai_summary) = &checkpoint.ai_summary {
            if ai_summary.is_meaningful {
                format!(
                    "{} → {}",
                    ai_summary.motivation.as_deref().unwrap_or("N/A"),
                    ai_summary.outcome.as_deref().unwrap_or("N/A"),
                )
            } else {
                checkpoint.prompt_summary.clone()
            }
        } else {
            checkpoint.prompt_summary.clone()
        };

        let time_ago = format_time_ago(&checkpoint.timestamp);
        let files = format_files_list(&checkpoint.files_changed);
        let checkpoint_short = if checkpoint.checkpoint_id.len() > 8 {
            &checkpoint.checkpoint_id[..8]
        } else {
            &checkpoint.checkpoint_id
        };
        let model_info = if checkpoint.model_used != "unknown" {
            format!(" [{}]", checkpoint.model_used)
        } else {
            String::new()
        };

        lines.push(format!(
            "- [{time_ago}] {checkpoint_short}{model_info}: {summary_text} ({files})"
        ));
    }

    lines.join("\n")
}

fn format_files_list(files: &[String]) -> String {
    if files.is_empty() {
        "no files".to_string()
    } else if files.len() <= 3 {
        files.join(", ")
    } else {
        format!("{}, +{} more", files[..2].join(", "), files.len() - 2)
    }
}

fn format_time_ago(timestamp: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(*timestamp);

    if duration.num_hours() < 1 {
        let mins = duration.num_minutes();
        if mins < 1 {
            "just now".to_string()
        } else {
            format!("{mins}m ago")
        }
    } else if duration.num_days() < 1 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_weeks() < 1 {
        format!("{}d ago", duration.num_days())
    } else if duration.num_weeks() < 4 {
        format!("{}w ago", duration.num_weeks())
    } else {
        format!("{}mo ago", duration.num_weeks() / 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;
    use std::path::Path;
    use std::process::Command;

    fn git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_with_env(repo_root: &Path, args: &[&str], envs: &[(&str, &str)]) {
        let mut command = Command::new("git");
        command.args(args).current_dir(repo_root);
        for (key, value) in envs {
            command.env(key, value);
        }
        let output = command.output().expect("run git with env");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn make_entire_repo() -> (tempfile::TempDir, String) {
        let repo_root = tempfile::tempdir().expect("create temp dir");
        git(repo_root.path(), &["init"]);
        git(repo_root.path(), &["config", "user.name", "Codex Tests"]);
        git(
            repo_root.path(),
            &["config", "user.email", "codex@example.com"],
        );

        let checkpoint_id = "checkpoint-123456789".to_string();
        std::fs::write(repo_root.path().join("feature.txt"), "baseline").expect("write file");
        git(repo_root.path(), &["add", "feature.txt"]);
        git(repo_root.path(), &["commit", "-m", "Initial commit"]);
        std::fs::write(repo_root.path().join("feature.txt"), "hello").expect("write file");
        git(repo_root.path(), &["add", "feature.txt"]);
        git_with_env(
            repo_root.path(),
            &[
                "commit",
                "-m",
                "Add Entire history plumbing",
                "-m",
                &format!("Entire-Checkpoint: {checkpoint_id}"),
            ],
            &[
                ("GIT_AUTHOR_DATE", "2025-01-02T03:04:05Z"),
                ("GIT_COMMITTER_DATE", "2025-01-02T03:04:05Z"),
            ],
        );

        git(
            repo_root.path(),
            &["checkout", "-b", &format!("entire/{checkpoint_id}")],
        );
        std::fs::write(
            repo_root.path().join("session.json"),
            r#"{"model_slug":"gpt-5.3-codex"}"#,
        )
        .expect("write session json");
        git(repo_root.path(), &["add", "session.json"]);
        git(
            repo_root.path(),
            &["commit", "-m", "Store session metadata"],
        );
        git(repo_root.path(), &["checkout", "-"]);

        let summary = EntireSummary {
            is_meaningful: true,
            motivation: Some("Keep upstream alignment context visible".to_string()),
            approach: Some("Surface recent Entire checkpoints to the lead agent".to_string()),
            challenges: None,
            tradeoffs: None,
            outcome: Some(
                "Future turns reuse prior rationale instead of duplicating work".to_string(),
            ),
        };
        codex_hooks::save_summary(repo_root.path(), &checkpoint_id, &summary)
            .await
            .expect("save summary");

        (repo_root, checkpoint_id)
    }

    #[tokio::test]
    async fn get_recent_entire_checkpoints_reads_git_history_and_saved_summaries() {
        let (repo_root, checkpoint_id) = make_entire_repo().await;

        let checkpoints = get_recent_entire_checkpoints_with_summaries(repo_root.path(), 3)
            .await
            .expect("read checkpoints");

        assert_eq!(checkpoints.len(), 1);
        assert_eq!(
            checkpoints[0],
            EntireCheckpoint {
                checkpoint_id,
                commit_hash: checkpoints[0].commit_hash.clone(),
                timestamp: Utc
                    .with_ymd_and_hms(2025, 1, 2, 3, 4, 5)
                    .single()
                    .expect("valid timestamp"),
                prompt_summary: "Add Entire history plumbing".to_string(),
                files_changed: vec!["feature.txt".to_string()],
                model_used: "gpt-5.3-codex".to_string(),
                ai_summary: Some(EntireSummary {
                    is_meaningful: true,
                    motivation: Some("Keep upstream alignment context visible".to_string()),
                    approach: Some(
                        "Surface recent Entire checkpoints to the lead agent".to_string()
                    ),
                    challenges: None,
                    tradeoffs: None,
                    outcome: Some(
                        "Future turns reuse prior rationale instead of duplicating work"
                            .to_string()
                    ),
                }),
            }
        );
    }

    #[test]
    fn format_checkpoints_summary_prefers_ai_summary_and_compacts_files() {
        let checkpoints = vec![EntireCheckpoint {
            checkpoint_id: "checkpoint-123456789".to_string(),
            commit_hash: "abc123".to_string(),
            timestamp: Utc::now(),
            prompt_summary: "fallback summary".to_string(),
            files_changed: vec![
                "src/lib.rs".to_string(),
                "src/codex.rs".to_string(),
                "src/hooks.rs".to_string(),
                "src/tests.rs".to_string(),
            ],
            model_used: "gpt-5.3-codex".to_string(),
            ai_summary: Some(EntireSummary {
                is_meaningful: true,
                motivation: Some("Preserve custom capabilities".to_string()),
                approach: None,
                challenges: None,
                tradeoffs: None,
                outcome: Some("Land an upstream-aligned Entire slice".to_string()),
            }),
        }];

        let summary = format_checkpoints_summary(&checkpoints);
        assert!(summary.contains("[just now] checkpoi [gpt-5.3-codex]"));
        assert!(
            summary
                .contains("Preserve custom capabilities → Land an upstream-aligned Entire slice")
        );
        assert!(summary.contains("src/lib.rs, src/codex.rs, +2 more"));
    }

    #[tokio::test]
    async fn get_recent_entire_checkpoints_returns_empty_outside_git_repo() {
        let repo_root = tempfile::tempdir().expect("create temp dir");
        let checkpoints = get_recent_entire_checkpoints_with_summaries(repo_root.path(), 3)
            .await
            .expect("read checkpoints");
        assert!(checkpoints.is_empty());
    }
}
