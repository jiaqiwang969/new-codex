# Guardian App-Server Wire Merge Design

**Date:** 2026-03-27
**Branch:** `probe/upstream-merge-9dbe09834`
**Scope:** collaboration spawn protocol, app-server mapping, rollout history rebuild

## Goal

Restore upstream-style `reasoning_effort` on completed collab spawn events while
keeping the local fork's required metadata:

- `MemoryLink`
- `agent_type`
- `model`
- `model_provider_id`

## Problem

The current branch carries a half-merged spawn wire shape:

- `CollabAgentSpawnBeginEvent` includes `model` and `reasoning_effort`
- `CollabAgentSpawnEndEvent` keeps local metadata fields, but dropped
  `reasoning_effort`
- app-server live mapping and rollout reconstruction therefore emit completed
  spawn items with `reasoning_effort: None`

That creates a real behavior gap in the app-server/TUI path. When a client sees
only the completed spawn item, it can lose the requested model/effort summary
even though upstream keeps that information on spawn completion.

## Requirements

Keep:

- upstream guardian runtime semantics
- local `MemoryLink`
- local `agent_type`, `model`, `model_provider_id`
- local account-pool and custom provider behavior untouched

Add back:

- completed spawn `reasoning_effort` on protocol events
- completed spawn `reasoning_effort` on app-server items rebuilt from events

## Non-Goals

- do not reintroduce old Smart Access or `endpoint-sec`
- do not revive `model_source` or `model_source_detail`
- do not change close/send/wait wire shapes
- do not alter provider selection logic or account-pool routing

## Options Considered

### Option A: Restore only `reasoning_effort` on completed spawn events

Pros:

- matches the upstream contract more closely
- keeps the local metadata that is still actively used
- small verification surface

Cons:

- leaves other local app-server wire divergences for later slices

### Option B: Rewrite the whole collab spawn wire to match upstream exactly

Pros:

- fewer local differences in one pass

Cons:

- would drop local metadata the branch still needs
- much larger conflict and regression surface

### Option C: Leave the protocol as-is and teach TUI/app-server to infer missing effort

Pros:

- smaller type change

Cons:

- bakes more local inference into UI code
- keeps the protocol divergence instead of fixing it at the source

## Decision

Choose **Option A**.

This slice should restore the missing upstream field at the protocol boundary,
then thread it through app-server mapping and rollout reconstruction without
touching the local metadata we still need.

## Design

### 1. Restore completed spawn effort in the shared protocol

Add `reasoning_effort: ReasoningEffortConfig` to
`CollabAgentSpawnEndEvent`.

Use `#[serde(default)]` so older rollout files that predate this field still
deserialize with the default effort.

### 2. Emit the effective effort from both spawn handlers

When a child agent is created, prefer the agent snapshot's effective reasoning
effort. If no snapshot exists, fall back to the requested effort or the default.

That matches the same fallback shape already used for the effective model.

### 3. Preserve completed spawn effort in app-server surfaces

Update both live app-server notification mapping and rollout history rebuild so
completed `ThreadItem::CollabAgentToolCall` records carry
`reasoning_effort: Some(...)`.

### 4. Keep local metadata intact

Do not remove or rename:

- `memory`
- `agent_type`
- `model: Option<String>`
- `model_provider_id`

These remain necessary for local memory continuity and subagent provenance.

## Expected Outcome

After this slice:

- completed spawn events once again carry the requested/effective effort
- app-server clients can reconstruct spawn summaries from completed items alone
- local memory and subagent metadata remain intact
- this part of the fork moves closer to upstream without losing branch-specific
  behavior

## Verification

- `cd codex-rs && just fmt`
- `cargo test -p codex-app-server-protocol thread_history`
- `cargo test -p codex-tui-app-server collab_spawn`
- `cargo test -p codex-tui collab_spawn`
- `PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`
