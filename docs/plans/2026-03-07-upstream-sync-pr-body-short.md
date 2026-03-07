## Summary

- align this customized Codex fork with current upstream content
- preserve and modernize custom capabilities including account pool, memory, collaboration/model_sub, Entire summaries, and git-graph
- keep the remaining delta against `upstream/main` intentional, narrow, and easier to maintain

## Highlights

- restored custom capability seams on top of the current upstream architecture
- absorbed upstream improvements in permissions, RMCP recovery, plugin marketplace backend, realtime startup context handling, protocol/schema generation, telemetry, and sqlite/runtime behavior
- kept upstream-preferred UX where it was better, including leaving `Ctrl+G` mapped to the external editor and exposing git-graph through `/graph`

## Verification

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

## Notes

- a follow-up content audit found no remaining upstream commits whose touched files still differ from this branch
- current `ahead/behind` graph divergence is historical and should not be interpreted as missing upstream content
