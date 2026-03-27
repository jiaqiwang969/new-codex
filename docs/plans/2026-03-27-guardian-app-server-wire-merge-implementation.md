# Guardian App-Server Wire Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** restore upstream-style `reasoning_effort` on completed collab spawn
events while preserving local memory and agent metadata fields.

**Architecture:** make the smallest shared-wire patch possible. Update the
protocol event, emit the effective effort from both spawn handlers, preserve it
in app-server live mapping and rollout history reconstruction, then verify with
focused protocol/TUI tests.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just argument-comment-lint-from-source`

---

### Task 1: Restore the field at the protocol boundary

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`

**Step 1: Add completed spawn effort back to the event**

Add `reasoning_effort: ReasoningEffortConfig` to
`CollabAgentSpawnEndEvent`.

**Step 2: Keep old rollout replay working**

Use `#[serde(default)]` on the new field so pre-existing local rollout logs
without this field still deserialize.

### Task 2: Emit effective effort from both spawn handlers

**Files:**
- Modify: `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`

**Step 1: Compute the effective effort**

Prefer the spawned agent snapshot's `reasoning_effort`; otherwise fall back to
the requested override or the default.

**Step 2: Include it in the completed spawn event**

Populate `CollabAgentSpawnEndEvent.reasoning_effort` in both handlers while
leaving `memory`, `agent_type`, `model`, and `model_provider_id` intact.

### Task 3: Keep app-server completed items faithful

**Files:**
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/tui_app_server/src/chatwidget.rs`

**Step 1: Preserve effort in live notifications**

Map completed spawn events to app-server items with
`reasoning_effort: Some(end_event.reasoning_effort)`.

**Step 2: Preserve effort during rollout reconstruction**

Rebuilt completed spawn items from historical rollout events should also carry
`Some(payload.reasoning_effort)`.

**Step 3: Keep completed-only UI fallback working**

When app-server chatwidget reconstructs a core spawn-end event from a completed
item, supply the same requested effort summary so history rendering can work
even if the begin item was not seen.

### Task 4: Update tests and verify

**Files:**
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/tui/src/multi_agents.rs`
- Modify: `codex-rs/tui/src/chatwidget/tests.rs`
- Modify: `codex-rs/tui_app_server/src/multi_agents.rs`
- Modify: `codex-rs/tui_app_server/src/chatwidget/tests.rs`

**Step 1: Update direct event constructors**

Any direct `CollabAgentSpawnEndEvent` construction now needs the restored
`reasoning_effort` field.

**Step 2: Tighten at least one completed-only fallback test**

Cover the app-server path where only the completed spawn item is available.

**Step 3: Run focused verification**

Run:

```bash
cd codex-rs && just fmt
cargo test -p codex-app-server-protocol thread_history
cargo test -p codex-tui-app-server collab_spawn
cargo test -p codex-tui collab_spawn
PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

**Step 4: Ask before any broader full-workspace test**

Do not run full `cargo test` / `just test` without user approval.
