# Agent Worktree Merge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve the live local agent-worktree behavior while aligning the implementation shape with `upstream/main` and trimming stale claims about sub-agent isolation that are not actually wired yet.

**Architecture:** Keep the feature narrowly scoped around a dedicated `agent_worktree` module plus three entry points: TUI fork, thread resume, and debug CLI recovery. Treat fork-session isolation, lease persistence, resume-time restore, and `codex debug agent-worktrees` as the real contract. Do not expand this slice into spawned-agent runtime isolation unless the code already proves that path is live.

**Tech Stack:** Rust, Cargo tests, `just fmt`, `just fix`, argument-comment lint, Markdown docs

---

### Task 1: Lock down the real agent-worktree contract in core tests

**Files:**
- Modify: `codex-rs/core/src/agent_worktree.rs`
- Modify: `codex-rs/core/src/thread_manager_tests.rs`

**Step 1: Add or tighten `agent_worktree` unit tests around lease semantics**

Keep or add tests proving:

- `build_lease()` preserves `thread_id`, `parent_thread_id`, `purpose`, and paths
- `write_lease()` followed by `read_lease()` round-trips the full lease object
- `list_leases()` ignores non-JSON files and returns sorted-equivalent lease sets
- `ensure_worktree_for_thread()` returns `None` cleanly when no trusted git repo or no lease exists

Target test names:

- `lease_roundtrip_json`
- `write_and_read_lease_roundtrip`
- `list_leases_ignores_non_json_files`
- `ensure_worktree_for_thread_returns_none_without_lease`

**Step 2: Run the targeted core tests first**

Run: `cargo test -p codex-core lease_roundtrip_json write_and_read_lease_roundtrip list_leases_ignores_non_json_files ensure_worktree_for_thread_returns_none_without_lease -- --exact`

Expected: PASS, or fail only because the new coverage is not implemented yet.

**Step 3: Add resume-path coverage in `thread_manager_tests.rs`**

Write tests proving:

- resumed threads switch `config.cwd` to the leased worktree path when restore succeeds
- resume keeps the original `cwd` when lease restore fails or no lease exists

Prefer extracting a small helper for the pre-spawn cwd rewrite if that makes the tests cheap and keeps `thread_manager.rs` readable.

Target test names:

- `resumed_thread_uses_leased_worktree_cwd_when_available`
- `resumed_thread_keeps_original_cwd_when_no_worktree_lease_exists`

**Step 4: Run the targeted thread-manager tests**

Run: `cargo test -p codex-core resumed_thread_uses_leased_worktree_cwd_when_available resumed_thread_keeps_original_cwd_when_no_worktree_lease_exists -- --exact`

Expected: PASS.

### Task 2: Keep runtime wiring minimal and upstream-shaped

**Files:**
- Modify: `codex-rs/core/src/thread_manager.rs`
- Modify if needed: `codex-rs/core/src/lib.rs`
- Modify if needed: `codex-rs/core/src/agent_worktree.rs`

**Step 1: Keep resume-time worktree restore isolated to a narrow helper**

If `thread_manager.rs` needs cleanup, keep the worktree-specific behavior behind a helper such as:

```rust
async fn restore_resumed_thread_worktree(
    cwd: &AbsolutePathBuf,
    initial_history: &InitialHistory,
) -> anyhow::Result<Option<AbsolutePathBuf>>
```

The helper should:

- only run for `InitialHistory::Resumed`
- read the lease from the trusted repo rooted at the current `cwd`
- call `ensure_worktree_for_thread()`
- return the restored worktree path when present
- leave the caller on the original `cwd` when nothing is leased or restore fails

**Step 2: Preserve the current runtime boundary**

Keep the `agent_worktree` contract narrow:

- `agent_worktree.rs` owns git worktree creation/removal/lease IO/restore
- `thread_manager.rs` only asks for a restored `cwd`
- do not thread agent-worktree state through protocol or app-server wire types

**Step 3: Re-run the targeted core tests**

Run: `cargo test -p codex-core agent_worktree resumed_thread_uses_leased_worktree_cwd_when_available resumed_thread_keeps_original_cwd_when_no_worktree_lease_exists -- --nocapture`

Expected: PASS.

### Task 3: Preserve TUI fork-session isolation without overreaching into sub-agents

**Files:**
- Modify: `codex-rs/tui/src/app.rs`
- Modify if needed: `codex-rs/tui/src/app_event.rs`
- Add/modify tests if feasible in: `codex-rs/tui/src/app.rs` or an extracted module

**Step 1: Keep fork flow scoped to live behavior**

The fork path should keep exactly these semantics:

- if `Feature::AgentWorktrees` is enabled, create a `ForkedSession` worktree before `fork_thread()`
- rewrite `config.cwd` to the new worktree path before spawning the fork
- on fork failure, remove the just-created worktree
- on fork success, write the lease for the new thread id

Do not expand this task into spawned-agent worktree creation.

**Step 2: If `app.rs` needs cleanup, extract a small helper instead of growing the file**

Example shape:

```rust
async fn prepare_fork_worktree(config: &mut Config) -> color_eyre::Result<Option<AgentWorktree>>
```

Keep the helper focused on:

- feature-flag gating
- worktree creation
- `cwd` rewrite
- cleanup on failure

**Step 3: Add targeted coverage if a clean seam exists**

Preferred test target:

- a small helper test proving the feature flag gates worktree creation and `cwd` rewrite

If no clean seam exists without large churn, document that TUI fork wiring is covered by the core worktree tests plus manual verification instead of forcing invasive test scaffolding into `app.rs`.

**Step 4: Run the TUI-targeted verification that matches the final seam**

If helper tests were added:

Run: `cargo test -p codex-tui prepare_fork_worktree -- --nocapture`

Otherwise:

Run: `cargo test -p codex-tui should_wait_for_initial_session -- --nocapture`

Expected: PASS.

### Task 4: Keep the debug CLI recovery path and correct stale docs

**Files:**
- Modify: `codex-rs/cli/src/main.rs`
- Modify: `codex-rs/features/src/lib.rs`
- Modify: `codex-rs/README.md`
- Modify: `codex-rs/config-examples/README.md`
- Modify if needed: `codex-rs/config-examples/config.toml`
- Modify if needed: `codex-rs/docs/design/claude-mcp-context-memory.tex`

**Step 1: Keep only the narrow debug CLI contract for worktrees**

Preserve:

- `codex debug agent-worktrees list`
- `codex debug agent-worktrees ensure --thread <SESSION_ID>`
- `codex debug agent-worktrees ensure --all`

If the current CLI code is tangled with unrelated local debug features, extract the worktree-specific helpers without changing their user-visible behavior.

**Step 2: Fix stale user-facing descriptions**

Update docs and feature text so they describe the real current contract:

- fork-session isolation
- lease persistence
- resume/restore of the correct worktree
- debug recovery commands

Do not keep saying “sub-agents already run in isolated worktrees” unless runtime code is actually wired for that path by the time this slice ends.

**Step 3: Run targeted CLI verification**

If unit tests are added:

Run: `cargo test -p codex-cli agent_worktrees -- --nocapture`

Also run:

Run: `cargo run -p codex-cli -- debug agent-worktrees list --help`

Expected: command help prints successfully.

### Task 5: Format, lint, and verify the full slice locally

**Files:**
- Verify touched files under:
  - `codex-rs/core/**`
  - `codex-rs/tui/**`
  - `codex-rs/cli/**`
  - `codex-rs/features/**`
  - `codex-rs/README.md`
  - `codex-rs/config-examples/**`

**Step 1: Run formatting**

Run: `cd codex-rs && just fmt`

Expected: PASS.

**Step 2: Run crate-scoped tests**

Run: `cd codex-rs && cargo test -p codex-core`

Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-cli`

Expected: PASS.

Run: `cd codex-rs && cargo test -p codex-tui`

Expected: PASS if TUI code changed.

**Step 3: Run crate-scoped lint fixes**

Run: `cd codex-rs && just fix -p codex-core`

Expected: PASS.

Run: `cd codex-rs && just fix -p codex-cli`

Expected: PASS if CLI Rust changed.

Run: `cd codex-rs && just fix -p codex-tui`

Expected: PASS if TUI Rust changed.

**Step 4: Run argument comment lint**

Run: `cd /Users/jqwang/.config/superpowers/worktrees/new-codex/probe-upstream-9dbe09834 && PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`

Expected: PASS.

**Step 5: Optional workspace verification**

Only after user approval:

Run: `cd codex-rs && cargo test`

Expected: PASS.

**Step 6: Commit**

```bash
git add docs/plans/2026-03-26-agent-worktree-analysis.md \
        docs/plans/2026-03-26-agent-worktree-merge-implementation.md \
        codex-rs/core/src/agent_worktree.rs \
        codex-rs/core/src/thread_manager.rs \
        codex-rs/core/src/thread_manager_tests.rs \
        codex-rs/tui/src/app.rs \
        codex-rs/cli/src/main.rs \
        codex-rs/features/src/lib.rs \
        codex-rs/README.md \
        codex-rs/config-examples/README.md \
        codex-rs/config-examples/config.toml
git commit -m "refactor: narrow agent worktree merge surface"
```
