# Upstream Sync With Custom Feature Preservation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Status (2026-03-31): Historical execution plan.**
> The final `main` branch deliberately did **not** execute every task below.
> In particular, do not read this plan as an instruction to restore
> `git-graph`, `session bar`, `ralph-loop`, agent worktrees, or the removed
> Smart Access / `endpoint-sec` / `/freeze` line.
> For the current runtime direction, see `docs/local-customizations.md`.

**Goal:** Rebuild our customized Codex branch on top of the latest `upstream/main` while preserving and improving key custom capabilities through explicit, feature-family-based migration.

**Architecture:** Start from a fresh branch based on the newest `openai/codex` upstream, then reintroduce custom behavior in isolated slices ordered by dependency and risk. Prefer upstream-native implementations where they already cover our needs, and reattach custom behavior through narrower extension points to reduce future merge cost.

**Tech Stack:** Git, Rust workspace in `codex-rs`, Cargo/Just, insta snapshots for TUI, app-server schema generation, existing custom modules and commits in this repository.

---

### Task 1: Capture branch topology and migration inventory

**Files:**
- Create: `docs/plans/2026-03-06-upstream-sync-migration-inventory.md`
- Modify: `docs/plans/2026-03-06-upstream-sync-implementation.md`

**Step 1: Record the branch topology**

Run: `git fetch upstream main --tags --prune`
Expected: latest `upstream/main` fetched successfully

Run: `git rev-list --left-right --count feature/upstream-sync...upstream/main`
Expected: two counts showing custom divergence and upstream divergence

Run: `git merge-base feature/upstream-sync upstream/main`
Expected: a single commit SHA identifying the fork base

**Step 2: Enumerate custom commits after the upstream baseline**

Run: `git log --reverse --oneline upstream/main..feature/upstream-sync > /tmp/upstream-sync-custom-commits.txt`
Expected: ordered list of custom commits written to a temporary file

**Step 3: Group custom commits by feature family**

Write `docs/plans/2026-03-06-upstream-sync-migration-inventory.md` with sections for:
- account-pool/provider routing
- memory
- collaboration/subagents
- Entire/rewind/hooks/sandbox integrations
- git-graph/TUI/UI
- build/test/schema/doc updates

For each section, include:
- exact commit SHAs
- exact touched files
- keep/upstream/merge recommendation
- risks and validation notes

**Step 4: Review upstream overlap before any code move**

Run: `rg -n "account pool|account_pool|memory|spawn_agent|entire|rewind|git-graph" codex-rs`
Expected: a rough cross-reference of current feature locations

**Step 5: Commit**

```bash
git add docs/plans/2026-03-06-upstream-sync-migration-inventory.md docs/plans/2026-03-06-upstream-sync-implementation.md
git commit -m "docs: map upstream sync migration inventory"
```


### Task 2: Create a fresh integration branch from latest upstream

**Files:**
- No code changes yet; branch/worktree operation only

**Step 1: Create isolated branch/worktree**

Run: `git worktree add ../new-codex-upstream-sync-b2 -b integration/upstream-main-b2 upstream/main`
Expected: a new worktree based directly on `upstream/main`

**Step 2: Verify branch identity**

Run: `git -C ../new-codex-upstream-sync-b2 status --short --branch`
Expected: clean working tree on `integration/upstream-main-b2`

**Step 3: Verify baseline crate metadata**

Run: `git -C ../new-codex-upstream-sync-b2 log --oneline -n 3`
Expected: latest upstream commits visible

**Step 4: Commit**

No commit expected for this task.


### Task 3: Establish a clean upstream baseline in the new worktree

**Files:**
- Potentially modify upstream-tracked generated files only if baseline regeneration is needed

**Step 1: Validate Rust toolchain and helpers**

Run in `../new-codex-upstream-sync-b2/codex-rs`: `just --version && cargo --version && rg --version`
Expected: required tooling is available

**Step 2: Run baseline formatter check**

Run in `../new-codex-upstream-sync-b2/codex-rs`: `just fmt`
Expected: no unexpected formatting drift

**Step 3: Run targeted baseline tests for likely impact crates**

Run in `../new-codex-upstream-sync-b2/codex-rs`: `cargo test -p codex-core`
Expected: upstream baseline for `codex-core` passes or any failures are documented as pre-existing

Run in `../new-codex-upstream-sync-b2/codex-rs`: `cargo test -p codex-tui`
Expected: upstream baseline for `codex-tui` passes or any failures are documented as pre-existing

Run in `../new-codex-upstream-sync-b2/codex-rs`: `cargo test -p codex-app-server`
Expected: upstream baseline for `codex-app-server` passes or any failures are documented as pre-existing

**Step 4: Record baseline findings**

Add a section to `docs/plans/2026-03-06-upstream-sync-migration-inventory.md` capturing any upstream-red failures before custom migration begins.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-06-upstream-sync-migration-inventory.md
git commit -m "docs: record upstream baseline validation"
```


### Task 4: Migrate account-pool and provider-routing behavior first

**Files:**
- Inspect and likely modify: `codex-rs/core/src/codex.rs`
- Inspect and likely modify: `codex-rs/core/src/exec.rs`
- Inspect and likely modify: `codex-rs/core/src/agent/role.rs`
- Inspect and likely modify: `codex-rs/core/src/plugins/manager.rs`
- Inspect and likely modify: `codex-rs/core/src/plugins/mod.rs`
- Inspect and likely modify: any provider/account-selection files discovered in inventory
- Test: relevant `codex-core` tests or newly added targeted tests in the matching crate

**Step 1: Write failing tests for expected routing semantics**

Add or update targeted tests covering:
- provider ID resolution
- account-pool selection behavior
- failover rotation behavior
- interaction with upstream defaults

Prefer asserting full objects or structured outputs, not individual fields.

**Step 2: Run targeted tests to verify failures**

Run in `codex-rs`: `cargo test -p codex-core <targeted-test-filter>`
Expected: failure demonstrating missing or regressed custom behavior

**Step 3: Implement minimal migration on top of upstream**

Port only the logic still needed after comparing against latest upstream code.

Constraints:
- avoid single-use helper methods
- collapse nested `if` statements where possible
- inline `format!` args where possible
- prefer exhaustive `match` handling

**Step 4: Run crate formatter and scoped lint fix**

Run in `codex-rs`: `just fix -p codex-core`
Expected: clippy fixes applied for `codex-core`

Run in `codex-rs`: `just fmt`
Expected: formatting complete

**Step 5: Re-run targeted tests**

Run in `codex-rs`: `cargo test -p codex-core <targeted-test-filter>`
Expected: pass

**Step 6: Commit**

```bash
git add codex-rs/core
git commit -m "feat(core): restore account-pool routing on upstream baseline"
```


### Task 5: Migrate memory propagation and app-server exposure

**Files:**
- Inspect and likely modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Inspect and likely modify: `codex-rs/app-server/src/codex_message_processor.rs`
- Inspect and likely modify: `codex-rs/app-server/tests/suite/v2/mcp_server_elicitation.rs`
- Inspect and likely modify: relevant `core` memory types and event wiring files found in inventory
- Potentially modify: `codex-rs/app-server/README.md`
- Potentially regenerate: app-server schema outputs if protocol shapes change

**Step 1: Write failing tests for memory link semantics**

Cover:
- propagation of `scope_version`
- propagation of `scope_kind`
- propagation of `summary_sha256`
- propagation of `binding_key`
- absence behavior when memory is empty

Prefer asserting complete `MemoryLink`-like values.

**Step 2: Run targeted tests to verify failures**

Run in `codex-rs`: `cargo test -p codex-app-server mcp_server_elicitation`
Expected: failures or missing assertions capturing the gap

**Step 3: Implement upstream-compatible memory wiring**

Port only the delta still needed, keeping wire formats aligned with current upstream behavior.

If API/protocol shapes change:
- update `codex-rs/app-server/README.md`
- run `just write-app-server-schema`
- run `cargo test -p codex-app-server-protocol`

**Step 4: Run scoped lint fix and formatter**

Run in `codex-rs`: `just fix -p codex-app-server`
Run in `codex-rs`: `just fmt`

**Step 5: Re-run targeted tests**

Run in `codex-rs`: `cargo test -p codex-app-server`
Expected: pass for affected app-server behavior

**Step 6: Commit**

```bash
git add codex-rs/app-server codex-rs/app-server-protocol codex-rs/core docs
 git commit -m "feat(app-server): restore memory link integration"
```


### Task 6: Migrate collaboration and multi-agent custom behavior

**Files:**
- Inspect and likely modify: `codex-rs/core/src/tools/orchestrator.rs`
- Inspect and likely modify: collaboration/subagent-related files discovered in inventory
- Inspect and likely modify: `codex-rs/tui/src/app.rs`
- Inspect and likely modify: any app-server protocol or event rendering files affected
- Test: matching `codex-core`, `codex-tui`, or `codex-app-server` tests

**Step 1: Write failing tests around custom collaboration semantics**

Cover whichever custom behaviors are still unique after upstream comparison, such as:
- preserved collaboration mode semantics
- agent spawning constraints
- role/rendering details
- review or awaiter behavior if customized

**Step 2: Run targeted tests to verify failures**

Run the smallest relevant test set in the affected crate.

**Step 3: Implement minimum viable migration**

Port only behavior not already present upstream.

**Step 4: If UI text/rendering changes, update snapshots**

Run in `codex-rs`: `cargo test -p codex-tui`
Run in `codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Review `.snap.new` files directly.
If all expected, run in `codex-rs`: `cargo insta accept -p codex-tui`

**Step 5: Run scoped lint fix and formatter**

Run in `codex-rs`: `just fix -p codex-core`
Run in `codex-rs`: `just fix -p codex-tui`
Run in `codex-rs`: `just fmt`

**Step 6: Re-run targeted tests**

Run the affected crate tests again and confirm pass.

**Step 7: Commit**

```bash
git add codex-rs/core codex-rs/tui codex-rs/app-server
 git commit -m "feat: restore collaboration customizations"
```


### Task 7: Migrate Entire hook, rewind, and sandbox-adjacent flows

**Files:**
- Inspect and likely modify sandbox-related files in `codex-rs/core/src/tools/`
- Inspect and likely modify: `codex-rs/core/src/seatbelt_permissions.rs`
- Inspect and likely modify: skill-loading and permission files in `codex-rs/core/src/skills/`
- Inspect and likely modify TUI command/rendering files for related commands
- Inspect and likely modify docs for these workflows if user-facing behavior changes

**Step 1: Write failing tests around the still-required Entire behaviors**

Cover only stable, reproducible semantics, such as:
- hook permission handling
- rewind-related request shaping
- custom sandbox argument generation
- filtering/suppression logic for background summaries or banners, if still applicable

Do not add tests that rely on external services when a unit/integration seam already exists.

**Step 2: Run targeted tests to verify failures**

Run the narrowest possible crate tests matching the modified behavior.

**Step 3: Port behavior using upstream extension points**

Constraints:
- do not modify code tied to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR`
- do not modify code tied to `CODEX_SANDBOX_ENV_VAR`
- prefer config gates or isolated permission/profile merging over broad behavior rewrites

**Step 4: Run scoped lint fix and formatter**

Run in `codex-rs`: `just fix -p codex-core`
Run in `codex-rs`: `just fmt`

**Step 5: Re-run targeted tests**

Run the affected crate tests again and confirm pass.

**Step 6: Commit**

```bash
git add codex-rs/core codex-rs/tui docs
 git commit -m "feat(core): restore Entire-integrated workflows"
```


### Task 8: Reintroduce git-graph and related UI integration

**Files:**
- Inspect: `codex-rs/git-graph/**`
- Inspect and likely modify: TUI files that preload, render, or interact with git graph data
- Potentially modify: root `README.md` or crate docs if user-visible behavior changes
- Test: `codex-rs/tui/src/chatwidget/tests.rs` or other TUI snapshot/unit tests as needed

**Step 1: Confirm whether `git-graph` crate already builds unchanged on latest upstream**

Run in `codex-rs`: `cargo test -p git-graph`
Expected: pass or clear baseline errors documented

**Step 2: Write failing tests for any custom TUI/UI integration**

Cover visible behavior such as:
- preload timing
- graph visibility
- command routing
- text rendering or layout expectations

Use snapshot coverage for intentional UI output changes.

**Step 3: Implement minimal reattachment**

Prefer preserving the crate as-is unless upstream API changes force a narrower adapter.

**Step 4: Update snapshots if needed**

Run in `codex-rs`: `cargo test -p codex-tui`
Run in `codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Review and accept intentional snapshot changes.

**Step 5: Run scoped lint fix and formatter**

Run in `codex-rs`: `just fix -p codex-tui`
Run in `codex-rs`: `just fmt`

**Step 6: Re-run targeted tests**

Run in `codex-rs`: `cargo test -p codex-tui`
Expected: pass for affected UI coverage

**Step 7: Commit**

```bash
git add codex-rs/git-graph codex-rs/tui README.md
 git commit -m "feat(tui): restore git-graph integration"
```


### Task 9: Refresh generated artifacts, docs, and crate-level validation

**Files:**
- Potentially modify: `codex-rs/core/config.schema.json`
- Potentially modify: `docs/**`
- Potentially modify: crate READMEs or protocol/app-server docs if behavior changed

**Step 1: Refresh config schema if config types changed**

Run in `codex-rs`: `just write-config-schema`
Expected: `codex-rs/core/config.schema.json` updated only if required

**Step 2: Refresh app-server schema if API shapes changed**

Run in `codex-rs`: `just write-app-server-schema`
Expected: schema artifacts updated only if required

**Step 3: Run targeted crate tests for all changed projects**

At minimum, run:
- `cargo test -p codex-core`
- `cargo test -p codex-app-server`
- `cargo test -p codex-tui`
- `cargo test -p git-graph`

If protocol changed, also run:
- `cargo test -p codex-app-server-protocol`

**Step 4: Run scoped `just fix` for changed projects and final formatter**

Run the minimal necessary set of:
- `just fix -p codex-core`
- `just fix -p codex-app-server`
- `just fix -p codex-tui`
- `just fmt`

Do not re-run tests after `fix` or `fmt` unless a later review requires it.

**Step 5: Commit**

```bash
git add codex-rs docs README.md
 git commit -m "chore: refresh generated artifacts after upstream sync"
```


### Task 10: Final verification and integration decision

**Files:**
- No required file changes; verification and handoff task

**Step 1: Summarize preserved custom capabilities**

Prepare a short checklist mapping each of the original requested capabilities to its final implementation state:
- preserved as-is
- preserved and modernized
- replaced by better upstream behavior

**Step 2: Run branch diff review**

Run: `git diff --stat upstream/main...HEAD`
Expected: a reviewable summary focused on intentional custom deltas

Run: `git log --oneline --decorate upstream/main..HEAD`
Expected: clear commit series for migrated features

**Step 3: Optionally run full suite only with explicit confirmation**

Run in `codex-rs`: `cargo test` or `just test`
Expected: full workspace signal, if requested

**Step 4: Commit**

No new commit expected unless final review yields a fix.
