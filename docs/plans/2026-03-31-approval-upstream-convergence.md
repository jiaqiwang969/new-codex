# Approval Upstream Convergence Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove fork-only approval behavior so this repository's approval flow
matches public `openai/codex` `upstream/main`, then prepare the branch for the
remaining upstream sync.

**Architecture:** First restore upstream approval semantics in core, tool
runtimes, protocol, docs, and tests by removing local `approval_runtime` and
guardian replay extensions. After approval behavior matches upstream again,
merge the remaining upstream main changes on a smaller conflict surface.

**Tech Stack:** Git worktrees, Rust, app-server protocol/schema fixtures,
`cargo test`, `just fmt`, `just fix`, `just argument-comment-lint`

---

### Task 1: Archive the design decision and identify approval rollback files

**Files:**
- Create: `docs/plans/2026-03-31-approval-upstream-convergence-design.md`
- Create: `docs/plans/2026-03-31-approval-upstream-convergence.md`
- Verify: `git diff --name-status upstream/main..main`

**Step 1: Reconfirm the upstream divergence set**

Run:

```bash
git diff --name-status upstream/main..main | rg 'approval_runtime|guardian|thread_history|tools/events|codex-rs/core/src/codex.rs|codex-rs/core/src/codex_delegate.rs|codex-rs/app-server/README.md'
```

Expected: output shows the local approval runtime files and guardian replay
touch points that must be removed or restored to upstream.

**Step 2: Save the approved design and plan docs**

Document the convergence decision so later execution does not try to preserve
fork-only approval features.

**Step 3: Commit the planning docs**

```bash
git add docs/plans/2026-03-31-approval-upstream-convergence-design.md docs/plans/2026-03-31-approval-upstream-convergence.md
git commit -m "docs: plan approval upstream convergence" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 2: Remove local approval runtime from core

**Files:**
- Delete: `codex-rs/core/src/approval_runtime/mod.rs`
- Delete: `codex-rs/core/src/approval_runtime/types.rs`
- Delete: `codex-rs/core/src/approval_runtime/tests.rs`
- Delete: `codex-rs/core/src/approval_runtime/hosted.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_delegate.rs`
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/codex_tests.rs`
- Modify: `codex-rs/core/src/codex_delegate_tests.rs`
- Modify: `codex-rs/core/src/thread_manager_tests.rs`
- Modify: `codex-rs/core/tests/common/test_codex.rs`
- Modify: `codex-rs/core/tests/suite/approvals.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`

**Step 1: Write the failing rollback check**

Restore the upstream versions of the core approval files in a draft state and
run the most targeted tests that still mention the local runtime:

```bash
cargo test -p codex-core approval_runtime
cargo test -p codex-core runtime_lease
```

Expected: FAIL because local tests and imports still depend on
`approval_runtime`.

**Step 2: Replace local runtime plumbing with upstream behavior**

Use `git restore --source upstream/main -- <path>` for every file whose desired
behavior is exactly the public upstream version. Delete files that do not exist
upstream, especially `codex-rs/core/src/approval_runtime/*`.

Do not preserve hidden compatibility types, wrappers, or config flags.

**Step 3: Rewrite or remove local tests that asserted runtime leases**

Any test that only proves local runtime leases, hosted backend behavior, or
fail-closed runtime decisions must be deleted or restored to the upstream test
shape.

**Step 4: Run targeted core verification**

Run:

```bash
cargo test -p codex-core approval_runtime
cargo test -p codex-core runtime_lease
```

Expected: PASS with zero tests selected, or PASS after the remaining upstream
tests no longer reference the removed runtime.

**Step 5: Commit**

```bash
git add codex-rs/core
git commit -m "refactor(core): remove local approval runtime" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 3: Restore upstream approval flow in destructive tool runtimes

**Files:**
- Modify: `codex-rs/core/src/tools/runtimes/shell.rs`
- Modify: `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- Modify: `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- Modify: `codex-rs/core/src/unified_exec/async_watcher.rs`
- Modify: `codex-rs/core/src/tools/events.rs`

**Step 1: Write the failing approval-flow check**

Run:

```bash
cargo test -p codex-core --test all suite::approvals::
cargo test -p codex-core --test all suite::unified_exec::
```

Expected: FAIL while the runtime-preflight and runtime-warning code still
expects the removed local approval runtime.

**Step 2: Restore upstream versions of the tool-runtime files**

Use `git restore --source upstream/main -- <path>` for each file above so the
destructive tool flow matches upstream approval behavior exactly.

Do not reimplement upstream logic manually unless a direct restore causes an
unrelated conflict that must be resolved.

**Step 3: Re-run the targeted approval and exec tests**

Run:

```bash
cargo test -p codex-core --test all suite::approvals::
cargo test -p codex-core --test all suite::unified_exec::
```

Expected: PASS for the targeted approval and unified exec suites.

**Step 4: Commit**

```bash
git add codex-rs/core/src/tools/runtimes codex-rs/core/src/unified_exec/async_watcher.rs codex-rs/core/src/tools/events.rs
git commit -m "refactor(core): restore upstream approval tool flow" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 4: Remove local guardian replay extensions

**Files:**
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/app-server/README.md`
- Modify: `codex-rs/app-server-protocol/tests/schema_fixtures.rs`
- Modify: generated schema outputs under `codex-rs/app-server-protocol/schema/`
- Modify: guardian replay tests and snapshots that mention
  `guardianApprovalReview`

**Step 1: Write the failing replay check**

Run:

```bash
cargo test -p codex-app-server-protocol guardian_review
```

Expected: FAIL because local replay tests still expect `guardianApprovalReview`
thread items instead of upstream temporary notifications.

**Step 2: Restore upstream protocol/docs behavior**

Use `git restore --source upstream/main -- <path>` for the protocol, README,
and schema files whose official behavior already exists upstream.

Delete local-only references to replayed `guardianApprovalReview` thread items.

**Step 3: Re-run protocol verification**

Run:

```bash
cargo test -p codex-app-server-protocol guardian_review
```

Expected: PASS with upstream guardian notification semantics.

**Step 4: Commit**

```bash
git add codex-rs/app-server-protocol codex-rs/app-server
git commit -m "refactor(app-server): restore upstream guardian review flow" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 5: Delete local-only approval docs and customization notes

**Files:**
- Delete: `docs/plans/2026-03-31-smart-access-runtime-integration-design.md`
- Delete: `docs/plans/2026-03-31-smart-access-runtime-integration.md`
- Delete: `docs/plans/2026-03-31-guardian-review-replay-design.md`
- Delete: `docs/plans/2026-03-31-guardian-review-replay.md`
- Delete: `docs/plans/2026-03-31-approval-runtime-hosted-backend-design.md`
- Delete: `docs/plans/2026-03-31-approval-runtime-hosted-backend.md`
- Modify: `docs/local-customizations.md`

**Step 1: Write the failing documentation check**

Run:

```bash
rg -n "approval_runtime|guardianApprovalReview|runtime lease|hosted approval runtime" docs codex-rs/app-server/README.md
```

Expected: output still points to local-only approval architecture that should no
longer be treated as active.

**Step 2: Delete or rewrite local-only approval docs**

Remove documents that describe now-abandoned approval architecture. Keep only
the new convergence design/plan plus any short archived note if needed.

**Step 3: Re-run the documentation check**

Run:

```bash
rg -n "approval_runtime|guardianApprovalReview|runtime lease|hosted approval runtime" docs codex-rs/app-server/README.md
```

Expected: only historical or upstream-aligned references remain.

**Step 4: Commit**

```bash
git add docs
git commit -m "docs: archive local approval experiments" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 6: Merge the remaining upstream main changes after approval convergence

**Files:**
- Modify: whichever files conflict during merge, especially:
  - `codex-rs/tui*`
  - `codex-rs/core/*`
  - `codex-rs/app-server*`
  - CI/Bazel files

**Step 1: Verify approval rollback branch is clean**

Run:

```bash
git status --short
```

Expected: clean working tree before taking the upstream merge.

**Step 2: Merge upstream main**

Run:

```bash
git merge upstream/main
```

Expected: either a clean merge or a smaller conflict set than before, with
approval-specific conflicts largely gone.

**Step 3: Resolve remaining conflicts in favor of upstream approval behavior**

For any conflict that touches approvals, guardian review rendering, or thread
history, treat upstream behavior as authoritative unless the conflict is
completely unrelated to approval semantics.

**Step 4: Run targeted post-merge verification**

Run:

```bash
cargo test -p codex-core --test all suite::approvals::
cargo test -p codex-app-server-protocol guardian_review
cargo test -p codex-tui guardian_review
```

Expected: PASS, or note the exact remaining failures caused by unrelated
upstream sync work.

**Step 5: Commit**

```bash
git add -A
git commit -m "merge: sync upstream after approval convergence" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 7: Run formatting, lint, and final verification

**Files:**
- Verify only

**Step 1: Format Rust code**

Run in `codex-rs`:

```bash
just fmt
```

Expected: PASS

**Step 2: Run project-scoped Clippy fixes**

Run in `codex-rs`:

```bash
just fix -p codex-core
just fix -p codex-app-server-protocol
just fix -p codex-tui
```

Expected: PASS for changed crates.

**Step 3: Run argument comment lint**

Run in repo root:

```bash
PATH="$HOME/.cargo/bin:$PATH" just argument-comment-lint
```

Expected: PASS

**Step 4: Run final targeted test set**

Run in `codex-rs`:

```bash
cargo test -p codex-core --test all suite::approvals::
cargo test -p codex-app-server-protocol guardian_review
cargo test -p codex-tui guardian_review
```

Expected: PASS

**Step 5: Commit any final lint/format changes**

```bash
git add -A
git commit -m "chore: verify approval upstream convergence" -m "Co-authored-by: Codex <noreply@openai.com>"
```
