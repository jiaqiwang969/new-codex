# Permission Mainline Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align the repository with upstream's split permission/sandbox model while preserving and improving custom collaboration, memory, Entire hooks, account-pool, and provider-management behavior.

**Architecture:** Implement the permission-mainline work in three controlled waves: (1) config/protocol semantics, (2) runtime sandbox plumbing, and (3) managed network allowlist integration. Keep custom features authoritative where they intersect with collaboration, memory, hooks, and pooled accounts, while converging everything else toward upstream semantics. Each wave must ship with focused red/green tests before broader validation.

**Tech Stack:** Rust workspace (`codex-core`, `codex-protocol`, `codex-app-server`, `codex-network-proxy`, `codex-tui`), Cargo tests, `just fix`, `just fmt`, config schema generation when needed.

---

### Task 1: Freeze the permission baseline and protect custom touchpoints

**Files:**
- Modify: `docs/plans/2026-03-07-permission-mainline-implementation.md`
- Inspect: `codex-rs/core/src/config/mod.rs`
- Inspect: `codex-rs/core/src/config/permissions.rs`
- Inspect: `codex-rs/protocol/src/permissions.rs`
- Inspect: `codex-rs/core/src/tools/spec.rs`
- Inspect: `codex-rs/core/src/memories/phase1.rs`
- Inspect: `codex-rs/app-server/src/codex_message_processor.rs`

**Step 1: Write the failing test**

Create or extend focused tests that lock in custom-critical invariants before permission refactors:

```rust
#[test]
fn collab_tools_remain_available_under_permission_profile_changes() {
    // Build tools config with collab enabled and assert agent tools remain present.
}

#[tokio::test]
async fn memory_startup_still_runs_under_split_permission_projection() {
    // Load config / runtime and assert memory startup path is not blocked.
}
```

**Step 2: Run test to verify it fails**

Run: `cd codex-rs && cargo test -p codex-core collab_tools_remain_available_under_permission_profile_changes -- --nocapture`
Expected: FAIL if permission refactor breaks collaboration tools or the new test is not yet wired.

Run: `cd codex-rs && cargo test -p codex-core memory_startup_still_runs_under_split_permission_projection -- --nocapture`
Expected: FAIL if split-policy handling breaks memory startup assumptions.

**Step 3: Write minimal implementation**

Add only the minimum assertions/helpers needed to codify the current intended custom behavior, without changing production semantics yet.

**Step 4: Run test to verify it passes**

Run: `cd codex-rs && cargo test -p codex-core collab_tools_remain_available_under_permission_profile_changes -- --nocapture`
Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-core memory_startup_still_runs_under_split_permission_projection -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-07-permission-mainline-implementation.md codex-rs/core/src/...
git commit -m "test: lock custom permission invariants"
```

### Task 2: Align config.toml permission profile semantics

**Files:**
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/permissions.rs`
- Modify: `codex-rs/protocol/src/permissions.rs`
- Modify: `codex-rs/protocol/src/protocol.rs`
- Test: `codex-rs/core/src/config/config_tests.rs`
- Test: `codex-rs/core/tests/suite/approvals.rs`
- Generate if needed: `codex-rs/core/config.schema.json`

**Step 1: Write the failing test**

Add tests for the new permission-profile language and effective projection behavior:

```rust
#[test]
fn config_permission_profile_projects_split_fs_and_network_policies() {
    // Parse config.toml profile and assert filesystem + network policies match.
}

#[test]
fn legacy_sandbox_projection_remains_compatible_for_custom_runtime_paths() {
    // Assert legacy SandboxPolicy projection still matches expected custom behavior.
}
```

**Step 2: Run test to verify it fails**

Run: `cd codex-rs && cargo test -p codex-core config_permission_profile_projects_split_fs_and_network_policies -- --nocapture`
Expected: FAIL because parsing/projection is incomplete.

Run: `cd codex-rs && cargo test -p codex-core legacy_sandbox_projection_remains_compatible_for_custom_runtime_paths -- --nocapture`
Expected: FAIL if legacy projection is still missing compatibility behavior.

**Step 3: Write minimal implementation**

Port the upstream semantics from `f82678b2a` and `b52c18e41`, but preserve custom behavior by:
- keeping collaboration, memory, hook, and account-pool callsites on the compatibility projection until runtime support is complete;
- using split-policy types as the source of truth;
- only deriving legacy `SandboxPolicy` as a projection.

If `ConfigToml` or nested config types change, run `cd codex-rs && just write-config-schema`.

**Step 4: Run test to verify it passes**

Run: `cd codex-rs && cargo test -p codex-core config_permission_profile_projects_split_fs_and_network_policies -- --nocapture`
Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-core legacy_sandbox_projection_remains_compatible_for_custom_runtime_paths -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add codex-rs/core/src/config/mod.rs codex-rs/core/src/config/permissions.rs codex-rs/protocol/src/permissions.rs codex-rs/protocol/src/protocol.rs codex-rs/core/src/config/config_tests.rs codex-rs/core/tests/suite/approvals.rs codex-rs/core/config.schema.json
git commit -m "config: align permission profile semantics"
```

### Task 3: Plumb split sandbox policies through runtime execution

**Files:**
- Modify: `codex-rs/core/src/exec.rs`
- Modify: `codex-rs/core/src/sandboxing/mod.rs`
- Modify: `codex-rs/core/src/spawn.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/tools/sandboxing.rs`
- Modify: `codex-rs/core/src/tools/js_repl/mod.rs`
- Modify: `codex-rs/core/src/tasks/user_shell.rs`
- Modify: `codex-rs/app-server/src/command_exec.rs`
- Test: `codex-rs/core/tests/suite/exec.rs`
- Test: `codex-rs/core/tests/suite/user_shell_cmd.rs`
- Test: `codex-rs/core/src/codex_tests.rs`

**Step 1: Write the failing test**

Add runtime tests that prove the execution layer consumes split filesystem/network policies directly:

```rust
#[tokio::test]
async fn split_policies_drive_exec_sandbox_selection() {
    // Assert runtime chooses the correct enforcement backend from fs+network inputs.
}

#[tokio::test]
async fn user_shell_preserves_custom_hook_and_collab_paths_under_split_policies() {
    // Assert Entire hooks / custom shell flow remain executable under aligned policies.
}
```

**Step 2: Run test to verify it fails**

Run: `cd codex-rs && cargo test -p codex-core split_policies_drive_exec_sandbox_selection -- --nocapture`
Expected: FAIL because runtime still depends on legacy behavior in some path.

Run: `cd codex-rs && cargo test -p codex-core user_shell_preserves_custom_hook_and_collab_paths_under_split_policies -- --nocapture`
Expected: FAIL if custom hook/collab behavior is not preserved.

**Step 3: Write minimal implementation**

Port the runtime pieces from `22ac6b9aa`, but explicitly preserve custom behavior by:
- keeping collaboration tools and agent jobs available;
- allowing memory-maintenance writable roots to survive projection changes;
- preserving hook/Entire-related shell paths and any custom provider/account-pool launch flows.

**Step 4: Run test to verify it passes**

Run: `cd codex-rs && cargo test -p codex-core split_policies_drive_exec_sandbox_selection -- --nocapture`
Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-core user_shell_preserves_custom_hook_and_collab_paths_under_split_policies -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add codex-rs/core/src/exec.rs codex-rs/core/src/sandboxing/mod.rs codex-rs/core/src/spawn.rs codex-rs/core/src/codex.rs codex-rs/core/src/tools/sandboxing.rs codex-rs/core/src/tools/js_repl/mod.rs codex-rs/core/src/tasks/user_shell.rs codex-rs/app-server/src/command_exec.rs codex-rs/core/tests/suite/exec.rs codex-rs/core/tests/suite/user_shell_cmd.rs codex-rs/core/src/codex_tests.rs
git commit -m "sandboxing: plumb split policies through runtime"
```

### Task 4: Integrate managed network allowlist controls

**Files:**
- Modify: `codex-rs/core/src/config/network_proxy_spec.rs`
- Modify: `codex-rs/network-proxy/src/runtime.rs`
- Modify: `codex-rs/network-proxy/src/state.rs`
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/app-server/src/config_api.rs`
- Modify: `codex-rs/tui/src/debug_config.rs`
- Test: `codex-rs/network-proxy/README.md`
- Test: `codex-rs/app-server/tests/suite/v2/config_rpc.rs`

**Step 1: Write the failing test**

Add tests for managed allowlist behavior and config visibility:

```rust
#[test]
fn managed_network_allowlist_controls_round_trip_through_config() {
    // Assert config read/write preserves managed allowlist semantics.
}

#[tokio::test]
async fn managed_network_allowlist_applies_without_breaking_custom_provider_routing() {
    // Assert proxy/runtime policy honors allowlists while preserving custom provider/account-pool paths.
}
```

**Step 2: Run test to verify it fails**

Run: `cd codex-rs && cargo test -p codex-app-server managed_network_allowlist_controls_round_trip_through_config -- --nocapture`
Expected: FAIL until config/API layers are aligned.

Run: `cd codex-rs && cargo test -p codex-network-proxy managed_network_allowlist_applies_without_breaking_custom_provider_routing -- --nocapture`
Expected: FAIL until runtime/state logic is aligned.

**Step 3: Write minimal implementation**

Port the targeted logic from `25fa97416`, preserving any custom provider/account-pool and managed proxy behavior already present in this fork.

Update `codex-rs/network-proxy/README.md` if user-visible behavior changes.

**Step 4: Run test to verify it passes**

Run: `cd codex-rs && cargo test -p codex-app-server managed_network_allowlist_controls_round_trip_through_config -- --nocapture`
Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-network-proxy managed_network_allowlist_applies_without_breaking_custom_provider_routing -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add codex-rs/core/src/config/network_proxy_spec.rs codex-rs/network-proxy/src/runtime.rs codex-rs/network-proxy/src/state.rs codex-rs/core/src/config/mod.rs codex-rs/app-server/src/config_api.rs codex-rs/tui/src/debug_config.rs codex-rs/app-server/tests/suite/v2/config_rpc.rs codex-rs/network-proxy/README.md
git commit -m "fix: support managed network allowlist controls"
```

### Task 5: Perform wave-level verification and handoff

**Files:**
- Modify: `docs/plans/2026-03-07-permission-mainline-implementation.md`
- Inspect: `codex-rs/core/src/tools/spec.rs`
- Inspect: `codex-rs/core/src/memories/phase1.rs`
- Inspect: `codex-rs/app-server/src/codex_message_processor.rs`

**Step 1: Write the failing test**

Use the wave-complete verification command suite as the acceptance gate. If any command fails, treat that as the failing test for the wave.

**Step 2: Run test to verify it fails**

Run the smallest failing command among the affected crates first if a regression is suspected.
Expected: any break in collaboration, memory, config, or runtime enforcement blocks completion.

**Step 3: Write minimal implementation**

Only fix regressions introduced by Tasks 1-4. Do not broaden scope.

**Step 4: Run test to verify it passes**

Run, in order:
- `cd codex-rs && cargo test -p codex-core`
- `cd codex-rs && cargo test -p codex-app-server --lib -- --nocapture`
- `cd codex-rs && cargo test -p codex-network-proxy -- --nocapture`
- `cd codex-rs && cargo test -p codex-protocol -- --nocapture`
- `cd codex-rs && just fix -p codex-core`
- `cd codex-rs && just fix -p codex-app-server`
- `cd codex-rs && just fix -p codex-network-proxy`
- `cd codex-rs && just fix -p codex-protocol`
- `cd codex-rs && just fmt`

Only ask before running workspace-wide `cargo test` beyond the affected crates.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-07-permission-mainline-implementation.md
# plus any final fixes
git commit -m "docs: record permission mainline verification"
```
