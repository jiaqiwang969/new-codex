# Codex Upstream Sync Design

**Date:** 2026-03-06

**Objective:** Rebase our customized Codex codebase onto the latest `openai/codex` while preserving and improving our differentiated capabilities, especially `git-graph`, Entire hook/rewind workflows, account-pool/provider routing, collaboration/multi-agent extensions, and memory-related behavior.

## Current State

- Working branch: `feature/upstream-sync`
- Existing custom work checkpointed at commit `38c01b6a4`
- Relative to latest `upstream/main`, current branch is substantially diverged in both directions
- Prior large-scale upstream merge already exists in history, which increases the risk of carrying forward stale compatibility shims if we continue merging in place

## Recommended Strategy

Adopt **Option B**: create a fresh integration branch from the latest `upstream/main`, then selectively reintroduce our custom capabilities by feature family.

This is preferred over continuing to merge on the existing branch because it allows us to:

- evaluate each customization against newer upstream implementations
- avoid dragging old conflict resolutions and workaround code into the new baseline
- reduce future sync cost by reconnecting custom behavior at cleaner extension points

## Architectural Principles

### 1. Upstream-first baseline

The new branch should compile and run as close to upstream as possible before custom features are reapplied.

### 2. Feature-family migration

Custom changes should be grouped and migrated by capability domain, not by historical commit order.

Primary domains:

- account-pool and provider-selection logic
- memory propagation and app-server exposure
- collaboration/subagent orchestration behavior
- Entire hook / rewind / sandbox-adjacent custom workflows
- git-graph integration and TUI/UI exposure

### 3. Best-of-both selection

For every overlapping behavior, choose one of three outcomes explicitly:

- keep upstream as-is
- keep our custom implementation
- merge both into a new, cleaner implementation

### 4. Minimize future merge pain

Where our behavior is still valuable but currently invasive, reattach it through narrower interfaces, config gates, or well-scoped modules instead of broad patch-style rewrites.

## Delivery Phases

### Phase 1: Inventory and mapping

- map our post-fork/custom commits to feature families
- identify which custom commits are already superseded by upstream
- identify high-risk files in `core`, `tui`, `app-server`, `protocol`, and sandbox-related code

### Phase 2: Fresh upstream integration branch

- branch from latest `upstream/main`
- verify clean baseline build/test status for affected Rust crates
- establish a migration worksheet of files, commits, and decisions

### Phase 3: Reintroduce custom capabilities in priority order

Recommended order:

1. account-pool / provider routing
2. memory behavior and app-server wiring
3. collaboration / multi-agent customizations
4. Entire hook / rewind / sandbox-integrated workflows
5. git-graph and user-facing TUI/UI integration

### Phase 4: Refinement

- remove obsolete compatibility code
- align docs and generated schema artifacts
- update snapshots when UI changes are intentional

### Phase 5: Validation and handoff

- run targeted crate tests first
- run formatter and scoped lint fixes
- only run full-suite validation after the targeted path is stable

## Risks and Mitigations

### Risk: hidden semantic regressions from old custom patches

Mitigation: migrate by feature family and test behavior, not by blind cherry-pick.

### Risk: upstream now contains overlapping features

Mitigation: explicitly compare semantics before porting any custom code.

### Risk: TUI/app-server protocol drift

Mitigation: validate UI snapshots and protocol/schema outputs as each family lands.

### Risk: sandbox/Entire behavior is fragile

Mitigation: isolate these changes late in the sequence, after core orchestration APIs are stable.

## Success Criteria

The effort is successful when:

- the new integration branch is based directly on the latest `upstream/main`
- our differentiated capabilities still exist or are improved
- obsolete patches and one-off migration artifacts are removed
- affected crates pass targeted tests and required generated artifacts are refreshed
- future upstream sync work is structurally simpler than it is today
