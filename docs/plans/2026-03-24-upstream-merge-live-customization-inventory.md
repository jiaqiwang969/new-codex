# Current Upstream Merge Live Customization Inventory

**Date:** 2026-03-24
**Current branch head:** `0fa92816c` (`fix merge followups for app-server and tests`)
**Compared against:** `upstream/main` at `9dbe09834`

> **Status:** Current working inventory for the ongoing upstream merge cleanup.
> This supersedes older assumptions that the local Smart Access / `endpoint-sec` /
> `/freeze` line was still part of the target architecture.

## Fresh upstream drift

After refreshing the live remote, `upstream/main` advanced from `047ea642d` to
`9dbe09834`.

The newly added upstream changes relevant to the current merge analysis are:

- `504aeb0e0` `Use AbsolutePathBuf for cwd state`
- `6b10e186c` `Add non-interactive resume filter option`
- `d273efc0f` `Extract codex-analytics crate`
- `91337399f` `[apps][tool_suggest] Remove tool_suggest's dependency on tool search.`
- `9dbe09834` `Extract codex-core-skills crate`

Files newly touched upstream in our highest-risk overlap set:

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/memories/phase2.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

Interpretation:

- upstream re-opened shared-file churn in Block 2 and the TUI reintegration
  area
- upstream still did **not** add local account-pool, `config-pool`,
  provider-family utility routing, `model_sub`, team-profile, or Entire UI
  semantics

## Companion Notes

Detailed follow-up analysis for the main preserved blocks lives in:

- `docs/plans/2026-03-24-upstream-merge-account-pool-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-execution-playbook.md`
- `docs/plans/2026-03-24-upstream-merge-guardian-model-sub-app-server-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-memory-entire-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-provider-family-utility-routing-analysis.md`
- `docs/plans/2026-03-24-upstream-merge-shared-attachment-points-analysis.md`
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

This was rechecked after the upstream merge follow-up work by scanning the
current runtime source roots under:

- `codex-rs/core/src`
- `codex-rs/tui/src`
- `codex-rs/app-server/src`
- `codex-rs/protocol/src`
- `codex-rs/cli/src`

## Docs Cleaned In This Round

To prevent stale guidance from confusing future merge work:

- `README.md`
  - removed the user-facing `/freeze` feature pitch
  - added a short note that old `/freeze`, `endpoint-sec`, and local Smart Access
    are no longer maintained
- `docs/reports/2026-03-15-upstream-gap-report.tex`
  - added an updated status note
  - re-labeled Endpoint Security and `/freeze` sections as historical

## Inactive Automation Removed

The previously noted freeze-like automation experiment has now been removed
from the repository source tree:

- `codex-rs/core/src/automation/compile_error_freezer.rs`
- `codex-rs/core/src/automation/fix_agent_coordinator.rs`
- `codex-rs/core/src/automation/mod.rs`
- `codex-rs/core/src/automation/snapshot.rs`
- `codex-rs/core/src/automation/undo_replacer.rs`
- `codex-rs/core/src/automation/utm_manager.rs`

Reason:

- it was not part of the active compiled `codex-core` module graph
- it was directionally much closer to the abandoned `/freeze` workflow than to
  the keep-set for upstream alignment
- deleting it reduces long-tail local divergence without changing current
  runtime behavior

## Docs Drift Resolved In This Round

The account-pool docs were normalized to match current runtime semantics:

- `codex-rs/README.md`
  - now reflects the local `config-pool.toml` / `auth-pool.json` workflow
  - now uses local Anthropic endpoint examples such as `https://code.ppchat.vip`
  - no longer claims the last successful account is persisted
- `docs/config.md`
  - now matches the current session-local cooldown window (`60s`)

This removes the most visible documentation mismatch in the provider/account-pool
area before the next merge-analysis pass.

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

### 2. Provider Family Expansion / Utility Routing

**Keep selectively.**

Representative local-only / heavily customized files:

- `codex-rs/core/src/anthropic_content.rs`
- `codex-rs/core/src/anthropic_streaming.rs`
- `codex-rs/core/src/gemini_content.rs`
- `codex-rs/core/src/gemini_streaming.rs`
- `codex-rs/core/src/model_compat.rs`
- `codex-rs/core/src/utility_model.rs`

Why it matters:

- this is the transport/routing layer that lets the fork mix OpenAI,
  Anthropic, Gemini, Grok, and antigravity families coherently
- `model_sub`, team profiles, memory summarization, and account-pool-aware
  utility routing all depend on this layer
- the core modules for this block are currently absent in `upstream/main`,
  which means this is a genuine local capability layer rather than a same-file
  merge artifact
- upstream provider/model preset work continues to move, so this will keep
  colliding with both config and runtime selection logic

### 3. Memory / Context Packet / Entire Integration

**Likely keep, but migrate carefully.**

Representative local-only files:

- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/core/src/context_packet.rs`
- `codex-rs/core/src/entire_integration.rs`

Why this will keep conflicting with upstream:

- memory and app-server wire shapes are still evolving upstream
- this fork pushes memory deeper into hooks, MCP, summaries, and session state

### 4. Agent Worktrees

**Keep.**

Representative local-only file:

- `codex-rs/core/src/agent_worktree.rs`

Why it matters:

- this is one of the clearest differentiated workflow features
- it is conceptually narrow and easier to preserve than the old security line
- it is actively wired into resume/fork workflows and the debug CLI surface,
  not just dormant code

### 5. Multi-Agent Extensions

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
- part of the current wire expansion is still only partially populated
  (`model_provider_id`, `model_source`, and `model_source_detail` are often
  emitted as `None`)

### 6. TUI Workbench Features

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

- `codex-rs/core/src/config/mod.rs`
- `4626` insertions, `5` deletions vs `upstream/main`
- `codex-rs/tui/src/chatwidget.rs`
  - `3536` insertions, `1111` deletions
- `codex-rs/core/src/codex.rs`
  - `1685` insertions, `92` deletions
- `codex-rs/tui/src/app.rs`
  - `1004` insertions, `27` deletions
- `codex-rs/core/src/client.rs`
  - `776` insertions, `16` deletions
- `codex-rs/app-server/src/bespoke_event_handling.rs`
  - `281` insertions, `15` deletions
- `codex-rs/protocol/src/protocol.rs`
  - `162` insertions, `4` deletions
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
  - `71` insertions, `1` deletion

Interpretation:

- `config/mod.rs`, `codex.rs`, and `client.rs` are now the real provider /
  account-pool / utility-routing conflict core
- `chatwidget.rs` and `app.rs` remain the TUI convergence sink where unrelated
  local workbench features pile up together
- app-server / protocol files have smaller raw diffs, but they remain
  disproportionately painful because upstream churn is fastest there

## Practical Merge Order From Here

1. Keep the security-line rollback intact.
2. Preserve account-pool / provider routing.
3. Preserve provider-family expansion / utility routing only as needed to keep
   `model_sub`, memory summarization, and heterogeneous endpoints working.
4. Preserve memory / context packet / Entire only after provider plumbing is stable.
5. Preserve agent worktrees as an isolated workflow feature.
6. Reattach TUI workbench features (`git-graph`, `session bar`, `ralph-loop`)
   on top of the newer upstream TUI surface.
7. Re-evaluate guardian / model-sub / app-server wire customizations last,
   because those have the highest upstream overlap and the most contract drift.

## Summary Judgment

At this point, the failed local security line is no longer the main merge
problem. The real long-term merge burden is now concentrated in:

- provider/account-pool plumbing
- provider-family transport / utility routing
- memory/context packet integration
- multi-agent custom semantics
- large TUI integration files
- app-server protocol drift

That is the live custom surface to manage going forward.
