# Guardian Review Replay Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add app-server v2 replay support for guardian approval review state by attaching it to replayable thread items and rendering it in TUI history.

**Architecture:** Reuse the existing `GuardianApprovalReview` shape as a `guardianReview` field on replayable tool items, populate it from persisted `GuardianAssessment` events in `thread_history.rs`, and have TUI replay route that item state through the existing guardian assessment rendering flow. Keep current standalone live notifications for compatibility.

**Tech Stack:** Rust, serde/schemars/ts-rs protocol types, app-server rollout replay builder, ratatui TUI snapshots, `cargo test`, `just` repo helpers

---

### Task 1: Add protocol tests for guardian review replay

**Files:**
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Test: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`

**Step 1: Write the failing test**

Add replay tests that build rollout items with:

- `TurnStarted`
- matching tool event (`ExecCommandEnd`, `PatchApplyEnd`, or `McpToolCallEnd`)
- `GuardianAssessment`
- `TurnComplete`

Assert the resulting `ThreadItem` includes `guardian_review: Some(...)` with the expected status/risk/rationale.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-app-server-protocol guardian_review`

Expected: FAIL because replayed `ThreadItem` variants do not yet expose or populate `guardian_review`.

**Step 3: Write minimal implementation**

Add the new field to eligible `ThreadItem` variants and update replay builder helpers to attach guardian review data by item id.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-app-server-protocol guardian_review`

Expected: PASS

**Step 5: Commit**

Run:

```bash
git add codex-rs/app-server-protocol/src/protocol/thread_history.rs codex-rs/app-server-protocol/src/protocol/v2.rs
git commit -m "feat: replay guardian review state on thread items"
```

### Task 2: Keep app-server/TUI replay behavior consistent

**Files:**
- Modify: `codex-rs/tui_app_server/src/chatwidget.rs`
- Test: `codex-rs/tui_app_server/src/chatwidget/tests.rs`

**Step 1: Write the failing test**

Add a replay-oriented TUI test that feeds a `ThreadItem::CommandExecution` or `ThreadItem::FileChange` carrying `guardian_review: Some(...)` and asserts the same guardian approved/denied history cell snapshot produced by live notifications.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-tui-app-server guardian_review`

Expected: FAIL because replayed items currently ignore guardian review state.

**Step 3: Write minimal implementation**

When replaying eligible `ThreadItem`s, route embedded `guardian_review` through the existing `on_guardian_assessment(...)` pathway without disturbing live notification handling.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-tui-app-server guardian_review`

Expected: PASS

**Step 5: Commit**

Run:

```bash
git add codex-rs/tui_app_server/src/chatwidget.rs codex-rs/tui_app_server/src/chatwidget/tests.rs
git commit -m "feat: replay guardian review state in tui"
```

### Task 3: Regenerate protocol/docs and run scoped verification

**Files:**
- Modify: `codex-rs/app-server/README.md`
- Modify: generated app-server schema outputs from `just write-app-server-schema`

**Step 1: Write the failing test**

No new behavior test here. Treat schema/doc drift as the failure surface.

**Step 2: Run verification to observe drift**

Run:

```bash
cargo test -p codex-app-server-protocol
```

Expected: Either PASS with outdated docs/schema still pending, or reveal any serialization regressions before schema regeneration.

**Step 3: Write minimal implementation**

Document `guardianReview` on relevant replayable thread items, then regenerate app-server schema fixtures.

**Step 4: Run verification to verify it passes**

Run:

```bash
cd codex-rs && just write-app-server-schema
just fmt
cargo test -p codex-app-server-protocol
cargo test -p codex-tui-app-server guardian_review
just argument-comment-lint
```

Expected: PASS, with schema fixtures and docs updated.

**Step 5: Commit**

Run:

```bash
git add codex-rs/app-server/README.md codex-rs/app-server-protocol codex-rs/tui_app_server
git commit -m "docs: describe guardian review replay"
```
