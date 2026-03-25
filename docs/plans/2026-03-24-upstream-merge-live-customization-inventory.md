# Current Upstream Merge Live Customization Inventory

**Date:** 2026-03-24
**Current branch head:** `17922658f` (`chore: remove legacy endpoint security and freeze flows`)
**Compared against:** `upstream/main` at `f9545278e`

> **Status:** Current working inventory for the ongoing upstream merge cleanup.
> This supersedes older assumptions that the local Smart Access / `endpoint-sec` /
> `/freeze` line was still part of the target architecture.

## Companion Notes

Detailed follow-up analysis for the main preserved blocks lives in:

- `docs/plans/2026-03-24-upstream-merge-account-pool-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-execution-playbook.md`
- `docs/plans/2026-03-24-upstream-merge-guardian-model-sub-app-server-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-memory-entire-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-tui-workbench-analysis.md`

## Confirmed Removed From Runtime

The current runtime code no longer contains live references to:

- `smart_access`
- `request_security_override`
- `endpoint-sec`
- `SecurityHost`
- `/freeze`

That means the failed local Smart Access + Endpoint Security line has already been
removed from runnable code. Remaining mentions are historical docs only.

## Docs Cleaned In This Round

To prevent stale guidance from confusing future merge work:

- `README.md`
  - removed the user-facing `/freeze` feature pitch
  - added a short note that old `/freeze`, `endpoint-sec`, and local Smart Access
    are no longer maintained
- `docs/reports/2026-03-15-upstream-gap-report.tex`
  - added an updated status note
  - re-labeled Endpoint Security and `/freeze` sections as historical

## Preserve: Local-Only Capabilities Still Alive

These are currently active, local-only capabilities that should be treated as
real customization assets unless intentionally dropped.

### 1. Account Pool / Provider Routing

**Keep.**

User requirement: keep account-pool / `config-pool` behavior and do not revert
local provider endpoints such as `https://code.ppchat.vip`.

Representative local-only / heavily customized files:

- `codex-rs/config-examples/config-pool.toml`
- `codex-rs/config-examples/auth-pool.json`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/auth.rs`
- `codex-rs/core/src/model_provider_info.rs`
- `codex-rs/core/src/config/mod.rs`

Why this will keep conflicting with upstream:

- provider selection and preset handling continue to change upstream
- this fork adds pool rotation, failover, custom base URLs, and provider remapping
- `config/mod.rs` and `client.rs` are both heavy conflict magnets

### 2. Memory / Context Packet / Entire Integration

**Likely keep, but migrate carefully.**

Representative local-only files:

- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/core/src/context_packet.rs`
- `codex-rs/core/src/entire_integration.rs`

Why this will keep conflicting with upstream:

- memory and app-server wire shapes are still evolving upstream
- this fork pushes memory deeper into hooks, MCP, summaries, and session state

### 3. Agent Worktrees

**Keep.**

Representative local-only file:

- `codex-rs/core/src/agent_worktree.rs`

Why it matters:

- this is one of the clearest differentiated workflow features
- it is conceptually narrow and easier to preserve than the old security line

### 4. Multi-Agent Extensions

**Keep selectively.**

Representative local-only files:

- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/model_sub_vouch.rs`
- `codex-rs/tui/src/team_profile.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`

Why this needs caution:

- these files are local-only, but they overlap with upstream collaboration work
- keep the differentiated workflow value, but do not assume the local wire
  contract should remain unchanged

### 5. TUI Workbench Features

**Keep.**

Representative local-only files:

- `codex-rs/git-graph/**`
- `codex-rs/tui/src/git_graph_widget.rs`
- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/ralph_loop.rs`

Why these are relatively safe:

- `git-graph` is self-contained
- the main risk is integration drift in `tui/src/app.rs` and `tui/src/chatwidget.rs`

## Reassess: Local-Only Features That Should Not Drive Architecture

### Guardian Approvals Surface

Current recommendation:

- do not treat the guardian implementation as a local architecture anchor
- the target direction remains upstream `approval_policy` +
  `approvals_reviewer` semantics
- the remaining local decisions in this area now mostly live around
  `model_sub`, provider/account-pool-aware routing, and app-server memory /
  collab metadata rather than guardian itself

## Highest Ongoing Conflict Magnets

These files are both active and heavily diverged from upstream:

- `codex-rs/tui/src/chatwidget.rs`
  - `3816` insertions, `1105` deletions vs `upstream/main`
- `codex-rs/core/src/config/mod.rs`
  - `1193` insertions, `121` deletions
- `codex-rs/core/src/client.rs`
  - `1075` insertions, `15` deletions
- `codex-rs/tui/src/app.rs`
  - `992` insertions, `26` deletions
- `codex-rs/app-server/src/bespoke_event_handling.rs`
  - `393` insertions, `7` deletions
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
  - `255` insertions, `41` deletions

Interpretation:

- `chatwidget.rs` and `app.rs` are where many separate customizations pile up
- `config/mod.rs`, `client.rs`, and provider metadata are the account-pool core
- app-server and protocol files are where local memory/collab/approval wire
  drift will continue to surface

## Practical Merge Order From Here

1. Keep the security-line rollback intact.
2. Preserve account-pool / provider routing.
3. Preserve memory / context packet / Entire only after provider plumbing is stable.
4. Preserve agent worktrees as an isolated workflow feature.
5. Reattach TUI workbench features (`git-graph`, `session bar`, `ralph-loop`)
   on top of the newer upstream TUI surface.
6. Re-evaluate guardian / model-sub / app-server wire customizations last,
   because those have the highest upstream overlap and the most contract drift.

## Summary Judgment

At this point, the failed local security line is no longer the main merge
problem. The real long-term merge burden is now concentrated in:

- provider/account-pool plumbing
- memory/context packet integration
- multi-agent custom semantics
- large TUI integration files
- app-server protocol drift

That is the live custom surface to manage going forward.
