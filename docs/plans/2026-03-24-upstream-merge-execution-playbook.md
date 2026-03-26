# Upstream Merge Execution Playbook

**Date:** 2026-03-24
**Branch state:** `0fa92816c0`
**Baseline:** `upstream/main` at `c9214192c5`

**Latest dry-run preview:**

- see `docs/plans/2026-03-26-upstream-main-c921-conflict-preview.md`

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
  - `model_sub`
  - memory/context/Entire continuity
  - agent worktrees
  - TUI workbench features

So the merge should prefer:

- upstream architecture where the feature is now upstream-owned
- local semantics where the feature is still a real branch differentiator

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

**Conflict rule:**

- preserve semantics, not exact local hunks
- when upstream refactors provider/config code, re-express local pool behavior
  in the new structure instead of replaying the old patch verbatim

### Block 3: Preserve `model_sub` On Top Of Provider Routing

**Decision:** preserve local feature, but fit it onto upstream collaboration.

**Refined note:**

- see `docs/plans/2026-03-26-model-sub-ux-analysis.md`

**Primary files:**

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/state/session.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/tools/spec.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/*.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

**Must survive:**

1. `model_sub`
2. `model_sub_responses`
3. child-role inheritance of `model_sub`
4. provider-family-aware utility routing
5. compatibility with account-pool/custom endpoints

**Keep only if still worth the complexity:**

- `team_profile`
- `model_sub_vouch`
- `team_profile_vouch`
- session-local calibration memory

**Why after Block 2:**

- `model_sub` is not trustworthy unless provider routing is already stable

**Conflict rule:**

- favor upstream collaboration lifecycle shape
- preserve local submodel selection semantics
- preserve config/protocol support for `model_sub` and `model_sub_responses`
- treat `team_profile` / `model_sub_vouch` / `team_profile_vouch` as follow-on
  TUI UX, not as shared-wire requirements
- do not force current local app-server metadata fields if they are only
  partially wired

### Block 4: Preserve Memory / Context Packet / Entire

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

### Block 5: Reattach `MemoryLink` / Continuity Wire

**Decision:** preserve the semantics, not the old field layout.

**Refined note:**

- see `docs/plans/2026-03-26-memorylink-contract-analysis.md`

**Primary files:**

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`

**Must survive:**

1. a `MemoryLink`-style continuity concept
2. `binding_key` as the primary continuity join key
3. `scope_version` as the short readable version id
4. turn-level memory continuity metadata
5. collab/MCP/review surfaces that can expose memory continuity when useful
6. app-server v2 `Turn.memory`
7. app-server v2 item-level `memory` on MCP/collab entries

**Compatibility, but not canonical:**

- hook payload flat fields such as `memory_scope_version` /
  `memory_binding_key`
- diagnostic extras such as `scope_kind` / `summary_sha256`

**Why not earlier:**

- wire contracts are the most conflict-prone part of the memory stack
- core logic should be stable before shared contract work

**Conflict rule:**

- preserve the boundary semantics
- adapt the exact payload shape to current upstream protocol conventions
- treat nested `memory` as canonical shape
- treat flat hook fields as compatibility output, not as a reason to grow the
  wire surface further

### Block 6: Preserve Agent Worktrees

**Decision:** preserve local feature.

**Refined note:**

- see `docs/plans/2026-03-26-agent-worktree-analysis.md`

**Primary files:**

- `codex-rs/core/src/agent_worktree.rs`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/cli/src/main.rs`

**Must survive:**

1. fork-session worktree creation
2. lease persistence under `.codex/leases`
3. resume-time switch back to leased worktree
4. worktree recreation from lease when needed
5. debug CLI list / ensure recovery path

**Important nuance:**

- the codebase currently proves fork-session isolation and resume/restore
  semantics
- `SpawnedAgent` worktree isolation exists in types/docs, but is not yet wired as
  a confirmed runtime path
- do not let merge scope expand around preserving a behavior that is not
  actually live yet

**Why here:**

- it is a clear local asset
- it is narrow enough to keep without driving shared protocol decisions

**Conflict rule:**

- preserve semantics, not the exact current entry-point wiring
- prefer protecting fork/resume/lease behavior over preserving every current
  mention of spawned-agent isolation
- preserve the feature as a workflow primitive
- integrate with newer upstream agent lifecycle code rather than insisting on
  current local attachment points

### Block 7: Preserve TUI Workbench Features

**Decision:** preserve local features, re-integrate against upstream TUI shell.

**Refined notes:**

- see `docs/plans/2026-03-26-tui-workbench-analysis.md`
- see `docs/plans/2026-03-26-ralph-loop-lifecycle-analysis.md`
- see `docs/plans/2026-03-26-git-graph-analysis.md`

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

### Block 8: Re-evaluate Extra App-Server Observability

**Decision:** keep `agent_type`; keep `model_provider_id` only if low-friction;
do not treat `model_source*` as a merge blocker.

**Refined note:**

- see `docs/plans/2026-03-26-collab-observability-analysis.md`

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

**Current reality:**

- `agent_type` is the only one currently emitted end-to-end
- `model_provider_id` has some thread-history shape, but spawn handlers still
  emit `None`
- `model_source` / `model_source_detail` currently have core TUI labels but no
  real producer path

**Keep:**

- `agent_type`

**Keep if cheap:**

- `model_provider_id`, because it is still useful in an account-pool /
  custom-endpoint branch

**Do not force through shared wire during this merge:**

- `model_source`
- `model_source_detail`

**Why last:**

- these are useful but not foundational
- some are only partially populated in the current local branch

**Conflict rule:**

- prefer upstream collab/app-server lifecycle structure
- preserve `agent_type`
- preserve `model_provider_id` only if it can be reattached cleanly to the new
  shape
- do not expand or defend app-server v2 for `model_source*` unless a real
  producer/consumer chain exists at merge time

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

1. After core `model_sub` routing is stable, do we still want to carry the
   local `team_profile` / `team_profile_vouch` / `model_sub_vouch` UX, or is it
   acceptable to defer some of that UI layer?
2. Is `git-graph` still worth carrying given the extra vendored crate and
   lockfile churn?
3. Is `ralph-loop` still important enough to re-wire against newer upstream TUI
   lifecycle code, given its dependency on current task-completion behavior?

## Recommended Next Move

Once this playbook is accepted, the next practical step is:

1. start the real `merge upstream/main`
2. resolve conflicts in block order from this document
3. stop and discuss only when a conflict requires one of the deferred product
   decisions above, rather than a mechanical port

That is the point where the research phase should hand off to merge execution.
