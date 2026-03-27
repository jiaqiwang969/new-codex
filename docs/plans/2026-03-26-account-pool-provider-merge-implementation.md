# Account Pool / Provider Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve local account-pool semantics while reshaping the implementation around smaller hook points that are easier to keep aligned with `upstream/main`.

**Architecture:** Keep `config-pool.toml`, `auth-pool.json`, custom endpoints, turn-scoped pool failover, and `env_key`-based auth lookup. Limit the local behavior to dedicated provider-pool modules and a small set of call sites in config loading, turn creation, and auth setup. Do not expand account-pool concepts into app-server or new TUI UX.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just fix`, argument-comment lint

---

### Task 1: Re-anchor provider schema and config overlay around logical providers

**Files:**
- Modify: `codex-rs/core/src/model_provider_info.rs`
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/provider_pool.rs`
- Modify: `codex-rs/core/src/model_provider_info_tests.rs`
- Verify/update if needed: `codex-rs/core/config.schema.json`
- Verify/update if needed: `docs/config.md`
- Verify/update if needed: `codex-rs/config-examples/config-pool.toml`

**Step 1: Write or tighten config-level regression coverage**

Keep or add tests proving:

- logical providers with `account_pool` do not select `base_url` / `env_key` on config load
- invalid pool entries are ignored without mutating the logical provider
- `config-pool.toml` can overlay a built-in provider such as `anthropic`
- built-in provider patching keeps the canonical family identity while allowing
  custom endpoints

Target tests:

- `account_pool_primary_entry_is_not_selected_on_config_load`
- `account_pool_ignores_invalid_entries_without_selecting_first_valid_entry`
- `config_pool_overlays_anthropic_account_pool_on_builtin_provider`
- `config_pool_accepts_account_pool_without_top_level_provider_row`

**Step 2: Run the targeted config tests first**

Run: `cargo test -p codex-core account_pool_primary_entry_is_not_selected_on_config_load config_pool_overlays_anthropic_account_pool_on_builtin_provider -- --exact`
Expected: PASS or fail only on the config/provider shape being refactored.

**Step 3: Adjust provider schema minimally**

Keep only the provider fields/helpers needed for runtime account resolution:

```rust
pub account_pool: Vec<ModelProviderAccount>;

pub fn current_account(&self) -> Option<ModelProviderAccount> { ... }
pub fn with_account(&self, account: &ModelProviderAccount) -> Self { ... }
```

Do not move runtime failover policy into this file.

**Step 4: Keep config loading logical, not concrete**

Ensure `Config::load...`:

- builds the upstream-style built-in provider map
- applies local overrides for known built-in families
- overlays `config-pool.toml`
- preserves the logical provider without selecting the first account

Keep the pool-specific mutation isolated to helper calls such as:

```rust
if let Some(pool_config) = load_pool_config(&codex_home) {
    overlay_pool_config(&mut model_providers, pool_config);
}
```

**Step 5: Re-run the full config-focused provider tests**

Run: `cargo test -p codex-core account_pool_ -- --nocapture`
Expected: PASS

### Task 2: Reconnect runtime provider selection without leaking pool state into config

**Files:**
- Modify: `codex-rs/core/src/provider_routing.rs`
- Modify: `codex-rs/core/src/provider_pool_runtime.rs`
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/utility_model.rs`
- Modify tests in:
  - `codex-rs/core/src/provider_pool_runtime.rs`
  - `codex-rs/core/src/utility_model.rs`
  - `codex-rs/core/src/codex_tests.rs`

**Step 1: Lock down runtime semantics with targeted tests**

Keep or add tests proving:

- cooled accounts are skipped at turn start
- all-cooling state forces a fresh probe from key 1
- utility-model preview uses the first configured pool account
- logical provider identity still resolves correctly when an active account is attached

**Step 2: Run the runtime-focused tests before implementation**

Run: `cargo test -p codex-core provider_pool_runtime -- --nocapture`
Expected: PASS or fail only where the runtime hook points are still being moved.

**Step 3: Keep account normalization and identity in dedicated helpers**

Keep these responsibilities in `provider_routing.rs`:

- normalize pool entries in config order
- compare providers while ignoring the currently attached account
- preview the first account for utility-model client setup

Do not duplicate that logic in `codex.rs` or `utility_model.rs`.

**Step 4: Resolve concrete accounts only at turn time**

Keep session runtime as the owner of cooldown state:

```rust
pub(crate) fn resolve_turn_provider(
    &mut self,
    provider_id: &str,
    provider: &ModelProviderInfo,
    now: Instant,
) -> ResolvedTurnProvider
```

`codex.rs` should call that helper when creating a turn context or an internal
utility/Entire turn. Config should remain unchanged.

**Step 5: Re-run runtime and utility-model tests**

Run: `cargo test -p codex-core provider_pool_runtime utility_model -- --nocapture`
Expected: PASS

### Task 3: Reconnect auth lookup and request-failure failover around the selected account

**Files:**
- Modify: `codex-rs/core/src/provider_auth.rs`
- Modify: `codex-rs/core/src/api_bridge.rs`
- Modify: `codex-rs/core/src/provider_pool_failover.rs`
- Modify: `codex-rs/core/src/client.rs`
- Modify: `codex-rs/core/src/client_tests.rs`
- Modify: `codex-rs/login/src/auth/manager.rs`
- Modify: `codex-rs/login/src/auth/auth_tests.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/provider_pool_failover_tests.rs`

**Step 1: Preserve auth lookup by selected `env_key`**

Keep auth lookup order explicit and shared:

```rust
resolve_provider_api_key(provider, auth.as_ref())
```

Expected behavior:

- first, the currently selected provider/account `env_key`
- second, stored auth entries by `env_key`
- third, generic fallback API key when the provider path permits it

**Step 2: Keep failover policy local to retry handling**

`provider_pool_failover.rs` should decide whether to:

- stay on current account
- switch within the current round
- restart from the first account

`codex.rs` should own the stateful side effects:

- mark current account cooling
- ask for the next account
- create the next turn context

**Step 3: Run auth and failover tests before implementation**

Run: `cargo test -p codex-core resolve_provider_api_key_uses_selected_account_env_key_after_provider_switch provider_pool_failover -- --nocapture`
Expected: PASS or fail only in the auth/failover seam being refactored.

**Step 4: Keep `client.rs` as a consumer, not the owner, of pool semantics**

`client.rs` should receive an already-resolved provider and should not decide:

- which pool account is active
- whether to cool an account
- whether to advance to the next account

It may still call the centralized auth helper during request setup.

**Step 5: Re-run auth/failover tests**

Run: `cargo test -p codex-core provider_pool_failover resolve_provider_api_key_ -- --nocapture`
Expected: PASS

### Task 4: Documentation and local verification

**Files:**
- Modify: `docs/config.md`
- Modify: `codex-rs/config-examples/README.md`
- Modify: `codex-rs/README.md`
- Modify if needed: `docs/plans/2026-03-24-upstream-merge-account-pool-analysis.md`
- Modify if needed: `codex-rs/core/config.schema.json`

**Step 1: Update docs to match the retained semantics**

Docs should describe:

- `config.toml` as logical provider config
- `config-pool.toml` as operational pool config
- `auth-pool.json` as stored multi-key auth material
- turn-scoped runtime failover rather than persistent provider mutation

Docs should not claim any new app-server or TUI account-pool UX.

**Step 2: Run required formatting and linting**

Run: `cd codex-rs && just fmt`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-core`
Expected: PASS

Run: `cd codex-rs && cargo test -p codex-core`
Expected: PASS

Run: `cd /Users/jqwang/.config/superpowers/worktrees/new-codex/probe-upstream-9dbe09834 && PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`
Expected: PASS

**Step 3: Optional full-workspace verification**

Only after user approval:

Run: `cd codex-rs && cargo test`
Expected: PASS

**Step 4: Commit**

```bash
git add docs/plans/2026-03-26-account-pool-provider-merge-design.md \
        docs/plans/2026-03-26-account-pool-provider-merge-implementation.md \
        codex-rs/core \
        codex-rs/login \
        codex-rs/config-examples \
        docs/config.md \
        codex-rs/README.md
git commit -m "refactor: isolate account pool provider hooks"
```
