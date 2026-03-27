# MemoryLink Minimal Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Status:** Implemented on 2026-03-27 in `probe/upstream-merge-9dbe09834`.

**Goal:** Reduce the shared nested `MemoryLink` contract to `scope_version + binding_key` while preserving turn/item continuity and hook compatibility diagnostics.

**Architecture:** Follow @superpowers:test-driven-development. First lock the reduced wire shape in replay/live/hook tests, then make the smallest protocol/core/app-server changes needed so nested `MemoryLink` only carries the two continuity keys. Leave hook flat compatibility fields in place and populate them from existing context data rather than the nested object.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just fix`, `insta`-free unit assertions

---

### Task 1: Lock the replayed app-server contract

**Files:**
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`

**Step 1: Write the failing tests**

Tighten the existing replay tests so they compare full `MemoryLink` values with
only:

- `scope_version`
- `binding_key`

and no `scope_kind` / `summary_sha256` in:

- `reconstructs_in_progress_collab_spawn_item_with_memory`
- the MCP begin/end replay tests that already assert `ThreadItem::McpToolCall.memory`

**Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test -p codex-app-server-protocol reconstructs_in_progress_collab_spawn_item_with_memory -- --exact
cargo test -p codex-app-server-protocol reconstructs_mcp_tool_call_items_with_memory_links -- --exact
```

Expected: FAIL because replay still reconstructs four-field nested memory.

### Task 2: Lock the live app-server notification contract

**Files:**
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`

**Step 1: Write the failing tests**

Tighten the existing live notification tests so nested `ThreadItem::McpToolCall.memory`
only contains `scope_version` and `binding_key` in:

- `test_construct_mcp_tool_call_begin_notification_with_memory_link`
- `test_construct_mcp_tool_call_end_notification_with_memory_link_in_snake_case`

**Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test -p codex-app-server test_construct_mcp_tool_call_begin_notification_with_memory_link -- --exact
cargo test -p codex-app-server test_construct_mcp_tool_call_end_notification_with_memory_link_in_snake_case -- --exact
```

Expected: FAIL because live notification conversion still emits four-field nested memory.

### Task 3: Lock the hook compatibility contract

**Files:**
- Modify: `codex-rs/hooks/src/types.rs`

**Step 1: Write the failing tests**

Update the stable wire-shape tests so:

- nested `memory` only contains `scope_version` and `binding_key`
- flat compatibility fields still include `memory_scope_kind` and
  `memory_summary_sha256`
- `memory_context` still includes the richer diagnostic values

Use the existing tests:

- `hook_payload_serializes_stable_wire_shape`
- `mcp_hook_payload_serializes_stable_wire_shape`

**Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test -p codex-hooks hook_payload_serializes_stable_wire_shape -- --exact
cargo test -p codex-hooks mcp_hook_payload_serializes_stable_wire_shape -- --exact
```

Expected: FAIL because hooks still serialize four-field nested memory.

### Task 4: Implement the minimal contract changes

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_thread.rs`
- Modify: `codex-rs/core/src/mcp_tool_call.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Modify: `codex-rs/hooks/src/types.rs`

**Step 1: Reduce canonical `MemoryLink`**

Change the protocol and app-server-protocol `MemoryLink` structs to only expose:

- `scope_version`
- `binding_key`

**Step 2: Keep core continuity generation**

Update core memory-link builders so nested `MemoryLink` only includes those two
fields, but keep enough local context available to continue populating hook
compatibility fields.

**Step 3: Keep replay/live app-server projections aligned**

Update replay and live converters so they parse and emit the smaller nested
shape while preserving `Turn.memory`, `McpToolCall.memory`, and
`CollabAgentToolCall.memory`.

**Step 4: Keep hook flat compatibility fields**

Populate `memory_scope_kind` / `memory_summary_sha256` from the existing
`memory_context` or nearby core context values instead of reading them from the
reduced nested `memory`.

### Task 5: Verify the slice

**Files:**
- Modify: `docs/plans/2026-03-27-memorylink-minimal-contract-design.md`
- Modify: `docs/plans/2026-03-27-memorylink-minimal-contract-implementation.md`

**Step 1: Run crate tests**

Run:

```bash
cargo test -p codex-app-server-protocol
cargo test -p codex-app-server
cargo test -p codex-hooks
```

Expected: PASS

**Step 2: Run required formatting and linting**

Run:

```bash
just write-app-server-schema
just write-hooks-schema
cd codex-rs && just fmt
cd codex-rs && just fix
cd /Users/jqwang/.config/superpowers/worktrees/new-codex/probe-upstream-9dbe09834 && PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Expected: PASS

**Step 3: Ask before full workspace tests**

Because this slice touches `protocol`, ask the user before running full
workspace `cargo test` / `just test`.

## Execution Notes

- Nested shared `MemoryLink` now keeps only `scope_version` and `binding_key`
  in `codex-rs/protocol` and `codex-rs/app-server-protocol`.
- Replay/live app-server projections still preserve `Turn.memory`,
  `ThreadItem::McpToolCall.memory`, and
  `ThreadItem::CollabAgentToolCall.memory`, but those nested values now use the
  reduced shape.
- Hook flat compatibility fields (`memory_scope_kind`,
  `memory_summary_sha256`) are still emitted and are now derived from
  `memory_context` instead of the nested object.
- App-server and hook schema fixtures were regenerated after the contract
  change.
