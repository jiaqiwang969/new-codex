# Git Graph Ctrl+G Realignment Design

## Goal

Resolve the first hard conflict between the local workbench line and
`upstream/main` by restoring the official `Ctrl+G` external-editor behavior in
the TUI.

## Context

The current branch wires `Ctrl+G` in `codex-rs/tui/src/app.rs` to a custom
`git-graph` overlay. Upstream uses the same shortcut for launching the external
editor, and `codex-rs/tui_app_server` still follows that official behavior.

That leaves the repo in an inconsistent state:

- TUI: `Ctrl+G` opens git graph
- TUI app-server: `Ctrl+G` launches the external editor
- footer/help text: still describes the official external-editor shortcut

## Decision

For this slice, align with upstream and restore `Ctrl+G -> external editor` in
the TUI.

`git-graph` is not being deleted as a concept in this slice. It is being
removed from the main merge surface so it no longer blocks official alignment on
the primary key path.

## Scope

In scope:

- restore the TUI `Ctrl+G` key path to request the external editor
- restore the missing helper/hint wiring needed by that flow
- add regression coverage proving the TUI requests the external editor instead
  of opening an overlay

Out of scope:

- deciding a replacement shortcut for `git-graph`
- deleting the vendored `codex-rs/git-graph` tree
- workspace dependency cleanup beyond what is strictly needed for this behavior

For avoidance of doubt: this slice may remove `git-graph` from the active Rust
workspace/build graph while still keeping the source tree parked in the repo.

## Rationale

This follows the user's stated merge strategy:

- prefer upstream behavior where local workbench experiments conflict
- keep account-pool and other critical custom work
- stop carrying failed or stale customizations in the mainline path

The external editor is official, already mirrored in `tui_app_server`, and
already documented in the footer. Restoring it reduces divergence immediately.
