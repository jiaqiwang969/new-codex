# API Pool Runtime Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make account-pool selection a per-turn, session-local runtime decision so every new or resumed session starts from `POOL_1`, later turns retry `POOL_1` after cooldown, and switched accounts are never persisted as the new default.

**Architecture:** Keep `Config` and `SessionConfiguration` on logical provider definitions, add session-local pool cooldown state in `SessionState`, resolve the concrete account only when constructing a `TurnContext`, and keep same-turn failover confined to the active turn. Update docs/examples to remove the old "persist last good key" behavior and explain the new cooldown/reprobe flow.

**Tech Stack:** Rust (`codex-core`), Tokio session state, ratatui-facing background events, markdown docs.

---

### Task 1: Lock the new config semantics with tests

**Files:**
- Modify: `core/src/config/mod.rs`

**Step 1: Write failing tests**

Add tests that prove:

- loading a provider with `account_pool` does not eagerly overwrite
  `config.model_provider.base_url/env_key` with the first pool entry
- `config-pool.toml` can define only `[[model_providers.<id>.account_pool]]`
  without a `[model_providers.<id>]` top-level row
- overlaying `config-pool.toml` keeps the pool entries but does not treat
  top-level `base_url/env_key` as the active account in pool mode

**Step 2: Run targeted test failure**

Run:

```bash
cargo test -p codex-core account_pool_
```

Expected: existing tests fail because they still assert eager primary-account
selection.

**Step 3: Implement minimal config changes**

Remove the eager primary-account mutation path from config load and update the
pool overlay logic so pool mode depends on `account_pool`, not on a top-level
"selected account".

**Step 4: Re-run targeted config tests**

Run:

```bash
cargo test -p codex-core account_pool_
```

Expected: config-pool semantics tests pass.

### Task 2: Add session-local pool runtime state

**Files:**
- Modify: `core/src/state/session.rs`
- Modify: `core/src/codex.rs`

**Step 1: Write failing unit tests**

Add tests that describe:

- a fresh session has no pool cooldown state
- failed accounts are marked cooling until `now + 1 minute`
- a later turn chooses the first non-cooled account in config order
- when every account is cooled, selection reports that a forced reprobe is
  required

**Step 2: Run targeted failure**

Run:

```bash
cargo test -p codex-core pool_runtime
```

Expected: new runtime-state helpers do not exist yet.

**Step 3: Implement runtime state**

Add a session-local pool runtime structure keyed by `provider_id` with helpers
for:

- normalizing account order
- checking cooldown expiry against `Instant`
- recording failure cooldowns
- resolving the initial account for a new turn
- producing message metadata for "skipped cooled" and "forced reprobe" cases

**Step 4: Re-run targeted runtime tests**

Run:

```bash
cargo test -p codex-core pool_runtime
```

Expected: runtime state tests pass.

### Task 3: Resolve the active pool account only when building a turn

**Files:**
- Modify: `core/src/codex.rs`
- Modify: `core/src/utility_model.rs`
- Modify: `core/src/tools/handlers/multi_agents.rs`

**Step 1: Write failing tests**

Add tests for:

- new turn creation starts from account 1
- resume creates a turn that starts from account 1 again
- utility-model/provider-family routing returns the logical provider, not a
  preselected pool account
- model-family auto-switch restore paths compare logical providers correctly

**Step 2: Run targeted failure**

Run:

```bash
cargo test -p codex-core provider_for_model_slug
```

Expected: tests still observe eager primary-account selection.

**Step 3: Implement logical-provider flow**

Change turn creation to:

- keep `SessionConfiguration.provider` logical
- resolve a concrete pool account before building `TurnContext`
- update model-family/utility-provider helpers to stop calling the eager
  primary-account selector
- stop copying active turn providers back into session config on resume/spawn

**Step 4: Re-run targeted tests**

Run:

```bash
cargo test -p codex-core provider_for_model_slug
```

Expected: provider routing tests pass with logical-provider semantics.

### Task 4: Keep failover inside the current turn and add user-facing messages

**Files:**
- Modify: `core/src/codex.rs`
- Modify: `tui/src/chatwidget/tests.rs`
- Update snapshots if rendered copy changes

**Step 1: Write failing tests**

Add tests that prove:

- same-turn failure cools the failed account and switches to the next account
  without mutating session configuration
- the next turn retries account 1 after cooldown expiry
- all-cooled start emits a forced-reprobe background event
- background events describe key selection and cooldown behavior clearly

**Step 2: Run targeted failure**

Run:

```bash
cargo test -p codex-core switch_provider_account
cargo test -p codex-tui chatwidget
```

Expected: failover behavior and TUI assertions still reflect persisted active
account switching.

**Step 3: Implement failover rewrite**

Refactor failover so it:

- records cooldown against the failed account
- resolves the next account from the logical provider for the same turn
- rebuilds only the turn-local provider/client
- never calls `persist_provider_account_selection()` in pool mode
- emits background messages for start, skip, switch, and forced reprobe cases

**Step 4: Re-run targeted tests and review snapshots**

Run:

```bash
cargo test -p codex-core switch_provider_account
cargo test -p codex-tui
cargo insta pending-snapshots -p codex-tui
```

Expected: core tests pass and any intended snapshot diffs are ready for review.

### Task 5: Update docs and examples

**Files:**
- Modify: `README.md`
- Modify: `config-examples/config-pool.toml`
- Modify: `docs/plans/2026-03-15-api-pool-redesign-design.md` if implementation details shift

**Step 1: Update docs**

Document that:

- pool order is always preferred from key 1
- cooldown lasts 1 minute
- later turns retry key 1 after cooldown
- resume starts from key 1 again
- all-cooled state triggers a forced reprobe from key 1
- `config-pool.toml` no longer needs a top-level provider row in pool mode

**Step 2: Re-read for consistency**

Check that docs, example config, and runtime messages say the same thing.

**Step 3: Run focused doc-adjacent validation**

Run:

```bash
rg -n "persisted|last good|POOL_3|top-level provider row|config-pool" README.md config-examples/config-pool.toml core/src
```

Expected: stale wording is removed or intentionally retained only in tests.

### Task 6: Format and verify

**Files:**
- Modify: any touched files from Tasks 1-5

**Step 1: Run formatter**

Run:

```bash
just fmt
```

Expected: formatting completes cleanly.

**Step 2: Run crate-specific tests**

Run:

```bash
cargo test -p codex-core
cargo test -p codex-tui
```

Expected: targeted crates pass.

**Step 3: Run scoped Clippy fixups for the changed crate**

Run:

```bash
just fix -p codex-core
```

Expected: lint-driven cleanup finishes without introducing new behavior.

**Step 4: Ask before full workspace validation**

If all crate-level verification passes and changes touched shared core behavior,
ask the user before running:

```bash
cargo test
```

Expected: full-suite approval is requested separately per repo policy.
