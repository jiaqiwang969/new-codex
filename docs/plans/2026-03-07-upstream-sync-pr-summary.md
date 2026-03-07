# Upstream Sync PR Summary

**Branch:** `integration/upstream-main-b2`
**Suggested base:** `main`
**Suggested PR title:** `Align customized Codex fork with upstream/main while preserving custom capabilities`

## Summary

- align the customized Codex branch to current upstream content while preserving and modernizing differentiators such as account pool, memory continuity, collaboration/model_sub flows, Entire hooks/summaries, and git-graph
- absorb upstream improvements in permissions, RMCP streamable HTTP recovery, plugin marketplace backend, app-server schema/runtime behavior, realtime startup context handling, telemetry/logging, sqlite/runtime behavior, and related protocol surfaces where upstream is now canonical or better
- keep intentional custom deltas narrow and reviewable so future upstream syncs are cheaper and less conflict-prone

## Custom capabilities preserved

- **Account pool / provider routing**
  - provider account pool primitives
  - provider-specific auth fallback and persistence overlay
- **Memory**
  - thread memory persistence
  - `MemoryLink` propagation through turns, app-server history, and multi-agent APIs
- **Collaboration / model_sub**
  - subagent model override support
  - calibration / vouch tools and memory continuity return values
- **Entire integration**
  - historical summary backfill
  - summary generation for mutating notify turns
  - side-effect-aware metadata refresh
- **Git graph**
  - restored standalone crate and TUI integration via `/graph`
  - intentionally keep upstream `Ctrl+G` behavior instead of reviving the legacy hotkey conflict

## Key upstream-alignment batches landed

- permission-model alignment and coverage restoration
- RMCP streamable HTTP session recovery
- curated plugin marketplace backend alignment
- realtime websocket startup context override support
- protocol/schema refreshes for app-server v2 and generated artifacts
- sqlite/runtime and telemetry-related upstream changes already effectively absorbed in content

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

## Review notes

- current branch state can still show `ahead/behind` divergence against `upstream/main`, but a follow-up content audit found no remaining upstream commits whose touched files still differ from this branch
- remaining delta against `upstream/main` is therefore intentional custom functionality plus related generated artifacts and docs, not unabsorbed upstream code
- if reviewers want an additional confidence pass, the next highest-value verification is a full workspace `cargo test` / `just test` in an environment with a higher file-descriptor limit and stable seatbelt/zsh behavior

## Merge checklist

- [ ] review the preserved custom seams: account pool, memory, collaboration/model_sub, Entire integration, git-graph
- [ ] spot-check generated artifacts under `codex-rs/app-server-protocol/schema/` and `codex-rs/core/config.schema.json`
- [ ] confirm `docs/plans/2026-03-07-upstream-sync-handoff.md` reflects the final audit and verification state
- [ ] decide whether to merge directly, open a PR, or keep the integration branch as the sync branch of record
