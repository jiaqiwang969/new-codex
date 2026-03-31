# Approval Runtime Hosted Backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a hosted file-backed backend to `approval_runtime` so current
runtime lease/preflight/postflight flows keep working on `main` without relying
only on the in-memory runtime client.

**Architecture:** Keep `ApprovalRuntimeClient` and current runtime decision types
unchanged. Add a new `HostedApprovalRuntimeClient` inside `approval_runtime`,
wire a small default factory from `codex_home`, and preserve the existing
destructive tool flow behavior.

**Tech Stack:** Rust, async traits, file-backed state under `codex_home`,
`cargo test -p codex-core`, `just fmt`, `just argument-comment-lint`

---

### Task 1: Add hosted backend persistence and recovery tests

**Files:**
- Create: `codex-rs/core/src/approval_runtime/hosted.rs`
- Modify: `codex-rs/core/src/approval_runtime/mod.rs`
- Modify: `codex-rs/core/src/approval_runtime/tests.rs`

**Step 1: Write the failing test**

Add unit tests for:

- registering a lease with one hosted client and reading usable state from a new
  hosted client pointing at the same runtime root
- deriving a child lease stores the parent linkage
- revoking a parent lease makes descendant preflight return
  `RuntimeHealth::FallbackToHuman`
- a stale runtime lock is recovered and surfaced as
  `RuntimeHealth::Recovery`

Prefer temp dirs and deep equality over field-by-field assertions.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-core approval_runtime`

Expected: FAIL because `HostedApprovalRuntimeClient` does not exist yet.

**Step 3: Write minimal implementation**

Create a hosted backend that:

- stores runtime state in a small JSON file under `codex_home`
- coordinates access with a lock file
- cleans up stale locks
- persists leases, parent-child links, and action ids
- maps revoked or missing leases to current fallback semantics

Do not port old `SecurityPermit`, endpoint-security event, or remote control
plane code into this first slice.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-core approval_runtime`

Expected: PASS

**Step 5: Commit**

```bash
git add codex-rs/core/src/approval_runtime/mod.rs codex-rs/core/src/approval_runtime/hosted.rs codex-rs/core/src/approval_runtime/tests.rs
git commit -m "feat(core): add hosted approval runtime backend" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 2: Wire the hosted backend into normal session startup

**Files:**
- Modify: `codex-rs/core/src/approval_runtime/mod.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_tests.rs`

**Step 1: Write the failing test**

Add tests that prove:

- `Codex::spawn` uses the default hosted approval runtime for root sessions
- delegated sessions still inherit the parent runtime client and parent runtime
  lease

Keep the assertions on behavior, not concrete internal field layout.

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-core runtime_lease`

Expected: FAIL because the default runtime factory does not yet depend on
`codex_home`.

**Step 3: Write minimal implementation**

Change the default runtime helper into a `codex_home`-aware factory and update
session spawn wiring to use it only when no runtime is inherited.

Do not add new config knobs in this slice.

**Step 4: Run test to verify it passes**

Run: `cargo test -p codex-core runtime_lease`

Expected: PASS

**Step 5: Commit**

```bash
git add codex-rs/core/src/approval_runtime/mod.rs codex-rs/core/src/codex.rs codex-rs/core/src/codex_tests.rs
git commit -m "feat(core): default approval runtime to hosted backend" -m "Co-authored-by: Codex <noreply@openai.com>"
```

### Task 3: Re-run existing runtime integration coverage

**Files:**
- Verify only

**Step 1: Run targeted runtime regression tests**

Run:

```bash
cargo test -p codex-core approval_runtime
cargo test -p codex-core --test all suite::unified_exec::runtime_
cargo test -p codex-core --test all suite::approvals::runtime_
```

Expected: PASS

**Step 2: Format**

Run in `codex-rs`: `just fmt`

Expected: PASS

**Step 3: Run argument comment lint**

Run in repo root: `just argument-comment-lint`

Expected: PASS, or the known nightly target-install failure if the local dylint
environment is still missing `aarch64-apple-darwin`.

**Step 4: Commit verification/doc touch-ups if needed**

```bash
git add -A
git commit -m "chore: verify hosted approval runtime backend" -m "Co-authored-by: Codex <noreply@openai.com>"
```
