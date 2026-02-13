# UML Diagrams

This folder contains PlantUML diagrams that explain the relationships between:

- `rollout.jsonl` (append-only source of truth)
- `state.sqlite` (projection/index + jobs/locks coordination)
- `memories/*/memory_summary.md` (active memory injected into turns)
- `hooks` (external automation/event bus)
- `codex-app-server` (JSON-RPC v2 control plane)

Generated artifacts:

- `*.puml` are the sources.
- `*.svg` and `*.png` are generated with PlantUML for easy viewing.

Regenerate:

```bash
plantuml -tsvg docs/uml/*.puml
plantuml -tpng docs/uml/*.puml
```

