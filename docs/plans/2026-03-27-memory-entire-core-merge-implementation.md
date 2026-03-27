# Memory Entire Core Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Lock the preserved memory/Entire core behavior behind real tests before continuing broader upstream merge cleanup in the surrounding integration points.

**Architecture:** Follow @superpowers:test-driven-development where behavior is not already pinned. Prefer unit tests at the real seam over broad integration tests that duplicate implementation logic. Keep this round inside `codex-rs/core` and `codex-rs/hooks`; do not touch `app-server-protocol/v2.rs`.

**Tech Stack:** Rust, Cargo unit/integration tests, `just fmt`, direct argument-comment lint runner

---

### Task 1: Replace the misleading Entire config fallback test

**Files:**
- Modify: `codex-rs/core/tests/entire_config_test.rs`

**Step 1: Remove the fake fallback-chain check**

Delete the test that reconstructs the fallback chain with local constants
instead of exercising `entire_summary_generator::model_slug()`.

**Step 2: Keep only config-default assertions**

Retain focused coverage for:

- `MemoriesConfig::default().entire_summary_enabled`
- `MemoriesConfig::default().entire_summary_model`
- `load_default_config_for_test(...).memories.entire_summary_enabled`

**Step 3: Run the targeted test**

Run:

```bash
cargo test -p codex-core --test entire_config_test -- --nocapture
```

Expected: PASS with only the focused config-default checks.

### Task 2: Lock the real Entire summary model resolution seam

**Files:**
- Modify: `codex-rs/core/src/entire_summary_generator.rs`

**Step 1: Add unit tests around `model_slug()`**

Cover:

- explicit `memories.entire_summary_model`
- fallback to `model_sub`
- fallback to `DEFAULT_ENTIRE_SUMMARY_MODEL`

Use real `Config` instances built for tests rather than duplicating the
resolution logic inline.

**Step 2: Run the targeted tests**

Run:

```bash
cargo test -p codex-core model_slug_ -- --nocapture
```

Expected: PASS

### Task 3: Lock the other preserved Block B1 helpers

**Files:**
- Modify: `codex-rs/core/src/thread_memory.rs`
- Modify: `codex-rs/core/src/entire_integration.rs`
- Modify: `codex-rs/hooks/src/entire_summary.rs`

**Step 1: Add helper-level tests**

Cover:

- thread-memory summary message normalization
- Entire checkpoint formatting when AI summaries are meaningful
- Entire checkpoint formatting fallback when AI summaries are not meaningful
- Entire summary save/load round-trip on disk

**Step 2: Run the targeted tests**

Run:

```bash
cargo test -p codex-core thread_memory -- --nocapture
cargo test -p codex-core checkpoints_summary -- --nocapture
cargo test -p codex-hooks entire_summary -- --nocapture
```

Expected: PASS

### Task 4: Format and lint the narrow slice

**Files:**
- Modify: `docs/plans/2026-03-27-memory-entire-core-merge-design.md`
- Modify: `docs/plans/2026-03-27-memory-entire-core-merge-implementation.md`

**Step 1: Run Rust formatting**

Run:

```bash
cd codex-rs && just fmt
```

Expected: PASS

**Step 2: Run argument comment lint from source**

Run:

```bash
cd /Users/jqwang/.config/superpowers/worktrees/new-codex/probe-upstream-9dbe09834 && PATH="/Users/jqwang/.local/share/cargo/bin:$PATH" ./tools/argument-comment-lint/run.sh
```

Expected: PASS

## Execution Notes

- This slice deliberately does not claim to finish all of Block B1.
- The purpose is to make the preserved memory/Entire contract safer before
  touching the larger `codex.rs` integration surface.
- If later work changes `context_packet` attachment policy or turn-complete
  memory wiring, extend tests at the real seam instead of adding more constant
  mirrors in integration tests.
