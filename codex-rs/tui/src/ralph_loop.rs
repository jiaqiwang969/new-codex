//! Ralph Loop is an iterative prompt-repeat loop.
//!
//! It re-injects the same prompt after each turn until the assistant emits the
//! configured completion promise inside a `<promise>...</promise>` tag, or until
//! the configured iteration limit is reached.

use std::path::Path;
use std::path::PathBuf;

use codex_protocol::ThreadId;
use uuid::Uuid;

use crate::text_formatting::truncate_text;

pub(crate) const RALPH_LOOP_DEFAULT_MAX_ITERATIONS: u32 = 50;
pub(crate) const RALPH_LOOP_DEFAULT_COMPLETION_PROMISE: &str = "COMPLETE";
pub(crate) const RALPH_LOOP_DEFAULT_DELAY_SECONDS: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RalphLoopTarget {
    ActiveThread,
    Thread(ThreadId),
}

impl RalphLoopTarget {
    pub(crate) fn matches_current_thread(&self, current_thread_id: Option<ThreadId>) -> bool {
        match self {
            Self::ActiveThread => true,
            Self::Thread(thread_id) => current_thread_id == Some(*thread_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRetry {
    generation: u64,
    due_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RalphLoopState {
    pub(crate) enabled: bool,
    pub(crate) iteration: u32,
    pub(crate) max_iterations: u32,
    pub(crate) completion_promise: String,
    pub(crate) original_prompt: String,
    pub(crate) started_at: String,
    pub(crate) delay_seconds: u64,
    instance_id: String,
    target: RalphLoopTarget,
    pending_retry: Option<PendingRetry>,
    next_retry_generation: u64,
}

impl RalphLoopState {
    pub(crate) fn new(
        target: RalphLoopTarget,
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
            instance_id: Uuid::new_v4().to_string(),
            target,
            pending_retry: None,
            next_retry_generation: 0,
        }
    }

    pub(crate) fn should_continue(&self) -> bool {
        self.enabled && (self.max_iterations == 0 || self.iteration < self.max_iterations)
    }

    pub(crate) fn next_iteration(&mut self) {
        self.iteration += 1;
        self.pending_retry = None;
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn target(&self) -> &RalphLoopTarget {
        &self.target
    }

    pub(crate) fn schedule_retry(&mut self) -> u64 {
        self.next_retry_generation = self.next_retry_generation.wrapping_add(1);
        let generation = self.next_retry_generation;
        let delay_seconds = i64::try_from(self.delay_seconds).unwrap_or(i64::MAX);
        self.pending_retry = Some(PendingRetry {
            generation,
            due_unix_seconds: current_unix_seconds().saturating_add(delay_seconds),
        });
        generation
    }

    pub(crate) fn clear_pending_retry(&mut self) {
        self.pending_retry = None;
    }

    pub(crate) fn pending_retry_due_now(&self) -> Option<u64> {
        self.pending_retry
            .as_ref()
            .filter(|retry| retry.due_unix_seconds <= current_unix_seconds())
            .map(|retry| retry.generation)
    }

    pub(crate) fn matches_pending_retry(&self, instance_id: &str, generation: u64) -> bool {
        self.instance_id == instance_id
            && self
                .pending_retry
                .as_ref()
                .is_some_and(|retry| retry.generation == generation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RalphLoopCommand {
    pub(crate) max_iterations: u32,
    pub(crate) completion_promise: String,
    pub(crate) prompt: Option<String>,
    pub(crate) delay_seconds: u64,
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
                if let Some(value) = parts.get(i) {
                    max_iterations = value
                        .parse()
                        .map_err(|_| format!("Invalid max-iterations value: {value}"))?;
                } else {
                    return Err("--max-iterations requires a value".to_string());
                }
            }
            "--completion-promise" | "-c" => {
                i += 1;
                if let Some(value) = parts.get(i) {
                    completion_promise = value.clone();
                } else {
                    return Err("--completion-promise requires a value".to_string());
                }
            }
            "--delay" | "-d" => {
                i += 1;
                if let Some(value) = parts.get(i) {
                    delay_seconds = value
                        .parse()
                        .map_err(|_| format!("Invalid delay value: {value}"))?;
                } else {
                    return Err("--delay requires a value".to_string());
                }
            }
            "--prompt" | "-p" => {
                i += 1;
                if i >= parts.len() {
                    return Err("--prompt requires a value".to_string());
                }

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
                prompt = Some(prompt_parts.join(" "));
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
        (!positional_prompt_parts.is_empty()).then(|| positional_prompt_parts.join(" "))
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

pub(crate) fn check_completion_promise(output: &str, promise: &str) -> bool {
    let start_tag = "<promise>";
    let end_tag = "</promise>";
    let Some(start_idx) = output.find(start_tag) else {
        return false;
    };
    let content_start = start_idx + start_tag.len();
    let Some(end_idx) = output[content_start..].find(end_tag) else {
        return false;
    };

    let extracted = &output[content_start..content_start + end_idx];
    let normalize =
        |text: &str| -> String { text.split_whitespace().collect::<Vec<_>>().join(" ") };
    normalize(extracted) == normalize(promise)
}

pub(crate) fn ralph_state_file_path(cwd: &Path) -> PathBuf {
    cwd.join(".codex").join("ralph-loop.local.md")
}

pub(crate) fn save_ralph_state_file(cwd: &Path, state: &RalphLoopState) {
    let path = ralph_state_file_path(cwd);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let prompt_preview = truncate_text(&state.original_prompt, /*max_graphemes*/ 100);
    let max_iterations = if state.max_iterations == 0 {
        "unlimited".to_string()
    } else {
        state.max_iterations.to_string()
    };
    let content = format!(
        r#"---
ralph_loop:
  enabled: {enabled}
  iteration: {iteration}
  max_iterations: {max_iterations}
  completion_promise: "{completion_promise}"
  delay_seconds: {delay_seconds}
  started_at: "{started_at}"
---

# Ralph Loop State

- Status: {status}
- Iteration: {iteration}/{max_iterations}
- Promise: {completion_promise}
- Delay: {delay_seconds}s (on error only)
- Prompt: {prompt_preview}
"#,
        enabled = state.enabled,
        iteration = state.iteration,
        completion_promise = state.completion_promise,
        delay_seconds = state.delay_seconds,
        started_at = state.started_at,
        status = if state.enabled { "Active" } else { "Stopped" },
    );
    let _ = std::fs::write(&path, content);
}

pub(crate) fn cleanup_ralph_state_file(cwd: &Path) {
    let path = ralph_state_file_path(cwd);
    let _ = std::fs::remove_file(&path);
}

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
  2. When the agent finishes, Codex checks for <promise>COMPLETE</promise>
  3. If not found and iterations remain, the same prompt is re-submitted
  4. The agent sees prior context and file changes, then continues
  5. On error, Codex waits --delay seconds before retrying
  6. Use /cancel-ralph to stop the loop at any time"#
    )
}

fn current_unix_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn parse_basic_defaults() {
        let cmd = parse_ralph_loop_args("").expect("defaults should parse");
        assert_eq!(cmd.max_iterations, RALPH_LOOP_DEFAULT_MAX_ITERATIONS);
        assert_eq!(
            cmd.completion_promise,
            RALPH_LOOP_DEFAULT_COMPLETION_PROMISE
        );
        assert_eq!(cmd.prompt, None);
        assert_eq!(cmd.delay_seconds, RALPH_LOOP_DEFAULT_DELAY_SECONDS);
    }

    #[test]
    fn parse_with_options() {
        let cmd = parse_ralph_loop_args("--max-iterations 30 --completion-promise DONE")
            .expect("options should parse");
        assert_eq!(cmd.max_iterations, 30);
        assert_eq!(cmd.completion_promise, "DONE");
    }

    #[test]
    fn parse_with_max_alias() {
        let cmd = parse_ralph_loop_args("--max 12").expect("alias should parse");
        assert_eq!(cmd.max_iterations, 12);
    }

    #[test]
    fn parse_with_prompt_flag() {
        let cmd =
            parse_ralph_loop_args("--prompt Build REST API").expect("prompt flag should parse");
        assert_eq!(cmd.prompt, Some("Build REST API".to_string()));
    }

    #[test]
    fn parse_with_positional_prompt() {
        let cmd = parse_ralph_loop_args(r#""Build REST API" -n 10 -c DONE"#)
            .expect("positional prompt should parse");
        assert_eq!(cmd.prompt, Some("Build REST API".to_string()));
        assert_eq!(cmd.max_iterations, 10);
        assert_eq!(cmd.completion_promise, "DONE");
    }

    #[test]
    fn parse_with_delay() {
        let cmd = parse_ralph_loop_args("-d 60 -n 20").expect("delay should parse");
        assert_eq!(cmd.delay_seconds, 60);
        assert_eq!(cmd.max_iterations, 20);
    }

    #[test]
    fn parse_rejects_prompt_flag_and_positional_prompt_together() {
        let err = parse_ralph_loop_args(r#"fix tests --prompt "fix docs""#)
            .expect_err("mixed prompt forms should fail");
        assert_eq!(
            err,
            "Provide the prompt either via --prompt/-p or as positional arguments, not both"
        );
    }

    #[test]
    fn help_request_detection() {
        assert!(is_ralph_loop_help_request("help"));
        assert!(is_ralph_loop_help_request("HELP"));
        assert!(is_ralph_loop_help_request("-h"));
        assert!(is_ralph_loop_help_request("--help"));
        assert!(!is_ralph_loop_help_request("implement feature"));
    }

    #[test]
    fn completion_promise_requires_tag() {
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
    fn state_should_continue_until_limit() {
        let mut state = RalphLoopState::new(
            RalphLoopTarget::ActiveThread,
            /*max_iterations*/ 5,
            "COMPLETE".into(),
            "test".into(),
            /*delay_seconds*/ 0,
        );
        assert!(state.should_continue());
        state.iteration = 5;
        assert!(!state.should_continue());
    }

    #[test]
    fn unlimited_iterations_keep_running() {
        let state = RalphLoopState::new(
            RalphLoopTarget::ActiveThread,
            /*max_iterations*/ 0,
            "COMPLETE".into(),
            "test".into(),
            /*delay_seconds*/ 0,
        );
        assert!(state.should_continue());
    }

    #[test]
    fn pending_retry_matches_instance_and_generation() {
        let mut state = RalphLoopState::new(
            RalphLoopTarget::ActiveThread,
            /*max_iterations*/ 2,
            "COMPLETE".into(),
            "test".into(),
            /*delay_seconds*/ 0,
        );
        let generation = state.schedule_retry();
        assert!(state.matches_pending_retry(state.instance_id(), generation));
        assert!(!state.matches_pending_retry("other-instance", generation));
        assert!(!state.matches_pending_retry(state.instance_id(), generation + 1));
    }
}
