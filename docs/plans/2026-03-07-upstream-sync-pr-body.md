## Summary

- align this customized Codex fork with the current upstream content while preserving and modernizing key differentiators such as account pool, memory continuity, collaboration/model_sub flows, Entire hooks/summaries, and git-graph
- absorb upstream improvements in permissions, RMCP streamable HTTP recovery, plugin marketplace backend, app-server protocol/schema behavior, realtime websocket startup context handling, telemetry/logging, and sqlite/runtime behavior where upstream is now canonical or better
- narrow the intentional custom delta so future upstream syncs are cheaper, more reviewable, and less conflict-prone

## What Changed

- restored and modernized custom capability seams
  - account pool / provider routing
  - memory persistence and `MemoryLink` propagation
  - collaboration / subagent / `model_sub` calibration flows
  - Entire summary backfill and side-effect-aware turn metadata refresh
  - git-graph via standalone crate and TUI `/graph`
- aligned upstream behavior in key batches
  - permission model alignment and coverage restoration
  - RMCP streamable HTTP session recovery
  - curated plugin marketplace backend alignment
  - realtime websocket startup context override support
  - app-server v2 protocol/schema refreshes and generated artifact updates
- preserved upstream-preferred UX where better
  - keep upstream `Ctrl+G` external-editor behavior instead of reintroducing the legacy git-graph hotkey conflict

## Verification

Validated on this branch in `codex-rs`:

- `cargo test -p codex-app-server`
- `cargo test -p codex-tui`
- `cargo test -p codex-git-graph`
- `cargo test -p codex-app-server-protocol`
- `cargo test -p codex-hooks`
- `cargo test -p codex-state`
- `cargo test -p codex-protocol`
- `cargo test -p codex-exec`
- `ulimit -n 4096 && cargo test -p codex-core`
- `just write-config-schema`
- `just write-app-server-schema`
- `just fix -p codex-core`
- `just fix -p codex-app-server`
- `just fix -p codex-app-server-protocol`
- `just fix -p codex-tui`
- `just fmt`

Fresh `codex-core` full-crate result under raised `nofile`:

- lib tests: `1530 passed; 0 failed; 5 ignored`
- `tests/all.rs`: `738 passed; 0 failed; 18 ignored`
- `responses_headers`: `4 passed; 0 failed`

## Review Notes

- a follow-up content audit found no remaining upstream commits whose touched files still differ from this branch
- the remaining delta against `upstream/main` is therefore intentional custom functionality plus related generated artifacts and docs, not unabsorbed upstream code
- current `ahead/behind` graph divergence is historical/structural and should not be interpreted as missing upstream content

## Suggested Review Focus

- account pool and provider routing behavior
- memory continuity and app-server v2 turn/history exposure
- collaboration / `model_sub` tools and related protocol surfaces
- Entire summaries, tool side effects, and turn metadata refresh behavior
- git-graph restoration via `/graph`
