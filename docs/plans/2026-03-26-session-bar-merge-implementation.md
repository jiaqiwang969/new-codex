# Session Bar Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve the local `session_bar` workflow while reattaching it onto the newer upstream TUI shell with the smallest possible `app.rs` diff.

**Architecture:** Keep `session_bar.rs`, `session_utils.rs`, and the optional `session_alias_manager.rs` as the feature-owned helpers. Limit runtime wiring to four `App` seams only: startup prefetch, focus switching, render layout split, and selection dispatch to `NewSession` or `ResumeSession`. Do not bundle `ralph-loop`, `git-graph`, `team_profile`, or `model_sub_vouch` into this slice.

**Tech Stack:** Rust, ratatui TUI, cargo test, `just fmt`, crate-scoped `just fix`

---

### Task 1: Preserve the helper modules as the feature boundary

**Files:**
- Verify/modify: `codex-rs/tui/src/session_bar.rs`
- Verify/modify: `codex-rs/tui/src/session_utils.rs`
- Verify/modify: `codex-rs/tui/src/session_alias_manager.rs`
- Verify/modify: `codex-rs/tui/src/lib.rs`

**Step 1: Keep the helper boundary narrow**

The helper layer should continue to own:

- session metadata caching under `session_bar_cache.v2.json`
- cwd-scoped session discovery from `~/.codex/sessions/**/*.jsonl`
- selection state and rendering for the bar
- optional alias persistence under `~/.codex/session_aliases.json`

It should not absorb broader `App` lifecycle logic.

**Step 2: Preserve or tighten the existing helper tests**

Existing test anchors already exist in:

- `session_utils::tests::session_details_cache_round_trip`
- `session_utils::tests::session_details_cache_ignores_old_version`
- `session_alias_manager::tests::test_alias_operations`

If helper extraction or cleanup is needed, keep these tests green or replace
them with equivalent coverage close to the helper code.

**Step 3: Run the helper-focused tests first**

Run:

```bash
cd codex-rs && cargo test -p codex-tui session_details_cache test_alias_operations -- --nocapture
```

Expected: PASS.

### Task 2: Reattach only the minimal `App` integration points

**Files:**
- Modify: `codex-rs/tui/src/app.rs`
- Modify if needed: `codex-rs/tui/src/app_event.rs`

**Step 1: Keep startup prefetch lightweight**

Preserve the current shape where `App`:

- constructs `SessionBar::new(config.cwd, config.codex_home)`
- warms session metadata in the background
- applies the results through `AppEvent::SessionBarPrefetched`

Do not expand this into a broader background-task framework.

**Step 2: Keep focus switching local to `App`**

Preserve only the minimal focus semantics:

- `Ctrl+P` toggles between chat and session-bar focus
- `Esc` exits session-bar focus
- arrow / tab-style navigation is handled while the bar is focused

If `app.rs` needs cleanup, prefer a small helper such as:

```rust
fn toggle_session_bar_focus(&mut self)
```

instead of scattering more inline branches.

**Step 3: Keep layout integration minimal**

Preserve the bottom-strip render split only:

- normal chat layout when the bar is unfocused
- expanded session-bar area when it is focused
- no extra coupling to unrelated overlays or status panes

**Step 4: Keep dispatch semantics stable**

`session_bar_enter_event()` should remain the stable seam:

- if the synthetic “new tab” row is selected, emit `AppEvent::NewSession`
- if a cached history row is selected, emit `AppEvent::ResumeSession`

Do not fold resume/new selection logic directly into key handlers.

**Step 5: Run the targeted app tests**

Run:

```bash
cd codex-rs && cargo test -p codex-tui session_bar_enter_event_ -- --nocapture
```

Expected: PASS.

### Task 3: Add only the missing app-side regression coverage

**Files:**
- Modify if needed: `codex-rs/tui/src/app.rs`

**Step 1: Keep the current selection-dispatch tests**

Preserve:

- `session_bar_enter_event_uses_resume_for_selected_history`
- `session_bar_enter_event_uses_new_for_new_tab`

**Step 2: Add one focused regression test only if the seam is cheap**

If upstream TUI layout/focus refactoring makes it easy, add one small test for
one of these behaviors:

- `SessionBarPrefetched` updates the bar cache only when the cwd still matches
- toggling focus resets the current selection predictably

Do not force invasive render scaffolding into `app.rs` just to test every key.

**Step 3: Re-run the session-bar slice tests**

Run:

```bash
cd codex-rs && cargo test -p codex-tui session_bar -- --nocapture
```

Expected: PASS.

### Task 4: Verify the slice without pulling in other Block 7 features

**Files:**
- Verify touched files under `codex-rs/tui/**`
- Update if needed: `docs/plans/2026-03-26-tui-workbench-analysis.md`

**Step 1: Guard the scope**

This slice should not introduce or revive:

- `ralph_loop`
- `git_graph_widget`
- `team_profile`
- `team_profile_vouch`
- `model_sub_vouch`

**Step 2: Run crate verification for the final seam**

Run:

```bash
cd codex-rs && cargo test -p codex-tui
```

Expected: PASS.

**Step 3: Run required hygiene after code changes**

Run:

```bash
cd codex-rs && just fmt
cd codex-rs && just fix -p codex-tui
PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Do not rerun tests after `fmt` / `fix`.
