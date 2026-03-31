# Memory / Context Packet / Entire Merge Analysis

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Goal of this note:** capture what the local memory/context/Entire line adds,
> why it conflicts with upstream, and what should be preserved if this block is
> migrated onto a newer upstream base.

## Bottom Line

This block is not one feature. It is a stack:

1. thread memory persistence
2. memory trace summarization
3. context packet injection
4. Entire checkpoint/history enrichment
5. MemoryLink propagation through protocol, hooks, and app-server

That is why conflicts show up in both core-only files and protocol/app-server
files.

## What The Local Branch Adds

### 1. Persistent thread memory

Representative file:

- `codex-rs/core/src/thread_memory.rs`

Behavior:

- after turns or compaction, the branch derives a memory trace from history
- it summarizes that trace
- it stores thread memory into the local state DB

Important implementation detail:

- tool outputs and reasoning payloads are filtered out of memory traces to avoid
  noisy summaries

### 2. Context Packet assembly

Representative file:

- `codex-rs/core/src/context_packet.rs`

Behavior:

- collects saved thread memory
- adds user instructions
- adds project memory / contextual information
- appends recent Entire checkpoint summaries when available

This is one of the fork's key workflow enhancements because it pushes memory and
historical context into downstream tool and agent execution.

### 3. Entire integration

Representative files:

- `codex-rs/core/src/entire_integration.rs`
- `codex-rs/core/src/entire_summary_generator.rs`
- `codex-rs/hooks/src/entire_summary.rs`

Behavior:

- read recent Entire checkpoints from git history
- load or generate WHY-focused AI summaries for those checkpoints
- format them into reusable context for later turns/agents

This is a real local asset. Upstream does not have an equivalent Entire-based
history workflow.

### 4. MemoryLink protocol propagation

Representative files:

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/hooks/src/types.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

Behavior:

- introduces or extends `MemoryLink`
- carries memory references through protocol payloads
- exposes them through hooks and app-server v2 surfaces

This is where local core behavior stops being private implementation and starts
affecting wire contracts.

## What Problem The Local Author Was Solving

The local branch is optimizing for workflow continuity across sessions:

- keep a lightweight memory of what a thread has already established
- avoid re-deriving context from scratch every turn
- preserve useful historical intent from previous AI sessions via Entire
- push that information into tools, hooks, and subagents automatically

In short:

- upstream tends to optimize around current-thread runtime behavior
- this fork also optimizes around cross-turn and cross-session continuity

## What Upstream Is Optimizing For

Upstream is still actively evolving:

- app-server protocol shapes
- collaboration/event metadata
- memory-related behavior
- hook payload conventions

That means this local stack is colliding not because it is obviously wrong, but
because upstream is still changing the same surface area for broader platform
needs.

## Why This Area Keeps Conflicting

### Conflict 1: local-only core modules meet shared protocol surfaces

The following core modules are local-only:

- `thread_memory.rs`
- `context_packet.rs`
- `entire_integration.rs`

Those are easy to keep in isolation.

The problem starts when their data is exposed through:

- `protocol`
- `hooks`
- `app-server`
- `app-server-protocol`

That is where upstream churn turns local workflow logic into merge conflicts.

### Conflict 2: app-server v2 is an unstable overlap zone

Representative file:

- `codex-rs/app-server-protocol/src/protocol/v2.rs`

Current local branch extends the wire contract with `MemoryLink`-related data.
Upstream is also evolving v2 rapidly, so even small local additions produce
large recurring conflicts.

### Conflict 3: bespoke event handling becomes a convergence sink

Representative file:

- `codex-rs/app-server/src/bespoke_event_handling.rs`

This file has to reconcile:

- core turn state
- protocol mapping
- app-server event emission
- local additions like memory propagation

That makes it a predictable hotspot.

## What Must Be Preserved

Based on the current branch direction, the semantics worth preserving are:

1. persistent thread memory summaries
2. trace summarization that excludes noisy tool/reasoning output
3. context packet injection of memory + user/project context
4. Entire checkpoint summaries as optional context enrichment
5. a `MemoryLink`-style concept that can travel through hooks and app-server

## What Can Change

These details can be reshaped during merge:

- exact `MemoryLink` wire shape
- exact app-server v2 field placement
- exact event names or conversion helpers
- exact fallback model selection path for memory summarization
- exact wording/format of Entire checkpoint summaries

The semantics matter more than the current patch layout.

## Recommended Merge Strategy

### Keep core workflow logic isolated first

Preserve in this order:

1. `thread_memory.rs`
2. `context_packet.rs`
3. `entire_integration.rs`
4. `entire_summary_generator.rs`

These are the easiest parts to preserve because they are mostly local logic.

### Reattach shared contracts second

After core behavior is stable, reintroduce:

1. `MemoryLink` in `protocol`
2. hook payload propagation
3. app-server mapping
4. app-server v2 schema exposure

This order keeps upstream protocol churn from destabilizing the core workflow.

### Do not overfit to the current v2 patch shape

The current `app-server-protocol/src/protocol/v2.rs` diff is large enough that
blindly replaying it onto newer upstream is high risk.

Prefer:

- preserve the idea of memory references
- adapt to current upstream payload conventions

instead of:

- preserving every local v2 field placement exactly

## Main Risk If We Merge Poorly

The dangerous failure mode is partial preservation:

- thread memory still exists
- but hooks stop receiving it
- or app-server drops it silently
- or context packets stop seeing Entire summaries
- or protocol clients receive stale/incorrect shapes

That creates a system that still "has memory" internally but loses the workflow
benefit at the boundaries.

## Current Judgment

This is a meaningful local capability and worth preserving.

But it should be split conceptually into two layers:

- local workflow logic:
  - thread memory
  - context packets
  - Entire summaries
- shared contract surface:
  - MemoryLink in protocol/hooks/app-server

That split is the cleanest way to keep the value while reducing future upstream
merge pain.
