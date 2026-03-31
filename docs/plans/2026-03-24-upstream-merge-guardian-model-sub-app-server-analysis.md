# Guardian / Model-Sub / App-Server Wire Merge Analysis

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Status (2026-03-31): Historical merge analysis.**
> The upstream merge has since landed on `main`.
> Read this as a branch-era architecture note, not as a promise that every
> local wire shape described below is still current.

> **Goal of this note:** separate the parts of this area that are now upstream
> baseline from the parts that are still local customization, so the eventual
> merge can anchor on the right architecture.

## Bottom Line

This area is no longer one problem.

1. `guardian` / `approvals_reviewer` / MCP elicitation are now upstream
   baseline.
2. The still-local branch value is mostly in:
   - `model_sub` and utility-model routing
   - provider/account-pool-aware child model selection
   - memory continuity metadata (`MemoryLink`) across turn/collab/app-server
   - extra collaboration metadata exposed to app-server clients
3. That means the merge should not preserve the local guardian/app-server patch
   shape as an architecture anchor. It should preserve the local model-routing
   and memory semantics, then reattach them onto newer upstream contracts.

## Fresh upstream drift after the previous local baseline

The refreshed upstream `main` added a small new round of shared-file churn on
top of the older `047ea642d` baseline:

- `504aeb0e0` moved session/config/TUI cwd state toward `AbsolutePathBuf`
- `9dbe09834` extracted core skills loading into a dedicated crate

Practical implication:

- this creates fresh merge pressure in `codex.rs`, `config/mod.rs`,
  `agent/role.rs`, `memories/phase2.rs`, `tui/src/app.rs`, and
  `tui/src/chatwidget.rs`
- it does **not** change the main architectural judgment of this note:
  guardian remains upstream-owned, while the local value still sits around
  `model_sub`, provider-aware child routing, and memory continuity wire

## What Upstream Already Covers

### 1. Guardian reviewer

Representative upstream-covered files:

- `codex-rs/core/src/guardian/**`
- `codex-rs/protocol/src/approvals.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

What this means:

- upstream already has `approvals_reviewer = user | guardian_subagent`
- upstream already has guardian assessment events
- upstream already maps guardian review lifecycle into app-server

Practical implication:

- guardian is no longer a reason to keep a big local fork in this area
- the current branch should follow upstream guardian semantics directly

### 2. MCP elicitation request/response flow

Representative upstream-covered files:

- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`

What this means:

- upstream already owns the app-server contract for `mcpServer/elicitation/request`
- this is not a local-only protocol anymore

## What The Local Branch Still Adds

### 1. `model_sub` and utility-model routing

Representative local-only / locally diverged files:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/tools/spec.rs`

Behavior:

- adds `model_sub` and `model_sub_responses` config
- teaches child-agent roles to inherit `model_sub`
- introduces utility-model routing separate from the main session model
- lets internal tasks choose a provider based on the target model family

Why it exists:

- the branch wants different models for leader vs utility/subagent work
- it also wants those utility tasks to keep working when the active session
  model is on Anthropic/Gemini/Grok rather than OpenAI

Why it conflicts:

- upstream collaboration code is active in the same area
- config, agent roles, and spawn-tool descriptions are still moving upstream

Current upstream confirmation:

- `codex-rs/core/src/utility_model.rs` does not exist in `upstream/main`
- `codex-rs/core/src/model_compat.rs` does not exist in `upstream/main`
- upstream does have collaboration mode plumbing, which means the overlap is at
  the integration layer rather than as a same-file feature baseline

### 2. `model_sub_vouch` and session-level auto selection

Representative local-only files:

- `codex-rs/core/src/model_sub_vouch.rs`
- `codex-rs/core/src/state/session.rs`
- `codex-rs/tui/src/team_profile.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`

Behavior:

- persists per-model win/loss history in `memories/model_sub_vouch.json`
- stores session-local calibration state and last recommended submodel
- supports model-sub ranking and future session pinning

Why it exists:

- the local branch is trying to learn which small model works best for
  sub-agent work instead of hardcoding one choice forever

Why it conflicts:

- upstream does not currently have this ledger concept
- the surrounding multi-agent wire/events now overlap upstream collaboration
  work even though the ledger itself is local-only
- the main TUI control surfaces for this line (`team_profile.rs`,
  `tui/model_sub_vouch.rs`) are also absent in `upstream/main`

### 3. Provider/account-pool-aware child routing

Representative files:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/state/session.rs`

Behavior:

- auto-switches providers when model families change
- preserves custom providers instead of forcing built-in upstream defaults
- resolves active accounts from `account_pool`
- keeps per-account cooldown state in session runtime
- drops incompatible encrypted reasoning history when a provider switch would
  make old encrypted items unusable

Why it exists:

- this is the glue that makes `model_sub` work with the local account-pool and
  custom endpoint setup
- without it, child-model selection would silently fall back to the wrong
  provider or wrong endpoint

Why it conflicts:

- it lands directly in `config/mod.rs` and `codex.rs`, which are already major
  upstream conflict hotspots
- it also overlaps any upstream provider/preset/refactor work

### 4. Memory continuity wire surface

Representative files:

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`

Behavior:

- introduces `MemoryLink`
- attaches memory continuity metadata to turn start/complete
- attaches memory metadata to collab lifecycle items
- attaches memory metadata to MCP tool-call items
- injects memory metadata into review-start responses

Why it exists:

- it exposes the local memory/context/Entire system at the boundaries where
  app-server clients, tools, and subagents can actually use it

Why it conflicts:

- it expands shared wire contracts, not just local runtime internals
- `protocol.rs`, `v2.rs`, and `bespoke_event_handling.rs` are exactly where
  upstream churn is fastest

### 5. Extra collaboration metadata in app-server wire

Representative files:

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/*.rs`

Behavior:

- adds `agent_type`
- adds optional `model` / `model_provider_id`
- adds `model_source` / `model_source_detail`
- carries some of that data on collab spawn/wait/send_input lifecycle events

Important caveat:

- this local wire shape is only partially wired today
- several handlers still emit `model_provider_id: None`,
  `model_source: None`, and `model_source_detail: None`

Interpretation:

- the branch clearly wanted richer observability for subagent routing
- but the exact app-server contract is not yet mature enough to treat the
  current local field layout as sacred

## What Problem The Local Author Was Solving

This block is solving three practical workflow problems:

1. use a better small model for subagents/internal tasks than the main model
2. keep that model routing compatible with local custom providers and
   account-pool endpoints
3. make memory continuity and child-agent routing visible outside core, so the
   TUI/app-server layer can explain what happened

That is a legitimate local product line. It is just no longer the same thing as
the old Smart Access security line.

## What Upstream Is Optimizing For

Upstream in this area is optimizing for:

- stable guardian approval semantics
- app-server v2 contract evolution
- collaboration lifecycle events
- cross-client protocol consistency
- collaboration mode presets rather than local team-profile / vouch workflows

So the overlap is not "local bad, upstream good". The overlap is:

- upstream owns the shared platform contracts
- the fork adds product-specific routing and memory semantics on top

## Main Merge Risks

### Risk 1: confusing upstream guardian with local-only customization

If we treat guardian as "our forked feature", we will preserve the wrong layer.

The safer view is:

- guardian base behavior belongs to upstream now
- local value sits around provider routing, model-sub selection, and memory wire

### Risk 2: preserving app-server wire shape too literally

The local app-server additions are meaningful, but some of them are still
half-populated. Preserving every current field placement exactly will increase
merge pain without necessarily preserving user value.

Most obvious example:

- local collab event payloads replaced upstream's required
  `reasoning_effort`-centric shape with optional routing metadata, but the new
  metadata is not fully populated yet

### Risk 3: breaking account-pool-aware child routing

If `model_sub` is kept without the provider/account-pool routing layer:

- child tasks may switch to the wrong provider family
- custom endpoints like `https://code.ppchat.vip` may stop applying
- pool cooldown / failover logic may disappear silently

That would preserve the config knob while breaking the real behavior.

### Risk 4: keeping memory core but losing boundary visibility

If core memory/Entire stays but `MemoryLink` propagation is dropped carelessly:

- app-server clients lose continuity metadata
- subagent and MCP surfaces become harder to explain
- the memory system still exists, but the workflow benefit gets weaker at the
  UI/API boundary

## What Must Be Preserved

### Preserve as real local product value

1. `model_sub` / `model_sub_responses`
2. utility-model routing that respects provider family and account-pool config
3. `model_sub_vouch` if we still want adaptive submodel selection
4. `MemoryLink`-style continuity semantics across the memory/context/Entire line

### Preserve only semantically, not by exact patch layout

1. app-server fields that expose memory continuity
2. collab metadata that explains child-agent routing decisions

Those should survive, but can be reshaped to fit newer upstream payload
conventions.

### Prefer upstream directly

1. guardian runtime behavior
2. approvals reviewer semantics
3. MCP elicitation request/response contract

## Recommended Merge Strategy

### Stage 1: accept upstream guardian as baseline

Do not spend merge energy trying to preserve an old local guardian shape.

### Stage 2: preserve local model-routing internals

Preserve together:

1. `model_sub` config
2. `utility_model.rs`
3. provider auto-switch/account-pool-aware resolution
4. session runtime state for pool cooldown and model-sub calibration

These pieces depend on each other.

### Stage 3: preserve memory continuity semantics

Reapply in this order:

1. core memory/context/Entire logic
2. `MemoryLink` in protocol
3. app-server mapping
4. any TUI/client rendering that depends on it

### Stage 4: re-evaluate extra collab wire metadata last

Fields like:

- `agent_type`
- `model_provider_id`
- `model_source`
- `model_source_detail`

should be kept only if they still have clear downstream consumers after the
upstream merge. They are useful, but they are not as foundational as
account-pool-aware routing or memory continuity.

## Current Judgment

The correct architectural split is:

- **Upstream-owned baseline**
  - guardian approvals
  - approvals reviewer semantics
  - MCP elicitation app-server contract
- **Local differentiated workflow layer**
  - `model_sub`
  - provider/account-pool-aware child routing
  - `model_sub_vouch`
  - memory continuity / `MemoryLink`

That split should drive the real merge work from here.
