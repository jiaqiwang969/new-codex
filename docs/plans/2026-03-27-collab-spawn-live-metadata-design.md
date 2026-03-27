# Collab Spawn Live Metadata Merge Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** app-server live collab spawn metadata parity

## Goal

Keep the smallest useful collab spawn metadata contract during the upstream
merge, and make the app-server live path honor it end-to-end.

## Problem

The current branch already carries three spawn metadata fields that are worth
keeping:

- `agent_type`
- `model`
- `model_provider_id`

`core` spawn handlers now emit these fields, and app-server thread history can
persist them, but the app-server TUI live path still drops them when converting
`ThreadItem::CollabAgentToolCall` back into local TUI history events. The
parallel `tui_app_server` renderer also ignores them, so live app-server
sessions lose provider/model attribution that core TUI sessions keep.

## Non-Goals

- Do not add new shared-wire metadata.
- Do not revive removed `model_source` / `model_source_detail` fields.
- Do not change MemoryLink or broader collab protocol contracts in this slice.
- Do not refactor non-spawn collab tools (`wait`, `resume`, `close`, `send_input`)
  beyond what is needed for consistency.

## Options Considered

### Option A: Keep minimal live metadata and wire it through app-server TUI

Preserve `agent_type`, `model`, and `model_provider_id`, and fix the app-server
TUI live conversion/rendering path so those fields appear in live spawn history.

Pros:

- preserves useful account-pool/provider observability
- aligns app-server TUI with core TUI behavior
- small, local patch in `tui_app_server`

Cons:

- keeps a small amount of local shared-wire extension alive

### Option B: Drop `model_provider_id` and keep only `agent_type`

Pros:

- smaller long-term merge surface

Cons:

- loses provider attribution that matters in this fork because account-pool and
  custom endpoints remain first-class features

### Option C: Defer this and move to MemoryLink first

Pros:

- no immediate code churn in collab UI

Cons:

- leaves a known half-wired contract in place
- preserves a misleading mismatch between core TUI and app-server TUI

## Decision

Choose **Option A**.

This keeps the smallest contract that already has real value in the fork while
avoiding any new protocol expansion. The fix should stay inside the app-server
TUI adapter and renderer:

1. preserve spawn metadata from `agents_states` when converting live app-server
   collab items back into `CollabAgentSpawnEndEvent`
2. render those fields in `tui_app_server::multi_agents::spawn_end` the same
   way core TUI already does

## Expected Outcome

After this slice:

- live app-server spawn history shows role/model/provider details
- core TUI and app-server TUI present the same spawn metadata contract
- no new protocol fields are introduced
- `model_source*` stays out of the merge surface

## Verification

- targeted `codex-tui-app-server` tests around live collab spawn items
- `cargo test -p codex-tui-app-server`
- `just fmt`
- `just fix -p codex-tui-app-server`
- `just argument-comment-lint-from-source`
