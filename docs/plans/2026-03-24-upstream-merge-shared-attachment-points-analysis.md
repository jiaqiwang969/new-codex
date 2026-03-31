# Shared Attachment Points Merge Analysis

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Status (2026-03-31): Historical merge analysis.**
> The upstream merge has since landed on `main`.
> Keep this as a map of where merge pain concentrated, but validate any file
> ownership assumptions against the current tree before acting on it.

> **Goal of this note:** identify the shared files where the preserved local
> features actually attach to upstream-owned architecture, so future merge work
> can resolve conflicts by file responsibility instead of by vague feature
> labels.

## Bottom Line

The hardest future merge work is not in the local-only modules.

It is in the shared files where local account-pool, provider-family routing,
`model_sub`, memory continuity, and TUI controls hook into upstream session,
protocol, and collaboration code.

Those attachment points are now clearer:

1. `config/mod.rs`, `codex.rs`, and `client.rs` are the real provider-routing
   core.
2. `protocol.rs` and `app-server-protocol/v2.rs` are the memory/collab wire
   core.
3. `chatwidget.rs` and `app.rs` are the TUI reattachment sink for local
   `model_sub` / team-profile / Entire controls.
4. `agent/role.rs` and `memories/phase2.rs` are smaller but important glue
   files that reveal the intended semantics.

## Fresh upstream drift after the previous local baseline

Refreshing `upstream/main` added a new round of churn on top of the older
`047ea642d` baseline.

The most relevant upstream changes for the shared attachment map are:

- `504aeb0e0` moving config/session/TUI cwd state to `AbsolutePathBuf`
- `9dbe09834` extracting core skills loading into a dedicated crate

This matters because both changes land exactly in shared files where local
provider routing and TUI customization already attach:

- `config/mod.rs`
- `codex.rs`
- `agent/role.rs`
- `memories/phase2.rs`
- `tui/src/app.rs`
- `tui/src/chatwidget.rs`

Interpretation:

- the shared-file map remains correct
- the hottest current upstream motion is still on the attachment points, not in
  the preserved local-only modules

## Shared-File Findings

### 1. `codex-rs/core/src/config/mod.rs`

This is the densest shared hotspot.

What local adds here:

- `config-pool.toml` / `auth-pool.json` support
- overlay of `account_pool` onto built-in providers
- `model_sub` and `model_sub_responses` config fields
- `model_sub_responses` validation against Responses compatibility
- `user_configured_provider` capture before auto-switching
- provider-family auto-switching that preserves custom providers for
  Anthropic/Gemini/Gemma/Grok/antigravity families

What upstream is doing in the same area:

- config loading
- built-in provider loading
- profile layering
- constraints / startup warning handling
- general provider selection baseline

Why it conflicts:

- local provider semantics are injected directly into the main config
  constructor
- upstream continues to evolve this constructor for unrelated config work
- losing this logic would keep the knobs while silently dropping the routing
  behavior underneath

Merge rule:

- preserve local pool overlay and provider-family auto-switch semantics
- do not preserve the exact hunk layout
- keep upstream config-loading shape where possible, then reattach the local
  routing semantics to it

### 2. `codex-rs/core/src/codex.rs`

This is where runtime provider behavior is made real.

What local adds here:

- `TurnContext::with_model()` recomputes provider via
  `utility_model::provider_for_model_slug()`
- first-account selection from `account_pool` when a model switch implies a new
  provider family
- runtime session configuration stores both `provider_id` and provider value
- `SessionConfiguration::apply()` auto-switches providers when model families
  change and restores the user-configured provider when they switch back
- provider switch labels with account/base-url hints
- account-pool normalization, cooldown helpers, and human-readable active-key
  labels

Direct upstream contrast rechecked:

- upstream `TurnContext::with_model()` keeps `self.provider.clone()`
- upstream `SessionConfiguration::apply()` does not perform this provider
  restore/auto-switch workflow and does not return a provider-switch label

Why it conflicts:

- upstream collaboration/session lifecycle code is concentrated here
- local provider/account semantics are mixed into the same state transitions
- this file also carries other local runtime features such as Entire and memory

Merge rule:

- preserve the runtime semantics of provider switching and restoration
- do not insist on the current session-configuration field layout
- reattach local provider/account behavior after upstream session lifecycle
  changes are accepted

### 3. `codex-rs/core/src/client.rs`

This file is the transport boundary.

What local adds here:

- `WireApi::Gemini` and `WireApi::Anthropic` streaming branches
- provider-specific request shaping and SSE parsing through the local Gemini and
  Anthropic adapter modules
- provider-specific auth lookup using `env_key`, auth cache, and custom
  provider headers
- Gemini/Anthropic-specific reasoning/image/tool handling

What upstream is optimizing for in the same file:

- Responses transport
- auth recovery / retry handling
- sticky headers / turn-state plumbing
- default OpenAI-compatible request path

Why it conflicts:

- upstream owns the core request/retry/auth architecture
- local adds new transport families directly inside the same dispatch path

Merge rule:

- keep upstream Responses/auth-recovery architecture as the base
- preserve local Gemini/Anthropic transport semantics as extra transport
  branches
- keep custom endpoint and `env_key` behavior aligned with account-pool/config-pool

### 4. `codex-rs/core/src/agent/role.rs`

This is a smaller file, but it exposes local intent clearly.

What local adds here:

- default/explorer role descriptions that explicitly assume `model_sub`
  inheritance
- extra built-in Claude roles
- role tags used by local multi-agent UX
- awaiter role restored in the built-in set

What upstream is doing in the same area:

- maintaining the built-in role catalog
- describing collaboration-role expectations

Why it conflicts:

- the local role catalog assumes the `model_sub` workflow exists
- upstream role descriptions do not currently anchor on `model_sub`

Interpretation:

- this file is not the source of provider-family logic
- it is evidence that the local multi-agent UX was designed around `model_sub`
  as a first-class default

Merge rule:

- preserve the `model_sub` inheritance semantics if the feature survives
- reassess whether every extra built-in role is still worth carrying as-is

### 5. `codex-rs/core/src/memories/phase2.rs`

This is a narrow but important glue point.

What local adds here:

- phase-2 memory consolidation resolves provider from the target model via
  `utility_model::provider_for_model_slug()`
- tests assert that GPT phase-2 work falls back to OpenAI and Claude phase-2
  work stays on Anthropic

What upstream is doing in the same area:

- memory phase-2 consolidation workflow
- default model selection for consolidation

Why it conflicts:

- without this glue, the local branch could keep provider-family routing for
  user turns but silently lose it for memory consolidation workers

Merge rule:

- preserve the phase-2 provider-resolution semantics
- treat this as a downstream consumer of Block 2 and Block 3, not as an
  independent architecture block

### 6. `codex-rs/protocol/src/protocol.rs`

This is the shared wire contract inside the core/protocol boundary.

What local adds here:

- `MemoryLink`
- memory metadata on turn start/complete and collab lifecycle events
- `CollabAgentModelSource` and `CollabAgentModelSourceDetail`
- optional routing metadata such as `agent_type`, `model_provider_id`,
  `model_source`, and `model_source_detail`

Direct upstream contrast rechecked:

- upstream collab spawn-end payload still centers on required `model` and
  `reasoning_effort`
- upstream does not have `MemoryLink` in these collab/turn payloads

Why it conflicts:

- this is a shared contract, not just local runtime state
- upstream collaboration payloads continue to evolve
- some local routing metadata is still only partially populated

Merge rule:

- preserve `MemoryLink` semantics
- be skeptical about preserving the exact current placement of every optional
  routing field
- favor upstream event-shape conventions unless a local field is proven useful

### 7. `codex-rs/app-server-protocol/src/protocol/v2.rs`

This is the externalized mirror of the protocol decision above.

What local adds here:

- `model_sub` and `model_sub_responses` in app-server config/profile payloads
- `MemoryLink` on thread/turn/tool/collab surfaces
- optional `agent_type`, `model`, and `model_provider_id` on collab agent
  states

Why it conflicts:

- upstream owns app-server v2 contract evolution
- local additions here are meaningful, but they increase external compatibility
  obligations

Merge rule:

- keep the config surface needed for `model_sub`
- keep `MemoryLink` only where downstream clients genuinely consume it
- do not freeze the current optional metadata layout just because it already
  exists

### 8. `codex-rs/tui/src/chatwidget.rs` and `codex-rs/tui/src/app.rs`

These files are the UI convergence sink.

What local adds here:

- `model_sub` selection and persistence
- `model_sub_responses` selection and persistence
- team-profile selection
- team-profile/model-sub vouch workflows
- Entire summary model picker

Direct upstream contrast rechecked:

- re-scanning `upstream/main` `tui/src/app.rs` and `tui/src/chatwidget.rs`
  found no `model_sub`, `model_sub_responses`, `team_profile`, or
  `PersistModelEntireSelection` hooks

Interpretation:

- these are real local UX layers, not merge residue
- but they are attached to two of the highest-churn upstream TUI files

Merge rule:

- keep the capability
- do not keep the current placement as sacred
- reattach these controls only after the core provider/memory semantics are
  stable again

## Practical Merge Use

This shared-file map suggests a cleaner execution order:

1. stabilize `config/mod.rs`, `codex.rs`, and `client.rs` first
2. then re-stabilize `memories/phase2.rs` on top of that provider baseline
3. then decide how much of `protocol.rs` / `v2.rs` memory and routing metadata
   is worth preserving
4. only after that, reattach `agent/role.rs` and TUI workflow surfaces

That is a better shape than trying to "merge model_sub" as if it were one
isolated patch.
