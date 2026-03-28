# Reclaim Multi-Agent Metadata Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the local-only multi-agent metadata wire path (`agent_type`, `model`, `model_provider_id`) while preserving upstream behavior, account-pool routing, and memory-link propagation.

**Architecture:** Reclaim this as one bounded protocol/UI slice. First, update tests to encode upstream-style expectations that collaboration events and thread history no longer expose local metadata. Then remove the metadata fields from protocol and app-server layers, and finally simplify TUI rendering/state that only exists to surface those fields.

**Tech Stack:** Rust, cargo test, app-server protocol v2, tui_app_server, exec JSONL output

---

### Task 1: Write failing protocol/history tests

**Files:**
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`

**Step 1: Write the failing test**

Add or update thread-history tests so collab reconstruction no longer expects spawned-agent metadata to survive into `agents_states`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-app-server-protocol reconstructs_collab_spawn_end_item_with_agent_metadata -- --exact`
Expected: FAIL because current reconstruction still stores metadata.

**Step 3: Write minimal implementation**

Remove known-agent metadata tracking and stop enriching reconstructed collab states with local-only metadata fields.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-app-server-protocol reconstructs_collab_spawn_end_item_with_agent_metadata -- --exact`
Expected: PASS

### Task 2: Write failing app-server integration tests

**Files:**
- Modify: `codex-rs/app-server/tests/suite/v2/turn_start.rs`

**Step 1: Write the failing test**

Update v2 turn-start coverage so spawned-agent notifications no longer expect `agent_type` or `model_provider_id` in returned agent states.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-app-server turn_start_emits_spawn_agent_item_with_model_metadata_v2 -- --exact`
Expected: FAIL because app-server notifications still expose local metadata.

**Step 3: Write minimal implementation**

Remove metadata population from bespoke event handling and related protocol conversions.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-app-server turn_start_emits_spawn_agent_item_with_model_metadata_v2 -- --exact`
Expected: PASS

### Task 3: Remove protocol and exec metadata fields

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/exec/src/exec_events.rs`
- Modify: `codex-rs/exec/src/event_processor_with_jsonl_output.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`

**Step 1: Remove local wire fields**

Delete `agent_type`, `model`, and `model_provider_id` from collaboration payloads and v2 collab agent state, but keep existing `memory` handling untouched.

**Step 2: Run focused tests**

Run: `cargo test -p codex-app-server-protocol`
Run: `cargo test -p codex-app-server turn_start_emits_spawn_agent_item_with_model_metadata_v2 turn_start_emits_spawn_agent_item_with_role_overrides_v2 -- --exact`
Expected: PASS

### Task 4: Reclaim TUI custom metadata rendering

**Files:**
- Modify: `codex-rs/tui_app_server/src/multi_agents.rs`
- Modify: `codex-rs/tui_app_server/src/chatwidget.rs`

**Step 1: Remove local UI metadata plumbing**

Delete local agent metadata caches and rendering helpers that only show `role/model/provider` for spawned agents.

**Step 2: Run focused tests**

Run: `cargo test -p codex-tui-app-server multi_agents -- --nocapture`
Expected: PASS

### Task 5: Format, lint, and verify slice

**Files:**
- Modify: affected files above

**Step 1: Run scoped fix/format**

Run: `just fix -p codex-app-server-protocol`
Run: `just fix -p codex-app-server`
Run: `just fix -p codex-tui-app-server`
Run: `just fix -p codex-exec`
Run: `just fmt`

**Step 2: Run verification**

Run: `cargo test -p codex-app-server-protocol`
Run: `cargo test -p codex-app-server`
Run: `cargo test -p codex-tui-app-server`
Run: `cargo test -p codex-exec`
Run: `./tools/argument-comment-lint/run.sh`
Expected: PASS

### Task 6: Commit

**Files:**
- Modify: all verified slice changes

**Step 1: Commit**

```bash
git add docs/plans/2026-03-28-reclaim-multi-agent-metadata.md codex-rs/protocol/src/protocol.rs codex-rs/exec/src/exec_events.rs codex-rs/exec/src/event_processor_with_jsonl_output.rs codex-rs/app-server-protocol/src/protocol/v2.rs codex-rs/app-server-protocol/src/protocol/thread_history.rs codex-rs/app-server/src/bespoke_event_handling.rs codex-rs/app-server/tests/suite/v2/turn_start.rs codex-rs/tui_app_server/src/multi_agents.rs codex-rs/tui_app_server/src/chatwidget.rs
git commit -m "chore: reclaim upstream multi-agent metadata" -m "Co-authored-by: Codex <noreply@openai.com>"
```
