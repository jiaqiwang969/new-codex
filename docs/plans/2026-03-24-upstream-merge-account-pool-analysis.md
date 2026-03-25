# Account Pool / Provider Routing Merge Analysis

**Date:** 2026-03-24
**Branch state:** `17922658f`
**Baseline:** `upstream/main` at `f9545278e`

> **Goal of this note:** explain what the local account-pool line actually
> changed, what problem it was solving, why it conflicts with upstream, and
> what should be preserved during future merge work.

## Bottom Line

This fork's account-pool behavior is not just a config example or a provider
alias. It changes four layers together:

1. provider schema
2. config loading
3. turn-time provider resolution
4. API key lookup / auth fallback

That is why this block keeps colliding with upstream in `config/mod.rs`,
`model_provider_info.rs`, `client.rs`, and `codex.rs`.

## What The Local Branch Adds

### 1. Provider-level account pool schema

Local branch adds:

- `ModelProviderAccount`
- `ModelProviderInfo.account_pool: Vec<ModelProviderAccount>`
- helper methods:
  - `current_account()`
  - `with_account()`

Relevant file:

- `codex-rs/core/src/model_provider_info.rs`

Meaning:

- one logical provider can carry multiple `(base_url, env_key)` accounts
- the logical provider definition stays stable
- the concrete account is chosen later at runtime

### 2. Separate pool config file

Local branch adds:

- `config-pool.toml`
- `auth-pool.json`
- `ConfigPoolToml`
- `load_pool_config(...)`

Relevant files:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/config-examples/config-pool.toml`
- `codex-rs/config-examples/auth-pool.json`

Meaning:

- pool config is intentionally isolated from regular `config.toml`
- provider logic lives in `config.toml`
- operational account rotation lives in `config-pool.toml`

This separation is important for your workflow and should be preserved.

### 3. Built-in provider overlay instead of upstream's insert-only merge

Upstream currently does:

- build built-in providers
- insert user-defined providers only when the key does not already exist

Local branch does:

- build built-in providers
- allow local config to patch or override built-in providers
- preserve canonical names for known provider families
- overlay `config-pool.toml` onto those providers

Relevant file:

- `codex-rs/core/src/config/mod.rs`

Meaning:

- local config can retarget built-in families like `anthropic`, `gemini`,
  `grok`, or `codex` without redefining the whole provider stack from scratch
- pool entries can be applied to known built-in providers directly

### 4. Turn-scoped pool selection and cooldowns

Local branch adds:

- session-local cooldown runtime state per provider/account
- each new turn starts from pool order again
- failed accounts cool down temporarily
- if all accounts are cooling, the next turn forces a fresh probe from key 1
- same-turn retries can switch to the next account

Relevant files:

- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/state/session.rs`

Meaning:

- account selection is not persisted as the new global provider default
- pool choice is a turn/runtime concern, not a config mutation concern

This is the most important semantic difference from naive "just swap base_url"
implementations.

### 5. API key fallback by `env_key`

Local branch extends auth lookup so the runtime can resolve a key by the
provider account's `env_key`.

Relevant files:

- `codex-rs/core/src/client.rs`
- `codex-rs/login/src/auth/manager.rs`

Meaning:

- runtime first checks the environment variable named by the selected account
- then checks `auth.json` provider-specific keys keyed by env var name
- then falls back to the generic API key when appropriate

This is what makes pool entries actually usable without rewriting environment
variables globally for every switch.

## What Problem The Local Author Was Solving

The local branch is optimized for operational multi-provider usage:

- multiple keys/accounts for one logical provider
- custom non-official endpoints
- temporary failover on 400/401/403/429 style issues
- avoiding permanent config drift after a temporary fallback
- keeping user-facing provider choice stable while rotating underlying accounts

In short:

- upstream's mental model is mostly "choose a provider"
- local branch's mental model is "choose a provider family, then let runtime
  choose an account within that family"

## What Upstream Is Optimizing For

Upstream is optimizing for:

- a cleaner, more standardized provider contract
- fewer runtime-specific provider mutations
- simpler built-in provider behavior
- less operational state hidden inside session runtime
- easier long-term maintenance across config, app-server, and clients

That explains why upstream currently does not have:

- `account_pool`
- `config-pool.toml`
- session-local pool cooldown state
- provider override behavior for built-in families at this depth

This is not an oversight. It is a different product priority.

## Why This Area Keeps Conflicting

### Conflict 1: provider schema drift

`ModelProviderInfo` is heavily customized locally:

- more provider families
- more wire APIs
- account-pool runtime helpers

Upstream keeps changing this type too, so conflicts are structural.

### Conflict 2: config loading semantics differ

Upstream:

- built-ins first
- user-defined providers inserted if absent

Local:

- built-ins first
- known built-ins may be patched/overridden
- pool overlay applied after that

This is a fundamental policy difference, not a formatting difference.

### Conflict 3: turn construction is different

Upstream session state has no provider-pool runtime.

Local session state stores:

- cooldowns per logical provider
- turn-time account selection state

So merge conflicts show up not only in config code, but also in runtime/session
state and retry handling.

### Conflict 4: auth lookup is no longer single-key

Upstream auth flow assumes a simpler provider-to-key mapping.

Local flow depends on:

- `env_key` chosen by the active account
- `auth.json` lookup by env var name
- generic fallback behavior

That means provider routing and auth routing are coupled.

## What Must Be Preserved

Based on current user requirements, the following semantics are non-negotiable:

1. Keep `config-pool.toml` as the source of operational account pool data.
2. Keep support for custom endpoints such as `https://code.ppchat.vip`.
3. Keep logical provider identity stable while runtime selects concrete accounts.
4. Keep turn-scoped reset to pool order rather than persisting last-successful
   fallback as the new default.
5. Keep cooldown-based temporary failover.
6. Keep auth lookup by selected account `env_key`.

## What Can Change

These are implementation details and can be reworked during merge:

- exact helper names
- exact struct layout
- exact background event text
- exact config parsing layering, if semantics stay the same
- exact integration point with utility-model selection

## Recommended Merge Strategy

### Preserve the semantics, not the exact old patch shape

Do not try to keep every local diff hunk in:

- `config/mod.rs`
- `model_provider_info.rs`
- `client.rs`
- `codex.rs`

Instead, preserve only these semantic checkpoints:

- provider can define `account_pool`
- pool config can be loaded from `config-pool.toml`
- runtime resolves a concrete account per turn
- retries can move to the next account
- session state tracks cooldowns
- auth can resolve a key for the active account

### Apply it in dependency order

1. provider schema
2. config loader overlay
3. runtime/session cooldown state
4. retry failover
5. auth fallback
6. docs/examples/tests

### Keep this block isolated from approval/security work

This account-pool line should not be coupled to:

- guardian approvals
- Smart Access
- `endpoint-sec`
- `/freeze`

Those were separate lines and mixing them made earlier merges harder.

## Main Risk If We Merge Poorly

The biggest failure mode is not a compile error. It is semantic drift:

- config still loads
- app still starts
- but provider selection silently stops using `config-pool`
- or Anthropic/Gemini/custom codex endpoints fall back to official defaults
- or the runtime persists fallback choice incorrectly
- or auth lookup stops matching the selected account

That would look "mostly fine" until real traffic hits the wrong provider/key.

## Current Judgment

This is a legitimate local asset and worth preserving.

But it should be treated as:

- a focused runtime/provider-routing extension

not as:

- a reason to keep the old security architecture
- a reason to keep the old Smart Access line
- a reason to fork every upstream provider/config decision wholesale

That separation is the key to merging upstream without losing the local account
pool workflow.
