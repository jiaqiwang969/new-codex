# Freeze And Obsolete Smart Access Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the failed local `freeze` cleanup remnants and obsolete Smart Access / `endpoint-sec` planning docs while keeping account-pool, `model_sub`, Entire, `git-graph`, and `ralph-loop`.

**Architecture:** First remove the last live config-facing `freeze_sandbox_debug` compatibility key so runtime/config behavior matches upstream. Then delete unreferenced local `automation/` freeze code and obsolete planning docs that describe the abandoned Smart Access + `endpoint-sec` line. Keep the surviving fork features untouched.

**Tech Stack:** Rust (`codex-features`, `codex-core`), generated config schema, Markdown plan docs, Cargo/Just.

---

### Task 1: Remove the `freeze_sandbox_debug` config surface

**Files:**
- Modify: `codex-rs/features/src/tests.rs`
- Modify: `codex-rs/features/src/lib.rs`
- Modify: `codex-rs/core/config.schema.json`

**Step 1: Write the failing test**

Add a test asserting `feature_for_key("freeze_sandbox_debug") == None`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-features freeze_sandbox_debug_is_not_a_feature_key`
Expected: FAIL because the legacy key still resolves to `Feature::FreezeSandboxDebug`.

**Step 3: Write minimal implementation**

Delete the `FreezeSandboxDebug` feature enum variant, its spec entry, and the old removed-feature test. Regenerate or refresh the config schema so the key disappears from schema output.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-features freeze_sandbox_debug_is_not_a_feature_key`
Expected: PASS.

### Task 2: Delete dead local freeze automation code

**Files:**
- Delete: `codex-rs/core/src/automation/compile_error_freezer.rs`
- Delete: `codex-rs/core/src/automation/fix_agent_coordinator.rs`
- Delete: `codex-rs/core/src/automation/mod.rs`
- Delete: `codex-rs/core/src/automation/snapshot.rs`
- Delete: `codex-rs/core/src/automation/undo_replacer.rs`
- Delete: `codex-rs/core/src/automation/utm_manager.rs`

**Step 1: Confirm code is unreferenced**

Run: `rg -n "CompileErrorFreezer|TimeTravelMiddleware|FixAgentCoordinator|UndoReplacer|UTMManager" codex-rs`
Expected: only hits inside the `automation/` files themselves.

**Step 2: Delete the dead files**

Remove the unused local freeze automation module tree.

**Step 3: Run targeted verification**

Run: `cargo test -p codex-core`
Expected: PASS.

### Task 3: Delete obsolete Smart Access / `endpoint-sec` / freeze planning docs

**Files:**
- Delete: `docs/plans/2026-03-16-smart-access-endpoint-sec-phase-2b-control-plane.md`
- Delete: `docs/plans/2026-03-20-machine-global-smart-access-endpoint-sec-design.md`
- Delete: `docs/plans/2026-03-20-machine-global-smart-access-endpoint-sec-implementation.md`
- Delete: `codex-rs/docs/plans/2026-03-22-smart-access-safe-local-read-design.md`
- Delete: `codex-rs/docs/plans/2026-03-22-smart-access-safe-local-read.md`

**Step 1: Delete only obsolete plan docs**

Remove historical plan files for the abandoned Smart Access + `endpoint-sec` + freeze direction.

**Step 2: Run lightweight verification**

Run: `git diff --check`
Expected: PASS with no whitespace or conflict-marker issues.

### Task 4: Format and final verification

**Files:**
- Modify: formatting-only updates if needed

**Step 1: Format Rust changes**

Run: `just fmt`

**Step 2: Run crate verification**

Run: `cargo test -p codex-features`
Expected: PASS.

Run: `cargo test -p codex-core`
Expected: PASS.

**Step 3: Run repository lint checks**

Run: `just argument-comment-lint`
Expected: PASS.

Run: `just fix -p codex-core`
Expected: PASS or apply scoped clippy fixes.

**Step 4: Optional wider verification**

Ask the user before running the full workspace suite because `codex-rs/core` changed.
