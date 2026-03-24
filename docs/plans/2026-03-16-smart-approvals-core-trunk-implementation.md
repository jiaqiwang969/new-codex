# Obsolete: Smart Approvals Core Trunk Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

> **Status:** Historical and obsolete for current merge work. This plan preserves the local `endpoint-sec` / `request_security_override` boundary, but the active direction is to remove that local flow and keep the upstream `approval_policy` + `approvals_reviewer` / guardian model.

**Goal:** Import the official Smart Approvals core runtime trunk into this customized repository without changing the current Endpoint Security enforcement boundary or pulling in the unstable app-server guardian notification surface.

**Architecture:** Port the feature in dependency order: first add the reviewer model and config migration, then import the guardian engine, then wire guardian review into core approval routing, and finally expose the minimum TUI surface needed to use and understand the reviewer choice. Keep `approval_policy` semantics unchanged; add `approvals_reviewer` as a separate axis exactly as upstream intended.

**Tech Stack:** Git worktrees, Rust workspace in `codex-rs`, Cargo, Just, generated config schema, ratatui/insta snapshots in `codex-tui`, existing local Endpoint Security and approval infrastructure.

---

### Task 1: Create an isolated worktree for the import

**Files:**
- No repository files changed yet

**Step 1: Create the feature worktree**

Run: `git worktree add ../new-codex-smart-approvals-core -b feature/smart-approvals-core`
Expected: a new worktree is created from the current `HEAD` without disturbing the dirty main worktree

**Step 2: Verify branch and cleanliness**

Run: `git -C ../new-codex-smart-approvals-core status --short --branch`
Expected: `## feature/smart-approvals-core` and a clean working tree

**Step 3: Capture the upstream reference for targeted porting**

Run: `git -C ../new-codex-smart-approvals-core show --stat --summary bc24017d64829d0b97b8bc6ed529a389e1e8bc1b`
Expected: the Smart Approvals upstream commit summary is available for manual hunk-by-hunk porting

**Step 4: Commit**

No commit expected for this task.


### Task 2: Add the reviewer config/runtime surface

**Files:**
- Modify: `codex-rs/protocol/src/approvals.rs`
- Modify: `codex-rs/protocol/src/config_types.rs`
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/core/src/config/types.rs`
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/profile.rs`
- Modify: `codex-rs/core/src/config/edit.rs`
- Modify: `codex-rs/core/src/features.rs`
- Modify: `codex-rs/core/config.schema.json`
- Test: `codex-rs/core/src/config/mod.rs`
- Test: `codex-rs/core/tests/suite/override_updates.rs`

**Step 1: Write the failing tests**

Add tests that prove:

- `approvals_reviewer = "guardian_subagent"` loads successfully from config
- `guardian_approval = true` backfills the reviewer only when `approvals_reviewer` is not already set
- turn/session overrides preserve `approval_policy` and `approvals_reviewer` as separate values

Example assertions:

```rust
assert_eq!(config.permissions.approvals_reviewer, ApprovalsReviewer::GuardianSubagent);
assert_eq!(config.permissions.approval_policy.value(), AskForApproval::OnRequest);
```

**Step 2: Run the targeted tests and confirm they fail**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core override_updates approvals`
Expected: failures because `approvals_reviewer` and the alias migration do not exist yet

**Step 3: Implement the minimal protocol/config surface**

Add the upstream-style reviewer axis:

- define `ApprovalsReviewer`
- thread it through runtime turn/session override types
- add config loading and profile merge support
- add `smart_approvals` as the rollout/UI gate only
- add the deprecated `guardian_approval` alias migration

Keep the runtime behavior unchanged unless the reviewer is explicitly configured.

**Step 4: Regenerate the config schema**

Run in `../new-codex-smart-approvals-core/codex-rs`: `just write-config-schema`
Expected: `codex-rs/core/config.schema.json` is updated to reflect the new config fields

**Step 5: Run the targeted tests and confirm they pass**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core override_updates approvals`
Expected: PASS

**Step 6: Commit**

```bash
git -C ../new-codex-smart-approvals-core add codex-rs/protocol/src/approvals.rs codex-rs/protocol/src/config_types.rs codex-rs/protocol/src/protocol.rs codex-rs/core/src/config/types.rs codex-rs/core/src/config/mod.rs codex-rs/core/src/config/profile.rs codex-rs/core/src/config/edit.rs codex-rs/core/src/features.rs codex-rs/core/config.schema.json codex-rs/core/tests/suite/override_updates.rs
git -C ../new-codex-smart-approvals-core commit -m "feat(core): add approvals reviewer runtime surface" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 3: Import the guardian engine and fail-closed review behavior

**Files:**
- Create: `codex-rs/core/src/guardian.rs`
- Create: `codex-rs/core/src/guardian_prompt.md`
- Create: `codex-rs/core/src/guardian_tests.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Test: `codex-rs/core/src/guardian_tests.rs`

**Step 1: Write the failing guardian tests**

Add tests for:

- approved review when `risk_score < 80`
- denied review when `risk_score >= 80`
- malformed guardian output aborts the review
- guardian timeout/error fails closed

Example shape:

```rust
assert_eq!(
    review,
    GuardianReview {
        status: GuardianReviewStatus::Approved,
        risk_score: Some(42),
        risk_level: Some(GuardianRiskLevel::Low),
        rationale: Some("safe read-only command".to_string()),
    }
);
```

**Step 2: Run the guardian tests and confirm they fail**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core guardian`
Expected: compile failures or test failures because the guardian module is not present yet

**Step 3: Port the guardian engine from upstream**

Manually port the core of upstream `guardian.rs` and the matching prompt, while preserving local module layout and style rules.

Constraints:

- keep the reviewer subagent read-only
- set guardian review to fail closed on any uncertainty
- do not add any `endpoint-sec` coupling in this task

**Step 4: Re-run the guardian tests**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core guardian`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-approvals-core add codex-rs/core/src/guardian.rs codex-rs/core/src/guardian_prompt.md codex-rs/core/src/guardian_tests.rs codex-rs/core/src/lib.rs
git -C ../new-codex-smart-approvals-core commit -m "feat(core): import guardian reviewer engine" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 4: Route reviewable core actions through the configured reviewer

**Files:**
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_delegate.rs`
- Modify: `codex-rs/core/src/mcp_tool_call.rs`
- Modify: `codex-rs/core/src/tools/context.rs`
- Modify: `codex-rs/core/src/tools/network_approval.rs`
- Modify: `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- Modify: `codex-rs/core/src/tools/runtimes/shell.rs`
- Modify: `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`
- Test: `codex-rs/core/tests/suite/codex_delegate.rs`
- Test: `codex-rs/core/tests/suite/unified_exec.rs`

**Step 1: Write the failing integration tests**

Add targeted tests that prove:

- low-risk shell/unified-exec actions are auto-approved by guardian when `approvals_reviewer = guardian_subagent`
- high-risk actions are denied or aborted conservatively
- managed-network approvals flow into guardian instead of always requiring a human reviewer
- MCP approvals flow into guardian
- delegated/subagent approval forwarding still works when the reviewer is guardian

Prefer asserting the whole approval/review object rather than individual fields.

**Step 2: Run the targeted integration tests and confirm they fail**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core approvals codex_delegate unified_exec`
Expected: FAIL because the runtime still routes reviewable actions to the user-only path

**Step 3: Implement the routing changes**

Port the smallest possible upstream slices that:

- preserve `approval_policy` meaning
- branch on `approvals_reviewer`
- call the guardian engine for reviewable requests
- keep all fail-closed behavior intact
- leave the current `request_security_override` / Endpoint Security path unchanged

Do not replace entire files. Apply narrow splices in the existing customized code.

**Step 4: Re-run the targeted integration tests**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core approvals codex_delegate unified_exec`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-approvals-core add codex-rs/core/src/codex.rs codex-rs/core/src/codex_delegate.rs codex-rs/core/src/mcp_tool_call.rs codex-rs/core/src/tools/context.rs codex-rs/core/src/tools/network_approval.rs codex-rs/core/src/tools/runtimes/apply_patch.rs codex-rs/core/src/tools/runtimes/shell.rs codex-rs/core/src/tools/runtimes/unified_exec.rs codex-rs/core/tests/suite/approvals.rs codex-rs/core/tests/suite/codex_delegate.rs codex-rs/core/tests/suite/unified_exec.rs
git -C ../new-codex-smart-approvals-core commit -m "feat(core): route approvals through guardian reviewer" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 5: Add the minimum TUI surface and snapshot coverage

**Files:**
- Modify: `codex-rs/utils/approval-presets/src/lib.rs`
- Modify: `codex-rs/tui/src/app.rs`
- Modify: `codex-rs/tui/src/chatwidget.rs`
- Modify: `codex-rs/tui/src/status/card.rs`
- Modify: `codex-rs/tui/src/bottom_pane/approval_overlay.rs`
- Test: `codex-rs/tui/src/chatwidget/tests.rs`
- Test: `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__approvals_selection_popup.snap`
- Test: `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__approvals_selection_popup@windows.snap`
- Test: `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__permissions_selection_history_after_mode_switch.snap`

**Step 1: Write the failing TUI tests**

Add or extend snapshot coverage so the UI proves:

- the current reviewer choice is visible in permissions/approvals UI
- switching into Smart Approvals mode selects `guardian_subagent` without changing the meaning of `approval_policy`
- pending or resolved guardian review is visible in session status/history

**Step 2: Run the TUI tests and confirm they fail**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-tui`
Expected: FAIL or produce snapshot drift because the reviewer UI does not exist yet

**Step 3: Implement the minimum TUI wiring**

Keep the UI scope narrow:

- expose reviewer selection through the existing approvals flow
- align any Smart Approvals toggle with the matching reviewer configuration
- avoid introducing app-server-specific guardian lifecycle UI in this round

**Step 4: Review and accept snapshots**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Expected: pending snapshots listed for the touched TUI expectations

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo insta accept -p codex-tui`
Expected: intended snapshot updates are accepted

**Step 5: Commit**

```bash
git -C ../new-codex-smart-approvals-core add codex-rs/utils/approval-presets/src/lib.rs codex-rs/tui/src/app.rs codex-rs/tui/src/chatwidget.rs codex-rs/tui/src/status/card.rs codex-rs/tui/src/bottom_pane/approval_overlay.rs codex-rs/tui/src/chatwidget/tests.rs codex-rs/tui/src/chatwidget/snapshots
git -C ../new-codex-smart-approvals-core commit -m "feat(tui): expose smart approvals reviewer state" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 6: Regenerate, lint, verify, and prepare handoff

**Files:**
- Modify: any generated files already touched above
- Modify: `docs/plans/2026-03-16-smart-approvals-core-trunk-design.md` only if the implemented scope changed
- Modify: `docs/plans/2026-03-16-smart-approvals-core-trunk-implementation.md` only if the implementation sequence changed materially

**Step 1: Run targeted crate verification**

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-core`
Expected: PASS for the affected core workspace crate

Run in `../new-codex-smart-approvals-core/codex-rs`: `cargo test -p codex-tui`
Expected: PASS for the affected TUI crate

**Step 2: Run scoped lint fixes for the affected crates**

Run in `../new-codex-smart-approvals-core/codex-rs`: `just fix -p codex-core`
Expected: clippy-driven fixes are applied or no changes are needed

Run in `../new-codex-smart-approvals-core/codex-rs`: `just fix -p codex-tui`
Expected: clippy-driven fixes are applied or no changes are needed

**Step 3: Run the formatter**

Run in `../new-codex-smart-approvals-core/codex-rs`: `just fmt`
Expected: formatting completes cleanly

**Step 4: Ask before the full workspace suite**

Before running `cargo test` for the entire workspace, ask the user for approval because the repo instructions require confirmation before full-suite validation on shared crates.

**Step 5: Create the final feature commit or mergeable stack tip**

```bash
git -C ../new-codex-smart-approvals-core status --short
git -C ../new-codex-smart-approvals-core log --oneline --decorate -n 5
```

Expected: a small, reviewable commit stack with no accidental changes from the original dirty worktree
