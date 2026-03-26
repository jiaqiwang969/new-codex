# TUI Workbench Analysis

**Date:** 2026-03-26
**Scope:** Block 7 from `docs/plans/2026-03-24-upstream-merge-execution-playbook.md`

## Summary

The Block 7 workbench features are real local differentiators, not stale merge
noise:

1. `session_bar`
2. `git-graph`
3. `ralph-loop`

Upstream `main` does not contain these feature files or matching entry points.
That means the merge question is not "which implementation wins"; it is whether
to keep carrying each local UX feature, and if so, how to reattach it onto the
newer upstream TUI shell without replaying large `app.rs` / `chatwidget.rs`
diffs.

This audit also exposed adjacent local-only TUI features that should not be
merged blindly as part of Block 7:

- `team_profile`
- `model_sub_vouch`
- `session_alias_manager`

The first two are better treated as Block 3 follow-on UX for `model_sub`; only
`session_alias_manager` is directly coupled to `session_bar`.

## Feature Inventory

### 1. Session Bar

**Primary files**

- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/session_utils.rs`
- `codex-rs/tui/src/session_alias_manager.rs`
- `codex-rs/tui/src/app.rs`

**What it does**

- Adds a bottom "tmux-like" session strip for the current working directory.
- Uses `Ctrl+P` to toggle focus between the chat pane and session bar.
- Shows recent sessions rooted under the current cwd.
- Lets the user resume an existing session or start a new one from the strip.
- Persists optional user-defined aliases in `~/.codex/session_aliases.json`.

**Why it exists**

- Faster local navigation than reopening the resume picker.
- Keeps the user's current project context visible while switching threads.

**Current implementation shape**

- `SessionBar` maintains cached session metadata and selection state.
- `session_utils.rs` scans `~/.codex/sessions/**/*.jsonl`, filters by cwd, and
  maintains a small cache file `session_bar_cache.v2.json`.
- `app.rs` owns focus switching and routes `Ctrl+P` / arrow-style navigation to
  the bar.

**Merge risk**

- Medium.
- The feature is mostly TUI-local, but it hooks into `App` focus management,
  thread selection, and render layout.
- The conflict risk is concentrated in `codex-rs/tui/src/app.rs`, not in the
  helper files.

**Recommendation**

- Keep the feature.
- Preserve the helper files almost as-is.
- Reattach only the minimal `App` integration points:
  - focus toggle
  - render area split
  - session selection -> resume/new dispatch
- Treat `session_alias_manager` as optional polish if app integration gets
  noisy; the bar itself is the primary value.

### 2. Git Graph Overlay

**Primary files**

- `codex-rs/git-graph/**`
- `codex-rs/tui/src/git_graph_widget.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/Cargo.toml`
- `codex-rs/tui/Cargo.toml`
- `codex-rs/Cargo.lock`
- `MODULE.bazel.lock`

**What it does**

- Adds a `Ctrl+G` overlay showing repository history as an interactive graph.
- Uses a vendored `git-graph` crate when available.
- Falls back to shelling out to `git log --graph` and converting ASCII output
  into a Unicode-styled overlay.

**Why it exists**

- Gives the user an in-TUI repository topology view during agent work.

**Current implementation shape**

- `app.rs` handles the hotkey and opens an overlay.
- `git_graph_widget.rs` prepares display lines and refresh callbacks.
- The feature brings in a whole workspace crate, plus lockfile and Bazel lock
  updates.

**Merge risk**

- Medium to high.
- The runtime hook in `app.rs` is small, but the dependency footprint is wider
  than the other Block 7 features.
- Carrying this feature means intentionally carrying the local `git-graph`
  workspace member too.

**Recommendation**

- Keep it only if the team still values the feature enough to justify the extra
  crate and lockfile churn.
- If kept, merge it as its own sub-block after the TUI runtime stabilizes.
- Do not bundle it with `session_bar` or `ralph-loop`.
- If dependency weight becomes a problem, the fallback `git log --graph`
  behavior suggests a lighter future direction without the vendored crate.

### 3. Ralph Loop

**Primary files**

- `codex-rs/tui/src/ralph_loop.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/app.rs`

**What it does**

- Adds `/ralph-loop` and `/cancel-ralph`.
- Replays the same prompt until the model emits
  `<promise>COMPLETE</promise>` or a max-iteration limit is reached.
- Persists loop state to `.codex/ralph-loop.local.md`.
- Optionally delays retries after error turns.

**Why it exists**

- Supports iterative self-correction loops without manually retyping follow-up
  prompts.

**Current implementation shape**

- `ralph_loop.rs` owns parsing, state, and state-file helpers.
- `chatwidget.rs` owns the operational lifecycle:
  - start loop
  - cancel loop
  - detect completion on task finish
  - queue delayed retries after errors
- `app.rs` only forwards the delayed retry event.

**Merge risk**

- High.
- The feature is TUI-only, but it is wired directly into task completion and
  queued message flow inside `chatwidget.rs`, which is already one of the most
  conflict-heavy files in this merge.

**Recommendation**

- Keep the behavior only if it is still considered an important local workflow.
- Reapply the feature by preserving the local lifecycle semantics, not the old
  `chatwidget.rs` hunks.
- The safest split is:
  1. keep `ralph_loop.rs`
  2. keep slash-command registration
  3. re-wire completion/error handling against the current upstream
     `chatwidget.rs`

## Adjacent Local-Only Features Found During Audit

### Team Profile / Model Sub Vouch

**Primary files**

- `codex-rs/tui/src/team_profile.rs`
- `codex-rs/tui/src/team_profile_vouch.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/app.rs`

**What they are**

- Local TUI controls for choosing and scoring leader/submodel routing presets.
- These are not generic workbench features; they are UX on top of local
  `model_sub` strategy.

**Recommendation**

- Do not merge these as part of Block 7.
- Reclassify them as Block 3 follow-on UX, after the core `model_sub` routing
  line is stable.

### Session Alias Manager

**Primary file**

- `codex-rs/tui/src/session_alias_manager.rs`

**What it is**

- A small local persistence layer used only by `session_bar`.

**Recommendation**

- Keep only if `session_bar` is kept.
- Treat it as optional quality-of-life state, not as a merge driver.

## Recommended Sub-Block Order Inside Block 7

1. `session_bar`
2. `ralph-loop`
3. `git-graph`

Reasoning:

- `session_bar` has the clearest user value and lowest dependency footprint.
- `ralph-loop` is valuable but deeply coupled to `chatwidget.rs`.
- `git-graph` has the widest dependency blast radius and is easiest to isolate.

## Main Conflict Hotspots

- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/Cargo.toml`
- `codex-rs/Cargo.lock`
- `MODULE.bazel.lock`

## Decision Notes For The Merge

- Do not replay large local `app.rs` or `chatwidget.rs` chunks just because the
  feature is local.
- Preserve feature semantics, then express them in the newer upstream layout.
- Treat `team_profile` / `model_sub_vouch` as separate from workbench UX even
  though they currently surface in TUI.
- Treat `git-graph` as a conscious dependency decision, not a small UI patch.
