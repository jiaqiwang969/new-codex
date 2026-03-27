# TUI Preset Vouch Prune Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the local TUI-only preset/vouch layer while preserving core `model_sub` and `model_sub_responses` routing semantics.

**Architecture:** Limit the change to `codex-rs/tui` so the cleanup only removes `/team-profile`, `/team-vouch`, model-sub auto-vouch helpers, related ledgers, and status/popup UX. Keep `codex-rs/core`, provider routing, app-server config wire fields, and direct utility-model persistence untouched.

**Tech Stack:** Rust, ratatui TUI, insta snapshots, cargo test, just fmt

---

### Task 1: Remove slash-command entry points and app events

**Files:**
- Modify: `codex-rs/tui/src/slash_command.rs`
- Modify: `codex-rs/tui/src/app_event.rs`
- Modify: `codex-rs/tui/src/lib.rs`

**Step 1: Remove the obsolete commands and event variants**

- Delete `SlashCommand::TeamProfile` and `SlashCommand::TeamVouch`.
- Delete `PersistTeamProfileSelection`, `RecordTeamProfileVouch`, `RecordTeamProfileDuelVouch`, `RecordModelSubVouch`, and `RecordModelSubDuelVouch`.
- Remove module declarations for `team_profile`, `team_profile_vouch`, and `model_sub_vouch`.

**Step 2: Build the minimal compile surface in TUI**

Run: `cargo test -p codex-tui slash_command -- --nocapture`
Expected: either pass or fail only on downstream references that still need cleanup.

### Task 2: Remove app/chatwidget wiring for preset and vouch flows

**Files:**
- Modify: `codex-rs/tui/src/app.rs`
- Modify: `codex-rs/tui/src/chatwidget.rs`
- Modify: `codex-rs/tui/src/bottom_pane/feedback_view.rs`

**Step 1: Delete the app-side handlers**

- Remove event handling branches for team-profile persistence and both vouch ledgers.

**Step 2: Delete the chatwidget command parsers and popup flow**

- Remove `/team-profile` handling.
- Remove `/team-vouch` handling.
- Remove `/model-sub auto|recommended` support that depends on vouch snapshots.
- Keep direct `/model-sub <slug>` and direct popup-based selection.

**Step 3: Remove feedback-driven team-profile scoring**

- Delete the automatic feedback hook that records team-profile verdicts.

**Step 4: Verify the targeted surface**

Run: `cargo test -p codex-tui chatwidget -- --nocapture`
Expected: fail only where snapshots or status expectations still reference the removed UX.

### Task 3: Remove status-card and ledger modules, then update tests

**Files:**
- Delete: `codex-rs/tui/src/team_profile.rs`
- Delete: `codex-rs/tui/src/team_profile_vouch.rs`
- Delete: `codex-rs/tui/src/model_sub_vouch.rs`
- Modify: `codex-rs/tui/src/status/card.rs`
- Modify: `codex-rs/tui/src/status/tests.rs`
- Delete: `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__team_profile_selection_popup.snap`
- Delete: `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__team_profile_selection_popup_prefers_vouched_profile.snap`

**Step 1: Remove status-card fields that only describe preset/vouch state**

- Delete team-profile display rows.
- Delete model-sub-vouch-derived auto source and recommendation rows.
- Keep configured/effective `model_sub` and `model_sub_responses` display.

**Step 2: Remove module-local tests and snapshot references**

- Delete or rewrite tests that mention team-profile popups or vouch-ledger status lines.

**Step 3: Run the crate tests and snapshot checks**

Run: `cargo test -p codex-tui`
Expected: PASS

### Task 4: Format and final verification

**Files:**
- Modify: `docs/plans/2026-03-24-upstream-merge-live-customization-inventory.md`
- Modify: `docs/plans/2026-03-26-model-sub-ux-analysis.md`

**Step 1: Keep the docs aligned with what actually shipped**

- If implementation scope changes, update the docs to match the code.

**Step 2: Run required local hygiene**

Run: `cd codex-rs && just fmt`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-tui`
Expected: PASS

Run: `cd codex-rs && PATH="$HOME/.local/share/cargo/bin:$PATH" just argument-comment-lint-from-source`
Expected: PASS

**Step 3: Commit**

```bash
git add docs/plans/2026-03-26-tui-preset-vouch-prune-implementation.md \
        codex-rs/tui/src
git commit -m "refactor: prune local tui preset and vouch ux"
```
