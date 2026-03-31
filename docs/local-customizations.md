# Local Customizations

This repository carries a number of local Codex workflow extensions on top of
upstream `main`. Upstream product docs remain the baseline for general Codex
usage; this page only calls out the local behaviors that are easy to miss when
working from upstream docs alone.

## Current source of truth

As of 2026-03-31, this page is the live capability map for local behavior on
`main`.

- Use this page for the current runtime and config shape.
- Use `docs/plans/` for merge history, implementation rationale, and archived
  execution notes.
- Treat the old detached archive branch and pre-merge worktree notes as
  historical material, not as the current product surface.

## Active local capability map

The current merge inventory still treats these areas as intentional local
customization, not accidental fork residue:

- account-pool overlays and provider failover
- provider-family expansion plus utility-model routing (`model_sub`,
  `model_sub_responses`)
- memory, context-packet, and Entire checkpoint integration
- approval-runtime closure for destructive tool flows
- guardian approval review replay in app-server extended history / TUI replay
- selected multi-agent extensions

The old local `smart-access`, `endpoint-sec`, and `/freeze` line is no longer
part of the runtime. Remaining mentions of that direction should be treated as
historical notes only.

What survived from that old direction is narrower:

- the deleted Smart Access product mode did not return
- but `main` now includes an internal runtime companion for destructive
  approval closure under the existing guardian + exec-policy architecture

Older merge notes that discuss agent worktrees or TUI workbench reattachment
describe preservation candidates from the detached archive branch, not features
that remain in the current `main` runtime.

## Account pool overlays

Provider account-pool routing is documented in [config.md](./config.md#provider-account-pool)
and the runnable examples live under
[`codex-rs/config-examples/`](../codex-rs/config-examples/README.md).

Local behavior to keep in mind:

- `~/.codex/config.toml` keeps the logical provider selection.
- `~/.codex/config-pool.toml` carries operational `account_pool` entries and
  endpoint overrides.
- `~/.codex/auth-pool.json` carries the API keys referenced by pool `env_key`
  values.
- Pool order is the source of truth. Each new turn starts from the first pool
  entry again.
- Failed accounts cool down for 1 minute in the current session. If every
  account is still cooling down, Codex still forces a fresh probe from the
  first account instead of failing immediately.

## Utility model overrides

This fork keeps two repo-specific utility-model config keys:

```toml
model_sub = "claude-sonnet-4-6"
model_sub_responses = "gpt-5.1-codex-mini"

[memories]
entire_summary_model = "claude-sonnet-4-6"
entire_summary_enabled = true
```

Semantics, verified from the current config schema and runtime:

- `model_sub` overrides the utility model used for internal tasks such as
  memory fallback work.
- `model_sub_responses` overrides only internal tasks that require the
  Responses API.
- `model_sub_responses` must be an OpenAI / Responses-compatible slug. If you
  point it at a non-Responses model, Codex clears the override and emits a
  startup warning.
- `memories.entire_summary_model` defaults to `model_sub` when unset.
- `memories.entire_summary_enabled` controls AI-generated WHY summaries for
  Entire checkpoints.

The example configuration lives in
[`codex-rs/config-examples/config.toml`](../codex-rs/config-examples/config.toml).

## Entire integration

This fork keeps the local Entire flow for AI-session checkpoint summaries and
context replay.

Useful entry points:

- Example config and hook wiring:
  [`codex-rs/config-examples/README.md`](../codex-rs/config-examples/README.md)
- Runtime summary generation:
  [`codex-rs/core/src/entire_summary_generator.rs`](../codex-rs/core/src/entire_summary_generator.rs)
- Checkpoint/history integration:
  [`codex-rs/core/src/entire_integration.rs`](../codex-rs/core/src/entire_integration.rs)

## Approval runtime and guardian replay

Current `main` also keeps two landed extensions around approval closure and
history reconstruction:

- an internal `approval_runtime` layer that adds runtime health, lease, permit,
  and fallback-to-human handling for destructive tool execution
- replayable `guardianApprovalReview` thread items in app-server extended
  history so resume/fork/TUI replay can reconstruct guardian review state

Useful entry points:

- Runtime companion:
  [`codex-rs/core/src/approval_runtime/`](../codex-rs/core/src/approval_runtime/)
- Destructive tool integration:
  [`codex-rs/core/src/tools/runtimes/shell.rs`](../codex-rs/core/src/tools/runtimes/shell.rs)
- Unified-exec runtime finish handling:
  [`codex-rs/core/src/unified_exec/async_watcher.rs`](../codex-rs/core/src/unified_exec/async_watcher.rs)
- Replay reconstruction:
  [`codex-rs/app-server-protocol/src/protocol/thread_history.rs`](../codex-rs/app-server-protocol/src/protocol/thread_history.rs)
- App-server contract notes:
  [`codex-rs/app-server/README.md`](../codex-rs/app-server/README.md)

## Example MCP presets

The local example config also carries two repo-specific MCP launcher presets in
[`codex-rs/config-examples/config.toml`](../codex-rs/config-examples/config.toml):

- `claude-code-mcp` via `npx -y @steipete/claude-code-mcp@latest`
- `watermark-remover` via `npx -y github:jiaqiwang969/watermark-removal-mcp`

The supporting notes live in
[`codex-rs/config-examples/README.md`](../codex-rs/config-examples/README.md).
Treat them as local example integrations, not upstream Codex defaults.

## Merge-analysis notes

The preserved upstream-merge analysis lives in `docs/plans/`. It remains worth
reading before touching provider routing, utility models, memory/Entire
wiring, or collaboration-related local extensions, but some notes also capture
preservation ideas that were later dropped from `main`:

- [2026-03-24-upstream-merge-live-customization-inventory.md](./plans/2026-03-24-upstream-merge-live-customization-inventory.md)
- [2026-03-24-upstream-merge-account-pool-analysis.md](./plans/2026-03-24-upstream-merge-account-pool-analysis.md)
- [2026-03-24-upstream-merge-provider-family-utility-routing-analysis.md](./plans/2026-03-24-upstream-merge-provider-family-utility-routing-analysis.md)
- [2026-03-24-upstream-merge-memory-entire-analysis.md](./plans/2026-03-24-upstream-merge-memory-entire-analysis.md)
- [2026-03-24-upstream-merge-tui-workbench-analysis.md](./plans/2026-03-24-upstream-merge-tui-workbench-analysis.md)

Treat those notes as historical merge context. This page is the current map of
what is intentionally local versus what should follow upstream structure as-is.
