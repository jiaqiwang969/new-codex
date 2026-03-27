//! AI-generated WHY-focused summaries for Entire checkpoints.
//!
//! This module generates concise, decision-focused summaries of AI coding sessions
//! that answer "why" questions rather than just "what" was done. These summaries
//! are stored alongside Entire checkpoints to provide future context about the
//! reasoning behind code changes.

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;

/// Input data for generating an Entire summary.
#[derive(Debug, Clone)]
pub struct EntireSummaryInput {
    pub thread_id: String,
    pub turn_id: String,
    pub user_prompt: String,
    pub ai_response: String,
    pub files_changed: Vec<String>,
}

/// Generated WHY-focused summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntireSummary {
    pub is_meaningful: bool,
    pub motivation: Option<String>,
    pub approach: Option<String>,
    pub challenges: Option<String>,
    pub tradeoffs: Option<String>,
    pub outcome: Option<String>,
}

/// Builds the WHY-focused prompt for summary generation.
pub fn build_why_prompt(input: &EntireSummaryInput) -> String {
    let files_list = if input.files_changed.is_empty() {
        "No files changed".to_string()
    } else {
        input.files_changed.join("\n")
    };

    format!(
        r#"Analyze this AI coding session and generate a WHY-focused summary.

This summary will be stored with git history to help future developers understand the reasoning behind changes.

User Request:
{user_prompt}

AI Response:
{ai_response}

Files Changed:
{files_list}

First, evaluate if this session contains meaningful work (e.g., code changes, technical discussions, architectural decisions, debugging). If it's just trivial chit-chat (like "hi", "how are you"), set `is_meaningful` to false and leave the rest as null.

If it IS meaningful, generate a structured summary that answers these key questions:
1. MOTIVATION: Why did the user need this? What problem were they solving?
2. APPROACH: What solution was chosen? Were alternatives considered?
3. CHALLENGES: What obstacles were encountered? How were they overcome? (optional if none)
4. TRADEOFFS: What compromises were made? Why were they acceptable? (optional if none)
5. OUTCOME: What was accomplished? What's the key insight?

Return a JSON object with this structure:
{{
  "is_meaningful": true or false,
  "motivation": "1-2 sentences explaining the problem/need (or null)",
  "approach": "1-2 sentences describing the chosen solution (or null)",
  "challenges": "1 sentence about obstacles (or null)",
  "tradeoffs": "1 sentence about compromises (or null)",
  "outcome": "1-2 sentences summarizing what was achieved (or null)"
}}

Focus on decision rationale and context that helps future developers understand "why this way?"
Keep it concise but informative. Return ONLY the JSON object."#,
        user_prompt = input.user_prompt,
        ai_response = truncate_response(&input.ai_response, /*max_chars*/ 8000),
        files_list = files_list
    )
}

/// Truncates a response to a maximum character count.
fn truncate_response(response: &str, max_chars: usize) -> String {
    if let Some((idx, _)) = response.char_indices().nth(max_chars) {
        format!("{}... [truncated]", &response[..idx])
    } else {
        response.to_string()
    }
}

/// Saves a summary to the .entire/summaries directory.
pub async fn save_summary(
    repo_root: &Path,
    checkpoint_id: &str,
    summary: &EntireSummary,
) -> Result<PathBuf> {
    let summaries_dir = repo_root.join(".entire").join("summaries");
    fs::create_dir_all(&summaries_dir)
        .await
        .context("Failed to create .entire/summaries directory")?;

    let summary_path = summaries_dir.join(format!("{checkpoint_id}.json"));
    let summary_json =
        serde_json::to_string_pretty(summary).context("Failed to serialize summary")?;

    fs::write(&summary_path, summary_json)
        .await
        .context("Failed to write summary file")?;

    Ok(summary_path)
}

/// Loads a summary from disk if it exists.
pub async fn load_summary(repo_root: &Path, checkpoint_id: &str) -> Result<Option<EntireSummary>> {
    let summary_path = repo_root
        .join(".entire")
        .join("summaries")
        .join(format!("{checkpoint_id}.json"));

    if !summary_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&summary_path)
        .await
        .context("Failed to read summary file")?;

    let summary: EntireSummary =
        serde_json::from_str(&content).context("Failed to parse summary JSON")?;

    Ok(Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[test]
    fn test_build_why_prompt() {
        let input = EntireSummaryInput {
            thread_id: "test-thread".to_string(),
            turn_id: "test-turn".to_string(),
            user_prompt: "Add authentication".to_string(),
            ai_response: "I've implemented JWT authentication...".to_string(),
            files_changed: vec!["src/auth.rs".to_string()],
        };

        let prompt = build_why_prompt(&input);
        assert!(prompt.contains("MOTIVATION"));
        assert!(prompt.contains("APPROACH"));
        assert!(prompt.contains("Add authentication"));
        assert!(prompt.contains("src/auth.rs"));
    }

    #[test]
    fn test_truncate_response() {
        let short = "short response";
        assert_eq!(truncate_response(short, 100), short);

        let long = "a".repeat(1000);
        let truncated = truncate_response(&long, 50);
        assert!(truncated.len() < 100);
        assert!(truncated.contains("[truncated]"));
    }

    #[tokio::test]
    async fn save_and_load_summary_round_trips() {
        let repo_root = TempDir::new().expect("temp dir");
        let summary = EntireSummary {
            is_meaningful: true,
            motivation: Some("keep memory continuity".to_string()),
            approach: Some("persist summary JSON".to_string()),
            challenges: None,
            tradeoffs: Some("test-only coverage slice".to_string()),
            outcome: Some("round-trip succeeded".to_string()),
        };

        let saved_path = save_summary(repo_root.path(), "checkpoint-1", &summary)
            .await
            .expect("save summary");
        assert_eq!(
            saved_path,
            repo_root
                .path()
                .join(".entire")
                .join("summaries")
                .join("checkpoint-1.json")
        );

        let loaded = load_summary(repo_root.path(), "checkpoint-1")
            .await
            .expect("load summary");
        assert_eq!(loaded, Some(summary));
    }
}
