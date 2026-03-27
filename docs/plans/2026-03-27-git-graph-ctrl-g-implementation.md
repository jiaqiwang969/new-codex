# Git Graph Ctrl+G Realignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restore the official TUI `Ctrl+G` external-editor flow and stop the
custom git-graph overlay from owning that shortcut.

**Architecture:** Reintroduce a small helper in `app.rs` that marks the
external editor as requested and reuses the existing draw-loop handoff to
`AppEvent::LaunchExternalEditor`. Replace the current `Ctrl+G` key branch with
that official path and cover it with a focused unit test.

**Tech Stack:** Rust, ratatui/crossterm TUI, existing app unit tests

---

### Task 1: Add a failing regression test

**Files:**
- Modify: `codex-rs/tui/src/app.rs`

**Step 1: Write the failing test**

Add a unit test that calls a dedicated `Ctrl+G` helper on `App` and asserts:

- it returns `true`
- `external_editor_state()` becomes `Requested`
- `overlay` stays `None`

**Step 2: Run test to verify it fails**

Run:

```bash
cd codex-rs && cargo test -p codex-tui ctrl_g_requests_external_editor_when_available -- --nocapture
```

Expected: FAIL because the helper does not exist yet.

### Task 2: Restore the official TUI path

**Files:**
- Modify: `codex-rs/tui/src/app.rs`

**Step 1: Implement the minimal code**

- restore the missing external-editor hint constant
- add a small helper that requests the external editor when the official
  preconditions hold
- update the `Ctrl+G` key branch to use that helper instead of creating the
  git-graph overlay

**Step 2: Run the focused test**

Run:

```bash
cd codex-rs && cargo test -p codex-tui ctrl_g_requests_external_editor_when_available -- --nocapture
```

Expected: PASS

### Task 3: Verify the touched crate

**Files:**
- Modify: `codex-rs/tui/src/app.rs`
- Modify: `codex-rs/tui/src/lib.rs`
- Modify: `codex-rs/tui/Cargo.toml`
- Modify: `codex-rs/Cargo.toml`

**Step 1: Run targeted crate tests**

Run:

```bash
cd codex-rs && cargo test -p codex-tui
```

Expected: PASS

**Step 2: Run hygiene**

Run:

```bash
cd codex-rs && just fmt
cd codex-rs && just fix -p codex-tui
PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Expected: PASS, or only unrelated pre-existing warnings.

**Step 3: Park git-graph outside the active Rust workspace**

- remove `mod git_graph_widget;` from `codex-rs/tui/src/lib.rs`
- remove the direct `git-graph` dependency from `codex-rs/tui/Cargo.toml`
- remove the parked `git-graph` member/dependency entries from
  `codex-rs/Cargo.toml`
- keep the `codex-rs/git-graph/` source tree in place for future reconsideration
