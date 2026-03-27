# Ralph Loop Merge Design

**Date:** 2026-03-27
**Scope:** preserve the local `ralph-loop` workflow while keeping the upstream
TUI lifecycle shape as intact as possible

## Goal

Keep `ralph-loop` as a local-only TUI workflow feature during the upstream
alignment work, but narrow its maintenance surface to the smallest set of
runtime hook points needed for it to keep working.

This slice is about preserving the existing behavior, not redesigning it.

## Non-Negotiable Semantics

The following behavior must remain true after this slice:

1. `/ralph-loop` starts an iterative loop over the same original prompt.
2. `/cancel-ralph` cancels the active loop and clears local state.
3. Completion still depends on matching `<promise>...</promise>` against the
   configured completion promise.
4. The loop stops at the configured max iteration count.
5. `.codex/ralph-loop.local.md` remains the local persistence file.
6. Generic error turns still honor `--delay` by scheduling the next iteration
   after the configured wait.

## Explicit Non-Goals

This slice must not:

- replay large historical `chatwidget.rs` hunks just because they used to work
- add new app-server or protocol surface for loop state
- change the current `ServerOverloaded` semantics
- change core turn semantics or assume new guarantees from `codex-core`
- bundle `git-graph`, `session_bar`, `team_profile`, or `model_sub_vouch`

`ServerOverloaded` currently does not set `ralph_loop_turn_had_error`, so it
does not participate in delayed retry semantics. That mismatch is real, but it
is a separate behavior decision and should not be folded into this merge slice.

## Rejected Approaches

### 1. Reapply the old `chatwidget.rs` patch shape wholesale

This is the easiest way to keep the feature compiling and the worst way to keep
future upstream merges manageable. `chatwidget.rs` is already one of the most
conflict-heavy files in the branch.

### 2. Expand the feature while reattaching it

Fixing `ServerOverloaded`, redefining replaced-turn behavior, or changing the
state-file format would make the slice harder to reason about and harder to
verify. The merge goal is continuity, not improvement.

### 3. Drop `ralph-loop`

That would reduce merge burden, but it would also discard a real local workflow
the branch has intentionally kept alongside the other workbench features.

## Recommended Approach

Preserve the feature by keeping the helper and command surface intact, then
reattaching only the minimal lifecycle seams required by the current upstream
TUI shell.

### Feature-owned boundary

Keep these pieces owned by the feature itself:

- `codex-rs/tui/src/ralph_loop.rs`
  - argument parsing
  - completion-promise matching
  - state-file path and persistence helpers
- `codex-rs/tui/src/slash_command.rs`
  - slash-command registration and descriptions

### Minimal runtime seams

Keep the runtime integration limited to these hook points:

- `ChatWidget::on_task_started`
  - clear the per-turn error marker
- `ChatWidget::on_error`
  - mark the turn as errored for later delayed retry handling
- `ChatWidget::on_task_complete`
  - run the Ralph Loop continuation helper before the normal queued-input drain
- `ChatWidget::handle_ralph_loop_delayed_continue`
  - requeue the original prompt via the normal queued-input path
- `AppEvent::RalphLoopDelayedContinue`
- `App` forwarding that event back into `ChatWidget`

No broader background-task or thread-lifecycle abstraction is needed for this
slice.

## Hidden Dependency To Preserve

The current implementation relies on a core turn contract:

- a turn that emits `Error` still reaches `TurnComplete`

That contract matters because delayed retry is split across two stages:

1. `on_error()` only sets `ralph_loop_turn_had_error = true`
2. `on_task_complete_for_ralph_loop()` later reads that flag and decides whether
   to queue immediately or schedule `AppEvent::RalphLoopDelayedContinue`

This slice should preserve that dependency, not attempt to redesign it.

## Testing Strategy

The existing helper coverage is not enough for lifecycle-heavy behavior, so the
merge slice should add focused `chatwidget` regression tests close to the real
hook points.

The priority regression cases are:

1. completion promise stops the loop and clears persisted state
2. generic error plus delay schedules `RalphLoopDelayedContinue`
3. delayed-continue handling requeues the original prompt
4. cancel clears in-memory and on-disk state

If those tests pass without runtime edits, treat that as success and avoid
churn.

## Validation Standard

The slice is complete only if:

- the feature stays scoped to `ralph_loop.rs`, `slash_command.rs`, minimal
  `chatwidget.rs` hooks, and the small app-event seam
- the new lifecycle regression tests pass
- `cargo test -p codex-tui` still passes after the change
- required Rust hygiene commands pass without reopening the feature scope
