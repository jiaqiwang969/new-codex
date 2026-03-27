# Agent Guards Residue Cleanup Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** `codex-rs/core/src/agent`

## Goal

Remove the unused historical `agent/guards.rs` module so the fork carries less
dead code into future upstream merges.

## Problem

The current branch still declares `mod guards;` in
`codex-rs/core/src/agent/mod.rs`, but the exported depth and spawn-slot helpers
come from `registry.rs`, and runtime call sites also use `registry`-backed
state.

That leaves `guards.rs` as duplicate implementation residue:

- it increases merge surface in `core/src/agent`
- it can confuse future edits by suggesting two active ownership paths
- it adds tests and logic that are not actually wired into production

## Requirements

Keep:

- current multi-agent runtime behavior
- `registry.rs` as the single live ownership point for spawn limits and depth
- existing account-pool, `model_sub`, memory, and TUI behavior untouched

Remove:

- the unused `mod guards;` declaration
- `codex-rs/core/src/agent/guards.rs`

## Non-Goals

- do not refactor `registry.rs`
- do not change agent limits, depth semantics, or nickname behavior
- do not touch app-server, TUI, provider routing, or memory code

## Options Considered

### Option A: Delete only the orphaned `guards.rs` module

Pros:

- smallest change
- lowest runtime risk
- immediately reduces merge noise

Cons:

- does not address larger noise sources elsewhere

### Option B: Merge `guards.rs` content into `registry.rs`

Pros:

- may look cleaner at first glance

Cons:

- larger code movement for no runtime gain
- increases review and verification scope

### Option C: Clean `guards.rs` and `model_presets.rs` in one slice

Pros:

- bigger noise reduction

Cons:

- mixes two unrelated cleanup scopes
- raises risk and conflict surface for this pass

## Decision

Choose **Option A**.

This slice should stay narrowly focused on removing the clearly unused
`guards.rs` module and leaving live behavior in `registry.rs` untouched.

## Design

### 1. Keep `registry.rs` as the single live owner

`agent/mod.rs` already re-exports the depth helpers from `registry.rs`, and the
runtime uses `AgentControl` plus `registry` state. No runtime call sites should
move in this slice.

### 2. Delete the orphaned module

Remove `mod guards;` from `agent/mod.rs` and delete `agent/guards.rs`.

### 3. Verify by rebuilding the live agent crate surface

Run focused `codex-core` tests that exercise agent registry behavior so we know
the removal did not break compilation or the actual active ownership path.

## Expected Outcome

After this slice:

- `core/src/agent` has one live ownership path for spawn guards
- future upstream merges in the agent area have less dead divergence
- no runtime behavior changes

## Verification

- `cargo test -p codex-core registry_tests`
- `cd codex-rs && just fmt`
- `cd codex-rs && just argument-comment-lint`
