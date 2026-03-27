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
- `docs/plans/2026-03-16-smart-approvals-core-trunk-design.md`
  and `docs/plans/2026-03-16-smart-approvals-core-trunk-implementation.md`
  - deleted obsolete implementation plans for the abandoned Smart Access +
    `endpoint-sec` carry-forward approach

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

**Split this block before making keep/drop decisions.**

#### Core routing semantics that should stay

Representative files:

- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/*.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

These carry the real differentiated semantics:

- `model_sub`
- `model_sub_responses`
- provider-aware utility routing
- default child-agent inheritance of `model_sub`

This is the part that must remain compatible with account-pool and custom
provider endpoints.

#### TUI-only preset and vouch UX that can be reevaluated separately

Representative files:

- `codex-rs/tui/src/team_profile.rs`
- `codex-rs/tui/src/team_profile_vouch.rs`
- `codex-rs/tui/src/model_sub_vouch.rs`
- `codex-rs/tui/src/status/card.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/app.rs`

Current audit result:

- these features do not leak into `codex-rs/core/**`
- they do not appear in app-server protocol state
- they do not have a mirrored implementation in `codex-rs/tui_app_server`

That means they are useful local UX, but they are not architecture anchors.
If future upstream sync cost is too high, they can be dropped without removing
core `model_sub` support.

### 5. TUI Workbench Features

**Keep selectively.**

Representative local-only files:

- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/ralph_loop.rs`

Current branch state:

- `session_bar` is active and kept
- `ralph-loop` is active and kept
- `git-graph` is no longer wired into the TUI key path; `Ctrl+G` now follows the
  upstream external-editor flow
- the vendored `codex-rs/git-graph/**` tree is parked legacy code, not an active
  merge driver

Why the active pieces are relatively safe:

- `session_bar` and `ralph-loop` are still intentional workflow features
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
5. Reattach active TUI workbench features (`session bar`, `ralph-loop`) on top
   of the newer upstream TUI surface, while keeping `git-graph` parked unless it
   is explicitly revived later.
6. Keep core `model_sub` / `model_sub_responses` routing, then decide
   separately whether to retain or prune `team_profile` / `team_profile_vouch`
   / `model_sub_vouch`.
7. Re-evaluate guardian / app-server wire customizations last, because those
   have the highest upstream overlap and the most contract drift.

## Summary Judgment

At this point, the failed local security line is no longer the main merge
problem. The real long-term merge burden is now concentrated in:

- provider/account-pool plumbing
- memory/context packet integration
- multi-agent core routing semantics
- active TUI workbench features (`session_bar`, `ralph-loop`)
- optional TUI preset/vouch UX layered onto multi-agent routing
- large TUI integration files
- app-server protocol drift

That is the live custom surface to manage going forward.
