# Historical: Upstream Sync Migration Inventory

**Date:** 2026-03-06
**Source branch:** `feature/upstream-sync`
**Target baseline:** latest `upstream/main`
**Checkpoint commit for current custom line:** `38c01b6a4`
**Merge base:** `79d6f80e41806d61b8a5ce7dbaff4bb1d6e38a91`
**Divergence:** `165` commits ahead of `upstream/main`, `221` commits behind `upstream/main`

> **Status:** Historical inventory only. The `/freeze` and related sandbox customizations noted below are not part of the current upstream merge target and should not be reintroduced during merge prep.
>
> **Status (2026-03-31): Historical migration snapshot.**
> The upstream-sync work has since landed on `main`, but not every feature
> family listed below survived the final selection.
> Treat `git-graph`, `session bar`, `ralph-loop`, and agent worktrees as
> historical candidates rather than current preservation requirements.

## Branch Topology Summary

- This branch already contains at least two prior large sync operations:
  - `8df3cbb32` — `merge: sync upstream origin/main (80 commits) into custom main`
  - `3f5daa387` — `Merge upstream main (resolve 82 conflicts)`
  - `c7930a592` — `merge: sync upstream openai/codex (136 commits)`
- Continuing to merge in place would likely drag old conflict resolutions and compatibility shims into the next baseline.
- Recommended execution model remains: branch fresh from latest `upstream/main`, then reintroduce custom behavior by feature family.

## Account-Pool / Provider Routing

**Recommendation:** `merge`

Preserve the capability, but reattach it to latest upstream provider/model plumbing instead of reusing old branch-wide conflict resolutions.

**Primary commits**
- `609062175` — `feat(core): add provider account pools and auth key fallback`
- `1b95722b8` — `core: select primary provider account from account_pool`
- `bb764bcf7` — `core: respect provider overrides and account pools`
- `d0519937c` — `feat: API account pool with multi-round failover`
- `53f041f12` — `fix(core): resolve Antigravity auto-switch provider misidentification issue`
- `0749b15ba` — `fix(core): ensure Antigravity model providers are correctly mapped during init`
- `22cac4838` — `fix(core): prevent config auto-switch loop during failover rotation`
- `437ca15c8` — `fix(core): improve resolve_provider_id_for_provider to match by name`

**Key touched files**
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/auth.rs`
- `codex-rs/core/src/auth/storage.rs`
- `codex-rs/core/src/model_provider_info.rs`
- `codex-rs/core/src/models_manager/manager.rs`
- `codex-rs/core/src/models_manager/model_info.rs`
- `codex-rs/core/src/models_manager/model_presets.rs`
- `codex-rs/core/src/model_compat.rs`
- `codex-rs/core/config.schema.json`
- `docs/config.md`
- `codex-rs/config-examples/config-pool.toml`
- `codex-rs/config-examples/auth-pool.json`

**Why not keep as-is**
- Latest upstream has materially evolved model/provider plumbing, fast mode defaults, model roles, and plugin/app awareness.
- Reapplying this code blindly would likely regress newer upstream selection logic.

**Validation notes**
- Add focused `codex-core` tests for provider ID resolution, account selection, auth fallback, and failover rotation.
- Verify no config-switch loop remains.
- If config types change, regenerate `codex-rs/core/config.schema.json` with `just write-config-schema`.

## Memory

**Recommendation:** `merge`

Preserve the memory link and thread-memory behavior, but align it with latest upstream memory and workspace-write semantics.

**Primary commits**
- `9ea6896c9` — `feat(memory): propagate MemoryLink through turns, hooks, and app-server v2`
- `52a287978` — `feat(app-server): include MemoryLink in turn/start response`
- `03351b773` — `feat: propagate memory context into hooks and MCP tool calls`
- `c160da40b` — `feat: wire hooks + memory links across core and app-server`
- `f743260f3` — `core: persist thread memory summaries`
- `5431ca4a1` — `core: include project memories in claude_code context`
- `29f70c8de` — `core: exclude tool outputs from memory trace normalization`
- `d3408f401` — `cli: add debug thread-memory backfill command`
- `e7335ea52` — `core: expose thread memory tooling helpers`

**Key touched files**
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`
- `codex-rs/app-server/tests/suite/v2/mcp_server_elicitation.rs`
- `codex-rs/app-server/README.md`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/codex_thread.rs`
- `codex-rs/core/src/thread_memory.rs`
- `codex-rs/core/src/mcp_tool_call.rs`
- `codex-rs/core/src/tasks/mod.rs`
- `codex-rs/core/src/tasks/user_shell.rs`
- `codex-rs/hooks/src/registry.rs`
- `codex-rs/hooks/src/types.rs`
- `codex-rs/hooks/src/user_notification.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/tui/src/chatwidget/tests.rs`

**Why not keep as-is**
- Upstream has since added its own memory-related work; schema and event shapes have drifted.
- This family touches app-server protocol, hooks, core tasks, and TUI rendering, so stale copies are risky.

**Validation notes**
- Add/refresh tests that compare complete `MemoryLink` values, not individual fields.
- Run `cargo test -p codex-app-server` and `cargo test -p codex-app-server-protocol`.
- If wire shapes change, run `just write-app-server-schema` and update `codex-rs/app-server/README.md`.

## Collaboration / Subagents

**Recommendation:** `merge`

Keep our differentiated collaboration semantics where they still add value, but prefer the newer upstream multi-agent prompt/orchestration foundation where equivalent behavior already exists.

**Primary commits**
- `23c0e4d40` — `feat(core): add model_sub and utility_model systems`
- `60a09c360` — `feat(protocol): add multi-agent metadata to events and responses`
- `62a4b456b` — `feat(tui): display agent metadata in multi-agent UI`
- `637f8ec93` — `feat(exec): update event processors for multi-agent metadata`
- `42cdc13e9` — `feat(core): use utility_model for memory and compact tasks`
- `60fc69f69` — `feat(tui): add team profile and model selection UI`
- `211054f4f` — `feat(core): add Anthropic native integration`
- `c11848169` — `docs: add configuration examples for multi-agent setup`

**Key touched files**
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/tools/orchestrator.rs`
- `codex-rs/core/src/utility_model.rs`
- `codex-rs/core/src/model_sub_vouch.rs`
- `codex-rs/core/src/models_manager/collaboration_mode_presets.rs`
- `codex-rs/core/src/config/profile.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/exec/src/event_processor_with_human_output.rs`
- `codex-rs/exec/src/event_processor_with_jsonl_output.rs`
- `codex-rs/tui/src/multi_agents.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/config-examples/config.toml`

**Likely upstream overlap**
- `d77384faa` — newer upstream multi-agent prompt improvements
- `2940796d1` — upstream role-prefix handoff changes
- various recent collaboration/app-server event changes already landed upstream

**Validation notes**
- Prefer restoring only semantics still missing after upstream comparison.
- Run targeted `codex-core`, `codex-tui`, and `codex-app-server` tests.
- If UI text changes, update `insta` snapshots in `codex-rs/tui`.

## Entire / Rewind / Hooks / Sandbox Integrations

**Recommendation:** `merge`

Preserve unique Entire-centered workflows, but isolate them behind narrow integration points and avoid reviving old sandbox patch layers wholesale.

**Primary commits**
- `df62a9d62` — `feat(entire): generate AI summary eagerly and update git-graph commits`
- `90d3d0749` — `feat(entire): strictly filter out trivial prompts to suppress UI banners and background summary generation`
- `dd8beff44` — `feat(sandbox): add opt-in /freeze command gated by freeze_sandbox_debug feature flag`
- `54d4fa2b3` — `feat(sandbox): make freeze snapshotting asynchronous to unblock TUI`
- `12b9792fd` — `feat(sandbox): auto-install entire CLI into sandbox for rewind operations`
- `1ca17677d` — `feat(sandbox): inject accurate git identity instead of dummy clone identity`
- `55c5e8367` — `feat(sandbox): prevent meta-agent from hanging on interactive entire rewind menu`
- `91d658c17` — `fix(hooks): Safely truncate entire summary string at character boundary`

**Key touched files**
- `codex-rs/core/src/entire_integration.rs`
- `codex-rs/core/src/entire_summary_generator.rs`
- `codex-rs/core/src/freeze_debug.rs`
- `codex-rs/core/src/features.rs`
- `codex-rs/core/src/git_info.rs`
- `codex-rs/core/src/git_side_effects.rs`
- `codex-rs/core/src/seatbelt_permissions.rs`
- `codex-rs/core/src/skills/loader.rs`
- `codex-rs/core/src/skills/permissions.rs`
- `codex-rs/core/src/tools/handlers/js_repl.rs`
- `codex-rs/core/src/tools/handlers/shell.rs`
- `codex-rs/core/src/tools/handlers/unified_exec.rs`
- `codex-rs/core/src/tools/sandboxing.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `scripts/freeze-debug-vm.sh`
- `README.md`

**Why not keep as-is**
- This family sits closest to fast-moving upstream sandbox/tooling internals.
- There are recent upstream changes in permission merging, unified exec, package manager integration, and sandbox behavior.
- This is the highest regression-risk area and should be migrated after core orchestration is stable.

**Validation notes**
- Do not modify any code tied to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Prefer reproducible unit/integration coverage over environment-coupled tests.
- Re-run targeted `codex-core` and `codex-tui` tests for slash-command and tool-routing behavior.

## Git-Graph / TUI / UI

**Recommendation:** `merge`

Keep `git-graph` as a distinct custom capability, but reconnect it to current TUI/session UI with minimal adaptation.

**Primary commits**
- `19c0d9f1f` — `feat: add Git Graph widget (Ctrl+G) and Session Bar (Ctrl+P)`
- `6da07bec3` — `tui: resume selected session from session bar`
- `55bbfad7d` — `feat: TUI improvements for ref-image, aspect-ratio, and batch processing`
- `62a4b456b` — `feat(tui): display agent metadata in multi-agent UI`

**Key touched files**
- `codex-rs/git-graph/Cargo.toml`
- `codex-rs/git-graph/src/main.rs`
- `codex-rs/git-graph/src/lib.rs`
- `codex-rs/git-graph/src/graph.rs`
- `codex-rs/git-graph/src/config.rs`
- `codex-rs/tui/src/git_graph_widget.rs`
- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/session_utils.rs`
- `codex-rs/tui/src/pager_overlay.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/tests.rs`

**Why not keep as-is**
- TUI internals, pending-render semantics, and slash-command behavior have changed significantly upstream.
- The `git-graph` crate itself is relatively self-contained; the risky part is the TUI integration surface.

**Validation notes**
- First check whether `codex-rs/git-graph` builds unchanged against the latest upstream workspace.
- Add/update `insta` coverage for all intentional user-visible UI changes.
- Run `cargo test -p codex-tui` and review pending snapshots before accepting.

## Build / Test / Schema / Docs Updates

**Recommendation:** `merge`

Treat these as trailing support work after functional migrations land.

**Representative commits**
- `e4a417e4a` — `docs: add integration guidelines for Antigravity proxy`
- `65b3c8975` — `docs: update README with custom fork capabilities overview`
- `1fdb6d0a5` — `docs: Add Entire integration architecture and config tests`
- `c29db2532` — `docs: add state/memory/hooks/automation notes and UML`
- `44200f691` — `docs: 补充指令统一与 thread memory 回填说明`
- `c11848169` — `docs: add configuration examples for multi-agent setup`

**Key touched files**
- `README.md`
- `docs/config.md`
- `codex-rs/app-server/README.md`
- `codex-rs/config-examples/README.md`
- `codex-rs/core/config.schema.json`
- `codex-rs/app-server-protocol/schema/**`

**Validation notes**
- If `ConfigToml` or nested config types change, run `just write-config-schema`.
- If app-server protocol/API shapes change, run `just write-app-server-schema` and `cargo test -p codex-app-server-protocol`.
- If `Cargo.toml` or `Cargo.lock` changes, run `just bazel-lock-update` and `just bazel-lock-check`.

## Upstream Overlap / Likely Superseded Items

These areas appear likely to be partially or fully superseded upstream and should be compared before any cherry-pick:

- upstream multi-agent prompt and role handoff improvements (`d77384faa`, `2940796d1` in branch history mirror upstream work)
- upstream app-server MCP elicitation support and related v2 protocol work
- upstream `js_repl` and unified exec changes
- upstream memories/workspace-write behavior (`f72ab43fd` lineage in upstream history)
- upstream fast-mode, model-role, and plugin/app tracking changes

Practical rule: if latest upstream already has the capability, port only our missing semantic delta.

## High-Risk Files

The following files are likely to conflict heavily or hide semantic regressions if migrated mechanically:

- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/codex_thread.rs`
- `codex-rs/core/src/mcp_tool_call.rs`
- `codex-rs/core/src/tools/orchestrator.rs`
- `codex-rs/core/src/tools/sandboxing.rs`
- `codex-rs/core/src/skills/permissions.rs`
- `codex-rs/core/src/seatbelt_permissions.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

## Recommended Migration Order

1. account-pool / provider routing
2. memory + app-server wiring
3. collaboration / subagents
4. Entire / rewind / hooks / sandbox integrations
5. git-graph + TUI integration
6. generated artifacts, docs, and cleanup

## Execution Guidance

- Start from a clean branch created directly from latest `upstream/main`.
- Use this file as the migration worksheet, not as a cherry-pick queue.
- For each family, explicitly decide `keep`, `upstream`, or `merge` before touching code.
- Prefer test-first migration for each family and validate the smallest affected crate first.
