# Obsolete: Smart Approvals Core Trunk Import Design

**Date:** 2026-03-16

> **Status:** Historical and obsolete for current merge work. This design assumed the local `endpoint-sec` / `request_security_override` safety line would remain in place. The active direction is to remove that local flow and follow upstream `approval_policy` + `approvals_reviewer` / guardian semantics instead.

**Objective:** Import the official Smart Approvals core runtime trunk into this customized Codex repository while preserving the existing local `endpoint-sec` / Endpoint Security enforcement model and avoiding the unstable app-server notification surface for now.

## Current State

- The local repository already has a custom approval and Endpoint Security flow, including `request_security_override` and macOS Endpoint Security integration.
- The official upstream Smart Approvals work landed in commit `bc24017d64829d0b97b8bc6ed529a389e1e8bc1b` and spans `core`, `protocol`, `app-server`, and `tui`.
- The upstream runtime model does **not** add a new `approval_policy`.
- Instead, upstream separates:
  - `approval_policy`: when approval is required
  - `approvals_reviewer`: who reviews approval requests (`user` or `guardian_subagent`)
- The local worktree is already dirty in a few files, notably:
  - `codex-rs/core/src/codex.rs`
  - `codex-rs/core/src/config/schema.rs`
  - `codex-rs/config-examples/config-pool.toml`

## Chosen Approach

Adopt a **manual, targeted port** of the upstream Smart Approvals core trunk instead of a direct cherry-pick of `bc24017d6`.

This is preferred because:

- the upstream commit is broad and mixes stable runtime semantics with unstable app-server surface area
- the local tree already contains custom approval and ES-related behavior that should not be overwritten wholesale
- targeted porting allows us to preserve the existing Endpoint Security safety boundary while importing upstream guardian semantics with much lower merge risk

## Scope

### In Scope

- Import the stable runtime concept of `approvals_reviewer = "user" | "guardian_subagent"`
- Import the upstream guardian review engine and prompt
- Route reviewable approval requests through guardian review in core for:
  - shell / unified-exec
  - apply_patch
  - managed network approvals
  - MCP approvals
  - delegated/subagent approval forwarding
- Add minimal TUI support needed to expose and understand the reviewer choice in local usage
- Preserve upstream semantics that `smart_approvals` is a rollout/UI gate rather than a replacement for `approval_policy`
- Preserve the deprecated `guardian_approval` alias migration so old configs retain the prior guardian-enabled behavior

### Explicitly Out of Scope

- app-server unstable guardian lifecycle notifications
- persistence of guardian review state onto app-server thread items
- integration of Smart Approvals into `endpoint-sec` or `request_security_override`
- introduction of a new `Security Host`
- redesigning `/approvals` into a new product mode such as `smart access`
- removal or relaxation of current OS-level Endpoint Security enforcement

## Architectural Model

### 1. Config and Runtime Switches

The import should preserve the upstream split between approval policy and approval reviewer:

- `approval_policy` remains the gate that decides whether an action is reviewable
- `approvals_reviewer` decides whether the reviewer is the human user or the guardian subagent
- `smart_approvals` remains a rollout/UI feature gate and does not silently rewrite runtime reviewer selection on config load
- the deprecated `guardian_approval = true` alias should backfill `approvals_reviewer = "guardian_subagent"` only when the reviewer is not already set in that scope

This preserves upstream semantics and avoids overloading the existing approval presets with a third conceptual axis too early.

### 2. Guardian Engine

The upstream guardian is a narrowly scoped reviewer subagent, not a replacement for OS enforcement.

The imported guardian should keep these invariants:

- read-only execution context
- `approval_policy = never`
- fail closed on timeout, malformed output, or execution error
- structured output including `risk_level`, `risk_score`, and `rationale`
- only auto-approve when the guardian score is below the upstream threshold

In this round, the guardian is only a runtime reviewer. It does not get permission to bypass the current Endpoint Security model.

### 3. Approval Routing Integration

Approval routing should be updated at the points where Codex currently decides whether to pause for manual approval:

- command execution paths
- patch application paths
- managed-network approval paths
- MCP approval paths
- delegated/subagent approval forwarding

The key change is not to alter whether something is reviewable. The key change is to alter **who receives the review request** once the action is reviewable.

### 4. TUI Exposure

This round only needs minimum TUI exposure:

- let the user see or select the reviewer through the existing permissions/approvals flow
- surface enough reviewer status that guardian-reviewed actions are understandable in-session
- keep the UI aligned with the existing `/approvals` model instead of inventing a new permission mode

The unstable app-server notification model is intentionally excluded, so TUI work should rely on the local runtime/event path only.

### 5. Endpoint Security Boundary

`endpoint-sec` remains the OS-level enforcement layer.

After this import, the boundary should remain:

- Smart Approvals decides whether a reviewable request appears safe enough to proceed automatically
- Endpoint Security still blocks operations that require OS-level permission and still enforces hard deny/allow policy at the host boundary

This means a guardian approval is not equivalent to a root-level allow. It is only a runtime-level decision that an action may proceed to the next enforcement stage.

## Import Sequence

The port should happen in this order:

1. Add protocol/config/runtime reviewer types and config migration behavior
2. Import the guardian engine and its tests
3. Wire guardian review into core approval routing
4. Add minimal TUI exposure and snapshot coverage
5. Regenerate schema artifacts and run targeted validation

This order keeps the import debuggable. If routing work becomes messy, the repository still benefits from the completed lower-level reviewer model and guardian engine.

## Validation Strategy

### Layer 1: Config and Type Validation

- verify config loading for explicit `approvals_reviewer`
- verify deprecated `guardian_approval` migration
- verify reviewer overrides survive turn/session update paths
- regenerate `codex-rs/core/config.schema.json`

### Layer 2: Guardian Behavior Validation

- low-risk review should auto-approve
- high-risk review should deny or fall back conservatively
- malformed guardian output should fail closed
- timeout or reviewer execution errors should fail closed

### Layer 3: Approval Routing Validation

- shell and unified-exec requests use guardian when configured
- apply_patch uses guardian when configured
- managed-network and MCP reviews flow into guardian instead of forcing a user-only path
- delegated/subagent approval forwarding still works when the reviewer is guardian

### Layer 4: UI Validation

- `/approvals` or equivalent permissions surface shows the reviewer choice correctly
- guardian-reviewed status renders clearly in the active thread
- updated snapshots reflect the intended UI change

## Risks and Mitigations

### Risk: conflict with local approval customizations

Mitigation: avoid wholesale file replacement, especially in `codex-rs/core/src/codex.rs` and config loading code; splice in only the upstream reviewer logic.

### Risk: confusing Smart Approvals with OS-level approval

Mitigation: preserve the current Endpoint Security enforcement path unchanged in this round and document the boundary clearly in code comments and user-facing wording.

### Risk: importing unstable surface area by accident

Mitigation: intentionally exclude `app-server` notification changes and thread-item persistence from this round.

### Risk: TUI drift without full upstream app-server support

Mitigation: keep TUI support minimal and scoped to local runtime-visible reviewer state.

## Success Criteria

This import is successful when:

- the repo supports `approvals_reviewer = user | guardian_subagent`
- guardian review can be enabled without introducing a new approval policy
- core approval paths route to guardian review when configured
- the current Endpoint Security / `request_security_override` path still behaves exactly as before
- targeted `codex-core` and `codex-tui` tests pass
- no unstable app-server Smart Approvals surface is introduced in this round
