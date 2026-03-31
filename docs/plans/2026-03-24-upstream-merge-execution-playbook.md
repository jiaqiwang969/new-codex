# Upstream Merge Execution Playbook

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Status (2026-03-31): Historical execution playbook.**
> The upstream merge has since landed on `main`, but the final outcome did
> **not** preserve every local feature family listed below.
> In particular, treat agent worktrees and TUI workbench restoration
> (`git-graph`, `session bar`, `ralph-loop`) as rejected historical branches,
> not current merge tasks.

> **Baseline refresh note:** after rechecking the live upstream remote, the
> tracked official baseline moved from `047ea642d` to `9dbe09834`. The newest
> upstream churn in this area is concentrated in `config/mod.rs`, `codex.rs`,
> and TUI cwd plumbing, not in local account-pool or `model_sub` semantics.

> **Purpose:** turn the current merge research into a practical execution
> order, with explicit preserve/drop decisions and conflict-handling rules for
> each feature family.

## Non-Negotiables

These constraints come from the current branch state plus explicit user
direction:

1. Keep account-pool / `config-pool` behavior.
2. Keep custom provider endpoints such as `https://code.ppchat.vip`.
3. Do not resurrect the removed local Smart Access / `endpoint-sec` /
   `request_security_override` / `/freeze` line.
4. Follow upstream guardian / approvals reviewer direction instead of the old
   local security line.
5. Treat dirty workspace state as part of the merge context, not as something
   to clean away first.

## Merge Philosophy

The safest way to merge `upstream/main` is not "ours vs theirs" per file.

The right split is:

- **Upstream baseline**
  - guardian approvals
  - approvals reviewer semantics
  - MCP elicitation contract
- **Local differentiated workflow**
  - account-pool/provider routing
  - provider-family transport / utility routing
  - `model_sub`
  - memory/context/Entire continuity
  - agent worktrees
  - TUI workbench features

So the merge should prefer:

- upstream architecture where the feature is now upstream-owned
- local semantics where the feature is still a real branch differentiator

## New Upstream Changes To Mostly Take As-Is

After refreshing `upstream/main`, the newest official commits touching our main
overlap set are mostly upstream-owned infrastructure changes rather than
competing local product semantics.

### 1. `504aeb0e0` `Use AbsolutePathBuf for cwd state`

Judgment: take upstream structure.

Reason:

- this is a general state/type hardening pass across config, session runtime,
  memory consolidation, and TUI
- it does not compete with local account-pool or `model_sub` intent

Merge caution:

- port local provider/account hooks onto the new absolute-cwd types
- do not accidentally drop local provider routing while rewriting the type
  plumbing

### 2. `9dbe09834` `Extract codex-core-skills crate`

Judgment: take upstream structure.

Reason:

- this is upstream codebase modularization
- local preserved value is not in the old skills import paths

Merge caution:

- reattach any local skill-loading integrations to the new upstream module
  boundaries

### 3. `d273efc0f` `Extract codex-analytics crate`

Judgment: take upstream structure.

Reason:

- this is upstream package/layout ownership
- it is orthogonal to local provider routing and TUI workflow differentiation

### 4. `6b10e186c` `Add non-interactive resume filter option`

Judgment: take upstream behavior unless it directly breaks a local TUI path.

Reason:

- this is a small upstream TUI/resume workflow improvement
- it does not appear to compete with local `model_sub` or account-pool logic

### 5. `91337399f` `[apps][tool_suggest] Remove tool_suggest's dependency on tool search.`

Judgment: take upstream behavior.

Reason:

- this is upstream `codex.rs` cleanup in an area not central to the preserved
  local semantics

Practical implication:

- the newest upstream motion increases mechanical merge work in shared files
- it does **not** materially change which semantics should be preserved from
  the local branch

## Block Order

### Block 0: Lock In The Deletions

**Decision:** preserve current local removal of the failed security line.

**Keep removed:**

- `endpoint-sec`
- `request_security_override`
- local Smart Access runtime
- `/freeze`

**Why first:**

- this is already the current branch direction
- it narrows the merge target before other conflicts are resolved

**Conflict rule:**

- if upstream touches adjacent approval/runtime files, keep upstream guardian
  semantics but do not reintroduce the deleted local security path

### Block 1: Accept Upstream Guardian / Approval Baseline

**Decision:** follow upstream.

**Primary files:**

- `codex-rs/core/src/guardian/**`
- `codex-rs/protocol/src/approvals.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

**Preserve from local only if still needed:**

- any glue that connects local differentiated features to guardian, not the
  guardian core itself

**Why second:**

- this removes a lot of false "customization" noise
- it gives the merge a stable approval model to build around

**Conflict rule:**

- favor upstream guardian/app-server approval lifecycle behavior by default
- do not try to preserve an older local guardian patch shape

### Block 2: Preserve Account Pool / Provider Routing

**Decision:** preserve local semantics.

**Primary files:**

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/model_provider_info.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/state/session.rs`
- `codex-rs/core/src/client.rs`

**Must survive:**

1. `account_pool` on logical providers
2. `config-pool.toml` overlay
3. turn-scoped provider/account resolution
4. cooldown-based temporary failover
5. auth lookup by selected account `env_key`

**Why early:**

- this is the most important explicit user requirement
- later blocks depend on correct provider routing

**Real attachment points inside shared files:**

- `config/mod.rs`
  - `config-pool.toml` overlay, `user_configured_provider`, provider-family
    auto-switch
- `codex.rs`
  - runtime provider restore/switch, account-pool cooldown, active-account
    labels
- `client.rs`
  - auth lookup by active account `env_key`

**Conflict rule:**

- preserve semantics, not exact local hunks
- when upstream refactors provider/config code, re-express local pool behavior
  in the new structure instead of replaying the old patch verbatim

### Block 3: Preserve Provider-Family Utility Routing Before `model_sub`

**Decision:** preserve local feature, but fit it onto upstream collaboration.

**Primary files:**

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/model_compat.rs`
- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/anthropic_content.rs`
- `codex-rs/core/src/anthropic_streaming.rs`
- `codex-rs/core/src/gemini_content.rs`
- `codex-rs/core/src/gemini_streaming.rs`

**Must survive:**

1. provider-family-aware utility routing for internal tasks
2. compatibility with account-pool/custom endpoints
3. local capability gating for Gemini / Gemma / Grok / Claude families
4. provider-specific transport shaping for Anthropic and Gemini families
5. a Responses-compatible path for utility work that cannot run on non-Responses
   providers

**Why after Block 2:**

- provider-family utility routing is not trustworthy unless provider routing is
  already stable
- `model_sub` depends on this layer, so this block has to be stabilized first

**Real attachment points inside shared files:**

- `config/mod.rs`
  - family-aware provider auto-switch must not clobber custom providers
- `codex.rs`
  - `TurnContext::with_model()` and runtime model switches must pick the
    right logical provider family
- `client.rs`
  - Responses vs Gemini vs Anthropic transport dispatch
- `memories/phase2.rs`
  - memory consolidation must resolve provider family consistently too

**Conflict rule:**

- preserve the semantics, not the current module boundaries
- treat local-only transport modules as reattachable workflow code, not as
  sacred patch shape
- when upstream refactors provider/config/runtime flow, re-express the routing
  layer against the newer attachment points

### Block 4: Preserve `model_sub` On Top Of Utility Routing

**Decision:** preserve local feature, but fit it onto upstream collaboration.

**Primary files:**

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/model_sub_vouch.rs`
- `codex-rs/core/src/state/session.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/tools/spec.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/*.rs`
- `codex-rs/tui/src/team_profile.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`

**Keep only if still worth the complexity:**

- `model_sub_vouch`
- session-local calibration memory

**Real attachment points inside shared files:**

- `agent/role.rs`
  - built-in role descriptions and defaults currently assume `model_sub`
    inheritance
- `protocol.rs`
  - collab events gained local `model_source` / `agent_type` / memory metadata
- `app-server-protocol/v2.rs`
  - app-server config/profile payloads gained `model_sub` and
    `model_sub_responses`
- `tui/src/app.rs` and `tui/src/chatwidget.rs`
  - local pickers, vouch flows, and team-profile UX all land in these
    high-churn files

**Conflict rule:**

- favor upstream collaboration lifecycle shape
- preserve local submodel selection semantics
- do not force current local app-server metadata fields if they are only
  partially wired

### Block 5: Preserve Memory / Context Packet / Entire

**Decision:** preserve local workflow layer.

**Primary files:**

- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/core/src/context_packet.rs`
- `codex-rs/core/src/entire_integration.rs`
- `codex-rs/core/src/entire_summary_generator.rs`

**Must survive:**

1. persistent thread memory summaries
2. filtered memory traces
3. context packet injection
4. Entire checkpoint summary enrichment

**Why here:**

- this block is mostly local core logic
- it should be stabilized before re-exposing it at protocol boundaries

**Conflict rule:**

- preserve the local semantics even if file boundaries change
- do not couple this step to app-server wire decisions yet

### Block 6: Reattach `MemoryLink` / Continuity Wire

**Decision:** preserve the semantics, not the old field layout.

**Primary files:**

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`

**Must survive:**

1. a `MemoryLink`-style continuity concept
2. turn-level memory continuity metadata
3. collab/MCP/review surfaces that can expose memory continuity when useful

**Why not earlier:**

- wire contracts are the most conflict-prone part of the memory stack
- core logic should be stable before shared contract work

**Shared-file caution:**

- keep `MemoryLink` semantics
- do not assume every current optional collab-routing field deserves to survive
  unchanged
- `protocol.rs` and `app-server-protocol/v2.rs` should follow upstream event
  shape conventions unless a local field has a real downstream consumer

**Conflict rule:**

- preserve the boundary semantics
- adapt the exact payload shape to current upstream protocol conventions

### Block 7: Preserve Agent Worktrees

**Decision:** preserve local feature.

**Primary files:**

- `codex-rs/core/src/agent_worktree.rs`

**Must survive:**

1. per-agent/per-fork git worktree creation
2. lease persistence under `.codex/leases`
3. worktree recreation from lease when needed

**Why here:**

- it is a clear local asset
- it is narrow enough to keep without driving shared protocol decisions

**Conflict rule:**

- preserve the feature as a workflow primitive
- integrate with newer upstream agent lifecycle code rather than insisting on
  current local attachment points

### Block 8: Preserve TUI Workbench Features

**Decision:** preserve local features, re-integrate against upstream TUI shell.

**Primary files:**

- `codex-rs/git-graph/**`
- `codex-rs/tui/src/git_graph_widget.rs`
- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/ralph_loop.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

**Must survive:**

1. `git-graph`
2. `session bar`
3. `ralph-loop`

**Why late:**

- these are user-facing integrations on top of already-shifting core/runtime
  structures
- they are easier to reattach once the runtime layer is stable

**Conflict rule:**

- preserve behavior
- do not preserve giant old `app.rs` / `chatwidget.rs` hunks verbatim

### Block 9: Re-evaluate Extra App-Server Observability

**Decision:** keep selectively.

**Primary files:**

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/*.rs`

**Fields to re-evaluate:**

- `agent_type`
- `model_provider_id`
- `model_source`
- `model_source_detail`

**Why last:**

- these are useful but not foundational
- some are only partially populated in the current local branch

**Conflict rule:**

- keep only fields that still have clear consumers after the upstream merge
- avoid preserving incomplete wire expansion just because it already exists

## High-Risk Files By Block

### Runtime / provider core

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/state/session.rs`

### Shared wire surface

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`

### TUI reintegration

- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

## Practical Merge Tactics

### Prefer semantic resolution over mechanical resolution

When a conflict lands in a hotspot file:

1. identify which block owns the behavior
2. decide whether that block is upstream-owned or local-owned
3. rewrite the smallest coherent merged version in the new file structure

### Do not mix unrelated blocks in one conflict pass

Bad pattern:

- solving provider routing, memory wire, and TUI integration in the same edit

Better pattern:

- land provider semantics first
- then memory core
- then memory wire
- then TUI attachments

### Validate by feature family, not only by workspace compile

Examples:

- account-pool:
  - provider overlay loads
  - custom endpoint survives
  - cooldown/failover still works
- model-sub:
  - child model selection still respects provider/account pool
- memory:
  - continuity metadata still appears at boundaries where expected
- TUI:
  - features remain reachable, not just compilable

## Questions To Defer Until Actual Conflict Resolution

These are not blockers for starting the merge, but they may need a decision
once the real conflict shape is visible:

1. Is `model_sub_vouch` still worth carrying if upstream collaboration evolves
   rapidly in a different direction?
2. Which extra app-server collab metadata fields still have real consumers?
3. Do any upstream TUI changes make `ralph-loop` or `session bar` integration
   substantially simpler than the current local wiring?

## Recommended Next Move

Once this playbook is accepted, the next practical step is:

1. start the real `merge upstream/main`
2. resolve conflicts in block order from this document
3. stop and discuss only when a conflict requires a real product decision,
   rather than a mechanical port

That is the point where the research phase should hand off to merge execution.
