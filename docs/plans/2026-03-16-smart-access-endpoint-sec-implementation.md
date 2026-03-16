# Smart Access + Embedded Endpoint Security Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the Phase 1 backbone of `Smart Access` so this repository has a real top-level smart security mode, a central `Security Host`, structured effect/permit/mismatch types, and a first bridge between Smart Approvals and the current Endpoint Security path.

**Architecture:** Implement in dependency order. First introduce a top-level `Smart Access` mode and shared security types. Then add `Security Host` as the single arbitration layer. Next upgrade Guardian from a tool reviewer into an effect predictor. After that, bridge legacy override flows through the new host and add the minimum TUI security trace needed to explain decisions. Do not replace the lightweight ES runtime in this phase.

**Tech Stack:** Rust workspace in `codex-rs`, protocol/config/runtime layers, ratatui TUI, insta snapshots, existing guardian reviewer engine, existing internal ES daemon plus external `endpoint-sec` semantics as architectural reference, Cargo, Just, targeted crate tests.

---

### Task 1: Create an isolated worktree for Smart Access Phase 1

**Files:**
- No repository files changed yet

**Step 1: Create the feature worktree**

Run: `git worktree add ../new-codex-smart-access -b feature/smart-access-phase1`
Expected: a new worktree is created from the current `HEAD` without disturbing the dirty main worktree

**Step 2: Verify branch and cleanliness**

Run: `git -C ../new-codex-smart-access status --short --branch`
Expected: `## feature/smart-access-phase1` and a clean working tree

**Step 3: Open the two design references for this work**

Run: `sed -n '1,240p' ../new-codex-smart-access/docs/plans/2026-03-16-smart-access-endpoint-sec-design.md`
Expected: the Smart Access design is visible locally in the new worktree

**Step 4: Commit**

No commit expected for this task.


### Task 2: Add a top-level Smart Access mode to protocol and config

**Files:**
- Modify: `codex-rs/protocol/src/config_types.rs`
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/core/src/config/types.rs`
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/profile.rs`
- Modify: `codex-rs/core/src/config/edit.rs`
- Modify: `codex-rs/core/src/features.rs`
- Modify: `codex-rs/core/config.schema.json`
- Test: `codex-rs/core/src/config_loader/tests.rs`
- Test: `codex-rs/core/tests/entire_config_test.rs`

**Step 1: Write the failing config tests**

Add tests that prove:

- the config can represent a top-level `smart-access` mode
- `Smart Access` remains distinct from `approval_policy` and `approvals_reviewer`
- older configs that only use `approvals_reviewer = guardian_subagent` still load into a compatible smart-access-derived runtime shape

Example assertion:

```rust
assert_eq!(config.security_mode, SecurityMode::SmartAccess);
assert_eq!(config.permissions.approval_policy.value(), AskForApproval::OnRequest);
assert_eq!(config.approvals_reviewer, ApprovalsReviewer::GuardianSubagent);
```

**Step 2: Run the targeted config tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core config_loader entire_config`
Expected: failures because the top-level Smart Access mode does not exist yet

**Step 3: Implement the minimal mode surface**

Add a dedicated top-level mode type such as `SecurityMode` / `AccessMode` that can represent:

- `Default`
- `SmartAccess`
- `FullAccess`

Thread it through the config and protocol layers without changing actual runtime behavior yet beyond storing and surfacing the new mode.

**Step 4: Regenerate the config schema**

Run in `../new-codex-smart-access/codex-rs`: `just write-config-schema`
Expected: `codex-rs/core/config.schema.json` is updated with the new mode field

**Step 5: Re-run the targeted config tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core config_loader entire_config`
Expected: PASS

**Step 6: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/protocol/src/config_types.rs codex-rs/protocol/src/protocol.rs codex-rs/core/src/config/types.rs codex-rs/core/src/config/mod.rs codex-rs/core/src/config/profile.rs codex-rs/core/src/config/edit.rs codex-rs/core/src/features.rs codex-rs/core/config.schema.json codex-rs/core/src/config_loader/tests.rs codex-rs/core/tests/entire_config_test.rs
git -C ../new-codex-smart-access commit -m "feat(core): add top-level smart access mode" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 3: Introduce shared Smart Access security types

**Files:**
- Create: `codex-rs/core/src/security_types.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Test: `codex-rs/core/src/security_types.rs`

**Step 1: Write the failing unit tests**

Add tests that serialize and compare whole objects for:

- `SecurityCapabilitySnapshot`
- `PredictedEffect`
- `SecurityPermit`
- `SecurityMismatch`

Example assertions:

```rust
assert_eq!(
    mismatch.classification,
    SecurityMismatchClassification::Underpredicted
);
```

**Step 2: Run the security type tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core security_types`
Expected: compile failures because the shared security types do not exist yet

**Step 3: Implement the shared types**

Create the first-phase versions of:

- `SecurityCapabilitySnapshot`
- `PredictedEffect`
- `PredictedEffectKind`
- `SecurityPermit`
- `SecurityPermitScope`
- `SecurityMismatch`
- `SecurityMismatchClassification`
- `SecurityArbitrationDecision`

Keep the first effect set intentionally small:

- `ProtectedDelete`
- `ProtectedMoveOut`
- `SensitiveRead`
- `SensitiveTransferOut`
- `TaintWriteOut`
- `ExecExfilTool`
- `TrustedIdentityMismatch`

**Step 4: Re-run the security type tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core security_types`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/security_types.rs codex-rs/core/src/lib.rs
git -C ../new-codex-smart-access commit -m "feat(core): add smart access security types" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 4: Add the Security Host skeleton and arbitration rules

**Files:**
- Create: `codex-rs/core/src/security_host/mod.rs`
- Create: `codex-rs/core/src/security_host/tests.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Test: `codex-rs/core/src/security_host/tests.rs`

**Step 1: Write the failing Security Host tests**

Add tests that prove:

- narrow protected delete can return `AllowWithPermit`
- a permit can be narrowed into `AllowWithAmendedPermit`
- sensitive transfer-out escalates to human review
- explicit high-risk mismatches are classified correctly
- trust-state uncertainty can trigger `DowngradeToDefault`

Example assertions:

```rust
assert_eq!(
    decision,
    SecurityArbitrationDecision::EscalateToHuman { .. }
);
```

**Step 2: Run the Security Host tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core security_host`
Expected: compile failures because the Security Host module does not exist yet

**Step 3: Implement the first Security Host**

Create a minimal host that can:

- accept a `SecurityCapabilitySnapshot`
- accept `PredictedEffect[]`
- map them to one of:
  - `AllowWithPermit`
  - `AllowWithAmendedPermit`
  - `EscalateToHuman`
  - `Deny`
  - `DowngradeToDefault`
- generate scoped TTL permits for low-risk actions

**Step 4: Re-run the Security Host tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core security_host`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/security_host/mod.rs codex-rs/core/src/security_host/tests.rs codex-rs/core/src/lib.rs
git -C ../new-codex-smart-access commit -m "feat(core): add security host arbitration backbone" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 5: Teach Guardian to emit structured predicted effects

**Files:**
- Modify: `codex-rs/core/src/guardian.rs`
- Modify: `codex-rs/core/src/guardian_prompt.md`
- Modify: `codex-rs/core/src/guardian_tests.rs`
- Test: `codex-rs/core/src/guardian_tests.rs`

**Step 1: Write the failing Guardian tests**

Add tests that prove Guardian output includes:

- risk score
- rationale
- structured `PredictedEffect[]`
- scoped paths for destructive or sensitive actions

Example assertion:

```rust
assert_eq!(
    assessment.predicted_effects,
    vec![PredictedEffect::protected_delete("/tmp/demo.txt")]
);
```

**Step 2: Run the Guardian tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core guardian`
Expected: FAIL because Guardian currently does not emit structured effect predictions

**Step 3: Update Guardian output and prompt**

Modify the guardian prompt and parser so the reviewer returns structured effect predictions instead of only free-form safety reasoning.

Keep fail-closed behavior:

- malformed output denies
- missing effects when effects are required denies
- low confidence can escalate

**Step 4: Re-run the Guardian tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core guardian`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/guardian.rs codex-rs/core/src/guardian_prompt.md codex-rs/core/src/guardian_tests.rs
git -C ../new-codex-smart-access commit -m "feat(core): teach guardian to predict effects" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 6: Route Smart Access approvals through Security Host

**Files:**
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/tools/context.rs`
- Modify: `codex-rs/core/src/tools/network_approval.rs`
- Modify: `codex-rs/core/src/mcp_connection_manager.rs`
- Modify: `codex-rs/core/src/mcp_tool_call.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`
- Test: `codex-rs/core/tests/suite/codex_message_processor_flow.rs`

**Step 1: Write the failing integration tests**

Add tests that prove:

- in `Smart Access`, reviewable actions go through:
  - Guardian
  - Security Host
  - arbitration result
- low-risk actions produce permits
- risky actions escalate or deny

Prefer asserting the whole decision object.

**Step 2: Run the targeted integration tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core approvals codex_message_processor_flow`
Expected: FAIL because runtime approval routing does not involve the Security Host yet

**Step 3: Implement the new approval path**

Wire the `Smart Access` branch so that:

- Guardian predicts effects
- Security Host arbitrates
- the runtime stores the resulting permit / mismatch context

Keep `Default` and `Full Access` behavior unchanged.

**Step 4: Re-run the targeted integration tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core approvals codex_message_processor_flow`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/codex.rs codex-rs/core/src/tools/context.rs codex-rs/core/src/tools/network_approval.rs codex-rs/core/src/mcp_connection_manager.rs codex-rs/core/src/mcp_tool_call.rs codex-rs/core/tests/suite/approvals.rs codex-rs/core/tests/suite/codex_message_processor_flow.rs
git -C ../new-codex-smart-access commit -m "feat(core): route smart access approvals through security host" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 7: Bridge legacy override requests through Security Host

**Files:**
- Modify: `codex-rs/core/src/tools/handlers/request_security_override.rs`
- Test: `codex-rs/core/src/tools/handlers/request_security_override.rs`

**Step 1: Write the failing handler tests**

Add tests that prove:

- override requests first request a `Security Host` decision
- denied or escalated decisions do not write the legacy override file
- allowed decisions still preserve the current TTL-based compatibility behavior

Example assertion:

```rust
assert_eq!(decision, SecurityArbitrationDecision::AllowWithPermit { .. });
```

**Step 2: Run the handler tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core request_security_override`
Expected: FAIL because the handler still writes policy directly

**Step 3: Implement the compatibility bridge**

Keep the existing file-based override mechanism for Phase 1, but change the flow to:

1. handler asks `Security Host`
2. host returns arbitration result
3. only approved legacy-compatible requests write the TTL override

Do not remove the old format yet.

**Step 4: Re-run the handler tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core request_security_override`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/tools/handlers/request_security_override.rs
git -C ../new-codex-smart-access commit -m "feat(core): bridge legacy security override through host" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 8: Add Smart Access preset and TUI status plumbing

**Files:**
- Modify: `codex-rs/utils/approval-presets/src/lib.rs`
- Modify: `codex-rs/tui/src/chatwidget.rs`
- Modify: `codex-rs/tui/src/status/card.rs`
- Modify: `codex-rs/tui/src/chatwidget/tests.rs`
- Test: `codex-rs/tui/src/chatwidget/tests.rs`

**Step 1: Write the failing TUI tests**

Add tests that prove:

- `Smart Access` appears as a top-level mode in the permissions UI
- status cards show `Smart Access` distinctly from `Default`
- selecting `Smart Access` does not collapse back into a hidden reviewer-only mode

**Step 2: Run the TUI tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-tui`
Expected: FAIL or produce snapshot drift because the new top-level mode is not yet shown

**Step 3: Implement the minimal TUI mode wiring**

Update preset selection, current-mode rendering, and status display so `Smart Access` is explicit and uses the new top-level mode shape.

**Step 4: Review and accept snapshots**

Run in `../new-codex-smart-access/codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Expected: pending snapshots for the touched TUI views

Run in `../new-codex-smart-access/codex-rs`: `cargo insta accept -p codex-tui`
Expected: intended snapshot updates are accepted

**Step 5: Re-run the TUI tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-tui`
Expected: PASS

**Step 6: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/utils/approval-presets/src/lib.rs codex-rs/tui/src/chatwidget.rs codex-rs/tui/src/status/card.rs codex-rs/tui/src/chatwidget/tests.rs codex-rs/tui/src/chatwidget/snapshots
git -C ../new-codex-smart-access commit -m "feat(tui): expose smart access mode" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 9: Add the initial TUI security trace

**Files:**
- Modify: `codex-rs/tui/src/chatwidget.rs`
- Modify: `codex-rs/tui/src/status/card.rs`
- Modify: `codex-rs/tui/src/bottom_pane/list_selection_view.rs`
- Modify: `codex-rs/tui/src/chatwidget/tests.rs`
- Test: `codex-rs/tui/src/chatwidget/tests.rs`

**Step 1: Write the failing UI tests**

Add tests or snapshots that prove the UI can display:

- Guardian risk score
- predicted effects
- permit summary
- mismatch summary

Keep the first version compact. A simple trace row is enough.

**Step 2: Run the TUI tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-tui`
Expected: FAIL or snapshot drift because no security trace exists yet

**Step 3: Implement the initial trace surface**

Render a thread-level trace summary for Smart Access decisions without attempting a full security dashboard in Phase 1.

**Step 4: Review and accept snapshots**

Run in `../new-codex-smart-access/codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Expected: pending snapshots for the new security-trace output

Run in `../new-codex-smart-access/codex-rs`: `cargo insta accept -p codex-tui`
Expected: intended snapshot updates are accepted

**Step 5: Re-run the TUI tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-tui`
Expected: PASS

**Step 6: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/tui/src/chatwidget.rs codex-rs/tui/src/status/card.rs codex-rs/tui/src/bottom_pane/list_selection_view.rs codex-rs/tui/src/chatwidget/tests.rs codex-rs/tui/src/chatwidget/snapshots
git -C ../new-codex-smart-access commit -m "feat(tui): add smart access security trace" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 10: Add mismatch classification and runtime event adapters

**Files:**
- Modify: `codex-rs/core/src/es_daemon/daemon.rs`
- Modify: `codex-rs/core/src/security_host/mod.rs`
- Test: `codex-rs/core/src/es_daemon/daemon.rs`
- Test: `codex-rs/core/src/security_host/tests.rs`

**Step 1: Write the failing runtime adapter tests**

Add tests that prove:

- legacy runtime denials can be adapted into `SecurityMismatch`
- current internal ES events can at least classify:
  - `ProtectedDelete`
  - `ProtectedMoveOut`
- the system can distinguish a permit miss from a direct high-risk deny

**Step 2: Run the targeted tests and confirm they fail**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core es_daemon security_host`
Expected: FAIL because the current daemon does not emit mismatch-aware events

**Step 3: Implement the Phase 1 adapter**

Do not replace the daemon. Add only the minimum adapter layer so the current daemon can:

- emit capability snapshots for its limited scope
- translate denial events into first-phase mismatch records

This keeps Phase 1 honest without attempting the full runtime merge yet.

**Step 4: Re-run the targeted tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core es_daemon security_host`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access add codex-rs/core/src/es_daemon/daemon.rs codex-rs/core/src/security_host/mod.rs
git -C ../new-codex-smart-access commit -m "feat(core): adapt legacy es runtime into smart access mismatch flow" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 11: Final formatting, crate-scoped lint fixes, and verification

**Files:**
- Modify: all files touched in Tasks 2-10

**Step 1: Run formatting**

Run in `../new-codex-smart-access/codex-rs`: `just fmt`
Expected: Rust formatting is normalized across touched crates

**Step 2: Run crate-scoped lint fixes for changed crates**

Run in `../new-codex-smart-access/codex-rs`: `just fix -p codex-core`
Expected: Clippy-guided fixes apply for the core crate without workspace-wide churn

Run in `../new-codex-smart-access/codex-rs`: `just fix -p codex-tui`
Expected: Clippy-guided fixes apply for the TUI crate without workspace-wide churn

**Step 3: Run targeted crate tests**

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-core`
Expected: PASS

Run in `../new-codex-smart-access/codex-rs`: `cargo test -p codex-tui`
Expected: PASS

**Step 4: Ask before running the full workspace suite**

Before running `cargo test` or `just test`, explicitly ask the user for approval because repository guidance requires user approval for the full suite after shared crate changes.

**Step 5: Commit the final integrated result**

```bash
git -C ../new-codex-smart-access add codex-rs
git -C ../new-codex-smart-access commit -m "feat: add smart access phase one backbone" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 12: Prepare the Phase 2 handoff note

**Files:**
- Modify: `docs/plans/2026-03-16-smart-access-endpoint-sec-design.md`
- Modify: `docs/plans/2026-03-16-smart-access-endpoint-sec-implementation.md`

**Step 1: Append a short Phase 2 handoff section**

Document the unresolved items that are intentionally deferred:

- full embedded `endpoint-sec` runtime merge
- sensitive read / taint / transfer / exec enforcement replacement
- trust identity cache and signature refresh runtime integration
- root helper and daemon lifecycle cleanup

**Step 2: Verify only expected docs changed**

Run: `git -C ../new-codex-smart-access status --short`
Expected: only the Phase 2 handoff notes or intentionally touched files remain

**Step 3: Commit**

```bash
git -C ../new-codex-smart-access add docs/plans/2026-03-16-smart-access-endpoint-sec-design.md docs/plans/2026-03-16-smart-access-endpoint-sec-implementation.md
git -C ../new-codex-smart-access commit -m "docs: record phase two smart access handoff" -m "Co-authored-by: Codex <noreply@openai.com>"
```
