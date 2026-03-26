# Upstream Main `c9214192c` Conflict Preview

**Date:** 2026-03-26
**Branch:** `probe/upstream-merge-9dbe09834`
**Current HEAD:** `0fa92816c03680c2ed49178e7763115ee6fe4700`
**Latest upstream/main:** `c9214192c52aef31758088b5e87e971fc57a0478`
**Merge base:** `047ea642d2989f4095a6dc5070aaa818554e550e`

## Summary

After refreshing `upstream/main`, a dry-run merge preview using:

```bash
git merge-tree --write-tree --name-only --messages HEAD upstream/main
```

shows:

- local branch is `200` commits ahead of upstream
- upstream is `42` commits ahead of local HEAD
- the `HEAD` vs latest upstream merge shape produces `14` content conflicts

That is better than the repository-wide dirty status suggests.

The most important positive signal is that several core hotspot files already
auto-merge against latest upstream:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui_app_server/src/chatwidget.rs`

So the current merge is not blocked by the worst possible scenario where all
core architecture anchors collide at once.

## Important Caveat: Dirty Worktree Overlap

This preview compares committed `HEAD` against latest upstream. It does not
directly merge the current uncommitted worktree state.

There are `9` overlap files that are both:

- predicted content conflicts in the dry-run merge, and
- already modified in the current dirty worktree

Those are:

- `codex-rs/core-skills/src/manager_tests.rs`
- `codex-rs/core/src/config/config_tests.rs`
- `codex-rs/core/src/contextual_user_message.rs`
- `codex-rs/core/src/mcp_tool_call.rs`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/core/src/tools/handlers/mod.rs`
- `codex-rs/core/src/unified_exec/process_manager.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/status/card.rs`

These nine files are the true high-risk set for the actual merge operation,
because they carry both:

- upstream divergence, and
- in-progress local worktree state

## Conflict Inventory

### 1. Config tests and config docs

Files:

- `codex-rs/core/src/config/config_tests.rs`
- `docs/config.md`

Local intent:

- preserve branch-specific config semantics, especially provider/account-pool
  and post-smart-access cleanup expectations

Upstream intent:

- document and test newer official config/runtime behavior such as MCP
  elicitation and recent sandbox/runtime adjustments

Why this is encouraging:

- the actual config runtime file `codex-rs/core/src/config/mod.rs` auto-merges
- the conflict landed in tests/docs, not in the primary config engine

Practical merge rule:

- preserve local account-pool/custom-endpoint semantics
- update tests/docs to reflect the merged runtime, not the other way around

### 2. Agent lifecycle and worktree-adjacent handlers

Files:

- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/close_agent.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`

Local intent:

- preserve local agent lifecycle integrations accumulated across earlier merge
  passes
- keep the branch's agent-worktree and resume/recovery line intact

Upstream intent:

- continue cleaning the spawn v1 lifecycle
- integrate newer session/environment-manager behavior

Practical merge rule:

- keep upstream collaboration lifecycle structure
- preserve local worktree lease/recovery semantics where they are still live
- resolve these together with Block 6, not as isolated file edits

### 3. Unified exec and handler-surface churn

Files:

- `codex-rs/core/src/tools/handlers/mod.rs`
- `codex-rs/core/src/unified_exec/async_watcher.rs`
- `codex-rs/core/src/unified_exec/process_manager.rs`

Local intent:

- keep the current branch's tool-handler layout after removing the failed
  security/freeze line
- preserve existing unified-exec behavior and event ordering

Upstream intent:

- prepare exec-server integration for unified exec
- move process tracking toward `ProcessId`
- remove deprecated handler surface such as artifact/read_file/grep_files

Why this conflict exists:

- both sides are simplifying the runtime surface, but in different directions
- upstream is doing active platform refactoring while local branch already
  removed a different set of legacy paths

Practical merge rule:

- accept upstream's current handler/exec architecture as the baseline
- re-express local surviving semantics on top of that structure
- do not resurrect removed local security or freeze paths while resolving this

### 4. Context, hooks, and MCP glue

Files:

- `codex-rs/core/src/contextual_user_message.rs`
- `codex-rs/core/src/mcp_tool_call.rs`
- `codex-rs/hooks/src/lib.rs`

Local intent:

- keep branch-specific context, memory, and model-selection plumbing
- keep local hook payload extensions that support the fork's workflow layer

Upstream intent:

- continue modularizing instructions
- improve custom MCP elicitation
- add richer hook delivery modes such as non-streaming shell-only `PostToolUse`

Practical merge rule:

- prefer upstream's newer public hook/MCP structure
- preserve only the local continuity/context semantics that still matter
- avoid treating older local payload layout as canonical

### 5. TUI shell and status surfaces

Files:

- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/status/card.rs`

Local intent:

- keep local workbench/UI features such as session-bar glue and model-sub
  follow-on UX
- keep smart-access cleanup already performed on this branch

Upstream intent:

- continue TUI shell refactors such as cwd/path handling and non-interactive
  resume filtering

Why this is manageable:

- `codex-rs/tui/src/chatwidget.rs` already auto-merges
- that means the main TUI collision zone is narrower than it looked earlier

Practical merge rule:

- preserve upstream shell structure in `app.rs`
- reattach local TUI workbench and Block 3 follow-on UX selectively
- keep status-card additions only when they still map cleanly onto merged
  runtime data

### 6. Mechanical test fallout

File:

- `codex-rs/core-skills/src/manager_tests.rs`

Local intent:

- none specific in this preview beyond in-progress worktree state

Upstream intent:

- continued fallout from the `codex-core-skills` extraction

Practical merge rule:

- treat this as mechanical follow-up, not as a product decision

## What Changed Since The Earlier Audit Baseline

Refreshing upstream from `9dbe0983490c2f952b8791e634b5ec3ce94edee9` to
`c9214192c52aef31758088b5e87e971fc57a0478` adds new official pressure in these
areas:

- exec-server / unified-exec refactor
- websocket auth in app-server
- removal of artifact/read_file/grep_files handlers
- MCP elicitation improvements
- plugin / instructions crate extraction follow-through
- TUI resume and path handling updates

Those are now the main official changes the merge must absorb.

## Recommended Next Move

The research phase is now detailed enough to start the real merge, but the
execution order should be strict:

1. merge `upstream/main` only after treating the nine dirty-overlap files as
   first-class conflict hotspots
2. resolve in playbook block order, not filename order
3. stop for discussion only on the remaining product-decision buckets:
   - keep/defer `team_profile` + vouch UX
   - keep/drop `git-graph`
   - how much effort to spend reattaching `ralph-loop`

That keeps the merge discussion focused on real product choices instead of
re-litigating already-audited architecture.
