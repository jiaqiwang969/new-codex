# Upstream Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align the custom branch with `upstream/main` while preserving the required local account-pool and Entire/memory features, and permanently drop the failed smart-access, freeze, and endpoint-sec lines.

**Architecture:** Treat the current divergence as feature buckets instead of replaying old commits. Freeze the preserved local stacks first, then split `protocol/hooks/app-server` into mandatory memory wiring versus optional local-only features, and only after that continue merging the remaining upstream deltas.

**Tech Stack:** Git, Rust workspace crates under `codex-rs`, app-server v2 protocol/schema generation, hooks, docs, targeted Cargo tests.

---

## Current Status

- `upstream/main` is already merged into `probe/upstream-merge-9dbe09834`.
- The worktree is clean and there is no cherry-pick in progress.
- `endpoint-sec` is no longer present in the remaining diff.
- `freeze` is no longer a functional feature line in the remaining diff.
- Commit `97eb3d953f` (`fix: strip null config extras from config read`) must stay because it preserves `model_sub` and `model_sub_responses` config-read behavior.
- Low-risk config/MCP test-splitting cleanup is already complete and verified:
  - `8d348b8664`
  - `2556abae3c`
  - `b897b028b3`
  - `c3d31b9043`

## Preserved Local Requirements

- Keep account-pool and provider routing:
  - `https://code.ppchat.vip`
  - `~/.codex/config-pool.toml`
  - `~/.codex/auth-pool.json`
  - `model_sub`
  - `model_sub_responses`
- Keep Entire / MemoryLink / memory propagation.
- Remove and do not revive:
  - smart access custom line
  - freeze
  - endpoint-sec / legacy security-guard line

### Task 1: Freeze Preserved Provider and Account-Pool Stack

**Files:**
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/provider_registry.rs`
- Modify: `codex-rs/core/src/config/provider_selection.rs`
- Modify: `codex-rs/core/src/provider_pool.rs`
- Modify: `codex-rs/core/src/provider_routing.rs`
- Modify: `codex-rs/core/src/model_provider_info.rs`
- Modify: `codex-rs/core/src/provider_inventory.rs`
- Modify: `codex-rs/core/src/utility_model.rs`
- Modify: `codex-rs/login/src/auth/manager.rs`
- Modify: `docs/config.md`
- Modify: `codex-rs/config-examples/config-pool.toml`
- Modify: `codex-rs/config-examples/auth-pool.json`

**Step 1: Treat provider/account-pool as a preserved local patch stack**

Record these files as protected during upstream cleanup. Do not overwrite them from upstream unless the change is explicitly reconciled with local pool behavior.

**Step 2: Preserve the provider config-read baseline**

Keep `97eb3d953f` in place so config RPC continues stripping null extras while retaining the local `model_sub` fields.

**Step 3: Preserve the pool-backed Anthropic/Gemini/Grok/Gemma behavior**

Do not revert:
- `config-pool.toml`
- `auth-pool.json`
- `code.ppchat.vip`
- builtin-family override behavior

**Step 4: Re-run focused provider/config tests after any provider merge cleanup**

Run:
```bash
cargo test -p codex-core mcp_requirements_tests
cargo test -p codex-core mcp_config_tests
cargo test -p codex-core mcp_stdio_edit_tests
cargo test -p codex-core mcp_http_edit_tests
cargo test -p codex-core mcp_servers_toml_ignores_unknown_server_fields -- --exact
```

Expected: PASS

### Task 2: Freeze Entire and Memory Propagation Stack

**Files:**
- Modify: `codex-rs/state/src/runtime/memories.rs`
- Modify: `codex-rs/rollout/src/state_db.rs`
- Modify: `codex-rs/rollout/src/list.rs`
- Modify: `codex-rs/rollout/src/policy.rs`
- Modify: `codex-rs/core/src/thread_memory.rs`
- Modify: `codex-rs/core/src/context_packet.rs`
- Modify: `codex-rs/core/src/context_packet_memory.rs`
- Modify: `codex-rs/core/src/entire_summary_generator.rs`
- Modify: `codex-rs/hooks/src/entire_summary.rs`

**Step 1: Treat Entire and thread memory as a preserved local stack**

Keep the memory persistence and context-packet flow introduced by:
- `f743260f39`
- `71ad044a1b`
- `36d066ca9c`
- `42cdc13e98`
- `c2201942fd`

**Step 2: Preserve hook-facing Entire integration**

Do not remove `hooks/src/entire_summary.rs` or the core-side Entire summary generator unless the user explicitly decides to abandon Entire.

**Step 3: Re-run focused Entire and memory tests after any merge cleanup**

Run:
```bash
cargo test -p codex-core entire_config_test
cargo test -p codex-core memories
```

Expected: PASS

### Task 3: Split Protocol/Hooks/App-Server into Keep vs Drop

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/hooks/src/types.rs`
- Modify: `codex-rs/hooks/src/user_notification.rs`
- Modify: `codex-rs/hooks/src/legacy_notify.rs`
- Modify: `codex-rs/hooks/src/registry.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Modify: generated files under `codex-rs/app-server-protocol/schema/`

**Step 1: Keep the MemoryLink wire contract**

Preserve the reduced memory-link surface introduced by:
- `9ea6896c98`
- `c160da40b4`
- `346df6ba10`

This includes:
- `MemoryLink`
- `memory_scope_version`
- hook payload memory fields
- app-server v2 memory propagation
- `mcp-tool-call-complete`

**Step 2: Treat reference-image controls as optional local-only features**

These are currently local-only and limited in scope:
- `Op::SetReferenceImages`
- `Op::ClearReferenceImages`
- `Op::SetImageQuality`
- `Op::SetAspectRatio`

They currently live in:
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/core/src/codex.rs`

Decision needed before cleanup:
- keep as part of the Gemini/Ralph Loop stack
- or drop them to move closer to upstream

**Step 3: Treat FileSystemMutated as optional local-only telemetry**

This line came from `81cfe1b101` and touches only a few files:
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/core/src/git_side_effects.rs`
- `codex-rs/mcp-server/src/codex_tool_runner.rs`
- `codex-rs/rollout/src/policy.rs`
- `codex-rs/tui_app_server/src/chatwidget.rs`

Decision needed before cleanup:
- keep as local telemetry
- or drop to reduce non-upstream event surface

**Step 4: Treat thought_signature as high-cost and do not touch casually**

`thought_signature` is local-only and spread across protocol, app-server, core Gemini integration, and many tests. Do not remove it unless the Gemini stack itself is being cut back.

**Step 5: Re-run focused protocol and hook tests after each cleanup slice**

Run:
```bash
cargo test -p codex-app-server-protocol
cargo test -p codex-hooks
cargo test -p codex-core mcp_tool_call_tests
```

Expected: PASS

### Task 4: Triage the Gemini, Anthropic, Ralph Loop, and Prompt Surface

**Files:**
- Modify: `codex-rs/core/src/gemini_content.rs`
- Modify: `codex-rs/core/src/gemini_streaming.rs`
- Modify: `codex-rs/core/src/gemini_types.rs`
- Modify: `codex-rs/core/src/anthropic_content.rs`
- Modify: `codex-rs/core/src/anthropic_streaming.rs`
- Modify: `codex-rs/core/src/anthropic_types.rs`
- Modify: `codex-rs/core/src/client/provider_support.rs`
- Modify: `codex-rs/protocol/src/models.rs`
- Modify: `codex-rs/core/src/agent/role.rs`
- Modify: `codex-rs/core/src/agent/builtins/*.toml`
- Modify: `codex-rs/core/*prompt.md`
- Modify: `codex-rs/core/templates/model_instructions/*`

**Step 1: Treat the Gemini stack as a deliberate local product line**

The remaining diff here is not accidental merge residue. It was introduced primarily by:
- `68615b4352`
- `d93ab4cbdb`
- `122d38f270`
- `18c5db71a4`
- `70e1dd4707`

**Step 2: Decide whether Ralph Loop and image infrastructure stay**

If Ralph Loop or local Gemini image UX is not required, this is the cleanest major bucket to reduce after provider/account-pool is frozen.

**Step 3: Decide whether Anthropic native integration stays**

Anthropic native support is also a local line and interacts with the provider stack. If it stays, it should stay on top of the preserved pool logic.

**Step 4: Keep prompt and agent-role changes only if backed by an active product feature**

Prompt/builtin/model-template changes should be evaluated feature-by-feature instead of preserved automatically.

### Task 5: Finish Remaining Upstream Alignment Slices

**Files:**
- Modify: remaining files from `git diff --name-only upstream/main...HEAD`

**Step 1: After freezing the preserved stacks, re-run the diff**

Run:
```bash
git diff --stat --compact-summary upstream/main...HEAD
```

Expected: a smaller diff focused on intentional local product areas

**Step 2: Merge remaining upstream-compatible slices one bucket at a time**

Preferred order:
1. provider/account-pool preserved baseline
2. Entire/memory preserved baseline
3. protocol/hooks/app-server keep-vs-drop cleanup
4. Gemini/Ralph Loop/prompt triage
5. residual app-server and role/model cleanup

**Step 3: Only request full-workspace verification after the user agrees**

If common/core/protocol changes accumulate and the preserved local state is stable, ask before running the full workspace suite:

```bash
cargo test
```

### Task 6: Final Verification and Commit Hygiene

**Files:**
- Modify: any files touched by cleanup

**Step 1: Run required formatting and linting for Rust edits**

Run:
```bash
cd codex-rs
just fmt
just fix -p codex-core
PATH=$HOME/.local/share/cargo/bin:$PATH ./tools/argument-comment-lint/run.sh
```

Expected: PASS

**Step 2: Make slice commits with explicit scope**

Do not create one giant merge-reconciliation commit. Commit by bucket with subjects that explain whether the slice preserves a local customization or realigns to upstream.

**Step 3: Ensure every commit message ends with the required trailer**

```text
Co-authored-by: Codex <noreply@openai.com>
```
