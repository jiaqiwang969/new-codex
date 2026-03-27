# Agent Guards Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the orphaned `codex-rs/core/src/agent/guards.rs` module so `registry.rs` remains the single live ownership path for agent spawn guards.

**Architecture:** Keep the change surgical. Do not refactor runtime logic. Remove the dead module declaration and file, then verify through focused `codex-core` agent-registry tests plus required formatting/linting.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just argument-comment-lint`

---

### Task 1: Lock scope to the orphaned module

**Files:**
- Modify: `codex-rs/core/src/agent/mod.rs`
- Delete: `codex-rs/core/src/agent/guards.rs`

**Step 1: Confirm the live ownership path**

Verify that `agent/mod.rs` re-exports spawn-depth helpers from `registry.rs`
and that runtime call sites use `registry`-backed behavior.

**Step 2: Remove the orphaned module declaration**

Delete `mod guards;` from `codex-rs/core/src/agent/mod.rs`.

**Step 3: Delete the unused module**

Delete `codex-rs/core/src/agent/guards.rs`.

### Task 2: Verify the live path still builds

**Files:**
- Test: `codex-rs/core/src/agent/registry_tests.rs`

**Step 1: Run focused agent tests**

Run:

```bash
cargo test -p codex-core registry_tests
```

Expected: PASS

**Step 2: Run required formatting and linting**

Run:

```bash
cd codex-rs && just fmt
cd codex-rs && just argument-comment-lint
```

Expected: PASS

### Task 3: Record the cleanup slice

**Files:**
- Modify: `docs/plans/2026-03-27-agent-guards-cleanup-design.md`
- Modify: `docs/plans/2026-03-27-agent-guards-cleanup.md`

**Step 1: Update implementation notes if verification reveals extra adjustments**

Keep the documentation aligned with the exact cleanup that landed.

**Step 2: Ask before any broader `cargo test`**

This slice touches `codex-core`, so do not run full workspace tests without
user approval.
