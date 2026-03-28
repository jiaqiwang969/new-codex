# Reclaim CLI Exec Config Dispatch Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the local-only pre-dispatch config eager-load from `codex` for `exec` and `review` so wrapped execution uses `codex-exec`'s native config-loading path and error formatting.

**Architecture:** Keep config loading owned by `codex_exec::run_main()`. The outer `codex` binary should only prepend root CLI overrides and dispatch to `codex-exec`, matching upstream behavior after the legacy endpoint-security daemon hook was removed.

**Tech Stack:** Rust, clap, `assert_cmd`, `codex_utils_cargo_bin`

---

### Task 1: Lock the visible regression in a CLI integration test

**Files:**
- Create: `codex-rs/cli/tests/exec_dispatch.rs`

**Step 1: Write the failing test**

Add an integration test that:
- creates a temporary `CODEX_HOME`
- writes an invalid `config.toml`
- runs `codex exec --skip-git-repo-check hi`
- asserts failure
- expects stderr to include `Error loading config.toml:`

**Step 2: Run test to verify it fails**

Run: `cargo test -p codex-cli exec_wrapper_uses_codex_exec_config_error_format -- --exact`
Expected: FAIL because the current wrapper still performs its own eager config load and emits the generic anyhow-style error instead.

### Task 2: Remove the redundant outer eager-load

**Files:**
- Modify: `codex-rs/cli/src/main.rs`

**Step 1: Write minimal implementation**

Delete the redundant `Config::load_with_cli_overrides_and_harness_overrides(...)` blocks from the `Exec` and `Review` dispatch arms, leaving only:
- remote-mode rejection
- root config flag prepending
- dispatch into `codex_exec::run_main(...)`

**Step 2: Run the focused test to verify it passes**

Run: `cargo test -p codex-cli exec_wrapper_uses_codex_exec_config_error_format -- --exact`
Expected: PASS with wrapped `codex exec` now surfacing `codex-exec`'s formatted config error.

### Task 3: Run scoped verification

**Files:**
- Verify: `codex-rs/cli/src/main.rs`
- Verify: `codex-rs/cli/tests/exec_dispatch.rs`

**Step 1: Lint and format**

Run:
- `just fix -p codex-cli`
- `just fmt`

**Step 2: Run crate tests**

Run: `cargo test -p codex-cli`
Expected: PASS

**Step 3: Run argument comment lint**

Run: `export PATH="$HOME/.local/share/cargo/bin:$PATH" && ./tools/argument-comment-lint/run.sh`
Expected: PASS

**Step 4: Commit**

```bash
git add codex-rs/cli/src/main.rs codex-rs/cli/tests/exec_dispatch.rs docs/plans/2026-03-28-reclaim-cli-exec-config-dispatch.md
git commit -m "chore: reclaim exec config dispatch"
```
