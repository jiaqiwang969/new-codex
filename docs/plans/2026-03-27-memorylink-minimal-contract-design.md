# MemoryLink Minimal Contract Merge Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** protocol/app-server/hooks memory continuity wire

## Goal

Shrink the shared `MemoryLink` wire contract to the smallest continuity shape
that still matters in this fork, while keeping the higher-value continuity
surfaces (`Turn.memory`, item-level memory, hooks nested memory) intact.

## Problem

The current branch carries a four-field `MemoryLink` shape across shared
protocols:

- `scope_version`
- `scope_kind`
- `summary_sha256`
- `binding_key`

That shape propagates through:

- `codex-rs/protocol`
- `codex-rs/app-server-protocol`
- app-server event replay and live notifications
- hooks nested payloads

This is more shared-wire surface than we need for upstream alignment. The
continuity contract that external automation actually needs is:

- a short readable version id: `scope_version`
- a stable join key: `binding_key`

The other two fields are still useful for diagnostics, but they do not need to
remain part of the shared nested `MemoryLink` object.

## Requirements

Keep:

- `Turn.memory`
- `ThreadItem::McpToolCall.memory`
- `ThreadItem::CollabAgentToolCall.memory`
- hooks nested `memory`
- hooks compatibility fields and `memory_context`

Do not keep as canonical shared-wire fields:

- `MemoryLink.scope_kind`
- `MemoryLink.summary_sha256`

## Non-Goals

- Do not remove memory/Entire behavior in core.
- Do not remove hook compatibility fields such as `memory_scope_kind` or
  `memory_summary_sha256` in this slice.
- Do not expand `MemoryLink` to new UI or protocol surfaces.
- Do not change account-pool/provider behavior.

## Options Considered

### Option A: Keep only `scope_version` and `binding_key` in shared `MemoryLink`

Pros:

- reduces long-term upstream merge surface
- preserves the continuity keys that external automation actually joins on
- keeps diagnostics available through hook compatibility fields and
  `memory_context`

Cons:

- requires touching several shared boundary layers at once

### Option B: Keep the current four-field shared shape

Pros:

- lowest immediate implementation risk

Cons:

- preserves unnecessary divergence from upstream in shared protocol types
- guarantees more future merge collisions

### Option C: Drop item/turn/hook nested memory and keep only core internals

Pros:

- smallest shared surface

Cons:

- removes continuity metadata the fork still relies on
- conflicts with the stated upstream merge policy for this branch

## Decision

Choose **Option A**.

The nested `MemoryLink` object should become the minimal continuity contract:

- `scope_version`
- `binding_key`

The richer diagnostics remain available through the hook compatibility layer:

- `memory_scope_kind`
- `memory_summary_sha256`
- `memory_context.active_scope_kind`
- `memory_context.active_memory_summary_sha256`

This keeps continuity useful while moving the shared wire closer to official
shapes and lowering future conflict pressure.

## Design

### 1. Shared protocol shape

Update `codex-rs/protocol::MemoryLink` and
`codex-rs/app-server-protocol::MemoryLink` to contain only:

- `scope_version`
- `binding_key`

Core should still compute diagnostic values internally, but it should stop
placing them into nested `MemoryLink`.

### 2. App-server projections

Preserve `Turn.memory`, `ThreadItem::McpToolCall.memory`, and
`ThreadItem::CollabAgentToolCall.memory`, but ensure those nested values now use
the reduced two-field shape.

Both replay paths (`thread_history.rs`) and live app-server paths
(`bespoke_event_handling.rs`, `codex_message_processor.rs`) should agree on that
shape.

### 3. Hook compatibility

Keep hooks nested `memory`, but reduce it to the same two-field shape.

Do not remove the flat compatibility fields yet. Instead:

- derive `memory_scope_version` and `memory_binding_key` from the reduced
  `memory`
- derive `memory_scope_kind` / `memory_summary_sha256` from
  `memory_context` or existing core-side context values

This keeps existing hook JSON and environment-variable behavior stable for
consumers that still read the flat fields.

## Expected Outcome

After this slice:

- shared nested `MemoryLink` only exposes `scope_version + binding_key`
- turn/item continuity stays available through app-server v2
- hooks keep compatibility-level diagnostics without forcing them into the
  canonical nested wire object
- future upstream merges only need to reason about the smaller continuity core

## Verification

- targeted `codex-app-server-protocol` tests for replayed turn/item memory
- targeted `codex-app-server` tests for live MCP/collab notifications
- targeted `codex-hooks` tests for nested-vs-flat memory payload shape
- `just write-app-server-schema`
- `just write-hooks-schema`
- `cargo test -p codex-app-server-protocol`
- `cargo test -p codex-app-server`
- `cargo test -p codex-hooks`
- `cd codex-rs && just fmt`
- `cd codex-rs && just fix`
- `PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`
