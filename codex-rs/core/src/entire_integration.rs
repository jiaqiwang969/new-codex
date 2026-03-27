use crate::client::ModelClient;
use crate::config::Config;
use crate::entire_summary_generator;
use crate::models_manager::manager::ModelsManager;
use chrono::DateTime;
use chrono::Utc;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct EntireCheckpoint {
    pub checkpoint_id: String,
    #[allow(dead_code)]
    pub commit_hash: String,
    pub timestamp: DateTime<Utc>,
    pub prompt_summary: String,
    pub files_changed: Vec<String>,
    pub model_used: String,
    pub ai_summary: Option<codex_hooks::EntireSummary>,
}

/// Get recent Entire checkpoints from git history with AI summaries
pub async fn get_recent_entire_checkpoints_with_summaries(
    cwd: &Path,
    limit: usize,
    base_client: Option<&ModelClient>,
    models_manager: Option<&Arc<ModelsManager>>,
    config: Option<&Config>,
) -> Result<Vec<EntireCheckpoint>, Box<dyn std::error::Error>> {
    let checkpoints = get_recent_entire_checkpoints(cwd, limit).await?;

    // If config is provided and entire_summary is enabled, try to load or generate summaries
    if let (Some(client), Some(manager), Some(cfg)) = (base_client, models_manager, config)
        && cfg.memories.entire_summary_enabled
    {
        let mut enriched = Vec::new();
        for mut checkpoint in checkpoints {
            // Try to load existing summary
            if let Some(summary) =
                entire_summary_generator::load_summary_if_exists(cwd, &checkpoint.checkpoint_id)
                    .await
            {
                checkpoint.ai_summary = Some(summary);
            } else {
                // Generate summary asynchronously in background
                let is_trivial_prompt = checkpoint.prompt_summary.len() < 10
                    && (checkpoint.prompt_summary.to_lowercase().contains("hi")
                        || checkpoint.prompt_summary.to_lowercase().contains("hello"))
                    && checkpoint.files_changed.is_empty();

                if !is_trivial_prompt {
                    // Generate summary asynchronously in background
                    spawn_summary_generation(
                        &checkpoint,
                        cwd.to_path_buf(),
                        client.clone(),
                        Arc::clone(manager),
                        cfg.clone(),
                    );
                }
            }
            enriched.push(checkpoint);
        }
        return Ok(enriched);
    }

    Ok(checkpoints)
}

/// Get recent Entire checkpoints from git history (without AI summaries)
pub async fn get_recent_entire_checkpoints(
    cwd: &Path,
    limit: usize,
) -> Result<Vec<EntireCheckpoint>, Box<dyn std::error::Error>> {
    // Check if we're in a git repository
    if !is_git_repo(cwd) {
        return Ok(Vec::new());
    }

    // Find recent commits with Entire-Checkpoint trailer
    let output = Command::new("git")
        .arg("log")
        .arg("--grep=Entire-Checkpoint")
        .arg(format!("-{}", limit * 2)) // Get more to filter
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
        let timestamp = match parts[1].parse::<i64>() {
            Ok(ts) => ts,
            Err(_) => continue,
        };
        let checkpoint_id = parts[2].trim().to_string();

        if checkpoint_id.is_empty() {
            continue;
        }

        // Read checkpoint details
        match read_checkpoint_details(cwd, &checkpoint_id, &commit_hash, timestamp).await {
            Ok(checkpoint) => checkpoints.push(checkpoint),
            Err(e) => {
                warn!("Failed to read checkpoint {}: {}", checkpoint_id, e);
                continue;
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
) -> Result<EntireCheckpoint, Box<dyn std::error::Error>> {
    // Get prompt summary from commit message
    let prompt_summary = get_commit_message_summary(cwd, commit_hash)?;

    // Get modified files
    let files_changed = get_checkpoint_files(cwd, commit_hash)?;

    // Try to get model from entire branch if available
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

fn spawn_summary_generation(
    checkpoint: &EntireCheckpoint,
    repo_root: std::path::PathBuf,
    base_client: ModelClient,
    models_manager: Arc<ModelsManager>,
    config: Config,
) {
    let checkpoint_id = checkpoint.checkpoint_id.clone();
    let input = codex_hooks::EntireSummaryInput {
        thread_id: "unknown".to_string(), // We don't have this from git history
        turn_id: "unknown".to_string(),
        user_prompt: checkpoint.prompt_summary.clone(),
        ai_response: format!("Modified {} files", checkpoint.files_changed.len()),
        files_changed: checkpoint.files_changed.clone(),
    };

    entire_summary_generator::generate_and_save_summary_async(
        input,
        checkpoint_id,
        repo_root,
        base_client,
        models_manager,
        config,
    );
}

fn is_git_repo(cwd: &Path) -> bool {
    Command::new("git")
        .arg("rev-parse")
        .arg("--git-dir")
        .current_dir(cwd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_commit_message_summary(
    cwd: &Path,
    commit_hash: &str,
) -> Result<String, Box<dyn std::error::Error>> {
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

    // Truncate to 200 characters
    let summary = message.chars().take(200).collect::<String>();
    Ok(if message.len() > 200 {
        format!("{summary}...")
    } else {
        summary
    })
}

fn get_checkpoint_files(
    cwd: &Path,
    commit_hash: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
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

fn get_checkpoint_model(
    cwd: &Path,
    checkpoint_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Try to read from entire branch
    let branch_name = format!("entire/{checkpoint_id}");

    let output = Command::new("git")
        .arg("show")
        .arg(format!("{branch_name}:session.json"))
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        return Err("Branch not found".into());
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
        // Use AI summary if available, otherwise use git commit summary
        let summary_text = if let Some(ai_summary) = &checkpoint.ai_summary {
            if ai_summary.is_meaningful {
                format!(
                    "{} → {}",
                    ai_summary.motivation.as_deref().unwrap_or("N/A"),
                    ai_summary.outcome.as_deref().unwrap_or("N/A")
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
    use chrono::Duration;

    #[test]
    fn format_checkpoints_summary_prefers_meaningful_ai_summary() {
        let checkpoint = EntireCheckpoint {
            checkpoint_id: "checkpoint-12345678".to_string(),
            commit_hash: "abc123".to_string(),
            timestamp: Utc::now() - Duration::minutes(5),
            prompt_summary: "fallback prompt".to_string(),
            files_changed: vec!["src/lib.rs".to_string()],
            model_used: "claude-sonnet-4-6".to_string(),
            ai_summary: Some(codex_hooks::EntireSummary {
                is_meaningful: true,
                motivation: Some("keep account pool".to_string()),
                approach: Some("preserve local merge slice".to_string()),
                challenges: None,
                tradeoffs: None,
                outcome: Some("locked memory contract".to_string()),
            }),
        };

        let summary = format_checkpoints_summary(&[checkpoint]);
        assert!(summary.contains("keep account pool → locked memory contract"));
        assert!(!summary.contains("fallback prompt"));
    }

    #[test]
    fn format_checkpoints_summary_falls_back_to_prompt_when_ai_summary_is_not_meaningful() {
        let checkpoint = EntireCheckpoint {
            checkpoint_id: "checkpoint-12345678".to_string(),
            commit_hash: "abc123".to_string(),
            timestamp: Utc::now() - Duration::minutes(5),
            prompt_summary: "fallback prompt".to_string(),
            files_changed: vec!["src/lib.rs".to_string()],
            model_used: "claude-sonnet-4-6".to_string(),
            ai_summary: Some(codex_hooks::EntireSummary {
                is_meaningful: false,
                motivation: None,
                approach: None,
                challenges: None,
                tradeoffs: None,
                outcome: None,
            }),
        };

        let summary = format_checkpoints_summary(&[checkpoint]);
        assert!(summary.contains("fallback prompt"));
    }

    #[test]
    fn format_files_list_compacts_long_lists() {
        let files = vec![
            "a.rs".to_string(),
            "b.rs".to_string(),
            "c.rs".to_string(),
            "d.rs".to_string(),
        ];

        assert_eq!(format_files_list(&files), "a.rs, b.rs, +2 more");
    }
}
