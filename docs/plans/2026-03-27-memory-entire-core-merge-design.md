# Memory Entire Core Merge Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** `codex-rs/core` and `codex-rs/hooks` memory/Entire internals only

## Goal

Stabilize the local memory/Entire core so later upstream merge work can keep
the feature line we still want without dragging along weak or misleading test
coverage.

## Problem

The branch still carries a large local-only Block B1 surface:

- `thread_memory.rs`
- `context_packet.rs`
- `entire_integration.rs`
- `entire_summary_generator.rs`
- the `codex.rs` / `tasks` / `mcp_tool_call` / `codex_thread` attachment points
- `hooks/src/entire_summary.rs`

That core behavior is intentionally preserved, but the current safety net is
uneven:

- `codex-rs/core/tests/entire_config_test.rs` checks a hand-written fallback
  chain instead of the real `entire_summary_generator::model_slug()` path
- some local-only helpers have little or no unit coverage
- future merge cleanup in `codex.rs` risks breaking preserved behavior without
  an obvious failing test at the real seam

## Requirements

Keep:

- persistent thread memory updates
- context packet helpers
- Entire checkpoint enrichment and formatting
- Entire summary persistence under `.entire/summaries`
- the `entire_summary_model -> model_sub -> built-in default` resolution order

Do not do in this slice:

- do not touch `app-server-protocol/v2.rs`
- do not change shared wire contracts
- do not reintroduce Smart Access, `endpoint-sec`, or `/freeze`
- do not alter account-pool/provider routing

## Options Considered

### Option A: Merge-clean the `codex.rs` integration first

Pros:

- attacks the biggest file immediately

Cons:

- poor signal when something breaks
- easy to conflate core regressions with attachment-point churn

### Option B: Lock the preserved Block B1 contracts in tests first

Pros:

- gives later merge cleanup a clear safety net
- keeps this round narrow and user-approved
- avoids broad protocol churn

Cons:

- delivers mostly test/documentation progress in this round

### Option C: Defer Block B1 until after more upstream wire cleanup

Pros:

- fewer files touched right now

Cons:

- leaves one of the intentional fork differentiators under-specified
- increases risk that later merges silently degrade preserved behavior

## Decision

Choose **Option B**.

This round should lock the preserved core behavior before making broader merge
edits around it. The purpose is not to redesign memory/Entire internals; it is
to make the intended contract explicit and verifiable.

## Design

### 1. Test the real Entire summary model resolution seam

Move fallback verification from an integration test that reimplements the logic
to unit tests in `entire_summary_generator.rs` that call `model_slug()` on real
`Config` values.

This keeps the preserved resolution rule explicit:

- `memories.entire_summary_model`
- `model_sub`
- `DEFAULT_ENTIRE_SUMMARY_MODEL`

### 2. Keep config-default coverage focused on config defaults

`codex-rs/core/tests/entire_config_test.rs` should only cover default config
facts:

- Entire summary generation is enabled by default in `MemoriesConfig`
- `entire_summary_model` defaults to `None`
- the shared test harness disables Entire generation in test configs

It should stop pretending to validate runtime model resolution.

### 3. Lock the local-only helper behavior that future merges can disturb

Add narrow unit coverage for:

- `thread_memory.rs` summary-message normalization
- `entire_integration.rs` AI-summary formatting and prompt fallback
- `hooks/src/entire_summary.rs` save/load round-trip

These are small seams, but they encode the behavior we still want to preserve.

## Expected Outcome

After this slice:

- the Entire summary fallback chain is tested at the real implementation seam
- misleading test coverage is removed
- local memory/Entire helper behavior has a tighter unit-test fence
- later Block B1 merge cleanup can change attachment points with less guesswork

## Verification

- `cargo test -p codex-core --test entire_config_test`
- `cargo test -p codex-core entire_summary`
- `cargo test -p codex-core thread_memory`
- `cargo test -p codex-hooks entire_summary`
- `cd codex-rs && just fmt`
- `PATH="/Users/jqwang/.local/share/cargo/bin:$PATH" ./tools/argument-comment-lint/run.sh`
