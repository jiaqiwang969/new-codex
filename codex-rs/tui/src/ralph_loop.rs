//! Ralph Loop: an iterative self-correction loop that re-injects the same prompt
//! until a completion promise is detected or the maximum iteration count is reached.

use std::path::Path;
use std::path::PathBuf;

pub(crate) const RALPH_LOOP_DEFAULT_MAX_ITERATIONS: u32 = 50;
pub(crate) const RALPH_LOOP_DEFAULT_COMPLETION_PROMISE: &str = "COMPLETE";
pub(crate) const RALPH_LOOP_DEFAULT_DELAY_SECONDS: u64 = 300;

/// State for an active Ralph Loop session.
#[derive(Debug, Clone)]
pub(crate) struct RalphLoopState {
    pub enabled: bool,
    pub iteration: u32,
    pub max_iterations: u32,
    pub completion_promise: String,
    pub original_prompt: String,
    pub started_at: String,
    pub delay_seconds: u64,
}

impl RalphLoopState {
    pub fn new(
        max_iterations: u32,
        completion_promise: String,
        original_prompt: String,
        delay_seconds: u64,
    ) -> Self {
        Self {
            enabled: true,
            iteration: 1,
            max_iterations,
            completion_promise,
            original_prompt,
            started_at: chrono::Utc::now().to_rfc3339(),
            delay_seconds,
        }
    }

    /// Whether the loop should continue (not yet at max iterations).
    pub fn should_continue(&self) -> bool {
        self.enabled && (self.max_iterations == 0 || self.iteration < self.max_iterations)
    }

    /// Advance to the next iteration.
    pub fn next_iteration(&mut self) {
        self.iteration += 1;
    }
}

/// Parsed Ralph Loop command arguments.
#[derive(Debug, Clone)]
pub(crate) struct RalphLoopCommand {
    pub max_iterations: u32,
    pub completion_promise: String,
    pub prompt: Option<String>,
    pub delay_seconds: u64,
}

impl Default for RalphLoopCommand {
    fn default() -> Self {
        Self {
            max_iterations: RALPH_LOOP_DEFAULT_MAX_ITERATIONS,
            completion_promise: RALPH_LOOP_DEFAULT_COMPLETION_PROMISE.to_string(),
            prompt: None,
            delay_seconds: RALPH_LOOP_DEFAULT_DELAY_SECONDS,
        }
    }
}

pub(crate) fn is_ralph_loop_help_request(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.eq_ignore_ascii_case("help") || matches!(trimmed, "-h" | "--help")
}

/// Parse ralph-loop arguments from the inline args string (everything after `/ralph-loop `).
pub(crate) fn parse_ralph_loop_args(input: &str) -> Result<RalphLoopCommand, String> {
    let parts: Vec<String> = shlex::split(input.trim())
        .unwrap_or_else(|| input.split_whitespace().map(ToString::to_string).collect());

    let mut max_iterations = RALPH_LOOP_DEFAULT_MAX_ITERATIONS;
    let mut completion_promise = RALPH_LOOP_DEFAULT_COMPLETION_PROMISE.to_string();
    let mut prompt: Option<String> = None;
    let mut delay_seconds = RALPH_LOOP_DEFAULT_DELAY_SECONDS;
    let mut positional_prompt_parts: Vec<String> = Vec::new();

    let mut i = 0;
    while i < parts.len() {
        match parts[i].as_str() {
            "--max-iterations" | "--max" | "-n" => {
                i += 1;
                if i < parts.len() {
                    max_iterations = parts[i]
                        .parse()
                        .map_err(|_| format!("Invalid max-iterations value: {}", parts[i]))?;
                } else {
                    return Err("--max-iterations requires a value".to_string());
                }
            }
            "--completion-promise" | "-c" => {
                i += 1;
                if i < parts.len() {
                    completion_promise = parts[i].clone();
                } else {
                    return Err("--completion-promise requires a value".to_string());
                }
            }
            "--delay" | "-d" => {
                i += 1;
                if i < parts.len() {
                    delay_seconds = parts[i]
                        .parse()
                        .map_err(|_| format!("Invalid delay value: {}", parts[i]))?;
                } else {
                    return Err("--delay requires a value".to_string());
                }
            }
            "--prompt" | "-p" => {
                i += 1;
                if i < parts.len() {
                    let mut prompt_parts = Vec::new();
                    while i < parts.len() {
                        if is_ralph_loop_option(&parts[i]) {
                            if prompt_parts.is_empty() {
                                return Err("--prompt requires a value".to_string());
                            }
                            i -= 1;
                            break;
                        }
                        prompt_parts.push(parts[i].clone());
                        i += 1;
                    }
                    if prompt_parts.is_empty() {
                        return Err("--prompt requires a value".to_string());
                    }
                    prompt = Some(prompt_parts.join(" "));
                } else {
                    return Err("--prompt requires a value".to_string());
                }
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("Unknown option: {other}"));
                }
                positional_prompt_parts.push(parts[i].clone());
            }
        }
        i += 1;
    }

    if prompt.is_some() && !positional_prompt_parts.is_empty() {
        return Err(
            "Provide the prompt either via --prompt/-p or as positional arguments, not both"
                .to_string(),
        );
    }

    let prompt = prompt.or_else(|| {
        if positional_prompt_parts.is_empty() {
            None
        } else {
            Some(positional_prompt_parts.join(" "))
        }
    });

    Ok(RalphLoopCommand {
        max_iterations,
        completion_promise,
        prompt,
        delay_seconds,
    })
}

fn is_ralph_loop_option(token: &str) -> bool {
    matches!(
        token,
        "--max-iterations"
            | "--max"
            | "-n"
            | "--completion-promise"
            | "-c"
            | "--prompt"
            | "-p"
            | "--delay"
            | "-d"
    )
}

/// Check if the agent output contains the completion promise wrapped in `<promise>` tags.
pub(crate) fn check_completion_promise(output: &str, promise: &str) -> bool {
    if let Some(extracted) = extract_promise_text(output) {
        normalize_promise_text(&extracted) == normalize_promise_text(promise)
    } else {
        false
    }
}

/// Extract text from `<promise>...</promise>` tags.
fn extract_promise_text(text: &str) -> Option<String> {
    let start_tag = "<promise>";
    let end_tag = "</promise>";
    if let Some(start_idx) = text.find(start_tag) {
        let content_start = start_idx + start_tag.len();
        if let Some(end_idx) = text[content_start..].find(end_tag) {
            return Some(text[content_start..content_start + end_idx].to_string());
        }
    }
    None
}

/// Normalize whitespace for promise comparison.
fn normalize_promise_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Return the path for the ralph loop state file.
pub(crate) fn ralph_state_file_path(cwd: &Path) -> PathBuf {
    cwd.join(".codex").join("ralph-loop.local.md")
}

/// Save the ralph loop state to a markdown file.
pub(crate) fn save_ralph_state_file(cwd: &Path, state: &RalphLoopState) {
    let path = ralph_state_file_path(cwd);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = create_state_file_content(state);
    let _ = std::fs::write(&path, content);
}

/// Remove the ralph loop state file.
pub(crate) fn cleanup_ralph_state_file(cwd: &Path) {
    let path = ralph_state_file_path(cwd);
    let _ = std::fs::remove_file(&path);
}

fn create_state_file_content(state: &RalphLoopState) -> String {
    let prompt_preview = truncate_string(&state.original_prompt, /*max_len*/ 100);
    format!(
        r#"---
ralph_loop:
  enabled: {enabled}
  iteration: {iteration}
  max_iterations: {max_iterations}
  completion_promise: "{promise}"
  delay_seconds: {delay}
  started_at: "{started_at}"
---

# Ralph Loop State

- **Status**: {status}
- **Iteration**: {iteration}/{max_iterations}
- **Promise**: `{promise}`
- **Delay**: {delay}s (on error only)
- **Prompt**: {prompt_preview}
"#,
        enabled = state.enabled,
        iteration = state.iteration,
        max_iterations = state.max_iterations,
        promise = state.completion_promise,
        delay = state.delay_seconds,
        started_at = state.started_at,
        status = if state.enabled { "Active" } else { "Stopped" },
        prompt_preview = prompt_preview,
    )
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut safe_len = max_len;
        while !s.is_char_boundary(safe_len) {
            safe_len -= 1;
        }
        format!("{}...", &s[..safe_len])
    }
}

/// Help text for the ralph loop feature.
pub(crate) fn ralph_loop_help_text() -> String {
    format!(
        r#"Ralph Loop - Iterative Self-Correction Loop

Usage:
  /ralph-loop
  /ralph-loop [prompt] [options]
  /ralph-loop --help
  /cancel-ralph

Options:
  -n, --max-iterations, --max <N>  Maximum iterations (default: {RALPH_LOOP_DEFAULT_MAX_ITERATIONS}, 0 = unlimited)
  -c, --completion-promise <STR>   Completion promise text (default: "{RALPH_LOOP_DEFAULT_COMPLETION_PROMISE}")
  -p, --prompt <TEXT>              Prompt to repeat each iteration
  -d, --delay <SECONDS>            Delay before retry on error (default: {RALPH_LOOP_DEFAULT_DELAY_SECONDS})

Examples:
  /ralph-loop "Build the API. Output <promise>COMPLETE</promise> when done." -n 30
  /ralph-loop --max 30 --completion-promise DONE --prompt "Fix all tests"
  /ralph-loop -p "Fix all tests" -c DONE -n 10
  /ralph-loop "Implement feature X" -n 20 -c FINISHED -d 60

How it works:
  1. The prompt is submitted to the agent
  2. When the agent finishes, the system checks for <promise>COMPLETE</promise>
  3. If not found and iterations remain, the same prompt is re-submitted
  4. The agent sees its previous work (files, git history) and continues
  5. On error, waits --delay seconds before retrying
  6. Use /cancel-ralph to stop the loop at any time"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_basic() {
        let cmd = parse_ralph_loop_args("").unwrap();
        assert_eq!(cmd.max_iterations, RALPH_LOOP_DEFAULT_MAX_ITERATIONS);
        assert_eq!(
            cmd.completion_promise,
            RALPH_LOOP_DEFAULT_COMPLETION_PROMISE
        );
        assert_eq!(cmd.prompt, None);
        assert_eq!(cmd.delay_seconds, RALPH_LOOP_DEFAULT_DELAY_SECONDS);
    }

    #[test]
    fn test_parse_with_options() {
        let cmd = parse_ralph_loop_args("--max-iterations 30 --completion-promise DONE").unwrap();
        assert_eq!(cmd.max_iterations, 30);
        assert_eq!(cmd.completion_promise, "DONE");
    }

    #[test]
    fn test_parse_with_max_alias() {
        let cmd = parse_ralph_loop_args("--max 12").unwrap();
        assert_eq!(cmd.max_iterations, 12);
    }

    #[test]
    fn test_parse_with_prompt() {
        let cmd = parse_ralph_loop_args("--prompt Build REST API").unwrap();
        assert_eq!(cmd.prompt, Some("Build REST API".to_string()));
    }

    #[test]
    fn test_parse_with_positional_prompt() {
        let cmd = parse_ralph_loop_args("\"Build REST API\" -n 10 -c DONE").unwrap();
        assert_eq!(cmd.prompt, Some("Build REST API".to_string()));
        assert_eq!(cmd.max_iterations, 10);
        assert_eq!(cmd.completion_promise, "DONE");
    }

    #[test]
    fn test_parse_with_delay() {
        let cmd = parse_ralph_loop_args("-d 60 -n 20").unwrap();
        assert_eq!(cmd.delay_seconds, 60);
        assert_eq!(cmd.max_iterations, 20);
    }

    #[test]
    fn test_is_help_request() {
        assert!(is_ralph_loop_help_request("help"));
        assert!(is_ralph_loop_help_request("HELP"));
        assert!(is_ralph_loop_help_request("-h"));
        assert!(is_ralph_loop_help_request("--help"));
        assert!(!is_ralph_loop_help_request("implement feature"));
    }

    #[test]
    fn test_check_completion_promise() {
        assert!(check_completion_promise(
            "Done! <promise>COMPLETE</promise>",
            "COMPLETE"
        ));
        assert!(!check_completion_promise("Done! COMPLETE", "COMPLETE"));
        assert!(check_completion_promise(
            "<promise> COMPLETE </promise>",
            "COMPLETE"
        ));
        assert!(!check_completion_promise(
            "<promise>WRONG</promise>",
            "COMPLETE"
        ));
    }

    #[test]
    fn test_state_should_continue() {
        let mut state = RalphLoopState::new(5, "COMPLETE".into(), "test".into(), 0);
        assert!(state.should_continue()); // iteration 1 < 5
        state.iteration = 5;
        assert!(!state.should_continue()); // iteration 5 == 5
    }

    #[test]
    fn test_unlimited_iterations() {
        let state = RalphLoopState::new(0, "COMPLETE".into(), "test".into(), 0);
        assert!(state.should_continue()); // max_iterations 0 = unlimited
    }

    #[test]
    fn test_truncate_string_handles_unicode_char_boundaries() {
        let prompt = "非常好，4k已经有了；接下来，重点就是优化算法，让4k也能稳定30fps；好好分析一下硬件底层和算法的匹配，继续";

        let truncated = truncate_string(prompt, /*max_len*/ 100);

        assert!(truncated.ends_with("..."));
        assert!(prompt.starts_with(truncated.trim_end_matches("...")));
    }
}
