# Model Presets Cleanup Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** `codex-rs/core/src/models_manager/model_presets.rs`

## Goal

Remove the fork-only hardcoded builtin model preset table so this branch aligns
with upstream's catalog-driven model listing while keeping the legacy migration
config keys that TUI still reads.

## Problem

The current fork still carries a large `model_presets.rs` file with:

- a hardcoded `PRESETS` table
- `builtin_model_presets()`
- upgrade helper constructors
- file-local tests for preset contents

That code is now dead:

- live callers only import the two migration config constants
- no production path calls `builtin_model_presets()`
- upstream already reduced this file to the two constants plus a compatibility
  comment

Keeping the dead preset table increases merge surface in `codex-core` and makes
future upstream syncs harder for no runtime value.

## Requirements

Keep:

- `HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG`
- `HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG`
- existing TUI and TUI app-server migration prompt behavior
- account-pool, custom Anthropic base URL, memory, `model_sub`, and worktree
  behavior untouched

Remove:

- `PRESETS`
- `builtin_model_presets()`
- hardcoded upgrade helper constructors
- file-local tests that only validate the dead preset table

## Non-Goals

- do not change runtime model catalog resolution
- do not reintroduce Smart Access, `endpoint-sec`, `/freeze`, or old vouch
  surfaces
- do not alter TUI picker behavior outside what upstream already does through
  the active catalog

## Options Considered

### Option A: Replace the file with the upstream minimal constants-only version

Pros:

- matches upstream exactly in behavior for this module
- removes the largest dead-code block in one pass
- minimizes future merge conflicts

Cons:

- removes fork-local tests that currently only exercise dead data

### Option B: Keep the preset table but stop exporting it

Pros:

- superficially less disruptive

Cons:

- leaves dead code in place
- preserves unnecessary merge noise
- makes future reviewers re-prove that the table is unused

### Option C: Rewrite the file to source presets from the active catalog

Pros:

- could retain a helper with live semantics

Cons:

- larger design change
- unnecessary because there are no live callers
- expands verification scope beyond this cleanup slice

## Decision

Choose **Option A**.

This slice should make `model_presets.rs` match upstream's minimal
compatibility role and remove the unused preset table entirely.

## Design

### 1. Keep only the legacy migration config keys

Retain the two public config key constants because both TUI frontends still use
them when deciding whether to hide old migration notices.

### 2. Delete the dead preset machinery

Remove the hardcoded preset list, helper constructors, and file-local tests.
No replacement is needed because no production path references them.

### 3. Verify the remaining callers still compile

Run focused `codex-core` and `codex-tui` verification so the constants-only
module still satisfies the importing code.

## Expected Outcome

After this slice:

- `model_presets.rs` becomes upstream-aligned
- dead preset data stops contributing merge noise
- TUI migration prompt compatibility remains intact

## Verification

- `cd codex-rs && just fmt`
- `cargo test -p codex-core model_presets`
- `cargo test -p codex-tui migration`
- `PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`
