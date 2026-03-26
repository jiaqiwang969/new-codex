# Ralph Loop Lifecycle Analysis

**Date:** 2026-03-26
**Scope:** local `ralph-loop` TUI feature in the upstream-merge branch

## Summary

`ralph-loop` is a real local-only workflow feature, but it is the highest-risk
piece of Block 7 because it is not isolated behind a small UI shell.

It currently depends on several `ChatWidget` lifecycle hooks plus one core turn
contract:

- `Error` is still followed by `TurnComplete`
- the queued-input pipeline remains the way follow-up turns are launched
- turn-complete remains the canonical point where post-turn logic runs

If upstream changes any of those assumptions while refactoring `chatwidget.rs`,
`ralph-loop` can silently degrade even when parser/unit tests still pass.

## Current Feature Shape

**Primary files**

- `codex-rs/tui/src/ralph_loop.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app_event.rs`

**What it does**

- Adds `/ralph-loop` and `/cancel-ralph`
- Replays the same prompt until the model emits
  `<promise>COMPLETE</promise>` or max iterations are reached
- Persists local loop state to `.codex/ralph-loop.local.md`
- Optionally waits `--delay` seconds before retrying after an error turn

## Exact Integration Surface

### 1. Slash-command registration

- `SlashCommand::RalphLoop`
- `SlashCommand::CancelRalph`
- inline-arg parsing path in `dispatch_command_with_args`

This part is low risk. It is mostly registration + parser dispatch.

### 2. ChatWidget state

`ChatWidget` carries:

- `ralph_loop_state: Option<RalphLoopState>`
- `ralph_loop_turn_had_error: bool`

These are local-only fields and must survive any future `ChatWidget` field
re-layout.

### 3. Turn lifecycle hooks

`ralph-loop` is wired into these methods:

- `on_task_started`
  - clears `ralph_loop_turn_had_error`
- `on_error`
  - sets `ralph_loop_turn_had_error = true`
- `on_task_complete`
  - calls `on_task_complete_for_ralph_loop(&last_agent_message)` before draining
    queued follow-up input
- `handle_ralph_loop_delayed_continue`
  - requeues the original prompt via the normal queued-input path

### 4. App-level delayed retry event

The only app-level glue is:

- `AppEvent::RalphLoopDelayedContinue`
- `app.rs` forwarding that event back into `chat_widget`

This part is small. The risky part is still inside `chatwidget.rs`.

## Hidden Dependency On Core Turn Semantics

The feature currently assumes that a failed turn still ends with
`TurnComplete`.

Evidence:

- `codex-rs/core/src/tasks/mod.rs` emits `TurnComplete` at turn end
- `codex-rs/core/tests/suite/stream_error_allows_next_turn.rs` explicitly
  asserts `Error` followed by `TurnComplete`

Why it matters:

- `on_error()` only marks `ralph_loop_turn_had_error = true`
- the actual retry scheduling happens later in
  `on_task_complete_for_ralph_loop()`
- if `TurnComplete` stops arriving after error turns, delayed retry logic stops
  running

This is the single most important merge-risk fact for this feature.

## Current Behavioral Nuances

### 1. Error-delay path relies on TurnComplete

The help text says:

- on error, wait `--delay` seconds before retry

Implementation reality:

- `on_error()` marks the turn as having errored
- `on_task_complete_for_ralph_loop()` later reads that flag and schedules the
  delayed retry

So the behavior is correct only as long as the `Error -> TurnComplete` contract
holds.

### 2. `ServerOverloaded` does not currently count as a delayed-retry error

`EventMsg::Error` routes `ServerOverloaded` into `on_server_overloaded_error()`,
not `on_error()`.

That means:

- `ralph_loop_turn_had_error` is **not** set for overloaded responses
- if `TurnComplete` follows, `ralph-loop` will immediately continue instead of
  honoring the configured retry delay

This is not necessarily a merge blocker, but it is a real semantic mismatch
between the help text and the current behavior.

### 3. Replaced-turn aborts are not a stable continuation path

`TurnAbortReason::Replaced` is handled by `on_error("Turn aborted: replaced by a new task")`.
That path finalizes the turn locally, but the durable continuation semantics for
`ralph-loop` are not clearly defined there.

This is another reason not to spread the feature deeper into new upstream
control flow unless needed.

## Test Coverage Reality

Current passing tests cover:

- `ralph_loop.rs` parser/state helpers
- help/parse-error UI snapshots

Current coverage does **not** verify:

- successful multi-iteration continuation
- delayed retry after error
- cancel flow during an active loop
- state-file cleanup after completion/cancel/error
- server-overloaded semantics

So the feature is presently under-tested relative to its lifecycle coupling.

## Merge Recommendation

Preserve the feature only by keeping its semantics and reattaching the smallest
possible hook set:

1. keep `ralph_loop.rs`
2. keep slash-command registration
3. keep one chatwidget state slot
4. reattach only these lifecycle hooks:
   - turn start
   - error path
   - turn complete
   - delayed retry dispatch

Do **not** preserve giant `chatwidget.rs` hunks verbatim.

## Recommended Follow-up

If `ralph-loop` remains in the branch after the upstream merge stabilizes,
add targeted behavior tests for:

- completion-promise loop success
- delayed retry after generic error
- cancel path
- state-file cleanup
- explicit decision for `ServerOverloaded`
