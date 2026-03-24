# Machine-Global Smart Access + endpoint-sec Design

**Date:** 2026-03-20

**Objective:** Turn Smart Access and `endpoint-sec` into one coordinated system where Codex predicts and arbitrates risk, while a machine-global `endpoint-sec` runtime enforces real system effects under a shared permit and feedback model.

## Scope

This design intentionally defers the future "residence / household ownership" model.

The immediate target is narrower and more urgent:

- make `endpoint-sec` truly usable in long-term `enable` mode
- eliminate the current Smart Access vs kernel gate mismatch loop
- replace legacy override-first coordination with permit-first coordination
- keep the machine-global root runtime and its GUI while upgrading it into a Smart Access-aware control plane

## Problem

Today there are two mismatched control languages:

- Smart Access reviews tool intent
- `endpoint-sec` enforces kernel-observed effects

That mismatch causes the exact pain seen in practice:

- Codex thinks an action is approved
- the kernel daemon still denies it
- the user sees `Operation not permitted`
- the agent burns time guessing whether the block was a true danger, a bad prediction, or stale policy state

The fix is not to remove `endpoint-sec`.

The fix is to make Smart Access and `endpoint-sec` speak the same effect language and share the same permit ledger.

## System Roles

Using the "small community / gate" analogy:

- `Codex main agent` = construction team
- `Guardian / Smart Approvals` = intelligent safety reviewer
- `SecurityHost` = property management approval desk
- machine-global `endpoint-sec` = physical gate, lock, and final enforcement runtime
- menubar app = security console for the whole community
- Codex TUI = per-household / per-session construction console

## Core Principle

There must be only one approval authority.

`SecurityHost` is the only authority that decides whether a predicted action is:

- auto-allowed
- auto-allowed with narrowed scope
- escalated to human
- denied
- downgraded because runtime trust is insufficient

`endpoint-sec` must not independently "re-approve" intent.

Its job is:

- accept permits
- enforce real effects
- return structured runtime events

If `endpoint-sec` blocks something, that means one of three things:

- `TrueRisk`
- `Underpredicted`
- `PolicyDrift`

It does not mean "a second reviewer disagreed."

## Effect Model

The shared first-phase effect model stays aligned to what both sides can already express:

- `ProtectedDelete`
- `ProtectedMoveOut`
- `SensitiveRead`
- `SensitiveTransferOut`
- `TaintWriteOut`
- `ExecExfilTool`
- `TrustedIdentityMismatch`

This is important:

- `endpoint-sec` enforces effects, not tool names
- `shell`, `apply_patch`, `unified_exec`, `network`, `MCP`, and `subagent forwarding` must all map into this shared effect ledger

Not every effect will be enforced by the same backend:

- machine-global `endpoint-sec` enforces kernel-observable machine effects
- Codex-side runtimes will continue to enforce network or app-level controls where Endpoint Security cannot directly observe them

But all of them must write into one common runtime event stream.

## Control Plane

The current `endpoint-sec` implementation already has:

- machine-global daemon lifecycle
- mode switching
- policy loading
- denial and audit logs
- temporary override handling
- menubar status and policy views

What it does not yet have is a real Smart Access control plane.

That control plane must add five first-class objects.

### 1. Lease

Represents one live Codex session or subagent identity at the machine level.

Required fields:

- `lease_id`
- `session_id`
- `parent_lease_id`
- `owner_kind` (`session` or `subagent`)
- `created_at`
- `expires_at`
- `last_heartbeat_at`
- `state`

### 2. Permit

Represents one scoped authorization issued by `SecurityHost`.

Required fields:

- `permit_id`
- `lease_id`
- `effect_kind`
- `scope`
- `issued_at`
- `expires_at`
- `risk_score`
- `issuer`
- `thread_id`
- `turn_id`
- `justification`

### 3. Action Scope

Represents one concrete action currently executing under a lease.

Required fields:

- `action_id`
- `lease_id`
- `tool_name`
- `summary`
- `predicted_effect_ids`
- `started_at`
- `ended_at`

### 4. Runtime Event

Represents an observed result from the runtime.

Required fields:

- `event_id`
- `lease_id`
- `action_id`
- `permit_id`
- `event_kind` (`allow`, `deny`, `would_deny`, `drift`, `health`)
- `reason_code`
- `effect_kind`
- `actual_scope`
- `process_name`
- `ancestor_name`
- `timestamp`
- `summary`

### 5. Capability Snapshot

Represents the current runtime enforcement boundary that Smart Access must review against.

Required fields stay aligned with Codex:

- `protected_zones`
- `sensitive_zones`
- `sensitive_export_allow_zones`
- `exec_exfil_tool_blocklist`
- `trusted_tools`
- `trusted_tool_identities`
- `taint_ttl_seconds`
- `read_gate_enabled`
- `transfer_gate_enabled`
- `exec_gate_enabled`
- `allow_vcs_metadata_in_ai_context`
- `allow_git_merge_pull_in_ai_context`

## Control Plane API

`endpoint-sec` should expose a machine-global runtime API with at least:

- `register_lease`
- `heartbeat_lease`
- `revoke_lease`
- `derive_child_lease`
- `install_permits`
- `revoke_permit`
- `begin_action_scope`
- `end_action_scope`
- `collect_events`
- `get_capability_snapshot`
- `get_runtime_health`

The current `temporary_overrides` request flow remains available only as an emergency compatibility path.

It is not the main Smart Access path anymore because it cannot express:

- effect type
- per-action identity
- permit narrowing
- per-subagent ownership
- structured mismatch reasoning

## Session and Subagent Model

There should be one machine-global runtime per machine, not one root runtime per Codex session.

The lifecycle should be:

1. Codex session starts
2. Codex registers one session lease with machine-global `endpoint-sec`
3. every subagent derives a child lease from that session lease
4. `SecurityHost` issues permits bound to the current lease
5. action execution begins and ends under that lease
6. if the session exits or crashes, the lease expires and all attached permits are reclaimed

This prevents "subagent escaped the boundary" gaps without requiring one root daemon per window.

## Three Runtime Modes

The existing three daemon modes are correct and should remain the public model:

- `enable`
- `silent`
- `off`

### `enable`

This is the intended long-term production mode.

- runtime really enforces
- deny means deny
- Smart Access can be low-friction because the hard gate is real

### `silent`

This is a calibration mode.

- runtime does not block
- it still emits `would_deny` events
- Smart Access can compare predictions against real observed effects without interrupting work

This is useful for tuning, but not acceptable as the steady-state trust model.

### `off`

This is a stop-the-guard mode.

- no runtime enforcement
- Smart Access must behave more conservatively because the enforcement chain is incomplete

## GUI / TUI Responsibilities

The two interfaces should remain separate because they serve different viewpoints.

### Menubar App

This is the machine-global security console.

It should show:

- current mode (`enable`, `silent`, `off`)
- daemon health
- capability snapshot version
- active leases
- active permits
- recent runtime events
- classification totals for `TrueRisk`, `Underpredicted`, and `PolicyDrift`

The current override-centric policy panel should be downgraded into:

- policy editing
- emergency override controls
- debugging tools

It should no longer be the main operating surface for normal Smart Access execution.

### Codex TUI

This is the per-session and per-turn execution console.

It should show:

- current security mode (`Default`, `Smart Access`, `Full Access`)
- predicted effects
- permit summaries
- runtime event summaries
- mismatch classification
- explicit reason when Smart Access falls back to human review

The TUI already has the right direction:

- top-level `Smart Access` mode
- permit summary rendering
- runtime mismatch rendering

It should now be upgraded from "message rendering" into "session security ledger rendering."

## Legacy Compatibility

`temporary_overrides` should remain, but only for:

- emergency manual unblock
- debugging
- old toolchain compatibility during migration

`request_security_override` should no longer be treated as the normal answer to Smart Access denials.

The normal answer should be:

- smarter predicted effects
- narrower permits
- better runtime capability synchronization

## Rollout Strategy

The implementation should proceed in four stages.

### Stage 1: endpoint-sec Control Plane

Add leases, permits, runtime events, health, and capability snapshot support to the machine-global daemon.

### Stage 2: Codex Smart Access Integration

Switch Codex from log scraping and override-first recovery into permit install plus structured event collection.

### Stage 3: Interface Upgrade

Upgrade the menubar app and Codex TUI to show the same permit and mismatch ledger from different scopes.

### Stage 4: Full Effect Coverage

Bring network, MCP, `apply_patch`, and subagent forwarding into the same permit and runtime event model so there are no "unguarded side channels."

## Success Criteria

This design is successful when:

- Codex can run in `Smart Access` with machine-global `endpoint-sec` in `enable`
- normal low-risk work flows through without repeated manual approval
- real dangerous effects are still hard-blocked
- runtime denials are explainable as `TrueRisk`, `Underpredicted`, or `PolicyDrift`
- subagents cannot outlive or out-scope their parent session lease
- the menubar app and Codex TUI both reflect the same underlying lease and permit state
