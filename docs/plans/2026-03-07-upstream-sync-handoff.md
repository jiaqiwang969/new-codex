# Upstream Sync Handoff Summary

**Date:** 2026-03-07
**Branch:** `integration/upstream-main-b2`
**Base:** latest `upstream/main` integrated on this branch
**Purpose:** final cleanup handoff for the "align to upstream while preserving custom capabilities" workstream

## Outcome Summary

This branch preserves the core custom capabilities requested for the upstream sync effort while reattaching them in a way that better matches the current upstream architecture.

The main strategy outcomes are:

- keep upstream behavior where it is already better or now canonical
- reintroduce custom capabilities where they remain differentiated
- narrow the integration surface so future upstream syncs are cheaper

## Capability Checklist

### Preserved and modernized

#### 1. Account pool / provider routing

**State:** preserved and modernized

**What landed**
- provider account pool primitives
- provider account rotation within pools
- auth key fallback by provider env key
- isolated persistence overlay for account-pool state

**Key files**
- `codex-rs/core/src/auth.rs`
- `codex-rs/core/src/auth/storage.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/config/mod.rs`

#### 2. Memory

**State:** preserved and modernized

**What landed**
- thread memory storage primitives
- auto-write thread memories after turns
- MemoryLink propagation on turn start and completion
- memory continuity exposure in multi-agent tools
- app-server v2 turn/history exposure for memory continuity
- Entire summary generation gated behind memory configuration

**Key files**
- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/state/src/runtime/memories.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server-protocol/src/protocol/thread_history.rs`

#### 3. Collaboration / subagents / model_sub

**State:** preserved and modernized

**What landed**
- spawn-agent model override
- spawn metadata propagation through collaboration events
- model_sub calibration tool
- model_sub vouch record tools
- auto-select model_sub for spawned agents
- tool-side memory continuity returned from collaboration APIs
- collaboration mode persistence after tool side effects

**Key files**
- `codex-rs/core/src/tools/handlers/multi_agents.rs`
- `codex-rs/core/src/model_sub_vouch.rs`
- `codex-rs/core/src/tools/spec.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`

#### 4. Entire / hooks / session context / side effects

**State:** preserved and modernized

**What landed**
- Entire session context reinjection
- historical Entire summary backfill
- summary generation for mutating notify turns
- turn metadata refresh after tool side effects
- file-system side effect tracking as first-class events
- summary generation gated by memories config
- hook-side summary utilities for Entire workflows

**Key files**
- `codex-rs/core/src/entire_integration.rs`
- `codex-rs/core/src/entire_summary_generator.rs`
- `codex-rs/core/src/git_side_effects.rs`
- `codex-rs/core/src/turn_metadata.rs`
- `codex-rs/hooks/src/entire_summary.rs`

#### 5. Git graph

**State:** preserved and modernized

**What landed**
- restored standalone `codex-git-graph` crate
- restored TUI integration via `/graph`
- non-git-repo fallback and snapshot coverage

**Why this is a deliberate modernization**
- the legacy `Ctrl+G` binding was not reclaimed because upstream now uses it for external editor flow
- `/graph` preserves the capability while keeping current upstream keyboard behavior intact

**Key files**
- `codex-rs/git-graph/src/lib.rs`
- `codex-rs/tui/src/git_graph_widget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/app.rs`

### Replaced by better upstream behavior

#### 6. Legacy git-graph hotkey behavior

**State:** replaced by better upstream behavior

**Decision**
- keep upstream `Ctrl+G` external-editor behavior
- expose git graph through `/graph` instead of restoring the old hotkey conflict

**Reasoning**
- this reduces merge friction and preserves both capabilities instead of forcing a regression in upstream UX

## Generated Artifacts and Docs Refreshed

The branch now includes refreshed generated artifacts and docs for the migrated protocol surface:

- `codex-rs/core/config.schema.json`
- `codex-rs/app-server-protocol/schema/json/**`
- `codex-rs/app-server-protocol/schema/typescript/**`
- `codex-rs/app-server/README.md`

Notable protocol/documentation updates include:

- `turn.memory` now appears in app-server v2 turn payloads
- `MemoryLink` is exported in generated TypeScript bindings
- `FileSystemMutatedEvent` is exported in generated TypeScript bindings

## Verification Summary

### Targeted checks run successfully

Validated in `codex-rs` on this branch:

- `cargo test -p codex-app-server`
- `cargo test -p codex-tui`
- `cargo test -p codex-git-graph`
- `cargo test -p codex-app-server-protocol`
- `cargo test -p codex-hooks`
- `cargo test -p codex-state`
- `cargo test -p codex-protocol`
- `cargo test -p codex-exec`
- `just write-config-schema`
- `just write-app-server-schema`
- `just fix -p codex-core`
- `just fix -p codex-app-server`
- `just fix -p codex-app-server-protocol`
- `just fix -p codex-tui`
- `just fmt`

### Current known verification gap

`cargo test -p codex-core` is not fully green on this machine when run as the full crate suite.

Observed failure classes:

- environment-specific `zsh` path expectation mismatch
- environment-specific seatbelt filesystem permission behavior
- low `ulimit -n` causing `Too many open files` during large parallel test execution

Concrete reproduction note from this branch:

- on this machine, `cargo test -p codex-core` fails quickly under the default soft limit of `256` file descriptors with `os error 24`
- the failing examples included `agent::role::tests::apply_role_preserves_unspecified_keys`, `agent::control::tests::spawn_agent_fork_injects_output_for_parent_spawn_call`, and several `tools::handlers::multi_agents::*` tests
- raising the soft limit first, for example `ulimit -n 4096 && cargo test -p codex-core`, removes the immediate `EMFILE` failures and allows the suite to keep progressing
- after raising `nofile`, `codex-core` still contains legitimately long-running integration tests; the harness reports some tests as "running for over 60 seconds", but that is distinct from the earlier file-descriptor failure mode

Important triage note:

- the migration-related tests that touch the changed collaboration, config-loading, and js-repl paths were rerun individually and passed
- this suggests the remaining red signal is dominated by local environment/test-harness sensitivity rather than by the migrated feature slices themselves

### Full workspace test suite

Per repo guidance, a full workspace `cargo test` / `just test` has **not** been run in this cleanup pass because that requires explicit confirmation.

## Review-Friendly Commit Sequence

Current branch tip includes the following high-value migration commits near the top of the stack:

- `acfc08780` — `chore(protocol): refresh app-server schema artifacts`
- `6bd9344a3` — `feat(tui): restore git-graph overlay via slash command`
- `d2e96f7d0` — `feat(rust): restore standalone git-graph crate`
- `761294d98` — `feat(core): gate Entire summaries behind memories config`
- `a67a29ae2` — `feat(core): backfill historical Entire summaries`
- `38ce0e02f` — `feat(core): generate Entire summaries for mutating notify turns`
- `9c5daee26` — `feat(core): refresh turn metadata after tool side effects`
- `e232add41` — `feat: add isolated account-pool persistence overlay`
- `19794cd88` — `feat: track tool side effects and persist collab mode`

## Final Content-Alignment Audit

A post-integration audit was run against current `upstream/main` to answer a simple question: are there still upstream code changes missing from this branch, or is the remaining divergence mostly commit-history shape plus intentional customizations?

### Audit method

The audit compared this branch to `upstream/main` in two ways:

- `git log --cherry-pick --right-only --no-merges integration/upstream-main-b2...upstream/main` to enumerate upstream commits not present by hash
- for each resulting upstream commit, compare the current branch against `upstream/main` for that commit's touched files

### Audit result

The result was that every currently outstanding upstream commit in the graph is already **effectively matched by content** on this branch. In other words:

- the branch still reports `ahead 70, behind 38` because the commit graph differs
- but the 38 "behind" commits do not currently represent missing upstream code/content
- the remaining diff against `upstream/main` is dominated by intentional custom capabilities and their generated/schema artifacts

### Practical interpretation

At this point, the mainline sync goal has effectively been achieved at the content level:

- upstream behavior has been reabsorbed where it is now canonical or better
- custom differentiators remain intentionally present where they add product value
- future sync work should focus on keeping these custom seams narrow rather than chasing the current `behind` count as if it were missing functionality

## Recommended Next Actions

1. Review the intentional custom delta against `upstream/main`, focusing on account pool, memory, collaboration/model_sub, Entire integration, and git-graph.
2. If desired, run the full workspace suite in an environment with a higher file-descriptor limit and stable seatbelt/zsh expectations.
3. Decide whether to keep this branch as the integration branch of record, open a PR from it, or later rebase/merge only for history hygiene.

## Bottom Line

This branch is in a clean, reviewable handoff state.

The requested custom capabilities are largely preserved, but they have been reattached with an upstream-first bias rather than carried forward as a wholesale fork replay.
