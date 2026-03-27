# Collab Spawn Live Metadata Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve `agent_type`, `model`, and `model_provider_id` for live app-server collab spawn items and render them in the app-server TUI.

**Architecture:** Keep the shared-wire contract unchanged. Fix the adapter layer in `tui_app_server/src/chatwidget.rs` so completed spawn items recover metadata from app-server `agents_states`, then mirror core TUI spawn rendering in `tui_app_server/src/multi_agents.rs`.

**Tech Stack:** Rust, Cargo tests, insta snapshots, `just fmt`, `just fix`

---

### Task 1: Lock down the live spawn regression in tests

**Files:**
- Modify: `codex-rs/tui_app_server/src/chatwidget/tests.rs`
- Test: `codex-rs/tui_app_server/src/chatwidget/snapshots/codex_tui_app_server__chatwidget__tests__app_server_collab_spawn_completed_renders_requested_model_and_effort.snap`

**Step 1: Write the failing test input**

Update the existing live spawn history test so the completed app-server item
includes:

- `agent_type = Some("explorer")`
- `model = Some("gpt-5-mini")`
- `model_provider_id = Some("anthropic")`

and keep the spawn request summary (`gpt-5 high`) on the in-progress item.

**Step 2: Run the targeted test to verify it fails**

Run:

```bash
cargo test -p codex-tui-app-server live_app_server_collab_spawn_completed_renders_requested_model_and_effort -- --exact
```

Expected: FAIL because the live adapter or renderer still drops the metadata.

### Task 2: Preserve metadata in the live app-server adapter

**Files:**
- Modify: `codex-rs/tui_app_server/src/chatwidget.rs`

**Step 1: Keep spawn metadata on completed items**

When converting a completed `ThreadItem::CollabAgentToolCall` for
`SpawnAgent`, recover the first receiver's state from `agents_states` and copy:

- `agent_type`
- `model`
- `model_provider_id`

into the synthesized `CollabAgentSpawnEndEvent`.

**Step 2: Keep the fallback behavior unchanged**

If there is no receiver or no matching `agents_states` entry, keep the current
fallback status and leave metadata empty.

### Task 3: Mirror core TUI spawn rendering in app-server TUI

**Files:**
- Modify: `codex-rs/tui_app_server/src/multi_agents.rs`
- Test: `codex-rs/tui_app_server/src/multi_agents.rs`
- Test: `codex-rs/tui_app_server/src/multi_agents/snapshots/codex_tui_app_server__multi_agents__tests__collab_agent_transcript.snap`

**Step 1: Render the metadata fields**

Update `spawn_end` to render detail lines for:

- role (`agent_type`)
- model (`model`)
- provider (`model_provider_id`)

before the prompt preview, matching the core TUI ordering.

**Step 2: Run the targeted renderer tests**

Run:

```bash
cargo test -p codex-tui-app-server collab_events_snapshot -- --exact
```

Expected: FAIL before accepting the new snapshot, PASS after implementation and
snapshot update.

### Task 4: Verify and format the slice

**Files:**
- Modify: `docs/plans/2026-03-27-collab-spawn-live-metadata-design.md`
- Modify: `docs/plans/2026-03-27-collab-spawn-live-metadata-implementation.md`

**Step 1: Run crate tests**

Run:

```bash
cargo test -p codex-tui-app-server
```

Expected: PASS

**Step 2: Run required formatting and linting**

Run:

```bash
cd codex-rs && just fmt
cd codex-rs && just fix -p codex-tui-app-server
cd /Users/jqwang/.config/superpowers/worktrees/new-codex/probe-upstream-9dbe09834 && PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source
```

Expected: PASS
