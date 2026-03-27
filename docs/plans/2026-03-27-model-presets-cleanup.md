# Model Presets Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the dead hardcoded builtin model preset table from `codex-rs/core/src/models_manager/model_presets.rs` while preserving the legacy migration config constants that TUI still imports.

**Architecture:** Align this module with upstream by reducing it to the two migration config constants and a compatibility comment. Because no live callers use the preset table, verification focuses on successful compile/test coverage for `codex-core` and the TUI crates that import the constants.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just argument-comment-lint-from-source`

---

### Task 1: Confirm the live surface

**Files:**
- Modify: `codex-rs/core/src/models_manager/model_presets.rs`
- Check: `codex-rs/tui/src/app.rs`
- Check: `codex-rs/tui_app_server/src/app.rs`

**Step 1: Confirm callers**

Verify that the only live imports from `model_presets.rs` are
`HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG` and
`HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG`.

**Step 2: Confirm preset helpers are unused**

Verify there are no production references to `PRESETS` or
`builtin_model_presets()`.

### Task 2: Reduce the module to upstream shape

**Files:**
- Modify: `codex-rs/core/src/models_manager/model_presets.rs`

**Step 1: Replace the file contents**

Keep only:

- the compatibility comment
- `HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG`
- `HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG`

**Step 2: Remove dead tests with the dead code**

Do not add replacement tests for data that no longer exists.

### Task 3: Run focused verification

**Files:**
- Test: `codex-rs/core/src/models_manager/model_presets.rs`
- Test: `codex-rs/tui/src/app.rs`
- Test: `codex-rs/tui_app_server/src/app.rs`

**Step 1: Format Rust changes**

Run:

```bash
cd codex-rs && just fmt
```

Expected: PASS

**Step 2: Verify `codex-core` still builds with the reduced module**

Run:

```bash
cd codex-rs && cargo test -p codex-core model_presets
```

Expected: PASS, including the acceptable case where no tests match but the crate
compiles cleanly.

**Step 3: Verify TUI imports still build**

Run:

```bash
cd codex-rs && cargo test -p codex-tui migration
```

Expected: PASS, including the acceptable case where filtered tests are zero but
the crate compiles cleanly.

**Step 4: Run repository lint required by this workspace**

Run:

```bash
PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Expected: PASS

### Task 4: Keep the merge slice narrow

**Files:**
- Modify: `docs/plans/2026-03-27-model-presets-cleanup-design.md`
- Modify: `docs/plans/2026-03-27-model-presets-cleanup.md`

**Step 1: Document the landed shape**

Keep the docs aligned with the final constants-only module.

**Step 2: Do not run broader workspace tests without approval**

This cleanup touches `codex-core`, so ask before any full `cargo test` or
`just test` run.
