# Entire Integration Architecture

## Overview

Codex integrates with [Entire CLI](https://github.com/jiaqiwang969/cli) to provide AI session history tracking with WHY-focused summaries. This document explains the architecture and usage.

## What is Entire?

Entire captures AI session snapshots (prompts, responses, model info, file diffs) and stores them on hidden git branches. When you commit code, a `prepare-commit-msg` hook links the commit to the AI session via an `Entire-Checkpoint` trailer.

**Git vs Entire:**
- **Git**: What code changed → code history
- **Entire**: Why code changed (AI context) → session history
- Together: Complete bidirectional traceability

## Architecture Components

### 1. Entire CLI (External)

Installed separately via `npm install -g @entire/cli`. Configured in `~/.codex/config.toml`:

```toml
notify = ["entire", "hooks", "codex", "notify"]
```

Entire runs automatically via the notify hook after each agent turn.

### 2. Entire Integration (codex-rs)

**Module**: `core/src/entire_integration.rs`

Queries Entire checkpoints and formats them for context packets:

```rust
pub fn query_entire_checkpoints(
    repo_path: &Path,
    max_checkpoints: usize,
) -> Result<Vec<EntireCheckpoint>>
```

Integrated into leader/default agent context via `context_packet.rs`.

### 3. Entire Summary Generation (codex-rs)

**Module**: `hooks/src/entire_summary.rs`

Generates WHY-focused summaries for checkpoints:

```rust
pub async fn generate_summary(
    input: EntireSummaryInput,
    model: &str,
) -> Result<EntireSummary>
```

**WHY-Focused Prompt** captures:
1. **MOTIVATION**: Why did user need this?
2. **APPROACH**: What solution chosen? Alternatives considered?
3. **CHALLENGES**: What obstacles? How overcome?
4. **TRADEOFFS**: What compromises? Why acceptable?
5. **OUTCOME**: What accomplished? Key insight?

### 4. Configuration

**File**: `core/src/config/types.rs`

```toml
[memories]
entire_summary_enabled = true
entire_summary_model = "claude-sonnet-4-6"  # Optional, defaults to model_sub
```

**Fallback chain**:
1. `memories.entire_summary_model` (explicit)
2. `model_sub` (general utility model)
3. `DEFAULT_MEMORY_PHASE_TWO_MODEL` (built-in default)

### 5. Runtime Model Selection

**Command**: `/model-entire`

Opens a popup to select the model for Entire summaries:
- Shows all available models
- "Inherit" option uses fallback chain
- Persists to `config.toml` under `[memories]`

**Implementation**:
- `tui/src/slash_command.rs`: Command definition
- `tui/src/chatwidget.rs`: Popup UI
- `tui/src/app.rs`: Event handling
- `core/src/config/edit.rs`: Config persistence

### 6. Status Display

**File**: `tui/src/status/card.rs`

Shows current Entire summary model in status card:

```
Entire summary    claude-sonnet-4-6 (memories.entire_summary_model)
```

## Data Flow

### Agent Turn Complete

```
1. Agent completes turn
   ↓
2. codex-rs fires notify hook
   ↓
3. Entire CLI creates checkpoint
   ↓
4. [TODO] codex-rs generates WHY summary
   ↓
5. Summary stored alongside checkpoint
```

### Context Packet Generation

```
1. Leader agent spawned
   ↓
2. Context packet builder queries Entire
   ↓
3. Recent checkpoints loaded (max 5)
   ↓
4. Formatted as markdown section
   ↓
5. Included in system prompt
```

## Configuration Examples

### Minimal (Use Defaults)

```toml
notify = ["entire", "hooks", "codex", "notify"]
model_sub = "claude-sonnet-4-6"

[memories]
entire_summary_enabled = true
# entire_summary_model inherits from model_sub
```

### Explicit Model Selection

```toml
notify = ["entire", "hooks", "codex", "notify"]
model_sub = "claude-sonnet-4-6"

[memories]
entire_summary_enabled = true
entire_summary_model = "claude-opus-4-6"  # Use Opus for deeper analysis
```

### Disable Entire Summaries

```toml
notify = ["entire", "hooks", "codex", "notify"]

[memories]
entire_summary_enabled = false
```

## Usage

### View Entire Status

```bash
entire status
```

### Explain a Commit

```bash
entire explain <commit-hash>
```

Shows the AI session that produced the commit.

### Resume Previous Session

```bash
entire resume <branch>
```

### Rewind Changes

```bash
entire rewind
```

### Change Summary Model at Runtime

In codex TUI:

```
/model-entire
```

Select from available models or choose "Inherit" to use fallback chain.

## Implementation Status

### ✅ Completed

- Entire checkpoint querying and context integration
- Configuration schema for `entire_summary_model`
- `/model-entire` command and UI
- Config persistence via `ConfigEdit::SetEntireSummaryModel`
- Status card display
- WHY-focused prompt template

### 🔄 In Progress

- Async summary generation in notify hook flow
- Integration with codex-rs utility_model system
- Summary storage and retrieval

### 📋 TODO

- Hook integration to trigger summary generation
- Model invocation via utility_model
- Summary caching and invalidation
- Error handling and retry logic
- Performance optimization for large repos

## Design Decisions

### Why Internal Models?

Use codex-rs internal models (not external Entire CLI) for summary generation:
- **Consistency**: Same model infrastructure as other codex features
- **Configuration**: Unified model selection via `/model-entire`
- **Performance**: No external process overhead
- **Control**: Better error handling and retry logic

### Why WHY-Focused?

Entire's default prompt focuses on WHAT (intent, outcome, learnings). Our enhancement adds WHY:
- **Decision Rationale**: Why this approach over alternatives?
- **Tradeoffs**: What compromises were made?
- **Context**: Why did the user need this?

This provides richer context for future AI sessions.

### Why Async Generation?

Summary generation happens in background after checkpoint creation:
- **Non-blocking**: Don't slow down agent turns
- **Resilient**: Failures don't break main flow
- **Scalable**: Can batch multiple summaries

## Troubleshooting

### Entire Not Running

```bash
entire doctor
```

Checks installation and git hooks.

### Summary Model Not Working

Check configuration:

```bash
codex config get memories.entire_summary_model
```

Verify model is available:

```
/model-entire
```

### Checkpoints Not Appearing in Context

Check configuration:

```toml
[memories]
include_entire_summary = true  # Default: true
max_entire_checkpoints = 5     # Default: 5
max_entire_summary_bytes = 8192  # Default: 8192
```

## References

- [Entire CLI](https://github.com/jiaqiwang969/cli)
- [Codex Multi-Agent System](./multi-agent.md)
- [Model Selection](./model-selection.md)
