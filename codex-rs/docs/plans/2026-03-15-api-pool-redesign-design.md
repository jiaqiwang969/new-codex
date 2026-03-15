# API Pool Runtime Redesign

## Goal

Redesign provider account-pool handling so that pool selection is a per-turn,
session-local runtime decision instead of a persisted mutation of provider
configuration.

This change must apply to every provider that defines `account_pool`, not only
the `codex` provider.

## Problem

The current implementation mixes three different concerns into one
`ModelProviderInfo` value:

1. The logical provider definition selected by config or model-family routing.
2. The currently active `(base_url, env_key)` account inside a pool.
3. The persisted "last good" account written back into `config-pool.toml`.

That design causes behavior that conflicts with the approved requirements:

- Config load eagerly selects the first valid pool account.
- Failover updates the session provider to the switched account.
- Failover persists the switched account back into `config-pool.toml`.
- Resume and later turns inherit the last switched account instead of retrying
  from `POOL_1`.

## Approved Requirements

- If `account_pool` is non-empty, pool order is the source of truth.
- A new session always probes from `POOL_1`.
- A resumed session also probes from `POOL_1`.
- Within one active session, failures move to the next pool account.
- Failed accounts cool down for 10 minutes.
- After cooldown expires, the next turn should retry `POOL_1` first because it
  is preferred and cheaper.
- If every account is still cooling down, do not fail immediately. Force a new
  probe from `POOL_1`, then continue through the pool in order.
- Top-level `base_url` / `env_key` entries in `config-pool.toml` are ignored in
  pool mode and can be removed from examples.
- The TUI/background messages must explain which pool key is being tried,
  skipped, cooled down, or force-probed.

## Design

### 1. Separate logical provider from active account

`Config.model_provider`, `Config.user_configured_provider`, and
`SessionConfiguration.provider` become logical provider definitions. They
describe the provider family, request settings, and `account_pool`, but they do
not represent the active pool account for the current turn.

`TurnContext.provider` remains the resolved provider used to send requests. It
contains a concrete `(base_url, env_key)` pair selected from the pool for that
turn.

### 2. Pool runtime state is session-local only

Add pool runtime state to `SessionState`, keyed by `provider_id`. Each entry
tracks:

- cooldown expiration per normalized `ModelProviderAccount`
- optional metadata needed for user-facing messages on the next turn

This state is never written to config files, rollout files, or persisted session
metadata. Exiting and resuming starts fresh from `POOL_1`.

### 3. Resolve the initial account at turn creation

Every time a turn is created, if the logical provider has a non-empty
`account_pool`, resolve the active account by scanning the normalized pool in
configuration order:

1. Start at account 1.
2. Pick the first account whose cooldown has expired.
3. If all accounts are cooling down, clear the "skip cooled accounts" decision
   for that turn and force a fresh probe starting from account 1.

The turn context keeps the selected concrete provider. Session configuration
keeps only the logical provider.

### 4. Failover stays inside the current turn

When a request fails with an account-switch-eligible error:

1. Mark the failed account as cooling down for 10 minutes in session-local
   runtime state.
2. Select the next account in pool order for the same turn.
3. Rebuild the turn-local provider/client only.
4. Do not mutate session configuration to the switched account.
5. Do not write the switched account into `config-pool.toml`.

This means a successful fallback can continue to serve the current turn, while
the next turn still re-evaluates the pool from account 1.

### 5. Cooldown preference behavior

Cooldown applies per account, not per provider:

- If `POOL_1` fails and `POOL_2` succeeds, the active turn keeps using
  `POOL_2`.
- On the next turn, selection scans from `POOL_1` again.
- If `POOL_1` is still cooling down, it is skipped and the turn starts from the
  first available later account.
- Once 10 minutes have passed, the next turn retries `POOL_1` first.

This preserves preference for the cheapest account without thrashing mid-turn.

### 6. All-cooled forced reprobe

If every account is cooling down when a new turn starts:

- emit a background event that all pool accounts are cooling down
- immediately force a fresh probe from `POOL_1`
- continue through the remaining accounts on failure

This avoids hard-failing just because the cooldown window has not expired yet.

### 7. `config-pool.toml` semantics

When `account_pool` is present:

- only `account_pool` order matters
- top-level `base_url` / `env_key` in `config-pool.toml` are ignored
- examples and docs should remove the redundant top-level row

The parser should continue accepting the file shape where users define only:

```toml
[[model_providers.codex.account_pool]]
base_url = "https://example.com/v1"
env_key = "OPENAI_API_KEY_POOL_1"
```

This keeps config concise and removes the false impression that a persisted
"default active key" still exists.

## Affected Paths

- `core/src/config/mod.rs`
  Remove eager primary-account selection and stop treating pool config top-level
  account fields as active state.
- `core/src/codex.rs`
  Add session-local pool runtime state, resolve per-turn active providers, and
  keep failover state inside the turn.
- `core/src/state/session.rs`
  Store pool cooldown runtime state.
- `core/src/utility_model.rs`
  Return logical providers, not eagerly resolved pool accounts.
- `core/src/tools/handlers/multi_agents.rs`
  Resume/spawn paths must carry logical providers only.
- `README.md` and `config-examples/config-pool.toml`
  Update the documented behavior and example shape.

## User-Facing Messaging

Background events should make these transitions explicit:

- starting a turn with `provider key 1/N`
- skipping cooled accounts and using the next available account
- switching accounts after a failure and showing the cooldown applied
- forcing a fresh probe because all accounts are cooling down
- retrying preferred account 1 after cooldown expiration

The intent is that the user can understand why a turn did not start on key 1 and
when Codex is deliberately re-checking key 1.

## Testing Strategy

Add coverage for:

- config pool parsing without a top-level provider row
- config load no longer selecting the first pool account as active state
- new session starts from account 1
- resumed session starts from account 1
- same-turn failover continues to later accounts
- later turns skip cooled accounts
- cooldown expiry makes later turns retry account 1
- all-cooled start forces a fresh probe from account 1
- utility-model/provider-family routing keeps logical providers instead of active
  switched accounts

## Out of Scope

- Persisting cooldowns across process restarts
- Cost-aware selection beyond strict pool order
- Background health probes outside normal turn creation
