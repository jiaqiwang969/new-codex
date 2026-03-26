# Model Sub UX Merge Analysis

**Date:** 2026-03-26
**Scope:** Block 3 from `docs/plans/2026-03-24-upstream-merge-execution-playbook.md`

## Summary

`model_sub` and `model_sub_responses` are real cross-layer routing semantics in
the current branch:

- core config loading keeps them alive across base config plus named profiles
- utility-model routing uses them to choose providers and Responses fallbacks
- child-agent role docs and defaults assume `model_sub` inheritance
- app-server config/profile payloads expose them on the wire

By contrast, `team_profile`, `team_profile_vouch`, and `model_sub_vouch` are
not shared routing contracts. They are local TUI UX layered on top of the
`model_sub` baseline:

- `team_profile` is a preset picker that writes several model fields together
- `team_profile_vouch` and `model_sub_vouch` are local JSON scoreboards under
  `~/.codex/memories`
- they influence TUI recommendations and status displays, not core routing
  correctness

That means Block 3 should be split in two layers during the upstream merge:

1. preserve the core `model_sub` routing semantics
2. reattach the TUI preset/vouch UX only after the core path is stable

## What Must Survive In Core

### 1. Config and profile loading

Current config reload logic still treats `model_sub` and
`model_sub_responses` as first-class fields:

- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/config/types.rs`

The important behavior is:

- named profile overrides can provide `model_sub`
- named profile overrides can provide `model_sub_responses`
- config reload normalizes these through `resolve_utility_model_overrides`

This is the actual persistence boundary that must survive merge conflicts.

### 2. Provider-aware utility routing

`codex-rs/core/src/utility_model.rs` is the center of gravity for the real
feature:

- `resolve_utility_model_overrides` validates and normalizes the two config
  fields
- `provider_for_model_slug` maps utility models onto the correct provider
  family
- `responses_utility_model_slug` keeps Responses-only internal work on an
  OpenAI-compatible fallback
- `client_and_model_for_slug` previews the chosen provider through the current
  account-pool logic

This is where Block 2 and Block 3 actually touch. If this layer is right, the
local account-pool/custom-endpoint requirement survives.

### 3. Collaboration defaults

Built-in child-agent role descriptions still assume `model_sub` is the default
child model when configured:

- `codex-rs/core/src/agent/role.rs`

This matters because it shows the feature is not just TUI chrome; it affects
how the collaboration model is described and expected to behave.

### 4. Shared config wire

The app-server config/profile contract carries these fields directly:

- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/tests/suite/v2/config_rpc.rs`

What survives here is simple:

- `Config.model_sub`
- `Config.model_sub_responses`
- `ProfileV2.model_sub`
- `ProfileV2.model_sub_responses`

There is no equivalent shared-wire concept for team-profile presets or vouch
ledgers.

## What The TUI Adds On Top

### 1. `team_profile` is a preset layer, not a routing primitive

`codex-rs/tui/src/team_profile.rs` defines four static presets. Each preset is
just a tuple of:

- leader model
- `model_sub`
- `model_sub_responses`
- memory phase 1 model
- memory phase 2 model

`codex-rs/tui/src/app.rs` persists a chosen preset by writing those values into
config through `ConfigEditsBuilder`.

That means `team_profile` does not own routing itself. It is a convenience
layer that batches multiple existing config writes.

### 2. `team_profile_vouch` and `model_sub_vouch` are local scoreboards

The two ledgers live entirely in TUI code:

- `codex-rs/tui/src/team_profile_vouch.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`

They persist to:

- `~/.codex/memories/team_profile_vouch.json`
- `~/.codex/memories/model_sub_vouch.json`

They are updated from TUI-only app events handled in:

- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app.rs`

They are not part of core session state, protocol state, or app-server state.

### 3. Chat commands and popups are the main consumers

`codex-rs/tui/src/chatwidget.rs` uses this layer for:

- `/team-profile`
- `/model-sub auto`
- `/team-vouch`
- team-profile selection popup

The operational shape is:

- load local vouch ledger
- compute recommended profile or recommended utility model
- persist the selected config values
- show TUI feedback messages

If this UX disappears temporarily, core `model_sub` routing still exists as
long as config + utility routing survive.

### 4. Status card is the secondary consumer

`codex-rs/tui/src/status/card.rs` adds extra explanatory lines:

- current matched team profile
- team profile vouch summary
- auto-recommended team profile
- auto-selected utility model from `model_sub_vouch`

This is useful local observability, but it is not part of the routing contract.

## Non-TUI Entanglement Check

The key audit question was whether these UX concepts leak outside TUI.

### What does leak outside TUI

- `model_sub`
- `model_sub_responses`

These appear in core config, utility routing, and app-server config/profile
types.

### What does not leak outside TUI

- `team_profile`
- `team_profile_vouch`
- `model_sub_vouch`

Current code search shows:

- no non-TUI references to `team_profile`
- no non-TUI references to `team_profile_vouch`
- no non-TUI references to `model_sub_vouch`
- no mirrored implementation in `codex-rs/tui_app_server`

This is the strongest reason to treat them as follow-on UX rather than core
merge blockers.

## Merge Decision

### Must preserve

- `model_sub`
- `model_sub_responses`
- provider-aware utility routing
- child-role inheritance/default semantics
- app-server config/profile wire fields

### Preserve if cheap after core is stable

- `team_profile` preset picker
- `team_profile_vouch`
- `model_sub_vouch`
- status-card rendering for these local signals

### Do not let this layer drive shared-wire conflicts

During merge conflict resolution:

- do not expand protocol/app-server surface for team-profile or vouch concepts
- do not block the merge on reattaching every TUI helper immediately
- do not resurrect the deleted `codex-rs/core/src/model_sub_vouch.rs` path from
  older branch history

## Conflict Guidance

If conflicts land in `codex-rs/tui/src/app.rs` or
`codex-rs/tui/src/chatwidget.rs`, the safe order is:

1. keep upstream TUI shell/lifecycle structure
2. reattach config persistence for `model_sub` and `model_sub_responses`
3. reattach `team_profile` preset selection if still desired
4. reattach vouch recording/recommendation only if the surrounding TUI shape is
   still coherent

The important idea is that Block 3 is successful even if the first merged
revision ships with core `model_sub` intact but some local preset/vouch UX
temporarily absent.

## Verification Notes

Targeted tests already passed for this layer:

- `cargo test -p codex-tui team_profile -- --nocapture`
- `cargo test -p codex-tui model_sub_vouch -- --nocapture`

Those results are enough for the current audit phase because this turn only
changes documentation, not runtime behavior.
