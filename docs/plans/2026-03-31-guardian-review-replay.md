# Guardian Review Replay Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Status (2026-03-31): Implemented on `main`.**
> The plan below has already been completed by `452f8e13e2`
> (`feat: replay guardian approval reviews`).
> Keep it as a historical execution record, not as an open task list.

**Goal:** Add app-server v2 replay support for guardian approval review state by reconstructing it as a replayable thread item and rendering it in TUI history.

**Architecture:** Reconstruct persisted `GuardianAssessment` events as a replay-only `guardianApprovalReview` thread item carrying `{id, targetItemId, review, action}`. TUI replay routes that item through the existing guardian assessment rendering flow. Keep current standalone live notifications for compatibility.

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

Assert the resulting `ThreadItem` is a `guardianApprovalReview` item with the expected `targetItemId`, status, risk metadata, and action payload.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-app-server-protocol guardian_review`

Expected: FAIL because replay does not yet emit `guardianApprovalReview` items.

**Step 3: Write minimal implementation**

Add the new `guardianApprovalReview` replay item variant and update replay builder helpers to upsert it from guardian assessment events.

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

Add a replay-oriented TUI test that feeds a `guardianApprovalReview` thread item and asserts the same guardian approved/denied history cell snapshot path produced by live notifications.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-tui guardian_review`

Expected: FAIL because replayed guardian review items are not yet recognized by the TUI.

**Step 3: Write minimal implementation**

When replaying `guardianApprovalReview`, route it through the existing `on_guardian_assessment(...)` pathway without disturbing live notification handling.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-tui guardian_review`

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
just write-app-server-schema
just fmt
cargo test -p codex-app-server-protocol
cargo test -p codex-tui guardian_review
just argument-comment-lint
```

Expected: PASS, with schema fixtures and docs updated.

**Step 5: Commit**

Run:

```bash
git add codex-rs/app-server/README.md codex-rs/app-server-protocol codex-rs/tui_app_server
git commit -m "docs: describe guardian review replay"
```
