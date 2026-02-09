//! Session utility functions for the session bar.
//!
//! Extracted from the reference project's `cxresume_picker_widget.rs` — only the
//! minimal set of types and helpers needed by `SessionBar`.

use std::fs;
use std::io::BufRead;
use std::path::Path;
use std::path::PathBuf;

/// Simplified session metadata (no TUMIX).
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub cwd: String,
    pub age: String,
    pub mtime: u64,
    pub message_count: usize,
    pub last_role: String,
    pub model: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan `codex_home/sessions/` for `.jsonl` rollout files whose `cwd` matches
/// (or is a child of) `cwd_raw`. Returns newest-first, capped at 100.
pub fn get_cwd_sessions_for(codex_home: &Path, cwd_raw: &Path) -> Result<Vec<SessionInfo>, String> {
    let cwd = cwd_raw
        .canonicalize()
        .unwrap_or_else(|_| cwd_raw.to_path_buf());
    let sessions_dir = codex_home.join("sessions");
    let mut sessions = Vec::new();

    fn find_sessions(
        dir: &Path,
        cwd: &Path,
        sessions: &mut Vec<SessionInfo>,
        max_depth: u32,
    ) -> Result<(), String> {
        if max_depth == 0 {
            return Ok(());
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                    if let Ok((id, session_cwd, msg_count, last_role, _tokens, model)) =
                        extract_session_meta(&path)
                        && should_include_session(&session_cwd, cwd)
                    {
                        let mtime = entry
                            .metadata()
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        let age = format_relative_time(mtime);

                        sessions.push(SessionInfo {
                            id,
                            path: path.clone(),
                            cwd: session_cwd,
                            age,
                            mtime,
                            message_count: msg_count,
                            last_role,
                            model,
                        });
                    }
                } else if path.is_dir() {
                    let _ = find_sessions(path.as_path(), cwd, sessions, max_depth - 1);
                }
            }
        }

        Ok(())
    }

    find_sessions(&sessions_dir, &cwd, &mut sessions, 4)?;

    // Sort by modification time (newest first)
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));

    sessions.retain(|session| session.message_count > 0);

    // Limit to recent 100 sessions for performance
    sessions.truncate(100);

    Ok(sessions)
}

/// Return the last User message's first `max_words` words as a snippet label.
pub fn last_user_snippet(path: &PathBuf, max_words: usize) -> Option<String> {
    let data = collect_session_messages(path);
    let last_user = data
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "User" && !m.content.trim().is_empty())?;
    let mut words = last_user
        .content
        .split_whitespace()
        .filter(|w| !w.is_empty());
    let mut taken: Vec<&str> = Vec::new();
    for _ in 0..max_words {
        if let Some(w) = words.next() {
            taken.push(w);
        } else {
            break;
        }
    }
    if taken.is_empty() {
        None
    } else {
        Some(taken.join(" "))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn extract_session_meta(
    path: &Path,
) -> Result<(String, String, usize, String, usize, String), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);
    let mut lines = reader.lines();

    let mut session_id = String::new();
    let mut cwd = String::new();
    let mut model = String::from("unknown");
    let mut last_role = String::from("-");
    let mut total_tokens = 0;

    // First pass: extract session metadata from first line
    if let Some(Ok(first_line)) = lines.next()
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&first_line)
        && let Some(payload) = json.get("payload")
    {
        session_id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            });

        cwd = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
            .unwrap_or_default();

        model = payload
            .get("model")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| "unknown".to_string());
    }

    // Second pass: gather message metadata
    let parsed = collect_session_messages(&path.to_path_buf());
    if let Some(tokens) = parsed.total_tokens {
        total_tokens = tokens;
    }
    let dialog_messages: Vec<&ParsedMessage> = parsed
        .messages
        .iter()
        .filter(|m| matches!(m.role.as_str(), "User" | "Assistant"))
        .collect();
    let message_count = dialog_messages.len();
    if let Some(last) = dialog_messages.last() {
        last_role = last.role.clone();
    }

    if session_id.is_empty() {
        session_id = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
    }

    Ok((
        session_id,
        cwd,
        message_count,
        last_role,
        total_tokens,
        model,
    ))
}

fn should_include_session(session_cwd: &str, cwd: &Path) -> bool {
    if session_cwd.is_empty() {
        return false;
    }

    let raw_path = PathBuf::from(session_cwd);
    let candidate = if raw_path.is_absolute() {
        raw_path
    } else {
        cwd.join(raw_path)
    };

    match candidate.canonicalize() {
        Ok(real_path) => real_path == cwd || real_path.starts_with(cwd),
        Err(_) => false,
    }
}

fn format_relative_time(mtime: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();

    let diff = now.saturating_sub(mtime);

    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else if diff < 2592000 {
        format!("{}w ago", diff / 604800)
    } else if diff < 31536000 {
        format!("{}mo ago", diff / 2592000)
    } else {
        format!("{}y ago", diff / 31536000)
    }
}

// ---------------------------------------------------------------------------
// Session message parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ParsedMessage {
    role: String,
    content: String,
    #[allow(dead_code)]
    timestamp: Option<String>,
}

#[derive(Default)]
struct ParsedSessionData {
    messages: Vec<ParsedMessage>,
    total_tokens: Option<usize>,
}

fn collect_session_messages(path: &PathBuf) -> ParsedSessionData {
    let mut data = ParsedSessionData::default();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return data,
    };

    let reader = std::io::BufReader::new(file);
    let mut first_line = true;
    let mut new_format = false;

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if first_line {
                first_line = false;
                if json.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
                    new_format = true;
                    if let Some(tokens) = extract_total_tokens(&json) {
                        data.total_tokens = Some(tokens);
                    }
                    continue;
                }
            }

            if let Some(tokens) = extract_total_tokens(&json) {
                data.total_tokens = Some(tokens);
            }

            let message = if new_format {
                parse_new_format_message(&json)
            } else {
                parse_legacy_format_message(&json)
            };

            if let Some(message) = message
                && !message.content.trim().is_empty()
            {
                data.messages.push(message);
            }
        }
    }

    data
}

fn parse_new_format_message(json: &serde_json::Value) -> Option<ParsedMessage> {
    if json.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
        return None;
    }

    let payload = json.get("payload")?;
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let (role, content_value) = match event_type {
        "user_message" => ("User", payload.get("message")),
        "agent_message" => ("Assistant", payload.get("message")),
        _ => return None,
    };

    let content = content_value
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .or_else(|| extract_text_from_content(payload.get("content")))?;

    let timestamp = json
        .get("timestamp")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("timestamp").and_then(|v| v.as_str()))
        .map(std::string::ToString::to_string);

    Some(ParsedMessage {
        role: role.to_string(),
        content,
        timestamp,
    })
}

fn parse_legacy_format_message(json: &serde_json::Value) -> Option<ParsedMessage> {
    let payload = json.get("payload");
    let raw_role = payload
        .and_then(|p| p.get("role").and_then(|v| v.as_str()))
        .or_else(|| json.get("role").and_then(|v| v.as_str()))
        .unwrap_or("");
    let role = normalize_role(raw_role);

    if role != "User" && role != "Assistant" && role != "System" {
        return None;
    }

    let content_node = payload
        .and_then(|p| p.get("content"))
        .or_else(|| json.get("content"));
    let content = extract_text_from_content(content_node)
        .or_else(|| {
            payload
                .and_then(|p| p.get("text").and_then(|v| v.as_str()))
                .map(std::string::ToString::to_string)
        })
        .or_else(|| {
            json.get("text")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
        })?;

    if content.trim().is_empty() {
        return None;
    }

    let timestamp = payload
        .and_then(|p| p.get("timestamp").and_then(|v| v.as_str()))
        .or_else(|| json.get("timestamp").and_then(|v| v.as_str()))
        .map(std::string::ToString::to_string);

    Some(ParsedMessage {
        role,
        content,
        timestamp,
    })
}

fn normalize_role(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "user" => "User".to_string(),
        "assistant" | "agent" => "Assistant".to_string(),
        "system" => "System".to_string(),
        other => other.to_string(),
    }
}

fn extract_text_from_content(node: Option<&serde_json::Value>) -> Option<String> {
    let mut segments: Vec<String> = Vec::new();
    if let Some(value) = node {
        collect_text_segments(value, &mut segments);
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n"))
    }
}

fn collect_text_segments(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.to_string()),
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::Object(map) = item {
                    if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                        out.push(text.to_string());
                    } else if let Some(content) = map.get("content") {
                        collect_text_segments(content, out);
                    }
                } else {
                    collect_text_segments(item, out);
                }
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                out.push(text.to_string());
            }
            if let Some(message) = map.get("message").and_then(|v| v.as_str()) {
                out.push(message.to_string());
            }
            if let Some(nested) = map.get("content") {
                collect_text_segments(nested, out);
            }
        }
        _ => {}
    }
}

fn extract_total_tokens(json: &serde_json::Value) -> Option<usize> {
    let payload = json.get("payload")?;

    if let Some(usage) = payload.get("usage")
        && let Some(total) = usage
            .get("total_tokens")
            .and_then(serde_json::Value::as_u64)
    {
        return Some(total as usize);
    }

    if let Some(info) = payload.get("info")
        && let Some(total) = info
            .get("total_token_usage")
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(serde_json::Value::as_u64)
    {
        return Some(total as usize);
    }

    None
}
