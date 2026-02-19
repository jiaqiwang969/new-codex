use chrono::DateTime;
use chrono::Utc;
use std::path::Path;
use std::process::Command;
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
}

/// Get recent Entire checkpoints from git history
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
        timestamp: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| Utc::now()),
        prompt_summary,
        files_changed,
        model_used,
    })
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
        format!("{}...", summary)
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
        .map(|s| s.to_string())
        .collect())
}

fn get_checkpoint_model(
    cwd: &Path,
    checkpoint_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Try to read from entire branch
    let branch_name = format!("entire/{}", checkpoint_id);

    let output = Command::new("git")
        .arg("show")
        .arg(format!("{}:session.json", branch_name))
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
            "- [{}] {}{}: {} (files: {})",
            time_ago, checkpoint_short, model_info, checkpoint.prompt_summary, files
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
            format!("{}m ago", mins)
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
