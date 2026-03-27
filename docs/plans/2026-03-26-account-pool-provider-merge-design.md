# Account Pool / Provider Merge Design

**Date:** 2026-03-26
**Scope:** preserve local account-pool semantics while aligning the implementation shape more closely with `upstream/main`

## Goal

Keep the local operational provider-routing behavior that depends on:

- `config-pool.toml`
- `auth-pool.json`
- custom provider endpoints such as `https://code.ppchat.vip`
- turn-scoped account selection with cooldown-based failover
- API-key lookup by the selected account's `env_key`

At the same time, reduce future merge pain by confining the local behavior to a
small set of well-defined hook points instead of carrying a broad forked patch
shape through config, runtime, and auth code.

## Non-Negotiable Semantics

The following behavior must remain true after refactoring:

1. `config-pool.toml` remains the operational source of account-pool data.
2. Built-in provider families such as `anthropic` can still be retargeted to
   local endpoints like `https://code.ppchat.vip`.
3. Config loading preserves a logical provider identity and does not silently
   select the first account in the pool.
4. Each new turn starts from config order again, skipping only accounts still in
   cooldown.
5. A failed account cools down temporarily, and same-turn retries may switch to
   the next account.
6. API key lookup follows the selected account's `env_key`, with fallback to
   stored auth when appropriate.

## Rejected Approaches

### 1. Keep the existing local patch shape intact

This preserves semantics, but it leaves large persistent conflicts in:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/model_provider_info.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/login/src/auth/manager.rs`

That is the easiest way to preserve behavior and the worst way to keep merging
upstream.

### 2. Drop runtime failover and keep only static endpoint overrides

This would be easier to align with upstream, but it would remove the main value
of the local account-pool line: turn-scoped failover and `env_key`-based
account switching. That does not satisfy the requirements.

## Recommended Approach

Preserve semantics, but rewrite them as a narrow local extension layer around an
upstream-shaped baseline.

### Upstream-shaped baseline

Keep these areas as close as possible to upstream:

- built-in provider families and normal provider selection defaults
- regular `config.toml` loading flow
- app-server protocol and TUI surface
- `model_sub` / `model_sub_responses` semantics

### Local extension hooks

Keep local behavior in a small number of dedicated hook points:

- provider schema:
  `codex-rs/core/src/model_provider_info.rs`
- pool config overlay:
  `codex-rs/core/src/provider_pool.rs`
- runtime provider selection:
  `codex-rs/core/src/provider_pool_runtime.rs`
- provider failover policy:
  `codex-rs/core/src/provider_pool_failover.rs`
- provider identity normalization:
  `codex-rs/core/src/provider_routing.rs`
- auth lookup by selected account:
  `codex-rs/core/src/provider_auth.rs`

High-conflict files should call into those hooks instead of owning the full
behavior directly:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/api_bridge.rs`
- `codex-rs/login/src/auth/manager.rs`

## Data Flow

### 1. Config load produces logical providers only

`Config::load...` should:

1. construct upstream-style built-in providers
2. apply local `config.toml` overrides for known built-in families
3. overlay `config-pool.toml` account-pool data
4. keep `base_url` / `env_key` unset on the logical provider when the pool is in
   use

This preserves a stable logical provider identity in config while keeping pool
data attached for later runtime resolution.

### 2. Turn creation resolves a concrete account

At turn start, session runtime chooses a concrete account from the logical
provider's `account_pool`:

- skip accounts still cooling down
- if all are cooling, restart from key 1
- emit background messaging only from the runtime layer

This logic belongs in `provider_pool_runtime` and session state, not in config
loading.

### 3. Client/auth consumes the selected account

By the time request setup reaches `client.rs` / `api_bridge.rs`, the provider
has already been resolved to a concrete account. Auth lookup should then follow
this order:

1. environment variable named by `provider.env_key`
2. stored auth lookup keyed by `env_key`
3. generic API-key fallback where permitted

### 4. Failover stays local to retry handling

Account switching after request failures should stay inside the turn retry path
in `codex.rs`, with failover policy isolated in `provider_pool_failover.rs`.
That keeps future upstream client changes from re-opening the whole patch area.

## Boundary Decisions

### Keep

- `config-pool.toml`
- `auth-pool.json`
- custom base URLs
- logical-provider identity with runtime account resolution
- cooldown-based turn-scoped failover
- `env_key` lookup semantics

### Do not expand

- no new app-server protocol surface for account pools
- no new TUI commands or account-pool UX
- no additional shared-wire concepts for pool state

## Validation Strategy

The refactor is only acceptable if these existing semantic checks remain true:

- config tests prove pool entries do not select a concrete account on load
- config tests prove `config-pool.toml` can overlay a built-in provider
- runtime tests prove cooled accounts are skipped and full cooldown restarts at
  key 1
- auth tests prove the selected account's `env_key` resolves the correct key
- utility-model tests prove preview routing keeps logical provider identity while
  previewing the first configured account

## Implementation Order

1. Rework provider schema and config overlay first.
2. Reconnect turn-scoped runtime resolution second.
3. Reconnect auth lookup and request-failure failover last.

This order keeps the highest-risk auth/runtime entanglement until after the
config semantics are already pinned down by tests.
