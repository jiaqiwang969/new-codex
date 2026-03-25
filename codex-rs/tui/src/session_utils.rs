//! Session utility functions for the session bar.
//!
//! Extracted from the reference project's `cxresume_picker_widget.rs` — only the
//! minimal set of types and helpers needed by `SessionBar`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::BufRead;
use std::path::Path;
use std::path::PathBuf;

use codex_core::path_utils::write_atomically;
use serde::Deserialize;
use serde::Serialize;

const SESSION_BAR_CACHE_FILE: &str = "session_bar_cache.v2.json";
const SESSION_BAR_CACHE_VERSION: u32 = 2;

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
    pub last_user_snippet: Option<String>,
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
    let mut details_cache = load_session_details_cache(codex_home);
    let mut cache_dirty = false;
    let mut candidates = Vec::new();
    let mut seen_paths = HashSet::new();

    fn find_sessions(
        dir: &Path,
        cwd: &Path,
        details_cache: &mut SessionDetailsCache,
        candidates: &mut Vec<SessionCandidate>,
        seen_paths: &mut HashSet<PathBuf>,
        cache_dirty: &mut bool,
        max_depth: u32,
    ) -> Result<(), String> {
        if max_depth == 0 {
            return Ok(());
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                    seen_paths.insert(path.clone());
                    let mtime = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    if let Some(cached) = details_cache.entries.get(&path)
                        && cached.mtime == mtime
                    {
                        if should_include_session(&cached.cwd, cwd) {
                            candidates.push(SessionCandidate {
                                id: cached.id.clone(),
                                path: path.clone(),
                                cwd: cached.cwd.clone(),
                                mtime,
                                model: cached.model.clone(),
                                cached_details: cached.details.clone(),
                            });
                        }
                        continue;
                    }

                    let header = match extract_session_header(&path) {
                        Ok(Some(header)) => header,
                        Ok(None) | Err(_) => {
                            if details_cache.entries.remove(&path).is_some() {
                                *cache_dirty = true;
                            }
                            continue;
                        }
                    };

                    let updated_entry = CachedSessionEntry {
                        mtime,
                        id: header.id.clone(),
                        cwd: header.cwd.clone(),
                        model: header.model.clone(),
                        details: None,
                    };
                    let previous = details_cache
                        .entries
                        .insert(path.clone(), updated_entry.clone());
                    if previous.as_ref() != Some(&updated_entry) {
                        *cache_dirty = true;
                    }

                    if should_include_session(&header.cwd, cwd) {
                        candidates.push(SessionCandidate {
                            id: header.id,
                            path: path.clone(),
                            cwd: header.cwd,
                            mtime,
                            model: header.model,
                            cached_details: None,
                        });
                    }
                } else if path.is_dir() {
                    let _ = find_sessions(
                        path.as_path(),
                        cwd,
                        details_cache,
                        candidates,
                        seen_paths,
                        cache_dirty,
                        max_depth - 1,
                    );
                }
            }
        }

        Ok(())
    }

    find_sessions(
        &sessions_dir,
        &cwd,
        &mut details_cache,
        &mut candidates,
        &mut seen_paths,
        &mut cache_dirty,
        /*max_depth*/ 4,
    )?;

    let cache_len_before = details_cache.entries.len();
    details_cache
        .entries
        .retain(|path, _| seen_paths.contains(path));
    if details_cache.entries.len() != cache_len_before {
        cache_dirty = true;
    }

    // Sort by modification time (newest first)
    candidates.sort_by(|a, b| b.mtime.cmp(&a.mtime));

    // Limit to recent 100 sessions before parsing full message history.
    candidates.truncate(100);

    let mut sessions = Vec::new();
    for candidate in candidates {
        let details = if let Some(cached_details) = candidate.cached_details {
            SessionDetails {
                message_count: cached_details.message_count,
                last_role: cached_details.last_role.clone(),
                last_user_snippet: cached_details.last_user_snippet.clone(),
            }
        } else {
            let details = extract_session_details(&candidate.path);
            let updated_details = CachedSessionDetails {
                message_count: details.message_count,
                last_role: details.last_role.clone(),
                last_user_snippet: details.last_user_snippet.clone(),
            };

            if let Some(cached_entry) = details_cache.entries.get_mut(&candidate.path) {
                if cached_entry.mtime == candidate.mtime
                    && cached_entry.details.as_ref() != Some(&updated_details)
                {
                    cached_entry.details = Some(updated_details.clone());
                    cache_dirty = true;
                }
            } else {
                let updated_entry = CachedSessionEntry {
                    mtime: candidate.mtime,
                    id: candidate.id.clone(),
                    cwd: candidate.cwd.clone(),
                    model: candidate.model.clone(),
                    details: Some(updated_details.clone()),
                };
                let previous = details_cache
                    .entries
                    .insert(candidate.path.clone(), updated_entry.clone());
                if previous.as_ref() != Some(&updated_entry) {
                    cache_dirty = true;
                }
            }

            details
        };
        sessions.push(SessionInfo {
            id: candidate.id,
            path: candidate.path,
            cwd: candidate.cwd,
            age: format_relative_time(candidate.mtime),
            mtime: candidate.mtime,
            message_count: details.message_count,
            last_role: details.last_role,
            model: candidate.model,
            last_user_snippet: details.last_user_snippet,
        });
    }

    sessions.retain(|session| session.message_count > 0);

    if cache_dirty {
        persist_session_details_cache(codex_home, &details_cache);
    }

    Ok(sessions)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct SessionCandidate {
    id: String,
    path: PathBuf,
    cwd: String,
    mtime: u64,
    model: String,
    cached_details: Option<CachedSessionDetails>,
}

struct SessionHeader {
    id: String,
    cwd: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedSessionDetails {
    message_count: usize,
    last_role: String,
    last_user_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedSessionEntry {
    mtime: u64,
    id: String,
    cwd: String,
    model: String,
    details: Option<CachedSessionDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionDetailsCache {
    version: u32,
    entries: HashMap<PathBuf, CachedSessionEntry>,
}

impl Default for SessionDetailsCache {
    fn default() -> Self {
        Self {
            version: SESSION_BAR_CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
}

fn session_details_cache_path(codex_home: &Path) -> PathBuf {
    codex_home.join(SESSION_BAR_CACHE_FILE)
}

fn load_session_details_cache(codex_home: &Path) -> SessionDetailsCache {
    let cache_path = session_details_cache_path(codex_home);
    let raw = match fs::read_to_string(cache_path) {
        Ok(raw) => raw,
        Err(_) => return SessionDetailsCache::default(),
    };
    let cache = match serde_json::from_str::<SessionDetailsCache>(&raw) {
        Ok(cache) => cache,
        Err(_) => return SessionDetailsCache::default(),
    };
    if cache.version == SESSION_BAR_CACHE_VERSION {
        cache
    } else {
        SessionDetailsCache::default()
    }
}

fn persist_session_details_cache(codex_home: &Path, cache: &SessionDetailsCache) {
    let cache_path = session_details_cache_path(codex_home);
    if let Ok(serialized) = serde_json::to_string(cache) {
        let _ = write_atomically(&cache_path, &serialized);
    }
}

fn extract_session_header(path: &Path) -> Result<Option<SessionHeader>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);

    let mut session_id = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut cwd = String::new();
    let mut model = String::from("unknown");

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let json = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(json) => json,
            Err(_) => continue,
        };

        if let Some(payload) = json.get("payload") {
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                session_id = id.to_string();
            }
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

        return Ok(Some(SessionHeader {
            id: session_id,
            cwd,
            model,
        }));
    }

    Ok(None)
}

struct SessionDetails {
    message_count: usize,
    last_role: String,
    last_user_snippet: Option<String>,
}

fn extract_session_details(path: &Path) -> SessionDetails {
    let mut details = SessionDetails {
        message_count: 0,
        last_role: "-".to_string(),
        last_user_snippet: None,
    };

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return details,
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

        let json = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(json) => json,
            Err(_) => continue,
        };

        if first_line {
            first_line = false;
            new_format = json.get("type").and_then(|v| v.as_str()) == Some("session_meta");
            if new_format {
                continue;
            }
        }

        let message = if new_format {
            parse_new_format_message(&json)
        } else {
            parse_legacy_format_message(&json)
        };
        if let Some(message) = message {
            if matches!(message.role.as_str(), "User" | "Assistant") {
                details.message_count += 1;
                details.last_role = message.role.clone();
            }
            if message.role == "User" {
                details.last_user_snippet = snippet_words(&message.content, /*max_words*/ 5);
            }
        }
    }

    details
}

fn should_include_session(session_cwd: &str, cwd: &Path) -> bool {
    if session_cwd.is_empty() {
        return false;
    }

    let session_path = Path::new(session_cwd);
    if session_path.is_absolute() {
        return session_path == cwd || session_path.starts_with(cwd);
    }

    let candidate = cwd.join(session_path);
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
    Some(ParsedMessage {
        role: role.to_string(),
        content,
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
    Some(ParsedMessage { role, content })
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

fn snippet_words(content: &str, max_words: usize) -> Option<String> {
    let words: Vec<&str> = content
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .take(max_words)
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn session_details_cache_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("/tmp/a.jsonl"),
            CachedSessionEntry {
                mtime: 123,
                id: "session-a".to_string(),
                cwd: "/tmp".to_string(),
                model: "gemma-3n".to_string(),
                details: Some(CachedSessionDetails {
                    message_count: 4,
                    last_role: "Assistant".to_string(),
                    last_user_snippet: Some("hello world".to_string()),
                }),
            },
        );
        let cache = SessionDetailsCache {
            version: SESSION_BAR_CACHE_VERSION,
            entries,
        };

        persist_session_details_cache(temp.path(), &cache);
        let loaded = load_session_details_cache(temp.path());

        assert_eq!(loaded, cache);
        Ok(())
    }

    #[test]
    fn session_details_cache_ignores_old_version() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let cache_path = session_details_cache_path(temp.path());
        let raw = serde_json::json!({
            "version": 0,
            "entries": {
                "/tmp/a.jsonl": {
                    "mtime": 1,
                    "id": "session-a",
                    "cwd": "/tmp",
                    "model": "gemma-3n",
                    "details": {
                        "message_count": 2,
                        "last_role": "User",
                        "last_user_snippet": "hi"
                    }
                }
            }
        });
        fs::write(cache_path, serde_json::to_string(&raw)?)?;

        let loaded = load_session_details_cache(temp.path());

        assert_eq!(loaded, SessionDetailsCache::default());
        Ok(())
    }
}
