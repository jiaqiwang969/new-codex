# Smart Access Safe Local Read Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Smart Access silently auto-approve non-sensitive local read-only `shell` and `unified_exec` commands while preserving strict handling for sensitive reads, writes, destructive actions, transfers, and runtime mismatches.

**Architecture:** Add a deterministic Smart Access fast path ahead of guardian review, reuse the existing safe-command parser for command-shape checks, add a narrow path-sensitivity classifier, and make Smart Access guardian/permit success paths silent. Keep human-visible output only for real escalation, denial, downgrade, or runtime mismatch outcomes.

**Tech Stack:** Rust (`codex-core`, `codex-tui`, `codex-shell-command`), Tokio async approval flow, ratatui UI tests, markdown docs.

---

### Task 1: Lock the desired shell behavior with approval tests

**Files:**
- Modify: `core/tests/suite/approvals.rs`

**Step 1: Write the failing test**

Add a Smart Access test that runs a safe external read such as:

```rust
let command = format!("sed -n '1,20p' {}", external_markdown.display());
```

Assert that the turn completes without surfacing a manual approval prompt or a
visible Smart Access success trace.

**Step 2: Run the targeted test**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: FAIL because safe local reads still flow through visible approval
machinery.

**Step 3: Add a failing sensitive-read regression test**

Add a second test for:

```rust
let command = "cat ~/.ssh/id_rsa".to_string();
```

Assert that Smart Access does not auto-approve the request.

**Step 4: Re-run the targeted tests**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: FAIL with current behavior still prompting or tracing success paths.

**Step 5: Commit**

```bash
git add core/tests/suite/approvals.rs
git commit -m "test: define Smart Access safe-read behavior"
```

### Task 2: Add Smart Access fast-path classification helpers

**Files:**
- Modify: `core/src/smart_access.rs`

**Step 1: Add the new internal types and function signatures**

Add:

```rust
enum SafeLocalReadScope {
    NoPath,
    Paths(Vec<AbsolutePathBuf>),
    RepoMetadata { repo_root: AbsolutePathBuf },
}

pub(crate) fn maybe_auto_approve_safe_local_read(
    turn: &TurnContext,
    request: &GuardianApprovalRequest,
) -> Option<ReviewDecision> {
    // stub
}
```

**Step 2: Write unit tests next to the helper**

Cover:

- `pwd` -> `NoPath`
- `sed -n '1,20p' file` -> one explicit path
- `rg foo dir` -> one explicit path
- `git status` -> `RepoMetadata`
- complex shell script -> `None`

**Step 3: Run the new unit tests**

Run:

```bash
cargo test -p codex-core smart_access::
```

Expected: FAIL because classification helpers are incomplete.

**Step 4: Implement minimal classification**

Use existing command-shape helpers instead of new parsing rules:

- `is_known_safe_command(...)`
- `canonicalize_command_for_approval(...)`
- `parse_shell_lc_plain_commands(...)`

Only return a fast-path scope when the command shape is unambiguously
read-only.

**Step 5: Re-run the unit tests**

Run:

```bash
cargo test -p codex-core smart_access::
```

Expected: PASS for the new classification tests.

**Step 6: Commit**

```bash
git add core/src/smart_access.rs
git commit -m "feat: classify Smart Access safe local reads"
```

### Task 3: Add deterministic sensitive-path rejection

**Files:**
- Modify: `core/src/smart_access.rs`

**Step 1: Write failing unit tests for sensitivity checks**

Add tests that prove:

- a path under `~/.ssh` is sensitive
- a path under `<codex_home>/auth.json` or `.credentials.json` is sensitive
- a normal external markdown file is not sensitive
- configured `sensitive_zones` override the fallback list

**Step 2: Run the targeted sensitivity tests**

Run:

```bash
cargo test -p codex-core sensitive_read_target
```

Expected: FAIL because the helper does not exist yet.

**Step 3: Implement the helper**

Add:

```rust
fn path_is_sensitive_read_target(
    turn: &TurnContext,
    path: &AbsolutePathBuf,
) -> bool
```

and a `default_sensitive_read_roots(...)` helper that covers the approved
minimal fallback set.

**Step 4: Wire sensitivity into fast-path approval**

Update `maybe_auto_approve_safe_local_read(...)` so it returns `None` when any
target path is sensitive.

**Step 5: Re-run the targeted tests**

Run:

```bash
cargo test -p codex-core sensitive_read_target
cargo test -p codex-core smart_access_safe_local_read
```

Expected: PASS for safe external docs and FAIL-to-approve for secret roots.

**Step 6: Commit**

```bash
git add core/src/smart_access.rs core/tests/suite/approvals.rs
git commit -m "feat: keep Smart Access fast path out of secret roots"
```

### Task 4: Invoke the fast path before guardian review

**Files:**
- Modify: `core/src/tools/runtimes/shell.rs`
- Modify: `core/src/tools/runtimes/unified_exec.rs`

**Step 1: Add failing integration coverage for both runtimes**

Extend tests so both:

- `shell`
- `exec_command`

use the same silent safe-read behavior under Smart Access.

**Step 2: Run the targeted runtime tests**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: FAIL because the runtime still always enters Smart Access review.

**Step 3: Implement the runtime hook**

In each runtime, change the approval flow to:

```rust
if let Some(decision) = maybe_auto_approve_safe_local_read(turn, &approval_request) {
    return decision;
}
```

Place this check before `review_smart_access_request(...)`.

**Step 4: Re-run the targeted runtime tests**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: PASS with no human prompt for safe local reads.

**Step 5: Commit**

```bash
git add core/src/tools/runtimes/shell.rs core/src/tools/runtimes/unified_exec.rs core/tests/suite/approvals.rs
git commit -m "feat: run Smart Access fast path before guardian"
```

### Task 5: Make Smart Access internal guardian review silent

**Files:**
- Modify: `core/src/guardian.rs`
- Modify: `core/src/smart_access.rs`

**Step 1: Write failing tests for visible guardian noise**

Add tests that prove Smart Access internal approval does not emit:

- `Automatic approval review approved ...`
- guardian progress/approved UI events for successful safe requests

**Step 2: Run the targeted tests**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: FAIL because guardian success is still user-visible.

**Step 3: Implement display modes**

Add:

```rust
pub(crate) enum GuardianReviewDisplay {
    Visible,
    Silent,
}
```

Split the detailed review helper so Smart Access can call the silent variant.

**Step 4: Re-run the targeted tests**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
```

Expected: PASS with Smart Access internal success now silent.

**Step 5: Commit**

```bash
git add core/src/guardian.rs core/src/smart_access.rs core/tests/suite/approvals.rs
git commit -m "refactor: silence guardian success inside Smart Access"
```

### Task 6: Hide successful Smart Access permit traces in the TUI

**Files:**
- Modify: `core/src/smart_access.rs`
- Modify: `tui/src/chatwidget/tests.rs`

**Step 1: Write the failing UI tests**

Add assertions that successful safe reads do not render:

- `Smart Access permit issued`
- `Smart Access narrowed and permitted`

while fallback, deny, downgrade, and runtime mismatch still render normally.

**Step 2: Run the TUI tests**

Run:

```bash
cargo test -p codex-tui guardian_smart_access
```

Expected: FAIL because permit-success traces are still shown.

**Step 3: Implement the visibility change**

Keep runtime context persistence for permit success, but skip user-visible trace
emission for:

- `AllowWithPermit`
- `AllowWithAmendedPermit`

**Step 4: Re-run the TUI tests**

Run:

```bash
cargo test -p codex-tui guardian_smart_access
```

Expected: PASS with only real escalation/risk states still visible.

**Step 5: Commit**

```bash
git add core/src/smart_access.rs tui/src/chatwidget/tests.rs
git commit -m "feat: hide successful Smart Access permit traces"
```

### Task 7: Update docs and verify the final behavior

**Files:**
- Modify: `README.md`
- Modify: `docs/plans/2026-03-22-smart-access-safe-local-read-design.md`

**Step 1: Update the user-facing docs**

Document that Smart Access silently auto-resolves non-sensitive local read-only
commands and only surfaces approvals for sensitive or higher-risk behavior.

**Step 2: Run formatter**

Run:

```bash
just fmt
```

Expected: formatting completes cleanly.

**Step 3: Run focused verification**

Run:

```bash
cargo test -p codex-core smart_access_safe_local_read
cargo test -p codex-core sensitive_read_target
cargo test -p codex-tui guardian_smart_access
```

Expected: all focused tests pass.

**Step 4: Run crate-level verification**

Run:

```bash
cargo test -p codex-core
cargo test -p codex-tui
```

Expected: both crates pass.

**Step 5: Run lint fixups**

Run:

```bash
just fix -p codex-core
just fix -p codex-tui
```

Expected: clippy fixes apply cleanly without introducing new behavior changes.

**Step 6: Commit**

```bash
git add README.md docs/plans/2026-03-22-smart-access-safe-local-read-design.md docs/plans/2026-03-22-smart-access-safe-local-read.md core/src/smart_access.rs core/src/guardian.rs core/src/tools/runtimes/shell.rs core/src/tools/runtimes/unified_exec.rs core/tests/suite/approvals.rs tui/src/chatwidget/tests.rs
git commit -m "feat: restore silent Smart Access behavior for safe local reads"
```
