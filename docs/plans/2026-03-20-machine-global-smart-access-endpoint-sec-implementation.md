# Obsolete: Machine-Global Smart Access + endpoint-sec Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

> **Status:** Historical and obsolete for current merge work. This plan builds on the deprecated local Smart Access + `endpoint-sec` flow and should not be used as active implementation guidance. Follow upstream approvals/guardian semantics instead.

**Goal:** Upgrade machine-global `endpoint-sec` into a Smart Access-aware security runtime, then connect `codex-rs` to it through leases, permits, action scopes, and structured runtime events.

**Architecture:** Keep one machine-global root daemon and its menubar app, but replace override-first coordination with permit-first coordination. `SecurityHost` remains the only approval authority; `endpoint-sec` becomes the shared runtime that enforces real machine effects and reports structured feedback. Codex TUI and the menubar app become two views over the same lease, permit, and mismatch ledger.

**Tech Stack:** Rust in `codex-rs`, Rust in `/Users/jqwang/00-nixos-config/endpoint-sec/agentsmith-rs`, SwiftUI in `/Users/jqwang/00-nixos-config/endpoint-sec/agentsmith-rs/menubar-app`, existing Smart Access logic, existing `endpoint-sec` policy and daemon lifecycle, Cargo, Just, SwiftPM, launchd.

---

### Task 1: Create coordinated worktrees and capture current baselines

**Files:**
- No product files changed yet

**Step 1: Create a Codex worktree**

Run: `git worktree add ../new-codex-machine-global-smart-access -b feature/machine-global-smart-access`
Expected: a clean dedicated worktree appears for `codex-rs` changes

**Step 2: Create an endpoint-sec worktree**

Run: `git -C /Users/jqwang/00-nixos-config/endpoint-sec worktree add ../endpoint-sec-smart-access -b feature/endpoint-sec-smart-access`
Expected: a clean dedicated worktree appears for daemon and menubar changes

**Step 3: Verify both trees are clean**

Run: `git -C ../new-codex-machine-global-smart-access status --short --branch`
Expected: `## feature/machine-global-smart-access`

Run: `git -C ../endpoint-sec-smart-access status --short --branch`
Expected: `## feature/endpoint-sec-smart-access`

**Step 4: Record the current Smart Access and daemon baselines**

Run: `cargo test -p codex-core security_host`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

Run: `cargo test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

**Step 5: Commit**

No commit for this setup task.


### Task 2: Define the machine-global runtime contract in Codex

**Files:**
- Create: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_runtime/mod.rs`
- Create: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_runtime/tests.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_types.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/lib.rs`

**Step 1: Write the failing contract tests**

Add tests covering:

- `SecurityLeaseRegistration`
- `SecurityLeaseHandle`
- `SecurityChildLeaseRequest`
- `SecurityPermitInstallation`
- `InstalledSecurityPermit`
- `SecurityActionScope`
- `SecurityRuntimeEvent`
- `SecurityRuntimeEventBatch`
- `RuntimeReasonCode`
- `RuntimeHealthState`

The assertions should compare complete objects, not field fragments.

**Step 2: Run the targeted tests and confirm failure**

Run: `cargo test -p codex-core security_runtime security_types`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: FAIL because the runtime contract types do not exist yet

**Step 3: Implement the contract**

Add a `SecurityRuntime` trait with:

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

Keep the first version transport-agnostic. Do not embed Unix socket client code in this task.

**Step 4: Re-run tests**

Run: `cargo test -p codex-core security_runtime security_types`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-machine-global-smart-access add codex-rs/core/src/security_runtime/mod.rs codex-rs/core/src/security_runtime/tests.rs codex-rs/core/src/security_types.rs codex-rs/core/src/lib.rs
git -C ../new-codex-machine-global-smart-access commit -m "feat(core): add machine-global security runtime contract" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 3: Add a transport client for the machine-global runtime

**Files:**
- Create: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_runtime/control_plane.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_runtime/mod.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/state/service.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/state/session.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/thread_manager.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/codex.rs`

**Step 1: Write failing client tests**

Add tests that simulate:

- session lease registration
- child lease derivation
- permit installation
- event collection cursor advancement
- runtime health fetch

Use an in-process fake control-plane server rather than the real daemon for these tests.

**Step 2: Run the targeted tests and confirm failure**

Run: `cargo test -p codex-core smart_access_runtime`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: FAIL because no machine-global client exists

**Step 3: Implement the control-plane client**

Add a client that talks to the machine-global daemon over one control-plane transport.

Initial requirements:

- request/response framing
- structured error propagation
- cursor-based event polling
- capability snapshot fetch
- health fetch

Do not yet wire it to the real endpoint-sec daemon.

**Step 4: Re-run tests**

Run: `cargo test -p codex-core smart_access_runtime`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-machine-global-smart-access add codex-rs/core/src/security_runtime/control_plane.rs codex-rs/core/src/security_runtime/mod.rs codex-rs/core/src/state/service.rs codex-rs/core/src/state/session.rs codex-rs/core/src/thread_manager.rs codex-rs/core/src/codex.rs
git -C ../new-codex-machine-global-smart-access commit -m "feat(core): add machine-global security runtime client" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 4: Add lease, permit, and event state to endpoint-sec

**Files:**
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/src/main.rs`
- Create: `../endpoint-sec-smart-access/agentsmith-rs/src/control_plane.rs`
- Create: `../endpoint-sec-smart-access/agentsmith-rs/src/control_plane_tests.rs`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/Cargo.toml`

**Step 1: Write failing daemon-side tests**

Add tests covering:

- lease registration and heartbeat
- child lease derivation
- automatic lease expiry
- permit installation and expiry
- action scope begin/end
- event cursor pagination

The tests should assert entire returned structs.

**Step 2: Run tests and confirm failure**

Run: `cargo test control_plane`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: FAIL because the daemon only supports override and denial/audit files

**Step 3: Implement control-plane state**

Add daemon-managed state for:

- active leases
- active permits
- active action scopes
- event ring buffer or append-only event store
- runtime health snapshot

Preserve the existing policy model and denial logging while layering the new control plane above it.

**Step 4: Re-run tests**

Run: `cargo test control_plane`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../endpoint-sec-smart-access add agentsmith-rs/src/main.rs agentsmith-rs/src/control_plane.rs agentsmith-rs/src/control_plane_tests.rs agentsmith-rs/Cargo.toml
git -C ../endpoint-sec-smart-access commit -m "feat(agentsmith): add machine-global lease and permit state" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 5: Expose the endpoint-sec control-plane transport

**Files:**
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/src/main.rs`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/agentsmith.plist`
- Modify: `../endpoint-sec-smart-access/flake.nix`

**Step 1: Write failing integration tests**

Add tests for:

- request decoding
- response encoding
- socket startup
- state directory creation
- recovery after daemon restart

**Step 2: Run tests and confirm failure**

Run: `cargo test control_plane_transport`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: FAIL because no runtime transport is exposed

**Step 3: Implement the transport**

Expose the control plane over the machine-global daemon transport used by launchd deployment.

Requirements:

- stable socket path from launchd environment
- protected state directory creation
- graceful reconnect after daemon restart
- structured request and response envelopes

Do not remove legacy override request directories in this task.

**Step 4: Re-run tests**

Run: `cargo test control_plane_transport`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../endpoint-sec-smart-access add agentsmith-rs/src/main.rs agentsmith-rs/agentsmith.plist flake.nix
git -C ../endpoint-sec-smart-access commit -m "feat(agentsmith): expose smart access control plane" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 6: Teach endpoint-sec enforcement to consult permits

**Files:**
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/src/main.rs`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/src/control_plane.rs`

**Step 1: Write failing enforcement tests**

Add tests for:

- permit-covered protected delete in `enable`
- permit miss protected delete in `enable`
- permit-covered move-out in `enable`
- permit miss move-out in `enable`
- `silent` mode producing `would_deny` instead of blocking
- expired permit denial after TTL

**Step 2: Run tests and confirm failure**

Run: `cargo test permit_enforcement`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: FAIL because current enforcement only knows policy and temporary overrides

**Step 3: Implement permit-aware enforcement**

During deny checks:

- match observed effect to active lease
- match effect scope to active permit
- allow when covered by a valid permit
- deny or `would_deny` when not covered
- emit structured runtime events either way

Keep existing denial logs, but derive them from the structured event data.

**Step 4: Re-run tests**

Run: `cargo test permit_enforcement`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../endpoint-sec-smart-access add agentsmith-rs/src/main.rs agentsmith-rs/src/control_plane.rs
git -C ../endpoint-sec-smart-access commit -m "feat(agentsmith): enforce machine-global permits" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 7: Wire Codex Smart Access to the machine-global runtime

**Files:**
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/smart_access.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/security_host/mod.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/tests/suite/approvals.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/tests/suite/unified_exec.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/tests/suite/mod.rs`

**Step 1: Write failing end-to-end tests**

Add tests proving:

- Smart Access installs permits before execution
- Smart Access begins and ends an action scope
- runtime events are collected with a cursor after execution
- mismatch classification uses structured runtime events instead of log scraping
- `silent` mode runtime events still surface as `PolicyDrift` or `Underpredicted` signals without blocking execution

**Step 2: Run tests and confirm failure**

Run: `cargo test -p codex-core approvals unified_exec smart_access_runtime`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: FAIL because Smart Access still uses session-local runtime context and legacy daemon summaries

**Step 3: Implement permit-first Smart Access flow**

Replace:

- log scraping
- legacy daemon summary parsing
- override-first recovery thinking

With:

- install permit
- begin action
- execute tool
- collect events from cursor
- build mismatch from typed runtime events

**Step 4: Re-run tests**

Run: `cargo test -p codex-core approvals unified_exec smart_access_runtime`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-machine-global-smart-access add codex-rs/core/src/smart_access.rs codex-rs/core/src/security_host/mod.rs codex-rs/core/tests/suite/approvals.rs codex-rs/core/tests/suite/unified_exec.rs codex-rs/core/tests/suite/mod.rs
git -C ../new-codex-machine-global-smart-access commit -m "feat(core): wire smart access to machine-global endpoint runtime" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 8: Keep legacy override as emergency-only compatibility

**Files:**
- Modify: `../new-codex-machine-global-smart-access/codex-rs/core/src/tools/handlers/request_security_override.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/README.md`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/src/main.rs`

**Step 1: Write failing compatibility tests**

Add tests proving:

- override path still works in explicit manual escalation flows
- override cannot silently widen a narrowed permit request
- override is labeled as emergency compatibility in user-visible messaging

**Step 2: Run tests and confirm failure**

Run: `cargo test -p codex-core request_security_override`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: FAIL because user-visible messaging still treats override as the main recovery tool

**Step 3: Implement the compatibility downgrade**

Update docs and user-facing messages so override is described as:

- emergency manual unblock
- debugging path
- migration compatibility path

Do not remove the feature yet.

**Step 4: Re-run tests**

Run: `cargo test -p codex-core request_security_override`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-machine-global-smart-access add codex-rs/core/src/tools/handlers/request_security_override.rs codex-rs/README.md
git -C ../new-codex-machine-global-smart-access commit -m "docs(core): downgrade legacy override to emergency compatibility" -m "Co-authored-by: Codex <noreply@openai.com>"
git -C ../endpoint-sec-smart-access add agentsmith-rs/src/main.rs
git -C ../endpoint-sec-smart-access commit -m "docs(agentsmith): mark temporary overrides as emergency compatibility" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 9: Upgrade the menubar app into the machine-global security console

**Files:**
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/AgentSmithViewModel.swift`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/DashboardView.swift`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/StatusPanel.swift`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/PolicyPanel.swift`
- Modify: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/Models.swift`

**Step 1: Write failing view-model and UI tests**

Add tests for:

- active lease count rendering
- active permit count rendering
- runtime mode rendering
- health state rendering
- mismatch classification summaries
- emergency override section labeling

**Step 2: Run tests and confirm failure**

Run: `swift test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app`
Expected: FAIL because the UI only knows denials, policy, and temporary overrides

**Step 3: Implement the console upgrade**

Promote the menubar app from:

- denial dashboard
- policy editor
- override panel

Into:

- runtime mode console
- lease and permit overview
- runtime event overview
- emergency override panel

Do not remove current mode switching.

**Step 4: Re-run tests**

Run: `swift test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../endpoint-sec-smart-access add agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/AgentSmithViewModel.swift agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/DashboardView.swift agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/StatusPanel.swift agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/PolicyPanel.swift agentsmith-rs/menubar-app/Sources/AgentSmithMenuBar/Models.swift
git -C ../endpoint-sec-smart-access commit -m "feat(menubar): show leases permits and runtime state" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 10: Upgrade the Codex TUI into a session security ledger

**Files:**
- Modify: `../new-codex-machine-global-smart-access/codex-rs/tui/src/chatwidget/tests.rs`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__guardian_smart_access_trace_renders_permit_summary.snap`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__guardian_smart_access_runtime_mismatch_renders_warning.snap`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/tui/src/...` (exact rendering files discovered while implementing)

**Step 1: Write failing snapshot tests**

Add or update snapshots covering:

- permit issue summaries with lease context
- runtime mismatch summaries with reason classification
- Smart Access fallback-to-human reasons
- silent-mode `would_deny` summaries

**Step 2: Run tests and confirm failure**

Run: `cargo test -p codex-tui`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: FAIL with pending snapshots or changed render output

**Step 3: Implement the TUI ledger**

Expose:

- predicted effects
- permit summary
- runtime event summary
- mismatch class
- fallback reason

Keep the top-level `Default / Smart Access / Full Access` selection intact.

**Step 4: Re-run tests and accept intended snapshots**

Run: `cargo test -p codex-tui`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS with snapshot changes

Run: `cargo insta pending-snapshots -p codex-tui`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: pending snapshots listed

Run: `cargo insta accept -p codex-tui`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: snapshot files updated in place

**Step 5: Commit**

```bash
git -C ../new-codex-machine-global-smart-access add codex-rs/tui/src/chatwidget/tests.rs codex-rs/tui/src/chatwidget/snapshots
git -C ../new-codex-machine-global-smart-access commit -m "feat(tui): render smart access security ledger" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 11: Integrate deployment and live-machine validation

**Files:**
- Modify: `../endpoint-sec-smart-access/flake.nix`
- Modify: `../new-codex-machine-global-smart-access/codex-rs/README.md`
- Create: `../new-codex-machine-global-smart-access/docs/plans/phase-2c-live-validation-checklist.md`

**Step 1: Write the live validation checklist**

Document exact checks for:

- daemon mode switch
- control-plane socket availability
- state directory creation
- lease creation on session start
- permit creation on low-risk Smart Access action
- deny event in `enable`
- `would_deny` event in `silent`
- cleanup on session exit

**Step 2: Validate Nix deployment wiring**

Run: `rg -n "agentsmith|control|socket|state" flake.nix`
Workdir: `../endpoint-sec-smart-access`
Expected: the deployment file contains the daemon environment and launch wiring you need to update

**Step 3: Implement the deployment updates**

Ensure `make macbook-pro-m4` deploys:

- the updated daemon
- the updated socket and state path configuration
- the updated control-plane environment

**Step 4: Run targeted validations**

Run: `cargo test -p codex-core smart_access_runtime`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

Run: `cargo test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

Run: `swift test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../endpoint-sec-smart-access add flake.nix
git -C ../endpoint-sec-smart-access commit -m "feat(nix): deploy machine-global smart access control plane" -m "Co-authored-by: Codex <noreply@openai.com>"
git -C ../new-codex-machine-global-smart-access add codex-rs/README.md docs/plans/phase-2c-live-validation-checklist.md
git -C ../new-codex-machine-global-smart-access commit -m "docs: add machine-global smart access validation checklist" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 12: Run formatting and targeted verification before wider rollout

**Files:**
- No new files; verification only

**Step 1: Format Rust in Codex**

Run: `just fmt`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: formatting completes without changes left unstaged

**Step 2: Run scoped Rust verification**

Run: `cargo test -p codex-core security_runtime security_host smart_access_runtime approvals unified_exec`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

Run: `cargo test -p codex-tui`
Workdir: `../new-codex-machine-global-smart-access/codex-rs`
Expected: PASS

**Step 3: Run endpoint-sec verification**

Run: `cargo test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs`
Expected: PASS

**Step 4: Run menubar verification**

Run: `swift test`
Workdir: `../endpoint-sec-smart-access/agentsmith-rs/menubar-app`
Expected: PASS

**Step 5: Commit**

No commit in this verification task unless formatting changed files unexpectedly.
