# Smart Access + Embedded Endpoint Security Design

**Date:** 2026-03-16

**Objective:** Design a unified `Smart Access` security architecture for this customized Codex repository so that Smart Approvals, a new `Security Host`, and embedded `endpoint-sec` cooperate as one end-to-end system instead of independent layers that frequently disagree.

## Current State

- Official Smart Approvals core trunk is already merged into this repository.
- The current upstream-style guardian reviewer is enabled only when `approval_policy = on-request` and `approvals_reviewer = guardian_subagent`.
- The current TUI still treats Smart Approvals as a `Default` variant instead of a top-level access mode.
- The repository already contains a lightweight internal Endpoint Security daemon, but it only covers protected delete / move-out plus temporary overrides.
- The external `endpoint-sec/agentsmith-rs` implementation is significantly richer. It covers:
  - protected delete / move-out
  - sensitive read gating
  - taint marking and taint inheritance
  - tainted write-out deny
  - sensitive transfer-out deny
  - exec exfil tool deny
  - trusted tool identity checks
  - override request queue and audit

## Problem Statement

The current system has two different security languages:

- Smart Approvals reviews **tool requests**
- `endpoint-sec` enforces **real system effects**

This mismatch creates the exact pain observed in daily use:

- Smart Approvals can decide a request looks safe
- the real OS-level enforcement can still deny it
- the user often sees only `permission not allow`
- the main task stalls while the agent tries to understand whether the denial means real danger or merely incomplete prediction

The goal is not to remove Endpoint Security. The goal is to make the smart reviewer understand the same effect language as the enforcement layer.

## Design Principles

### 1. User Boundary Sovereignty

User-defined protected boundaries always outrank agent judgment.

If the user declares a protected zone, sensitive zone, export allow-zone, or dangerous exec boundary, then any real operation that crosses that boundary must be treated as high risk unless it is covered by a precise permit.

### 2. Intent Layer Can Be Optimistic; Impact Layer Must Be Conservative

The smart reviewer may interpret user intent and reduce unnecessary human prompts.

The runtime enforcement layer must remain conservative and must decide based on real effects, not based on the tool's self-description or the agent's intention summary.

### 3. One Judge, Not Two Competing Judges

Guardian and `endpoint-sec` should not act as two unrelated approval authorities.

The correct control flow is:

1. Guardian predicts likely effects
2. Security Host arbitrates and issues a precise permit
3. embedded `endpoint-sec` validates real effects against that permit
4. mismatches are recorded and fed back into future review behavior

If `endpoint-sec` denies an action, that should not be interpreted as "the other judge disagreed". It should be interpreted as "the approved prediction did not fully match the real effect".

### 4. Fail Closed on Real Impact; Fail Safe in User Experience

When the real runtime security state is uncertain, the system should not silently widen permissions.

Instead:

- real impact enforcement remains fail-closed
- user interaction falls back to human approval or `Default` mode with clear explanation

## Chosen Approach

Design for a **fully embedded end state**, but execute it in phases.

### End-State Architecture

- `Codex main agent`: construction team
- `Smart Approvals / Guardian`: intelligent safety reviewer
- `Security Host`: central approval desk and permit issuer
- embedded `endpoint-sec`: physical gate, lock, and final system-impact validator

### Why Not Keep the Original Smart Approvals Shape?

Original Smart Approvals is simpler and more direct, but it stays at the intent/tool-request layer.

That is not sufficient when danger is hidden in:

- scripts invoked by apparently harmless shell commands
- child processes and exec chains
- trusted tool spoofing or signature mismatch
- tainted data propagation after sensitive reads
- broad deletes or moves masked as "cleanup"

The design therefore accepts more complexity in exchange for a tighter binding between predicted risk and real system effect.

## Core Runtime Objects

### 1. `SecurityCapabilitySnapshot`

This is the structured capability digest that teaches Guardian what the local security runtime can actually enforce.

It should include at least:

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

Guardian must consume this snapshot before reviewing tool requests.

### 2. `PredictedEffect`

This is Guardian's structured output.

Guardian should stop returning only "approve/deny because this looks safe" and instead emit one or more predicted effects, for example:

- `ProtectedDelete`
- `ProtectedMoveOut`
- `SensitiveRead`
- `SensitiveTransferOut`
- `TaintWriteOut`
- `ExecExfilTool`
- `TrustedToolUse`
- `TrustedIdentitySensitivePath`
- `NetworkEgress`
- `McpExternalEffect`

Each effect should carry scoped context such as:

- `target_path`
- `source_path`
- `dest_path`
- `tool`
- `scope`
- `confidence`
- `why`

### 3. `SecurityPermit`

This is the artifact issued by `Security Host`.

It is not a generic allow. It is a precise permit that constrains:

- effect type
- path scope
- destination scope
- process scope
- TTL
- issuing authority
- risk score
- reason / justification
- thread and turn identifiers

All automatically granted power in `Smart Access` should flow through permits.

### 4. `SecurityMismatch`

This closes the loop after execution.

If runtime enforcement observes a real effect outside the approved permit, the system should produce a structured mismatch containing:

- `permit_id`
- predicted effects
- actual operation
- actual reason code
- actual source / destination paths
- process and ancestor
- classification
- summary

Mismatch classification is fixed to:

- `TrueRisk`
- `Underpredicted`
- `PolicyDrift`

These categories are critical:

- `TrueRisk`: the action genuinely crossed the user's protected boundary
- `Underpredicted`: Guardian approved too narrow a permit or missed a real effect
- `PolicyDrift`: the runtime security state or policy diverged from what Guardian thought was active

## Top-Level Product Modes

The repository should no longer treat smart approvals as just a reviewer variant underneath `Default`.

The product should expose three top-level modes:

- `Default`
- `Smart Access`
- `Full Access`

### `Default`

- human approval centric
- simple mental model
- lower automation

### `Smart Access`

- intelligent approval as the primary mode
- `Security Host` permit issuance
- embedded Endpoint Security validation
- full security audit trail

### `Full Access`

- unrestricted mode
- intended for exceptional cases only

This makes `Smart Access` a product mode, not a hidden implementation detail.

## System Control Flow

The intended control flow for a reviewable action is:

1. User selects `Smart Access`
2. Main agent prepares a tool request
3. Guardian reviews the request using `SecurityCapabilitySnapshot`
4. Guardian emits:
   - risk score
   - rationale
   - `PredictedEffect[]`
5. `Security Host` evaluates those predicted effects against local policy
6. `Security Host` returns one of:
   - `AllowWithPermit`
   - `AllowWithAmendedPermit`
   - `EscalateToHuman`
   - `Deny`
   - `DowngradeToDefault`
7. If allowed, a scoped `SecurityPermit` is issued
8. Execution proceeds
9. embedded `endpoint-sec` validates real runtime effects
10. the action is classified as:
   - `WithinPermit`
   - `TrueRisk`
   - `Underpredicted`
   - `PolicyDrift`

## Arbitration Outcomes

### `AllowWithPermit`

Used when:

- predicted effect is clear
- path scope is narrow
- TTL is short
- no sensitive data leaves allowed zones
- no broad destructive operation is involved

### `AllowWithAmendedPermit`

Used when:

- Guardian broadly understood the action
- but `Security Host` needs to narrow the permit before approval

### `EscalateToHuman`

Used when:

- effect scope is broad
- risk is high
- runtime state is uncertain
- trusted identity is unresolved
- policy changes or root-impacting actions are involved

### `Deny`

Used when:

- the action clearly crosses user-defined security boundaries

### `DowngradeToDefault`

Used when:

- the smart security stack is currently not trustworthy enough to auto-issue permits
- the system should continue safely through human approval rather than widening access

## Endpoint Security Positioning

`endpoint-sec` remains necessary even after Smart Access exists.

It covers the final-impact layer that tool-level approval does not reliably see:

- real process tree / AI ancestor detection
- real file movement across boundary zones
- sensitive reads and taint propagation
- child process inheritance
- trusted binary identity checks
- actual exec usage of exfiltration tools

This means the runtime must remain present even if Guardian becomes much better at prediction.

## Observability and TUI

`Smart Access` should be observable, not mysterious.

The TUI should expose a thread-level security timeline containing:

- active mode
- Guardian risk score and rationale
- predicted effects
- issued permits
- runtime denials
- mismatch classification
- searchable audit rows

This is required to distinguish:

- true danger
- permit too narrow
- policy drift or trust-state problems

Without this, the product degrades back into opaque "permission denied" behavior.

## Migration Strategy

### Phase 1: Unify the Language and the Arbitration Layer

Deliver:

- independent top-level `Smart Access`
- `SecurityCapabilitySnapshot`
- `PredictedEffect`
- `SecurityPermit`
- `SecurityMismatch`
- `Security Host`
- Guardian effect prediction
- legacy override bridge through `Security Host`
- initial TUI security trace

Do not yet:

- fully replace the internal ES daemon
- fully merge the external `agentsmith-rs` runtime
- rebuild the entire root helper / daemon stack

### Phase 2: Replace the Lightweight Internal ES Runtime

Replace the current simplified internal daemon with an embedded runtime that includes the richer `endpoint-sec` semantics:

- sensitive read gate
- taint propagation
- taint write-out deny
- sensitive transfer-out deny
- exec exfil deny
- trusted identity checks
- override queue / audit integration

### Phase 3: Close the Full Loop

Finish:

- subagent forwarding coverage
- unified tool-to-effect modeling
- runtime mismatch memory
- richer TUI audit tools
- stable Smart Access operations for daily use

## Risks and Mitigations

### Risk: Product Complexity Increases

Mitigation:

- keep `Default` available
- keep `Full Access` available
- make `Smart Access` explicit and observable

### Risk: Guardian Still Mis-predicts Runtime Effects

Mitigation:

- require structured `PredictedEffect[]`
- record `SecurityMismatch`
- use mismatch classifications to improve prompts and routing

### Risk: Runtime State Drift Causes False Blocks

Mitigation:

- maintain an explicit `SecurityCapabilitySnapshot`
- classify signature / trust / policy divergence as `PolicyDrift`
- support `DowngradeToDefault`

### Risk: Over-broad Permits Recreate Full Access by Accident

Mitigation:

- issue only narrow permits
- require TTL on automatic permits
- forbid no-expire or broad-path automatic overrides

## Success Criteria

This design is successful when:

- `Smart Access` is a real top-level mode
- Guardian predicts effect-level risk instead of only tool-level risk
- `Security Host` becomes the only approval arbitrator
- runtime denials are explainable in terms of `TrueRisk`, `Underpredicted`, or `PolicyDrift`
- `endpoint-sec` remains the final-impact validator
- the user can inspect the complete security trail in the TUI

## Non-Goals

This design explicitly does not attempt to:

- remove Endpoint Security enforcement
- silently replace all other approval modes with Smart Access
- allow the AI to self-issue broad or no-expire overrides
- complete the entire root/helper/signing/runtime refactor in Phase 1
- rely on free-form prompt memory instead of structured contracts
