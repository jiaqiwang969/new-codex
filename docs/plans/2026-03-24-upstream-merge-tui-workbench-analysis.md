# TUI Workbench Merge Analysis

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Goal of this note:** explain which TUI enhancements are genuinely local
> assets, where they attach to the main TUI runtime, and why this block is
> lower-risk than protocol work but still a merge hotspot.

## Bottom Line

The local TUI workbench line has three main assets:

1. `git-graph`
2. `session bar`
3. `ralph-loop`

The good news:

- each feature has a fairly self-contained core implementation

The bad news:

- they all attach through very large TUI orchestration files

That means:

- feature logic is preservable
- integration churn is where merges hurt

## What The Local Branch Adds

### 1. Git Graph

Representative files:

- `codex-rs/git-graph/**`
- `codex-rs/tui/src/git_graph_widget.rs`

Behavior:

- a dedicated git graph crate
- TUI overlay opened with `Ctrl+G`
- visual commit graph presentation inside the workbench

Judgment:

- this is a strong local differentiator
- the standalone crate is relatively safe to preserve

### 2. Session Bar

Representative files:

- `codex-rs/tui/src/session_bar.rs`
- `codex-rs/tui/src/session_utils.rs`

Behavior:

- tmux-style bottom bar
- session list/navigation
- current cwd-aware session discovery
- prefetch so `Ctrl+P` feels instant

Judgment:

- this is a workflow enhancement, not a protocol fork
- likely worth preserving with minimal architectural controversy

### 3. Ralph Loop

Representative files:

- `codex-rs/tui/src/ralph_loop.rs`
- `codex-rs/tui/src/chatwidget.rs`

Behavior:

- iterative self-correction loop
- prompt resubmission until a completion promise is observed
- bounded retries / delays / cancellation

Judgment:

- this is a very local workflow tool
- low coupling to upstream platform goals
- worth preserving if the user still wants it

## Where The Real Merge Pain Is

### `app.rs`

Representative integration points:

- background warmup for session data
- `Ctrl+G` overlay open path
- `Ctrl+P` session bar toggle/focus
- current thread/session state wiring

Problem:

- `app.rs` is already a central high-churn orchestration file upstream
- local workbench integrations add yet more responsibilities there

### `chatwidget.rs`

Representative integration points:

- Ralph Loop command parsing
- Ralph Loop activation / cancellation / continuation
- loop-state lifecycle inside task completion

Problem:

- `chatwidget.rs` is one of the single largest conflict magnets in the fork
- it already absorbs unrelated TUI, command, and workflow changes

## What Problem The Local Author Was Solving

This branch is optimizing for Codex as a daily engineering workbench:

- inspect repo history without leaving the TUI
- switch sessions quickly inside the current project
- drive iterative self-repair loops without retyping prompts

This is a different focus from upstream's broader multi-client/platform
evolution. It is more opinionated, but it is also very coherent for heavy local
developer usage.

## What Upstream Is Optimizing For

Upstream TUI work continues to prioritize:

- shared interaction patterns
- generalized multi-agent/session behavior
- app-server-aligned event handling
- continued evolution of the main terminal UX

That means upstream is not directly opposed to these features, but it is also
not organized around preserving this exact workbench model.

## What Must Be Preserved

If this block is kept, the semantics worth preserving are:

1. `git-graph` as a first-class local repo visualization tool
2. `session bar` as a fast session navigation surface
3. cwd-aware session discovery and prefetch
4. `ralph-loop` as a reusable iterative workflow command

## What Can Change

These implementation details can be adapted freely:

- exact keybinding plumbing in `app.rs`
- exact overlay rendering details
- exact session bar layout
- exact Ralph Loop status wording
- exact integration hooks inside `chatwidget.rs`

The features are more important than the current wiring shape.

## Recommended Merge Strategy

### Keep the self-contained cores first

Preserve first:

1. `codex-rs/git-graph/**`
2. `codex-rs/tui/src/git_graph_widget.rs`
3. `codex-rs/tui/src/session_bar.rs`
4. `codex-rs/tui/src/session_utils.rs`
5. `codex-rs/tui/src/ralph_loop.rs`

### Reattach them to the current TUI shell second

Only after that, adapt:

- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`

This keeps feature logic from being lost inside giant conflict resolutions.

### Avoid treating current `app.rs` / `chatwidget.rs` diffs as sacred

Those files are too large and too shared.

Prefer:

- preserve behavior
- re-integrate against current upstream TUI structure

instead of:

- replaying the old giant diffs verbatim

## Main Risk If We Merge Poorly

The main failure mode is feature survival with bad ergonomics:

- `git-graph` still compiles but is no longer reachable
- `session bar` exists but no longer reflects current thread/cwd correctly
- `ralph-loop` still parses but no longer continues correctly after task
  completion

These are easy to miss if merge validation only checks compile success.

## Current Judgment

This TUI workbench block is one of the safer local asset groups to keep.

It is not the place to simplify by deletion unless intentionally desired.

The right move is:

- preserve the features
- stop preserving the exact current giant integration diffs

That will reduce future upstream merge pain without giving up the workbench
experience.
