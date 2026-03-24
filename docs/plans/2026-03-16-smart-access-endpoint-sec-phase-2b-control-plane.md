# Obsolete: Smart Access Phase 2B Control Plane Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

> **Status:** Historical and obsolete for current merge work. This plan extends the removed local Smart Access / `endpoint-sec` line and should not be used as active implementation guidance. Follow upstream approvals/guardian semantics instead.

**Goal:** Replace Smart Access's session-local log scraping with a machine-wide security runtime control plane built around leases, permits, action scopes, and structured runtime events.

**Architecture:** Implement Phase 2B in control-plane order. First define the shared runtime contract and event language. Next adapt the current lightweight ES daemon behind that contract so the old delete/move protections keep working. Then bootstrap one machine-wide runtime owner, issue per-session and per-subagent leases, and route tool execution through permit install plus action-scope begin/end calls. Finish by surfacing downgrade and denial reasons in the existing event/TUI path so blocked work is explainable instead of opaque.

**Tech Stack:** Rust workspace in `codex-rs`, existing Smart Access and `Security Host` modules, current internal Endpoint Security daemon, async runtime/bootstrap code in `codex-core`, protocol event types, ratatui TUI snapshots, Cargo, Just.

---

### Task 1: Create an isolated worktree for Phase 2B control-plane work

**Files:**
- No repository files changed yet

**Step 1: Create the feature worktree**

Run: `git worktree add ../new-codex-smart-access-2b -b feature/smart-access-phase2b`
Expected: a new worktree is created from the current `HEAD` without disturbing the dirty main worktree

**Step 2: Verify branch and cleanliness**

Run: `git -C ../new-codex-smart-access-2b status --short --branch`
Expected: `## feature/smart-access-phase2b` and a clean working tree

**Step 3: Open the design references before touching code**

Run: `sed -n '1,260p' ../new-codex-smart-access-2b/docs/plans/2026-03-16-smart-access-endpoint-sec-design.md`
Expected: the approved Smart Access design, including failure handling and acceptance criteria, is visible in the worktree

**Step 4: Commit**

No commit expected for this task.


### Task 2: Define the shared security runtime contract

**Files:**
- Create: `codex-rs/core/src/security_runtime/mod.rs`
- Create: `codex-rs/core/src/security_runtime/tests.rs`
- Modify: `codex-rs/core/src/security_types.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Test: `codex-rs/core/src/security_runtime/tests.rs`
- Test: `codex-rs/core/src/security_types.rs`

**Step 1: Write the failing contract tests**

Add tests that compare complete objects for the new contract types:

- `SecurityLeaseRegistration`
- `SecurityLeaseHandle`
- `SecurityPermitInstallation`
- `SecurityActionScope`
- `SecurityRuntimeEvent`
- `RuntimeReasonCode`
- `RuntimeHealthState`

Example assertions:

```rust
assert_eq!(
    event.reason_code,
    Some(RuntimeReasonCode::ProtectedZoneAiDelete)
);
assert_eq!(lease.parent_lease_id, Some("lease-parent".to_string()));
```

**Step 2: Run the targeted tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core security_runtime security_types`
Expected: compile failures because the runtime contract module and types do not exist yet

**Step 3: Implement the minimal runtime contract**

Add a new `security_runtime` module that defines the contract the rest of Smart Access will program against.

Required API surface:

```rust
#[async_trait]
pub trait SecurityRuntime: Send + Sync {
    async fn register_lease(
        &self,
        registration: SecurityLeaseRegistration,
    ) -> Result<SecurityLeaseHandle>;
    async fn heartbeat_lease(&self, lease_id: &str) -> Result<()>;
    async fn revoke_lease(&self, lease_id: &str) -> Result<()>;
    async fn derive_child_lease(
        &self,
        request: SecurityChildLeaseRequest,
    ) -> Result<SecurityLeaseHandle>;
    async fn install_permits(
        &self,
        request: SecurityPermitInstallation,
    ) -> Result<Vec<InstalledSecurityPermit>>;
    async fn revoke_permit(&self, permit_id: &str) -> Result<()>;
    async fn begin_action_scope(
        &self,
        action: SecurityActionScope,
    ) -> Result<SecurityActionHandle>;
    async fn end_action_scope(&self, action_id: &str) -> Result<()>;
    async fn collect_events(&self, cursor: Option<String>)
        -> Result<SecurityRuntimeEventBatch>;
    async fn get_capability_snapshot(&self) -> Result<SecurityCapabilitySnapshot>;
    async fn get_runtime_health(&self) -> Result<RuntimeHealthState>;
}
```

Keep the first version deliberately small and control-plane focused. Do not add sensitive-read or taint semantics beyond what is needed for typed placeholders.

**Step 4: Re-run the targeted tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core security_runtime security_types`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/security_runtime/mod.rs codex-rs/core/src/security_runtime/tests.rs codex-rs/core/src/security_types.rs codex-rs/core/src/lib.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): add security runtime contract" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 3: Adapt the current ES daemon behind the runtime contract

**Files:**
- Create: `codex-rs/core/src/security_runtime/legacy.rs`
- Modify: `codex-rs/core/src/es_daemon/mod.rs`
- Modify: `codex-rs/core/src/es_daemon/daemon.rs`
- Modify: `codex-rs/core/src/security_runtime/mod.rs`
- Modify: `codex-rs/core/src/security_host/mod.rs`
- Test: `codex-rs/core/src/security_runtime/tests.rs`
- Test: `codex-rs/core/src/security_host/tests.rs`

**Step 1: Write the failing adapter tests**

Add tests for:

- converting a protected delete deny into a typed `SecurityRuntimeEvent`
- converting a protected move-out deny into a typed `SecurityRuntimeEvent`
- mapping runtime deny reason codes onto `SecurityMismatchClassification`
- returning a stable `next_cursor` from event collection

Example assertions:

```rust
assert_eq!(
    batch.events[0].reason_code,
    Some(RuntimeReasonCode::ProtectedZoneAiDelete)
);
assert_eq!(batch.next_cursor, Some("2".to_string()));
```

**Step 2: Run the targeted tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core security_runtime security_host`
Expected: FAIL because the legacy daemon still emits only ad-hoc deny summaries and file-backed override state

**Step 3: Implement `LegacySecurityRuntime`**

Wrap the existing lightweight ES daemon in an adapter that satisfies `SecurityRuntime`.

Required behavior:

- register machine/session leases in memory
- translate legacy protected delete and move denies into structured runtime events
- expose `collect_events(cursor)` without scanning `/tmp/codex-es-daemon.log`
- keep the existing delete and move enforcement behavior unchanged

Use the existing daemon logic as the enforcement seed; do not expand effect coverage in this task.

**Step 4: Re-run the targeted tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core security_runtime security_host`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/security_runtime/legacy.rs codex-rs/core/src/es_daemon/mod.rs codex-rs/core/src/es_daemon/daemon.rs codex-rs/core/src/security_runtime/mod.rs codex-rs/core/src/security_host/mod.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): adapt legacy es daemon to security runtime" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 4: Bootstrap one machine-wide runtime owner and per-session leases

**Files:**
- Create: `codex-rs/core/src/security_runtime/bootstrap.rs`
- Modify: `codex-rs/core/src/state/service.rs`
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/thread_manager.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Create: `codex-rs/core/tests/suite/smart_access_runtime.rs`
- Modify: `codex-rs/core/tests/suite/mod.rs`
- Test: `codex-rs/core/tests/suite/smart_access_runtime.rs`

**Step 1: Write the failing lifecycle tests**

Add integration tests that prove:

- a Smart Access session registers exactly one lease
- two sessions receive different lease IDs
- session shutdown revokes only that session's lease
- runtime bootstrap is reused instead of starting one root-capable runtime per session

Example assertions:

```rust
assert_eq!(runtime.registered_leases().len(), 2);
assert_ne!(lease_a.id, lease_b.id);
assert_eq!(runtime.active_runtime_instances(), 1);
```

**Step 2: Run the lifecycle tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core smart_access_runtime`
Expected: FAIL because session services still store Smart Access runtime state locally and do not bootstrap a shared runtime owner

**Step 3: Implement runtime bootstrap and ownership**

Create a small bootstrap/owner layer that:

- starts or connects to one machine-wide runtime
- stores a shared runtime handle in services
- registers a session lease when a Smart Access session starts
- heartbeats the lease while the session is alive
- revokes the lease during orderly shutdown

Do not mix privileged daemon startup logic into the main session object beyond the bootstrap boundary.

**Step 4: Re-run the lifecycle tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core smart_access_runtime`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/security_runtime/bootstrap.rs codex-rs/core/src/state/service.rs codex-rs/core/src/state/session.rs codex-rs/core/src/thread_manager.rs codex-rs/core/src/codex.rs codex-rs/core/src/lib.rs codex-rs/core/tests/suite/smart_access_runtime.rs codex-rs/core/tests/suite/mod.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): bootstrap shared smart access runtime" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 5: Replace Smart Access log scraping with permit and event flow

**Files:**
- Modify: `codex-rs/core/src/smart_access.rs`
- Modify: `codex-rs/core/src/security_host/mod.rs`
- Modify: `codex-rs/core/src/state/service.rs`
- Modify: `codex-rs/core/tests/suite/approvals.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`
- Modify: `codex-rs/core/tests/suite/smart_access_runtime.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`
- Test: `codex-rs/core/tests/suite/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/smart_access_runtime.rs`

**Step 1: Write the failing end-to-end Smart Access tests**

Add tests for:

- permit installation happens before a low-risk action executes
- runtime event cursor is captured before execution and drained after execution
- a protected deny becomes `TrueRisk`
- a same-effect but out-of-scope deny becomes `Underpredicted`
- snapshot/runtime divergence becomes `PolicyDrift`

Example assertions:

```rust
assert_eq!(decision.classification, SecurityMismatchClassification::Underpredicted);
assert_eq!(runtime.permit_install_count(), 1);
assert!(runtime.collected_events().iter().all(|event| event.action_id.is_some()));
```

**Step 2: Run the targeted Smart Access tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core approvals unified_exec smart_access_runtime`
Expected: FAIL because `smart_access.rs` still persists session-local predicted effects and reads `/tmp/codex-es-daemon.log`

**Step 3: Implement the runtime-driven Smart Access path**

Change `smart_access.rs` so it:

- requests capability snapshot from the runtime
- asks `Security Host` to arbitrate against that snapshot
- installs permits through `SecurityRuntime`
- records a pre-execution event cursor
- collects only the new runtime events for that action after execution
- classifies denials through typed runtime reason codes instead of string matching

Delete the dependency on `smart_access_runtime_contexts` and log-file parsing once the new path is complete.

**Step 4: Re-run the targeted Smart Access tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core approvals unified_exec smart_access_runtime`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/smart_access.rs codex-rs/core/src/security_host/mod.rs codex-rs/core/src/state/service.rs codex-rs/core/tests/suite/approvals.rs codex-rs/core/tests/suite/unified_exec.rs codex-rs/core/tests/suite/smart_access_runtime.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): drive smart access through runtime permits" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 6: Attach action-scope correlation to reviewable tools

**Files:**
- Modify: `codex-rs/core/src/tools/context.rs`
- Modify: `codex-rs/core/src/tools/events.rs`
- Modify: `codex-rs/core/src/tools/runtimes/shell.rs`
- Modify: `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- Modify: `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- Modify: `codex-rs/core/src/tools/handlers/unified_exec.rs`
- Modify: `codex-rs/core/src/mcp_tool_call.rs`
- Modify: `codex-rs/core/tests/suite/apply_patch_cli.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`
- Modify: `codex-rs/core/tests/suite/smart_access_runtime.rs`
- Test: `codex-rs/core/tests/suite/apply_patch_cli.rs`
- Test: `codex-rs/core/tests/suite/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/smart_access_runtime.rs`

**Step 1: Write the failing tool-correlation tests**

Add tests that prove:

- `shell`, `unified_exec`, and `apply_patch` each open an action scope before execution
- the emitted runtime event contains the matching `action_id`
- MCP-driven external effects are tagged with thread, turn, and call identifiers

Example assertions:

```rust
assert_eq!(action.tool_name, Some("unified_exec".to_string()));
assert_eq!(event.action_id.as_deref(), Some(action.id.as_str()));
```

**Step 2: Run the targeted tool tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core apply_patch_cli unified_exec smart_access_runtime`
Expected: FAIL because reviewable tools do not yet begin or end runtime action scopes

**Step 3: Implement action-scope begin/end integration**

Route every reviewable tool through:

1. `begin_action_scope`
2. actual tool execution
3. `end_action_scope`
4. `collect_events(cursor)`

Populate each scope with `lease_id`, `thread_id`, `turn_id`, `call_id`, `tool_name`, and any process identifier the tool runtime already knows.

**Step 4: Re-run the targeted tool tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core apply_patch_cli unified_exec smart_access_runtime`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/tools/context.rs codex-rs/core/src/tools/events.rs codex-rs/core/src/tools/runtimes/shell.rs codex-rs/core/src/tools/runtimes/unified_exec.rs codex-rs/core/src/tools/runtimes/apply_patch.rs codex-rs/core/src/tools/handlers/unified_exec.rs codex-rs/core/src/mcp_tool_call.rs codex-rs/core/tests/suite/apply_patch_cli.rs codex-rs/core/tests/suite/unified_exec.rs codex-rs/core/tests/suite/smart_access_runtime.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): add action scope correlation for smart access" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 7: Derive child leases for subagents and revoke them correctly

**Files:**
- Modify: `codex-rs/core/src/agent/control.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/codex_delegate.rs`
- Modify: `codex-rs/core/src/thread_manager.rs`
- Modify: `codex-rs/core/tests/suite/hierarchical_agents.rs`
- Modify: `codex-rs/core/tests/suite/subagent_notifications.rs`
- Modify: `codex-rs/core/tests/suite/smart_access_runtime.rs`
- Test: `codex-rs/core/tests/suite/hierarchical_agents.rs`
- Test: `codex-rs/core/tests/suite/subagent_notifications.rs`
- Test: `codex-rs/core/tests/suite/smart_access_runtime.rs`

**Step 1: Write the failing subagent-lease tests**

Add tests that prove:

- subagent creation derives a child lease from the parent lease
- child lease scope cannot exceed the parent scope
- child lease TTL cannot exceed parent TTL
- parent shutdown or kill revokes child leases automatically

Example assertions:

```rust
assert_eq!(child_lease.parent_lease_id, Some(parent_lease.id.clone()));
assert!(child_lease.expires_at <= parent_lease.expires_at);
assert_eq!(runtime.active_child_leases(parent_lease.id.as_str()).len(), 0);
```

**Step 2: Run the targeted subagent tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core hierarchical_agents subagent_notifications smart_access_runtime`
Expected: FAIL because subagents are not yet bound to derived Smart Access leases

**Step 3: Implement lease derivation and revocation**

Change the parent/subagent lifecycle so that:

- parent session holds the root session lease
- each subagent gets a derived child lease when created
- child leases are revoked on child completion
- child leases are also revoked when the parent exits or is interrupted

Never allow a subagent to fall back to the parent lease as an implicit default.

**Step 4: Re-run the targeted subagent tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core hierarchical_agents subagent_notifications smart_access_runtime`
Expected: PASS

**Step 5: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/core/src/agent/control.rs codex-rs/core/src/codex.rs codex-rs/core/src/codex_delegate.rs codex-rs/core/src/thread_manager.rs codex-rs/core/tests/suite/hierarchical_agents.rs codex-rs/core/tests/suite/subagent_notifications.rs codex-rs/core/tests/suite/smart_access_runtime.rs
git -C ../new-codex-smart-access-2b commit -m "feat(core): bind subagents to derived smart access leases" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 8: Surface downgrade and denial reasons through protocol and TUI

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/core/src/smart_access.rs`
- Modify: `codex-rs/core/src/tools/handlers/request_security_override.rs`
- Modify: `codex-rs/tui/src/status/card.rs`
- Modify: `codex-rs/tui/src/status/tests.rs`
- Test: `codex-rs/core/tests/suite/permissions_messages.rs`
- Test: `codex-rs/tui/src/status/tests.rs`

**Step 1: Write the failing observability tests**

Add coverage that proves:

- `DowngradeToDefault` is surfaced as a first-class status instead of a generic failure
- the latest mismatch classification is shown as `TrueRisk`, `Underpredicted`, or `PolicyDrift`
- the user sees why Smart Access stopped auto-issuing permits

Example assertions:

```rust
assert_eq!(status.security_mode, "default");
assert_eq!(status.last_mismatch, Some("policy_drift".to_string()));
```

**Step 2: Run the targeted protocol/TUI tests and confirm they fail**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core permissions_messages`
Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-tui status`
Expected: FAIL because the downgrade and mismatch states are not yet exposed clearly to the UI

**Step 3: Implement the minimal observability path**

Expose runtime-driven Smart Access state through the protocol and render it in the status surface:

- current mode
- current runtime health
- last permit decision
- last mismatch classification
- last downgrade rationale

Keep the first UI slice intentionally small. A full searchable security timeline can land in the next phase.

**Step 4: Re-run the targeted protocol/TUI tests**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core permissions_messages`
Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-tui status`
Expected: PASS

**Step 5: Accept any intended TUI snapshot changes**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo insta pending-snapshots -p codex-tui`
Expected: pending snapshot list that matches the intentional status-card changes

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo insta accept -p codex-tui`
Expected: updated snapshots are accepted into the worktree

**Step 6: Commit**

```bash
git -C ../new-codex-smart-access-2b add codex-rs/protocol/src/protocol.rs codex-rs/core/src/smart_access.rs codex-rs/core/src/tools/handlers/request_security_override.rs codex-rs/tui/src/status/card.rs codex-rs/tui/src/status/tests.rs
git -C ../new-codex-smart-access-2b commit -m "feat(tui): show smart access runtime state" -m "Co-authored-by: Codex <noreply@openai.com>"
```


### Task 9: Run formatting, linting, and the required verification sweep

**Files:**
- Modify: any files touched in Tasks 2-8
- Test: `codex-rs/core/src/security_runtime/tests.rs`
- Test: `codex-rs/core/tests/suite/smart_access_runtime.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`
- Test: `codex-rs/core/tests/suite/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/apply_patch_cli.rs`
- Test: `codex-rs/core/tests/suite/hierarchical_agents.rs`
- Test: `codex-rs/core/tests/suite/subagent_notifications.rs`
- Test: `codex-rs/core/tests/suite/permissions_messages.rs`
- Test: `codex-rs/tui/src/status/tests.rs`

**Step 1: Run the targeted core verification set**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-core security_runtime smart_access_runtime approvals unified_exec apply_patch_cli hierarchical_agents subagent_notifications permissions_messages`
Expected: PASS

**Step 2: Run the targeted TUI verification set**

Run in `../new-codex-smart-access-2b/codex-rs`: `cargo test -p codex-tui status`
Expected: PASS

**Step 3: Run formatting**

Run in `../new-codex-smart-access-2b/codex-rs`: `just fmt`
Expected: Rust formatting updates, if any

**Step 4: Run scoped lint fixes**

Run in `../new-codex-smart-access-2b/codex-rs`: `just fix -p codex-core`
Expected: clippy-driven cleanup across the touched core crates

Run in `../new-codex-smart-access-2b/codex-rs`: `just fix -p codex-tui`
Expected: clippy-driven cleanup across the touched TUI crate

Do not re-run tests after `just fmt` or `just fix`.

**Step 5: Ask before the full workspace test**

Ask the user whether to run the full workspace suite because `codex-core` changed shared runtime behavior.

If approved, run in `../new-codex-smart-access-2b/codex-rs`: `cargo test`
Expected: PASS across the workspace

**Step 6: Commit**

```bash
git -C ../new-codex-smart-access-2b add .
git -C ../new-codex-smart-access-2b commit -m "chore: finalize smart access phase 2b control plane" -m "Co-authored-by: Codex <noreply@openai.com>"
```
