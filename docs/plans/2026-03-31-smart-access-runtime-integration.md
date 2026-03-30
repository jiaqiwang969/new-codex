# Smart Access Runtime Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Recover the runtime/control-plane value from Smart Access `phase2b` by adding a small runtime companion to current `main` that closes destructive approval flows without restoring the deleted Smart Access product mode.

**Architecture:** Add a new internal `approval_runtime` layer in `codex-rs/core` that owns runtime health checks, lease lifecycle, permit/action-scope helpers, and typed runtime decisions. Thread that layer through session/subagent lifecycle and destructive tool runtimes, then surface runtime recovery/drift/mismatch outcomes through existing warning and history rendering in `tui_app_server`.

**Tech Stack:** Rust, async session state, guardian + exec-policy approval flow, unified exec/apply-patch runtimes, ratatui TUI snapshots, `cargo test`, `just fmt`, `just fix`, `just argument-comment-lint`

---

### Task 1: Add the runtime companion module

**Files:**
- Create: `codex-rs/core/src/approval_runtime/mod.rs`
- Create: `codex-rs/core/src/approval_runtime/types.rs`
- Create: `codex-rs/core/src/approval_runtime/tests.rs`
- Modify: `codex-rs/core/src/lib.rs`

**Step 1: Write the failing test**

Add unit tests for:

- healthy preflight returns `RuntimeDecision::Ok`
- unhealthy-but-recoverable preflight returns `RuntimeDecision::Recovery`
- explicit runtime fallback returns `RuntimeDecision::FallbackToHuman`
- policy drift returns `RuntimeDecision::PolicyDrift`
- mismatch returns `RuntimeDecision::Mismatch`

Prefer a small fake runtime client so the tests do not depend on host endpoint-security state.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-core approval_runtime`

Expected: FAIL because `approval_runtime` does not exist yet.

**Step 3: Write minimal implementation**

Add the smallest useful API:

```rust
pub(crate) enum RuntimeDecision {
    Ok,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
    Mismatch { summary: String },
    PolicyDrift { summary: String },
}

pub(crate) trait ApprovalRuntimeClient {
    async fn preflight(&self, request: &RuntimePreflightRequest) -> Result<RuntimePreflight>;
    async fn finish(&self, request: &RuntimeFinishRequest) -> Result<RuntimeDecision>;
}
```

Keep this module narrowly focused on health, leases, permits, and typed outcomes. Do not reintroduce the deleted `smart_access` / `security_runtime` product surface.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-core approval_runtime`

Expected: PASS

**Step 5: Commit**

Run:

```bash
git add codex-rs/core/src/approval_runtime codex-rs/core/src/lib.rs
git commit -m "feat: add approval runtime companion" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 2: Thread session and child lease state through the current lifecycle

**Files:**
- Modify: `codex-rs/core/src/state/service.rs`
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_delegate.rs`
- Modify: `codex-rs/core/src/tools/handlers/agent_jobs.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- Test: `codex-rs/core/src/state/session_tests.rs`
- Test: `codex-rs/core/src/codex_delegate_tests.rs`

**Step 1: Write the failing test**

Add tests that prove:

- a session acquires and stores one runtime lease
- a delegated/subagent turn derives a child lease from the parent session lease
- parent lease invalidation clears child lease usability

Prefer deep object equality and explicit fake lease ids over field-by-field assertions.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-core runtime_lease
```

Expected: FAIL because session/subagent lifecycle does not persist runtime lease state.

**Step 3: Write minimal implementation**

Add lease state to session/service ownership, then derive child leases where subagents are created. Keep the write surface small:

- session owns the root lease handle
- subagent creation asks the runtime companion for a child lease
- cleanup/invalidation happens with existing session teardown paths

Avoid adding one-off helper methods that are only called once.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-core runtime_lease
```

Expected: PASS

**Step 5: Commit**

Run:

```bash
git add codex-rs/core/src/state/service.rs codex-rs/core/src/state/session.rs codex-rs/core/src/codex.rs codex-rs/core/src/codex_delegate.rs codex-rs/core/src/tools/handlers/agent_jobs.rs codex-rs/core/src/tools/handlers/multi_agents_common.rs codex-rs/core/src/state/session_tests.rs codex-rs/core/src/codex_delegate_tests.rs
git commit -m "feat: wire runtime leases through session lifecycle" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 3: Integrate runtime preflight/postflight into destructive tool flows

**Files:**
- Modify: `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- Modify: `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- Modify: `codex-rs/core/src/tools/runtimes/shell.rs`
- Modify: `codex-rs/core/src/tools/events.rs`
- Test: `codex-rs/core/tests/suite/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`

**Step 1: Write the failing test**

Add targeted integration tests for:

- destructive exec installs runtime preflight and finishes cleanly
- destructive exec emits a runtime warning and fails closed on policy drift
- destructive patch can downgrade to `fallback_to_human`
- non-destructive patch does not request a runtime permit

Use new test names that include `runtime_` so they can be filtered directly.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-core --test all suite::unified_exec::runtime_
cargo test -p codex-core --test all suite::approvals::runtime_
```

Expected: FAIL because tool runtimes do not yet call the runtime companion.

**Step 3: Write minimal implementation**

For command and patch flows:

- call runtime preflight only after static approval is complete
- install permits only for destructive predicted effects
- open/close action scope around execution
- map post-execution runtime results into existing warning/execution surfaces
- pause automatic continuation on `PolicyDrift` or `Mismatch`

Do not rewrite guardian or exec-policy semantics. This task is about dynamic closure after approval, not a new approval model.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-core --test all suite::unified_exec::runtime_
cargo test -p codex-core --test all suite::approvals::runtime_
```

Expected: PASS

**Step 5: Commit**

Run:

```bash
git add codex-rs/core/src/tools/runtimes/unified_exec.rs codex-rs/core/src/tools/runtimes/apply_patch.rs codex-rs/core/src/tools/runtimes/shell.rs codex-rs/core/src/tools/events.rs codex-rs/core/tests/suite/unified_exec.rs codex-rs/core/tests/suite/approvals.rs
git commit -m "feat: close destructive tool flows with runtime checks" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 4: Render runtime warnings in the current TUI surface

**Files:**
- Modify: `codex-rs/tui_app_server/src/chatwidget.rs`
- Modify: `codex-rs/tui_app_server/src/chatwidget/status_surfaces.rs`
- Test: `codex-rs/tui_app_server/src/chatwidget/tests.rs`
- Modify: `codex-rs/tui_app_server/src/chatwidget/snapshots/*.snap`

**Step 1: Write the failing test**

Add snapshot-oriented tests for:

- runtime recovery warning
- runtime policy drift warning
- runtime mismatch warning
- fallback-to-human warning

Route them through the same warning/history path used by current guardian and command events. Do not add a separate Smart Access mode panel.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-tui-app-server runtime_
```

Expected: FAIL because the current TUI does not render these runtime warning variants distinctly.

**Step 3: Write minimal implementation**

Teach the app-server chat widget to:

- recognize runtime warning payloads
- render concise human-readable labels
- keep execution history grouped with the corresponding command/patch context

Prefer existing status/history helpers over adding a new standalone rendering subsystem.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-tui-app-server runtime_
```

Expected: PASS, producing reviewed snapshot diffs where UI text changed intentionally.

If snapshots changed intentionally:

```bash
cargo insta pending-snapshots -p codex-tui-app-server
```

Review the generated `*.snap.new` files directly, then accept only the intended ones.

**Step 5: Commit**

Run:

```bash
git add codex-rs/tui_app_server/src/chatwidget.rs codex-rs/tui_app_server/src/chatwidget/status_surfaces.rs codex-rs/tui_app_server/src/chatwidget/tests.rs codex-rs/tui_app_server/src/chatwidget/snapshots
git commit -m "feat: render runtime closure warnings in tui" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 5: Run repo formatting, scoped verification, and final docs cleanup

**Files:**
- Modify: files touched in Tasks 1-4
- Modify: `docs/plans/2026-03-31-smart-access-runtime-integration-design.md` only if implementation forced a scope correction

**Step 1: Run formatter**

Run:

```bash
cd codex-rs
just fmt
```

Expected: PASS

**Step 2: Run scoped lint cleanup**

Run:

```bash
cd codex-rs
just fix -p codex-core
just fix -p codex-tui-app-server
just argument-comment-lint
```

Expected: PASS

**Step 3: Run scoped verification**

Run:

```bash
cd codex-rs
cargo test -p codex-core approval_runtime
cargo test -p codex-core --test all suite::unified_exec::runtime_
cargo test -p codex-core --test all suite::approvals::runtime_
cargo test -p codex-tui-app-server runtime_
```

Expected: PASS

If implementation ended up touching shared protocol/common surfaces, ask the user before running workspace-wide `cargo test`.

**Step 4: Update docs if implementation changed scope**

If the implementation diverged from the approved design in a meaningful way, update `docs/plans/2026-03-31-smart-access-runtime-integration-design.md` in the same branch before the final commit.

**Step 5: Commit**

Run:

```bash
git add codex-rs docs/plans/2026-03-31-smart-access-runtime-integration-design.md
git commit -m "chore: finish runtime integration verification" -m "Co-authored-by: Codex <noreply@openai.com>"
```
