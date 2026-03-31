# Smart Access Runtime Integration Design

**Date:** 2026-03-31

> **Status (2026-03-31): Implemented on `main`.**
> The runtime-integration line described here landed across:
> `34d1ec3196`, `28c95fafe0`, `12de9aea64`, and `e185d62ed7`.
> Treat this file as rationale for the landed architecture, not as a pending
> design proposal.

**Goal:** Reuse the valuable runtime/control-plane work from the unfinished Smart Access `phase2b` worktree without reviving the deleted Smart Access mode stack on `main`.

## Context

The previously merged `probe/upstream-merge-9dbe09834` work is already finished on `main` as `491ebcc125`. The remaining unfinished security-related work lives only as uncommitted state in the old Smart Access worktrees, especially `feature-smart-access-phase2b-control-plane`.

That worktree contains real progress:

- a lease-based runtime abstraction
- scoped permit install/revoke flows
- runtime health checks
- structured runtime mismatch and policy-drift reasons
- TUI warnings for runtime recovery, drift, mismatch, and fallback-to-human

But `main` deliberately removed the old Smart Access / endpoint-security product model on 2026-03-24:

- `a4fb653f67` `revert: remove smart access mode`
- `17922658f3` `chore: remove legacy endpoint security and freeze flows`

So the continuation problem is not "how to merge the old branch." It is "which parts of `phase2b` should be transplanted into the current approval architecture."

## Post-Landing Residue Audit

A follow-up audit on 2026-03-31 confirmed that
`feature-smart-access-phase2b-control-plane` no longer carries unique commits beyond
`main`; the remaining delta is entirely uncommitted worktree state.

That residue falls into three buckets:

1. `security_runtime/{bootstrap,hosted,remote,legacy}`:
   real runtime bootstrap and backend experiments that were not part of the
   landed `approval_runtime` scope on `main`, which currently uses the smaller
   in-memory runtime client by default.
2. `smart_access.rs`, `security_host`, `es_daemon`, and related session plumbing:
   broad changes tied to the deleted Smart Access product surface and legacy
   endpoint-security control flow.
3. Legacy TUI rendering work under `codex-rs/tui`:
   warning/snapshot changes aimed at the pre-merge UI path, whereas the landed
   runtime warning rendering now lives in `codex-rs/tui_app_server`.

The practical conclusion is that there is no safe "merge the residue" path left.
Any future continuation should selectively extract backend/bootstrap ideas into
`approval_runtime`, not replay the old Smart Access stack or its stale UI hooks.

## Constraints

- Do not restore the deleted `smart_access.rs` / `security_mode` stack as a top-level product mode.
- Do not reintroduce the old session-local endpoint-security bridge and log-scraping behavior.
- Keep the current `main` approval chain intact:
  - `guardian` for automatic approval review
  - `exec_policy` for rule-driven approval requirements
  - tool runtimes for execution
  - existing `Warning` / execution events for UI surfaces
- Land the continuation in current `main` architecture, not by replaying a stale branch diff from `f8fafb5bea`.
- Scope the first deliverable to destructive effect closure only. Do not pull read, network, or MCP enforcement into the initial slice.

## Recommended Approach

Add a small internal runtime companion layer to `main` that supplies dynamic enforcement signals to the existing approval flow.

This layer should not be a revived Smart Access mode. It should be an internal runtime client that:

- acquires and manages `session lease` and `child lease` state
- installs and revokes narrow, TTL-bound permits for destructive operations
- records action-scope metadata around execution
- checks runtime health before execution
- collects runtime events after execution
- classifies runtime outcomes into typed decisions that current `main` can surface and enforce

Static approval continues to flow through the existing code:

- `guardian` still answers "should this be auto-approved?"
- `exec_policy` still answers "does policy require approval or rejection?"
- the tool runtime still executes the command or patch

The new runtime companion only closes the gap between approved intent and observed enforcement.

## Architecture

### New internal module

Introduce a new internal module in `codex-rs/core` dedicated to runtime closure. The important design choice is to use a new module name and a smaller scope, instead of reviving the deleted `security_runtime` / `smart_access` top-level stack.

Responsibilities:

- own the client used to talk to the external/local runtime control plane
- model runtime health and action outcomes with typed enums
- provide helpers to open/close action scopes
- hide lease and permit lifecycle details from `guardian` and the tool runtimes

### Current-`main` integration points

The runtime companion plugs into current `main` at four points:

1. Session lifecycle in `codex.rs`
2. Subagent lifecycle in delegate / multi-agent flows
3. Destructive tool runtimes before and after execution
4. Existing warning surfaces in TUI/app-server rendering

That means the feature is additive to `main`. It does not replace guardian review, exec-policy evaluation, or tool orchestration.

## Execution Flow

### `exec_command` / unified exec

1. `exec_policy` decides whether the action is allowed, rejected, or requires approval.
2. If the turn routes approvals through `guardian`, guardian review completes first.
3. Before spawning the command, the runtime companion performs a preflight:
   - read runtime health
   - confirm the current `session lease`
   - install a permit if the predicted effect requires one
   - open an action scope
4. Execution proceeds through the existing runtime path.
5. After the command completes, the runtime companion:
   - closes the action scope
   - collects runtime events tied to the action
   - maps them into typed runtime decisions
6. The command output remains the command output. Runtime status is surfaced separately through warnings and execution-adjacent events.

### `apply_patch`

`apply_patch` uses the same runtime companion flow, but predicted effects come from the patch summary rather than a shell command.

Only destructive effects should request runtime permits in the first iteration, such as:

- protected delete
- moving files out of protected locations

Routine file edits should continue without permit overhead.

### Subagents

- the parent session acquires a `session lease`
- each subagent derives a `child lease`
- child leases are valid only for that child's turn/action scope
- revoking or losing the parent lease invalidates all descendants

This reuses the strongest idea from `phase2b` without restoring the old Smart Access session controller.

## Runtime Decisions and Failure Semantics

Approval failure and runtime-closure failure must remain separate.

### Approval-layer failures

These keep current `main` behavior:

- `exec_policy` rejection
- approval-policy rejection
- guardian denial

They reject before execution starts and do not install runtime permits.

### Runtime recovery / degraded-health cases

If the runtime is unhealthy or needs recovery but there is no confirmed policy conflict:

- downgrade to a more conservative path
- emit a structured warning
- stop relying on automatic runtime-backed approval until health recovers

This is a product degradation, not proof the requested action itself was malicious.

### Runtime mismatch / policy drift

These fail closed.

- `runtime_mismatch` means predicted permitted effects did not match observed enforcement effects
- `policy_drift` means the capability/policy information used before execution no longer matches what enforcement used during execution

On either outcome:

- automatic approval for comparable follow-up actions is paused
- the agent must not silently retry through workaround behavior
- the user sees a direct explanation that the security closure failed, not a vague command-error message

### User-visible surfaces

Reuse current `main` surfaces:

- `GuardianAssessment` continues to explain approval decisions
- `Warning` carries runtime recovery, mismatch, drift, and fallback-to-human messages
- normal exec/apply-patch begin/end events continue to describe the operation itself

No new standalone Smart Access mode UI is required in the first slice.

## Scope of the First Deliverable

Include:

- runtime-health preflight
- session/child lease lifecycle
- permit/action-scope support for destructive effects
- structured runtime decisions:
  - `runtime_recovery`
  - `runtime_mismatch`
  - `policy_drift`
  - `fallback_to_human`
- warning rendering in the current TUI/app-server path

Do not include:

- restored Smart Access mode selection or product surfaces
- the deleted legacy endpoint-security log bridge
- read gating
- network gating
- MCP enforcement
- the old `phase2a` legacy override bridge

## Alternatives Considered

### 1. Re-merge the old Smart Access stack

Rejected. It directly conflicts with the explicit removal on `main` and would reintroduce modules and product concepts that trunk deliberately deleted.

### 2. Telemetry-only continuation

Not recommended as the main path. It is the safest slice for rollout, but by itself it does not recover the permit / lease / action-scope closure that made `phase2b` worth preserving.

### 3. Runtime companion inside current `main`

Recommended. It preserves the strongest `phase2b` ideas while keeping `main`'s current approval architecture intact.

## Testing Strategy

1. Add unit tests for the new runtime companion module:
   - health mapping
   - runtime decision classification
   - lease lifecycle
   - permit install/revoke bookkeeping
2. Add core integration tests around destructive execution flows:
   - approval succeeds, runtime healthy
   - approval succeeds, runtime falls back to human
   - approval succeeds, runtime mismatch fails closed
   - approval succeeds, policy drift fails closed
3. Add TUI/app-server snapshot coverage showing runtime warnings render through the existing history/status surfaces.
4. Keep host-specific endpoint-security verification as smoke coverage, not a hard daily CI dependency.

## Rollout Plan

### Phase 1: Visibility only

- add runtime health checks
- collect runtime decisions
- render warnings

### Phase 2: Destructive permit closure

- install permits for destructive exec/apply-patch actions
- wire action scopes and post-execution collection

### Phase 3: Subagent lease closure

- derive child leases
- revoke descendants with parent invalidation

This staged rollout keeps the initial change small enough to integrate into current `main` without restoring the old Smart Access product model.

## Non-Goals

- bringing back the deleted Smart Access mode
- restoring the removed endpoint-security log-bridge architecture
- reviving `phase2a` override-file semantics
- expanding the first slice into general read/network/MCP enforcement
