# Upstream Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align the custom branch with `upstream/main` while preserving the required local account-pool and Entire/memory features, and permanently drop the failed smart-access, freeze, and endpoint-sec lines.

**Architecture:** Treat the current divergence as feature buckets instead of replaying old commits. Freeze the preserved local stacks first, then split `protocol/hooks/app-server` into mandatory memory wiring versus optional local-only features, and only after that continue merging the remaining upstream deltas.

**Tech Stack:** Git, Rust workspace crates under `codex-rs`, app-server v2 protocol/schema generation, hooks, docs, targeted Cargo tests.

---

## Current Status

- `upstream/main` is already merged into `probe/upstream-merge-9dbe09834`.
- Cleanup checkpoint `9e86f782f3` (`chore: align upstream cleanup batch`) is now committed.
- There is no cherry-pick in progress.
- The latest committed cleanup batch is `5a95257662` (`chore: align explorer and model catalog metadata`):
  - keep the default/explorer spawn-tool-spec wording aligned with local `model_sub` inheritance
  - avoid advertising a locked `gpt-5.2` / `xhigh` explorer preset
  - preserve the provider/model-catalog baseline while trimming role-layer drift
- The current uncommitted batch is a prompt/permissions cleanup plus this plan doc update:
  - remove legacy `Protected Filesystem Operations on macOS` prompt residue from shared/provider prompt files
  - restore the `request_permissions` approval prompt scope to upstream `network` + `file_system` only
- `endpoint-sec` is no longer present in the remaining diff.
- `freeze` is no longer a functional feature line in the remaining diff.
- Residual code search confirms `endpoint-sec` and `smart access` now appear only in this plan doc; `freeze` only remains in unrelated generic wording and the non-feature test key `freeze_sandbox_debug`.
- Commit `97eb3d953f` (`fix: strip null config extras from config read`) must stay because it preserves `model_sub` and `model_sub_responses` config-read behavior.
- Commit `4acb2280d6` (`refactor: drop local-only protocol surfaces`) is already complete and should stay:
  - dropped `SetReferenceImages`
  - dropped `ClearReferenceImages`
  - dropped `SetImageQuality`
  - dropped `SetAspectRatio`
  - dropped external `FileSystemMutated`
  - kept internal `side_effects_files` tracking
- Low-risk config/MCP test-splitting cleanup is already complete and verified:
  - `8d348b8664`
  - `2556abae3c`
  - `b897b028b3`
  - `c3d31b9043`
- Fresh verification on the committed tree after the spark model-surface cleanup:
  - `cargo test -p codex-core`: PASS (`929 passed`, `0 failed`, `14 ignored`) plus:
    - `tests/entire_config_test.rs`: PASS (`3 passed`)
    - `tests/responses_headers.rs`: PASS (`4 passed`)
- Reverification on committed tree `9a5a59b5da` was re-run and progressed through the full unit phase cleanly:
  - unit tests: PASS (`1807 passed`, `0 failed`, `5 ignored`)
  - the follow-up `tests/all.rs` integration sweep is long-running but has shown no failures during repeated polling
- Current working-tree verification for the prompt/permissions cleanup:
  - `cargo test -p codex-protocol`: PASS (`140 passed`, `0 failed`)
  - `cargo test -p codex-core --test all 'suite::compact_remote::remote_compact_trim_estimate_uses_session_base_instructions' -- --exact`: PASS (`1 passed`, `0 failed`)
  - `cargo test -p codex-core --test all suite::compact_remote::`: PASS (`23 passed`, `0 failed`, `1 ignored`)
  - `cargo test -p codex-core`: one transient failure in `suite::compact_remote::remote_compact_trim_estimate_uses_session_base_instructions`
    - isolated rerun and the full `compact_remote` subset both passed immediately afterward
    - the failing full-run logs showed a transient `502 Bad Gateway` on proxied local `/responses` traffic rather than an assertion mismatch
  - previously re-run focused checks still remain green:
    - `cargo test -p codex-app-server-protocol`
    - `cargo test -p codex-hooks`
    - `cargo test -p codex-app-server collaboration_mode_list`
    - `cargo test -p codex-tui collaboration_mode`
- `just argument-comment-lint` is still blocked by missing repo artifact `./tools/argument-comment-lint/run-prebuilt-linter.sh`; treat this as a pre-existing environment/tooling issue until the script is restored.

## Post-Checkpoint Inventory (2026-03-30)

- Relative to `upstream/main`, the branch is now `0 behind / 281 ahead`.
- The remaining branch-level divergence is still large in raw volume:
  - `264 files changed`
  - `27,261 insertions`
  - `3,258 deletions`
- Raw size alone is misleading because a large part of the diff is generated schema, test movement, or preserved local product lines that were intentionally frozen during the previous cleanup slices.

### Remaining Divergence Buckets

- Bucket A: preserved provider/account-pool/auth stack
  - `core/src/config/{mod,provider_registry,provider_selection}.rs`
  - `core/src/{provider_pool,provider_pool_runtime,provider_pool_failover,provider_routing,provider_inventory,provider_auth,model_provider_info,utility_model}.rs`
  - `login/src/auth/{manager,storage}.rs`
  - `config-examples/{config-pool.toml,auth-pool.json,README.md}`
  - `docs/config.md`
  - status: protected local product line; do not trim casually

- Bucket B: preserved Entire/memory/context packet stack
  - `core/src/{thread_memory,context_packet,context_packet_memory,entire_summary_generator,entire_integration,hook_memory}.rs`
  - `hooks/src/entire_summary.rs`
  - `state/src/runtime/memories.rs`
  - `rollout/src/{state_db,list}.rs`
  - status: protected local product line; already validated by `memories` and `entire_config_test`

- Bucket C: memory wire contract and app-server replay surface
  - `protocol/src/protocol.rs`
  - `app-server-protocol/src/protocol/{v2,thread_history}.rs`
  - `app-server/src/bespoke_event_handling.rs`
  - generated app-server protocol schema/typescript files
  - status: mostly fallout from Bucket B; not a good candidate for blind upstream rollback

- Bucket D: native provider transport and model catalog surface
  - `core/src/{anthropic_*,gemini_*,model_compat}.rs`
  - `core/src/client/provider_support.rs`
  - `core/src/models_manager/model_info.rs`
  - prompt/model template files under `core/*.md` and `core/templates/model_instructions/`
  - status: mixed but tightly coupled to Bucket A; likely the next highest-cost area to classify, not the next place for blind deletions

- Bucket E: residual low-priority non-product tails
  - `app-server/tests/common/mcp_process.rs`
  - `network-proxy/src/{http_proxy,network_policy,runtime}.rs`
  - selected test refactors and harness hardening
  - status: low-risk to revisit later, but low leverage compared with Buckets A-D

### Recommended Next Batch

- Next analysis/execution target should be Bucket D on top of the already-frozen Bucket A baseline.
- Reason:
  - it is the largest remaining area that still mixes true local product intent with possible upstream drift
  - it controls whether Anthropic/Gemini/native provider support, prompt expansions, and model catalog growth are all intentional keepers or still contain reducible local tail
  - it can be reviewed without reopening the already-closed `smart access`, `freeze`, or `endpoint-sec` lines

## Latest Classification Notes

- Provider/account-pool is a preserved local product line, not merge noise:
  - `config-pool.toml` and `auth-pool.json` cover `codex`, `gemini`, `grok`, and `anthropic`
  - `https://code.ppchat.vip` must remain for the local Anthropic and Codex-compatible path
  - `model_sub` and `model_sub_responses` are part of this baseline
- Entire/memory is also a preserved local stack:
  - `MemoryLink`
  - hook memory payloads
  - thread/turn memory propagation
  - Entire summary generation and persistence
- Role/prompt/model metadata is not just unresolved merge fallout:
  - `core/src/models_manager/model_info.rs` is now a local model catalog entry point
  - `core/src/agent/role.rs` contains a required provider-reroute behavior so role-selected models still honor local provider/account-pool routing
  - `core/src/agent/builtins/explorer.toml` was local drift rather than a provider requirement; locking `explorer` to `gpt-5.2` / `xhigh` conflicted with the local `model_sub` inheritance wording, so that built-in file is being restored toward upstream-empty content while keeping the reroute behavior
  - Anthropic/Gemini/Grok/Gemma prompt and model metadata should not be removed casually because they sit on top of the preserved provider stack
  - `core/src/client/provider_support.rs` and `core/src/model_provider_info.rs` are part of the same protected provider surface; trimming them would effectively mean shrinking the provider product line, not just resolving merge noise
  - extra built-in role presets are a separable product surface from the provider stack itself
  - dead built-in artifact `core/src/agent/builtins/gemini-3.1-pro-preview.toml` can be dropped safely because nothing references it
  - `claude-opus`, `claude-sonnet`, and `claude-haiku` built-in roles can be removed while keeping Anthropic provider/model support intact, because their references were isolated to `role.rs` / `role_tests.rs`
  - `awaiter.toml` should stay available as embedded built-in content, but the role declaration itself should follow upstream and remain disabled
  - `explorer` is a live built-in role used across app-server, TUI, and core tests, so it must stay even though its config/description may still need later wording cleanup
- App-server/schema is mostly preserved baseline now, not the next best trim target:
  - `app-server-protocol/src/protocol/v2.rs` adds `model_sub`, `model_sub_responses`, `Turn.memory`, and item-level `memory`
  - `thread_history.rs` and `bespoke_event_handling.rs` mainly propagate `MemoryLink`
  - most schema/test expansion is generated fallout from those preserved fields
  - only tiny residual app-server diffs remain unrelated to product behavior, for example formatting or test-harness details
- Hooks/protocol wire growth is also mostly preserved baseline:
  - `protocol/src/protocol.rs` adds `MemoryLink` to turn and collab events, which is the core wire contract for thread-memory continuity
  - `hooks/src/types.rs`, `hooks/src/user_notification.rs`, and `hooks/src/legacy_notify.rs` add `provider_name`, `model_slug`, memory metadata, and `mcp-tool-call-complete`
  - those hook payload changes are tied to preserved provider/account-pool observability plus memory continuity, not to the removed smart-access or endpoint-sec lines
  - `app-server-protocol/src/protocol/thread_history.rs` and `app-server/src/bespoke_event_handling.rs` mostly replay that same memory metadata into `thread/read` and live notifications, including camelCase/snake_case MCP argument parsing for memory-link recovery
- `thought_signature` remains high-cost:
  - it is spread across Gemini protocol/content/streaming, provider support, protocol models, app-server, and many tests
  - do not touch it unless intentionally shrinking the Gemini product line
- `network-proxy` test hardening should stay for now:
  - the remaining diff in `network-proxy/src/http_proxy.rs`, `network-proxy/src/network_policy.rs`, and `network-proxy/src/runtime.rs` is test-only
  - attempted reversion to upstream hostnames made `cargo test -p codex-network-proxy` fail in 8 places
  - failures all collapsed onto environment-sensitive hostname handling (`not_allowed_local`) rather than product behavior
  - local use of public IP literals and selective `allow_local_binding` in tests is currently justified as hermetic test hardening
- `login` auth storage changes are preserved baseline, not optional noise:
  - `login/src/auth/storage.rs` adds `GEMINI_API_KEY` plus flattened `provider_api_keys`
  - `login/src/auth/manager.rs` consumes those fields and also loads `auth-pool.json`
  - this is part of the required multi-provider and account-pool credential path and should not be reverted
- `app-server` MCP harness diff remains low-risk but low-priority:
  - `app-server/tests/common/mcp_process.rs` came from `410c862de9` (`app-server: tests: stabilize MCP process harness`)
  - intent is clearly process/test stability: process semaphore, proxy env cleanup, quieter logging
  - it does not look like product-surface divergence, but it also does not need to be the next cleanup slice
- `app-server` test fixtures around auth/config/model list are not simple merge noise:
  - `tests/common/auth_fixtures.rs` and `tests/suite/v2/app_list.rs` must include the newer auth shape (`gemini_api_key`, `provider_api_keys`) because that is required by the preserved auth-pool path
  - `tests/suite/v2/config_rpc.rs` coverage for `model_sub` and `model_sub_responses` must stay because config/read is part of the preserved local baseline
  - `tests/common/models_cache.rs` and `tests/suite/v2/model_list.rs` are mixed upstream/local history: upstream already moved away from hardcoded preset lists, while local follow-up adjusted expectations for the utility-model/model-catalog line, so this bucket is not a quick revert target
- Guardian approvals is now clearly an upstream trunk, not residual local smart-access logic:
  - upstream already contains `e84ee33cc0` (`Add guardian approval MVP (#13692)`) and later guardian-reviewer follow-up commits
  - current diffs under `protocol/config_types.rs`, `app-server-protocol/src/protocol/v2.rs`, `app-server/src/codex_message_processor.rs`, `tui_app_server/src/app.rs`, and `tui_app_server/src/chatwidget.rs` are dominated by preserved local `model_sub` / memory wiring rather than custom approval routing
  - there are no remaining `endpoint-sec` or `smart access` code paths in those files
  - `docs/config.md` had local wording drift around approvals aliases and should track actual runtime behavior: `guardian_approval` is the experimental rollout gate, while deprecated `smart_approvals` is ignored
- Tiny standalone cleanup can proceed selectively:
  - `git-utils/src/info.rs` is already aligned again
  - `app-server-protocol/src/protocol/common.rs` had a whitespace-only merge residue and is now aligned
  - `protocol/bindings/.gitignore` should stay unless generated TS bindings hygiene is revisited separately, because local generated files may otherwise reappear as untracked noise
- Additional low-coupling cleanup already looks justified:
  - legacy macOS protected-filesystem prompt residue was safe to strip from shared/provider prompt files
  - `request_permissions` approval prompt scope is now back to upstream `network` + `file_system` wording
  - `core/templates/collaboration_mode/collaborative.md` and `app-server/tests/suite/v2/collaboration_mode_list.rs` are already aligned and no longer active cleanup targets
  - `app-server/src/main.rs` debug-only gate on `MANAGED_CONFIG_PATH_ENV_VAR` should stay; it is a small correctness/cleanliness fix rather than a product fork

### Bucket D Split (2026-03-30 follow-up)

- D1: provider runtime and wire adapters
  - `core/src/client/provider_support.rs`
  - `core/src/model_compat.rs`
  - `core/src/{anthropic_content,anthropic_streaming,anthropic_types}.rs`
  - `core/src/{gemini_content,gemini_streaming,gemini_types}.rs`
  - all of these files are absent from `upstream/main`
  - they are not dead sidecars: current `core/src/client.rs` imports `provider_support` directly, while `provider_routing.rs`, `config/provider_selection.rs`, `models_manager/manager.rs`, and `utility_model.rs` import `model_compat`
  - consequence: removing this slice would not be "aligning prompts"; it would actively shrink the preserved non-OpenAI provider product line

- D2: model compatibility policy
  - `model_compat.rs` is the policy layer for namespaced slug normalization and provider capability quirks:
    - Grok slug normalization and capability gates
    - Gemma slug normalization
    - Anthropic slug normalization, including `antigravity/*`
    - legacy Gemini alias rewriting
  - this file is also where current capability exceptions live, for example:
    - Grok restrictions for `web_search`, `external_web_access`, `reasoning.effort`, and memory trace summarize
    - text-only handling for `gpt-5.3-codex-spark`
  - status: protected as long as Bucket A keeps multi-provider routing

- D3: model catalog and prompt binding
  - `core/src/models_manager/model_info.rs` is mixed:
    - part of it is hard dependency for Bucket A/D1 because it assigns shell/tool/reasoning/context defaults for Anthropic, Gemini, Grok, and Gemma
    - another part is prompt/catalog expansion surface layered on top of that runtime support
  - provider-specific prompt files are all absent from `upstream/main` and currently only enter the runtime through `model_info.rs`:
    - `core/gemini_prompt.md`
    - `core/grok_prompt.md`
    - `core/claude_prompt.md`
    - `core/gpt-5.3-codex-spark_prompt.md`
    - `core/templates/model_instructions/gpt-5.3-codex-spark_instructions_template.md`
  - implication: if we want to keep the runtime provider stack but still reduce local surface, the likely place to trim later is `model_info.rs` plus these prompt/template bindings, not D1 transport code

- D4: Gemini `thought_signature` cross-cut
  - the protocol addition in `protocol/src/models.rs` is small by itself, but it fans out widely:
    - Gemini content/streaming code
    - app-server thread-history schema
    - generated protocol/app-server schema JSON
    - many tests across `core`, `protocol`, `app-server`, `state`, and `rollout`
  - the field is internal-only (`skip_serializing`, `ts(skip)`) and exists to preserve Gemini round-tripping state between turns
  - status: high-cost target; only revisit if we intentionally shrink or remove the Gemini product line

- D5: likely next reducible tail inside Bucket D
  - the best remaining candidate is not provider transport, but optional model/prompt product surface:
    - extra prompt families and prompt wording
    - model-catalog-only entries like `gpt-5.3-codex-spark`
  - before trimming any of that, decide explicitly whether those model variants are product requirements or just historical local additions

## Preserved Local Requirements

- Keep account-pool and provider routing:
  - `https://code.ppchat.vip`
  - `~/.codex/config-pool.toml`
  - `~/.codex/auth-pool.json`
  - `model_sub`
  - `model_sub_responses`
- Keep Entire / MemoryLink / memory propagation.
- Remove and do not revive:
  - smart access custom line
  - freeze
  - endpoint-sec / legacy security-guard line

### Task 1: Freeze Preserved Provider and Account-Pool Stack

**Files:**
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/provider_registry.rs`
- Modify: `codex-rs/core/src/config/provider_selection.rs`
- Modify: `codex-rs/core/src/provider_pool.rs`
- Modify: `codex-rs/core/src/provider_routing.rs`
- Modify: `codex-rs/core/src/model_provider_info.rs`
- Modify: `codex-rs/core/src/provider_inventory.rs`
- Modify: `codex-rs/core/src/utility_model.rs`
- Modify: `codex-rs/login/src/auth/manager.rs`
- Modify: `docs/config.md`
- Modify: `codex-rs/config-examples/config-pool.toml`
- Modify: `codex-rs/config-examples/auth-pool.json`

**Step 1: Treat provider/account-pool as a preserved local patch stack**

Record these files as protected during upstream cleanup. Do not overwrite them from upstream unless the change is explicitly reconciled with local pool behavior.

**Step 2: Preserve the provider config-read baseline**

Keep `97eb3d953f` in place so config RPC continues stripping null extras while retaining the local `model_sub` fields.

**Step 3: Preserve the pool-backed Anthropic/Gemini/Grok/Gemma behavior**

Do not revert:
- `config-pool.toml`
- `auth-pool.json`
- `code.ppchat.vip`
- builtin-family override behavior

**Step 4: Re-run focused provider/config tests after any provider merge cleanup**

Run:
```bash
cargo test -p codex-core mcp_requirements_tests
cargo test -p codex-core mcp_config_tests
cargo test -p codex-core mcp_stdio_edit_tests
cargo test -p codex-core mcp_http_edit_tests
cargo test -p codex-core mcp_servers_toml_ignores_unknown_server_fields -- --exact
```

Expected: PASS

### Task 2: Freeze Entire and Memory Propagation Stack

**Files:**
- Modify: `codex-rs/state/src/runtime/memories.rs`
- Modify: `codex-rs/rollout/src/state_db.rs`
- Modify: `codex-rs/rollout/src/list.rs`
- Modify: `codex-rs/rollout/src/policy.rs`
- Modify: `codex-rs/core/src/thread_memory.rs`
- Modify: `codex-rs/core/src/context_packet.rs`
- Modify: `codex-rs/core/src/context_packet_memory.rs`
- Modify: `codex-rs/core/src/entire_summary_generator.rs`
- Modify: `codex-rs/hooks/src/entire_summary.rs`

**Step 1: Treat Entire and thread memory as a preserved local stack**

Keep the memory persistence and context-packet flow introduced by:
- `f743260f39`
- `71ad044a1b`
- `36d066ca9c`
- `42cdc13e98`
- `c2201942fd`

**Step 2: Preserve hook-facing Entire integration**

Do not remove `hooks/src/entire_summary.rs` or the core-side Entire summary generator unless the user explicitly decides to abandon Entire.

**Step 3: Re-run focused Entire and memory tests after any merge cleanup**

Run:
```bash
cargo test -p codex-core entire_config_test
cargo test -p codex-core memories
```

Expected: PASS

### Task 3: Split Protocol/Hooks/App-Server into Keep vs Drop

**Files:**
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/hooks/src/types.rs`
- Modify: `codex-rs/hooks/src/user_notification.rs`
- Modify: `codex-rs/hooks/src/legacy_notify.rs`
- Modify: `codex-rs/hooks/src/registry.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/thread_history.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Modify: generated files under `codex-rs/app-server-protocol/schema/`

**Step 1: Keep the MemoryLink wire contract**

Preserve the reduced memory-link surface introduced by:
- `9ea6896c98`
- `c160da40b4`
- `346df6ba10`

This includes:
- `MemoryLink`
- `memory_scope_version`
- hook payload memory fields
- app-server v2 memory propagation
- `mcp-tool-call-complete`

**Step 2: Treat reference-image controls as optional local-only features**

These were local-only and limited in scope:
- `Op::SetReferenceImages`
- `Op::ClearReferenceImages`
- `Op::SetImageQuality`
- `Op::SetAspectRatio`

They previously lived in:
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/core/src/codex.rs`

Status:
- already dropped in `4acb2280d6`

**Step 3: Treat FileSystemMutated as optional local-only telemetry**

This line came from `81cfe1b101` and touched only a few files:
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/core/src/git_side_effects.rs`
- `codex-rs/mcp-server/src/codex_tool_runner.rs`
- `codex-rs/rollout/src/policy.rs`
- `codex-rs/tui_app_server/src/chatwidget.rs`

Status:
- external protocol surface already dropped in `4acb2280d6`
- internal side-effect file tracking must remain

**Step 4: Treat thought_signature as high-cost and do not touch casually**

`thought_signature` is local-only and spread across protocol, app-server, core Gemini integration, and many tests. Do not remove it unless the Gemini stack itself is being cut back.

**Step 5: Re-run focused protocol and hook tests after each cleanup slice**

Run:
```bash
cargo test -p codex-app-server-protocol
cargo test -p codex-hooks
cargo test -p codex-core mcp_tool_call_tests
```

Expected: PASS

### Task 4: Triage the Gemini, Anthropic, Ralph Loop, and Prompt Surface

**Files:**
- Modify: `codex-rs/core/src/gemini_content.rs`
- Modify: `codex-rs/core/src/gemini_streaming.rs`
- Modify: `codex-rs/core/src/gemini_types.rs`
- Modify: `codex-rs/core/src/anthropic_content.rs`
- Modify: `codex-rs/core/src/anthropic_streaming.rs`
- Modify: `codex-rs/core/src/anthropic_types.rs`
- Modify: `codex-rs/core/src/client/provider_support.rs`
- Modify: `codex-rs/protocol/src/models.rs`
- Modify: `codex-rs/core/src/agent/role.rs`
- Modify: `codex-rs/core/src/agent/builtins/*.toml`
- Modify: `codex-rs/core/*prompt.md`
- Modify: `codex-rs/core/templates/model_instructions/*`

**Step 1: Treat the Gemini stack as a deliberate local product line**

The remaining diff here is not accidental merge residue. It was introduced primarily by:
- `68615b4352`
- `d93ab4cbdb`
- `122d38f270`
- `18c5db71a4`
- `70e1dd4707`

**Step 2: Decide whether Ralph Loop and image infrastructure stay**

If Ralph Loop or local Gemini image UX is not required, this is the cleanest major bucket to reduce after provider/account-pool is frozen.

**Step 3: Decide whether Anthropic native integration stays**

Anthropic native support is also a local line and interacts with the provider stack. If it stays, it should stay on top of the preserved pool logic.

**Step 4: Keep prompt and agent-role changes only if backed by an active product feature**

Prompt/builtin/model-template changes should be evaluated feature-by-feature instead of preserved automatically.

### Task 5: Finish Remaining Upstream Alignment Slices

**Files:**
- Modify: remaining files from `git diff --name-only upstream/main...HEAD`

**Step 1: After freezing the preserved stacks, re-run the diff**

Run:
```bash
git diff --stat --compact-summary upstream/main...HEAD
```

Expected: a smaller diff focused on intentional local product areas

**Step 2: Merge remaining upstream-compatible slices one bucket at a time**

Preferred order:
1. provider/account-pool preserved baseline
2. Entire/memory preserved baseline
3. protocol/hooks/app-server keep-vs-drop cleanup
4. Gemini/Ralph Loop/prompt triage
5. residual app-server and role/model cleanup

**Step 3: Only request full-workspace verification after the user agrees**

If common/core/protocol changes accumulate and the preserved local state is stable, ask before running the full workspace suite:

```bash
cargo test
```

### Task 6: Final Verification and Commit Hygiene

**Files:**
- Modify: any files touched by cleanup

**Step 1: Run required formatting and linting for Rust edits**

Run:
```bash
cd codex-rs
just fmt
just fix -p codex-core
PATH=$HOME/.local/share/cargo/bin:$PATH ./tools/argument-comment-lint/run.sh
```

Expected: PASS

**Step 2: Make slice commits with explicit scope**

Do not create one giant merge-reconciliation commit. Commit by bucket with subjects that explain whether the slice preserves a local customization or realigns to upstream.

**Step 3: Ensure every commit message ends with the required trailer**

```text
Co-authored-by: Codex <noreply@openai.com>
```
