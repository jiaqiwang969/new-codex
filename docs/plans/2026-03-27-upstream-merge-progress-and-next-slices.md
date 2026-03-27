# Upstream Merge Progress And Next Slices

**Date:** 2026-03-27
**Branch state:** `dd7ca0d65`
**Baseline:** `upstream/main`

> **Purpose:** capture the current upstream-merge cleanup status after the
> recent guardian/model-sub/app-server work, and define the next merge slices
> in the order that best matches the current user decisions.

## Current Decisions

### Preserve

- account-pool and `config-pool.toml` / `auth-pool.json`
- custom provider endpoints such as `https://code.ppchat.vip`
- `model_sub` and `model_sub_responses`
- Entire / memory continuity
- agent worktrees
- TUI `session_bar`
- TUI `ralph-loop`
- local collab/app-server metadata:
  - `MemoryLink`
  - `agent_type`
  - `model`
  - `model_provider_id`

### Drop Or Do Not Revive

- local Smart Access runtime
- `endpoint-sec`
- `request_security_override`
- `/freeze`
- TUI-only `team_profile`
- TUI-only `team_profile_vouch`
- TUI-only `model_sub_vouch`
- shared-wire `model_source`
- shared-wire `model_source_detail`

## Progress Since The First Merge Playbook

### 1. Failed security line is no longer a runtime blocker

Confirmed current state:

- no live `smart_access` runtime references
- no live `endpoint-sec` runtime references
- no live `request_security_override` runtime references
- no active `/freeze` user flow in runtime code

Remaining references are historical analysis or cleanup notes, not runnable
behavior.

### 2. Guardian baseline is now being treated as upstream-owned

Completed in this round:

- removed legacy local agent guards module
- aligned model presets back to an upstream-style constants-only shape
- preserved upstream-required collaboration/app-server `reasoning_effort`
  semantics for completed spawn items

Implication:

- guardian and app-server collaboration lifecycle now have a cleaner upstream
  baseline
- local customization in this area is no longer being treated as "guardian
  fork logic"

### 3. Shared-wire trimming is mostly done

Confirmed current state:

- `MemoryLink` remains intentionally alive
- `agent_type` remains intentionally alive
- `model` remains intentionally alive
- `model_provider_id` remains intentionally alive
- `model_source` / `model_source_detail` are not active shared-wire fields

Remaining `*_source` references are TUI status labels, not protocol or
app-server merge drivers.

### 4. `freeze` feature cleanup is effectively complete in code

Current `codex-rs/features` delta versus `upstream/main` is now limited to:

- local `agent_worktrees`
- a JS REPL menu-description text change inherited from newer upstream text

There is no longer a live `FreezeSandboxDebug` feature branch to carry forward.

### 5. Obsolete Smart Access plan docs are not the current blocker

The originally targeted obsolete Smart Access / `endpoint-sec` plan files are
already gone from the active tree. What remains are historical inventory or
execution notes that explain why this line was dropped.

## Remaining Local Customization Blocks

These are the real merge drivers that still matter after the cleanup above.

### Block A: Account Pool / Provider Routing

Why it matters:

- this is the highest-priority user requirement
- it is what keeps `https://code.ppchat.vip` and pool failover behavior alive
- later `model_sub` and child-agent routing depend on it

Hotspots:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/state/session.rs`
- `codex-rs/core/src/model_provider_info.rs`

What must survive:

1. logical-provider `account_pool`
2. `config-pool.toml` overlay
3. turn-scoped account selection
4. cooldown-based failover
5. auth lookup by selected account `env_key`

Suggested sub-slices:

1. provider schema and pool overlay
   - `codex-rs/core/src/model_provider_info.rs`
   - `codex-rs/core/src/provider_pool.rs`
   - `codex-rs/core/src/provider_routing.rs`
   - `codex-rs/core/src/config/mod.rs`
   - protects `account_pool`, built-in-family override, and
     `config-pool.toml` loading
2. turn runtime and failover state
   - `codex-rs/core/src/provider_pool_runtime.rs`
   - `codex-rs/core/src/provider_pool_failover.rs`
   - `codex-rs/core/src/state/session.rs`
   - account-pool-specific paths inside `codex-rs/core/src/codex.rs`
   - protects per-turn probe order, cooldowns, and in-round failover
3. client/auth binding
   - `codex-rs/core/src/client.rs`
   - `codex-rs/core/src/provider_auth.rs`
   - `codex-rs/core/src/config/mod.rs`
   - protects selected-account `env_key` lookup and provider-aware client
     session construction

Current checkpoint:

- `config-pool.toml` overlay is now account-pool-only again
- legacy top-level pool-provider `base_url` / `env_key` overlay behavior has
  been removed
- verified against the local `~/.codex/config-pool.toml` shape: active Anthropic
  `https://code.ppchat.vip` routing comes from `account_pool`, not the removed
  legacy top-level path
- committed follow-up checkpoint `c3134a01c` now locks three runtime seams:
  - startup prewarm uses the resolved account-pool provider, not the logical
    provider shell
  - provider-switch background text no longer previews a stale first pool
    account before the resolved turn context appends the actual selected key
  - single-account pools no longer restart into the same key as a fake
    failover target
- targeted verification is green for:
  - provider-pool overlay tests
  - provider-pool runtime cooldown / retry-order tests
  - selected-account `env_key` resolution test
  - startup prewarm / provider-switch / single-account failover regressions

Remaining risk inside Block A:

- account-pool runtime is now better covered, but the largest merge surface is
  still the broader `client.rs` / `codex.rs` churn around provider-aware turn
  reconstruction
- next work should stay focused on preserving pool semantics while trimming
  unnecessary divergence from upstream in those files
- `config/mod.rs` still mixes true account-pool requirements with separate
  local provider expansion work (`Gemini`, `Grok`, `Gemma`, Anthropic-native
  additions), so the next review pass should split "pool semantics" from
  "extra provider inventory" instead of treating that whole file as one merge
  block

### Block B: Memory / Context Packet / Entire Core

Why it matters:

- this is a real workflow differentiator
- core logic is more stable to preserve than shared-wire layout

Hotspots:

- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/core/src/context_packet.rs`
- `codex-rs/core/src/entire_integration.rs`
- `codex-rs/core/src/entire_summary_generator.rs`

What must survive:

1. persistent thread memory summaries
2. filtered memory traces
3. context packet injection
4. Entire checkpoint context enrichment

Suggested sub-slices:

1. local memory and context core
   - `codex-rs/core/src/thread_memory.rs`
   - `codex-rs/core/src/context_packet.rs`
   - `codex-rs/core/src/entire_integration.rs`
   - `codex-rs/core/src/entire_summary_generator.rs`
2. boundary reattachment
   - `codex-rs/protocol/src/protocol.rs`
   - `codex-rs/app-server-protocol/src/protocol/v2.rs`
   - `codex-rs/app-server/src/bespoke_event_handling.rs`
   - `codex-rs/app-server/src/codex_message_processor.rs`
   - protects `MemoryLink` exposure after the core layer is stable

### Block C: MemoryLink / Collab / App-Server Reattachment

Why it matters:

- local continuity becomes visible only when the boundary surfaces keep it
- this is where upstream churn is still strongest

Hotspots:

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`

What must survive:

1. `MemoryLink`
2. `agent_type`
3. `model`
4. `model_provider_id`
5. completed-spawn `reasoning_effort`

Merge rule:

- preserve local semantics
- follow upstream lifecycle shape
- do not expand the shared wire again for `model_source*`

### Block D: Agent Worktrees

Why it matters:

- it is a narrow, differentiated workflow feature
- unlike the old security line, it does not force new shared-wire concepts

Hotspots:

- `codex-rs/core/src/agent_worktree.rs`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/cli/src/main.rs`

What must survive:

1. fork-session isolated worktree creation
2. lease persistence under `.codex/leases`
3. resume-time recovery and restoration

### Block E: TUI Workbench Reattachment

Why it matters:

- `session_bar` and `ralph-loop` are still explicitly wanted
- they are lower-risk features than protocol work, but they attach through the
  two most conflict-heavy TUI files

Hotspots:

- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/ralph_loop.rs`

Current judgment:

- `session_bar` is active and should stay
- `ralph-loop` is active and should stay
- `git-graph` exists as parked legacy code, but is not the current active
  merge driver

## Recommended Next Execution Order

1. Finish the current guardian/app-server cleanup batch and commit it as one
   coherent slice.
2. Tackle account-pool/provider routing before any further `model_sub` work.
3. Preserve memory/context/Entire core logic before touching more wire shape.
4. Reattach `MemoryLink` and remaining collab metadata against current upstream
   contracts.
5. Reattach agent worktrees.
6. Reattach `session_bar` and `ralph-loop` against the newer TUI structure.

## Current High-Risk Files

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

## Immediate Conclusion

The merge is no longer blocked by the failed Smart Access / `endpoint-sec` /
`freeze` line. The real remaining work is to preserve the still-valuable local
workflow blocks while continuing to accept upstream ownership of guardian and
shared lifecycle structure.
