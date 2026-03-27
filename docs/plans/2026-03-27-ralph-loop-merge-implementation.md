# Ralph Loop Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve the local `ralph-loop` workflow on the upstream-aligned TUI shell without replaying large historical `chatwidget.rs` patches.

**Architecture:** Keep `ralph_loop.rs` as the feature-owned helper for parsing, promise detection, and state-file persistence. Reattach only the minimal `ChatWidget` and `App` seams that drive turn-start reset, error marking, turn-complete continuation, and delayed retry dispatch. Do not change `ServerOverloaded` behavior in this slice.

**Tech Stack:** Rust, ratatui TUI, Tokio timers, cargo test, `just fmt`, crate-scoped `just fix`

---

### Task 1: Preserve the helper and command boundary

**Files:**
- Verify/modify: `codex-rs/tui/src/ralph_loop.rs`
- Verify/modify: `codex-rs/tui/src/slash_command.rs`
- Verify/modify: `codex-rs/tui/src/lib.rs`

**Step 1: Keep `ralph_loop.rs` feature-owned**

The helper layer should continue to own:

- inline argument parsing for `/ralph-loop`
- completion-promise parsing and normalization
- local state-file path and persistence helpers
- user-facing help text

Do not move broader turn lifecycle logic into this module.

**Step 2: Tighten helper tests only if needed**

Keep the existing parser/state tests in `codex-rs/tui/src/ralph_loop.rs`
passing. If current helper tests are too weak for the state-file helpers, add
small local tests there instead of pushing file-helper assertions into
`chatwidget/tests.rs`.

**Step 3: Run helper-focused verification**

Run:

```bash
cd codex-rs && cargo test -p codex-tui ralph_loop -- --nocapture
```

Expected: PASS.

### Task 2: Add focused lifecycle regression tests before changing runtime hooks

**Files:**
- Modify: `codex-rs/tui/src/chatwidget/tests.rs`

**Step 1: Add a completion-path regression test**

Add a test that activates a Ralph Loop, simulates a completed turn whose final
agent message contains the configured promise, and asserts that:

- `ralph_loop_state` is cleared
- `ralph_loop_turn_had_error` is reset
- no follow-up prompt is queued
- the state file is removed when `current_cwd` is set

Name it with a `ralph_loop_` prefix so it is easy to target.

**Step 2: Add an error-delay regression test**

Add a test that sets up an active Ralph Loop with a short nonzero delay,
simulates a generic error turn followed by task completion, and asserts that:

- the loop advances to the next iteration
- `AppEvent::RalphLoopDelayedContinue` is emitted after the delay
- the prompt is not immediately queued on the error-delay path

Use a short delay suitable for tests; do not change production defaults.

**Step 3: Add a delayed-continue or cancel regression test**

Add one more focused regression test for one of these seams:

- `handle_ralph_loop_delayed_continue()` requeues the original prompt
- `/cancel-ralph` clears in-memory and on-disk state

Prefer the cheaper seam that uses existing test helpers cleanly.

**Step 4: Run the targeted lifecycle tests**

Run:

```bash
cd codex-rs && cargo test -p codex-tui ralph_loop_ -- --nocapture
```

Expected: PASS.

If one of the new tests already passes before any runtime code change, keep the
test and treat that seam as already preserved.

### Task 3: Reattach only the minimal runtime hooks if tests expose drift

**Files:**
- Modify if needed: `codex-rs/tui/src/chatwidget.rs`
- Modify if needed: `codex-rs/tui/src/app.rs`
- Modify if needed: `codex-rs/tui/src/app_event.rs`

**Step 1: Keep the `ChatWidget` state surface narrow**

Preserve only these state slots:

- `ralph_loop_state: Option<RalphLoopState>`
- `ralph_loop_turn_had_error: bool`

Do not spread Ralph Loop state into unrelated status, replay, or app-server
structures.

**Step 2: Keep the lifecycle hook set minimal**

If the new tests fail, restore only these seams:

- `on_task_started` clears `ralph_loop_turn_had_error`
- `on_error` sets `ralph_loop_turn_had_error = true`
- `on_task_complete` calls the Ralph Loop continuation helper before the normal
  queued-input drain
- `handle_ralph_loop_delayed_continue` requeues the original prompt

Do not reapply unrelated historical `chatwidget.rs` behavior while fixing these
hooks.

**Step 3: Keep app-level glue small**

If needed, preserve only:

- `AppEvent::RalphLoopDelayedContinue`
- the `app.rs` branch that forwards that event back into `chat_widget`

Do not introduce a broader timer manager or feature-specific app runtime.

**Step 4: Guard the behavior boundary**

Do not change these semantics in this slice:

- `ServerOverloaded` still follows its current path
- delayed retry still depends on the existing `Error -> TurnComplete` contract
- the prompt still re-enters through the normal queued-input path

**Step 5: Re-run the targeted Ralph Loop tests**

Run:

```bash
cd codex-rs && cargo test -p codex-tui ralph_loop -- --nocapture
```

Expected: PASS.

### Task 4: Verify the slice without pulling in other workbench features

**Files:**
- Verify touched files under `codex-rs/tui/**`
- Update if needed: `docs/plans/2026-03-26-tui-workbench-analysis.md`

**Step 1: Guard the scope**

This slice should not introduce or revive:

- `git_graph_widget`
- `session_bar`
- `team_profile`
- `team_profile_vouch`
- `model_sub_vouch`

**Step 2: Run full crate verification**

Run:

```bash
cd codex-rs && cargo test -p codex-tui
```

Expected: PASS.

If any new snapshots were intentionally updated, also run:

```bash
cd codex-rs && cargo insta pending-snapshots -p codex-tui
```

Expected: no unreviewed snapshot drift remains.

**Step 3: Run required hygiene after Rust code changes**

Run:

```bash
cd codex-rs && just fmt
cd codex-rs && just fix -p codex-tui
PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Do not rerun tests after `fmt` / `fix`.
