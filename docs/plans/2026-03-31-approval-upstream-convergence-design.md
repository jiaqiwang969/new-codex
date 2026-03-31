# Approval Upstream Convergence Design

**Date:** 2026-03-31

**Status:** Proposed

**Goal:** Converge all approval-related behavior back to public `openai/codex`
`upstream/main`, even when that means removing locally added approval features.

## Context

The local fork currently diverges from public `openai/codex` in two important
approval-related ways:

1. It adds a local `approval_runtime` layer under
   `codex-rs/core/src/approval_runtime/` with runtime leases, preflight and
   postflight checks, hosted persistence, and fail-closed warnings.
2. It extends guardian replay so app-server history can reconstruct a local
   `guardianApprovalReview` thread item instead of relying only on the official
   temporary `item/autoApprovalReview/*` notifications.

Those additions were useful during local exploration, but they are not part of
public `upstream/main`. Public upstream still treats guardian approval review as
temporary standalone notifications, and it does not expose the local runtime
closure stack.

The user explicitly chose the strictest convergence policy:

- approval behavior must match upstream as closely as possible
- local fork-only approval semantics should be removed rather than adapted
- old Smart Access / endpoint-security ideas should be abandoned

This changes the objective. The task is no longer "preserve our approval
enhancements while syncing upstream." The task is "remove local approval
product surface until the fork matches upstream approval semantics."

## Problem Statement

If we keep local approval semantics while also merging current upstream, we end
up carrying a private approval model on top of a moving official one:

- local runtime leases versus upstream guardian-only approval review
- local replayable guardian items versus upstream temporary notifications
- local runtime fail-closed warnings versus upstream item lifecycle behavior
- local `tui_app_server`-era approval changes against upstream's `tui`
  rename and ongoing guardian work

That creates exactly the long-term maintenance trap the user wants to avoid.

## Constraints

- Public `upstream/main` is the source of truth for approval behavior.
- Do not revive `smart_access`, `security_host`, `security_runtime`,
  `es_daemon`, or other deleted local approval products.
- Do not preserve `approval_runtime` behind compatibility layers, feature flags,
  or hidden config.
- Do not keep local guardian replay semantics if upstream still treats guardian
  review as temporary notifications.
- Prefer direct file-level restoration to upstream behavior over incremental
  compatibility shims.

## Options Considered

### Option 1: Keep local approval stack and only merge unrelated upstream work

This preserves current local capability, but it violates the user's goal.
Approval drift would continue to grow, and every upstream guardian change would
need bespoke reconciliation.

Rejected.

### Option 2: Keep a thin compatibility layer around local approval features

This lowers immediate churn, but it still leaves private approval semantics in
place. The maintenance burden remains because the fork must support behaviors
that upstream does not recognize.

Rejected.

### Option 3: Revert approval to upstream first, then sync newer upstream work

This removes local approval divergence early, restores a single approval model,
and reduces conflict surface before the larger upstream rename and tool-spec
changes land.

Recommended.

## Recommended Design

Use a two-stage convergence strategy:

### Stage 1: Approval rollback to upstream semantics

Restore upstream behavior in all approval-sensitive areas before taking the rest
of the upstream sync.

That means removing local fork-only approval behavior from:

- `codex-rs/core/src/approval_runtime/`
- session and subagent runtime-lease plumbing in `codex-rs/core/src/codex.rs`,
  `codex-rs/core/src/codex_delegate.rs`, and session state
- destructive tool runtime preflight and postflight hooks
- runtime warning and fail-closed helper paths
- guardian replay extensions in app-server history/docs/schema

The target state after Stage 1 is:

- approvals continue to use upstream guardian review behavior
- guardian review remains surfaced through `item/autoApprovalReview/*`
  notifications
- no local runtime lease, runtime decision, or hosted approval backend remains

### Stage 2: Upstream sync after approval rollback

Once approval divergence is removed, merge or replay the missing upstream main
changes, including:

- `codex-rs/tui_app_server` to `codex-rs/tui` rename
- tool spec extraction work
- auth changes
- newer TUI/app-server fixes

This sequencing matters because approval conflicts otherwise get multiplied by
the rename and broader refactors already present upstream.

## File-Level Scope

### Remove local approval runtime

- `codex-rs/core/src/approval_runtime/mod.rs`
- `codex-rs/core/src/approval_runtime/types.rs`
- `codex-rs/core/src/approval_runtime/tests.rs`
- `codex-rs/core/src/approval_runtime/hosted.rs`
- imports and uses in `codex-rs/core/src/lib.rs`

### Remove runtime-lease plumbing

- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/codex_delegate.rs`
- `codex-rs/core/src/state/session.rs`
- related unit/integration tests

### Restore upstream tool approval flow

- `codex-rs/core/src/tools/runtimes/shell.rs`
- `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- `codex-rs/core/src/unified_exec/async_watcher.rs`
- `codex-rs/core/src/tools/events.rs`

### Remove guardian replay extension

- `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- generated schema outputs if needed
- `codex-rs/app-server/README.md`
- related TUI/app-server tests and snapshots

### Remove local-only docs

The following docs should either be deleted or rewritten as archived historical
notes, not active architecture:

- `docs/plans/2026-03-31-smart-access-runtime-integration*.md`
- `docs/plans/2026-03-31-guardian-review-replay*.md`
- `docs/plans/2026-03-31-approval-runtime-hosted-backend*.md`
- approval sections in `docs/local-customizations.md`

## Risks

### Temporary capability loss

The fork will lose local runtime-closure behavior before upstream has any public
replacement. This is intentional and aligned with the chosen convergence
policy.

### Wider conflict surface during upstream merge

Even after approval rollback, upstream still has 42 mainline commits not yet in
the fork. The biggest structural conflict is the `tui_app_server` to `tui`
rename.

### Hidden test coupling

Local tests currently reference `approval_runtime` and guardian replay. Those
tests must be removed or rewritten to upstream expectations as part of the
rollback, not patched around.

## Non-Goals

- preserving local Smart Access semantics
- keeping the hosted approval runtime backend
- preserving replayable `guardianApprovalReview` thread items
- carrying forward any legacy endpoint-security bridge
- merging stale uncommitted residue from
  `feature-smart-access-phase2b-control-plane`

## Success Criteria

This design is complete when:

- there is no local `approval_runtime` module left
- approval behavior matches public `upstream/main`
- guardian review uses upstream temporary notification flow only
- approval-sensitive tests pass against upstream semantics
- the fork is ready to take the remaining upstream main changes without
  approval-specific compatibility shims
