# Next Upstream Alignment Batch Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Continue aligning this customized Codex fork with `upstream/main` in small, low-risk batches while preserving and improving custom capabilities like git-graph, Entire hooks/summaries, account pool, collaboration, and memory.

**Architecture:** Work from the current `integration/upstream-main-b2` branch and prefer upstream commits that either apply cleanly or are already effectively present. For each candidate, verify whether our customized tree already covers the behavior before cherry-picking or porting. Keep each batch small, test the touched Rust crates, and preserve local custom flows unless upstream is clearly better.

**Tech Stack:** Git, Rust/Cargo, Codex CLI, workspace tests, config schema generation.

---

### Task 1: Identify the next safe upstream batch

**Files:**
- Inspect: `git log`, `git cherry`, current branch history
- Review: likely touched files under `codex-rs/`, `docs/`, and custom integration areas

**Step 1: List candidate upstream commits after the last integrated batch**

Run: `git log --oneline upstream/main --not HEAD --reverse | head -n 40`
Expected: a chronological list of upstream commits not yet aligned locally.

**Step 2: Rank candidates by risk**

Run: compare touched files and skip commits that overlap heavily with custom systems (`memory`, `collaboration`, account pool, Entire hooks) unless clearly beneficial.
Expected: a short list of low-risk commits or docs cleanups.

**Step 3: Confirm whether each candidate is already effectively present**

Run: inspect local code/tests before applying.
Expected: either “already covered” or “needs alignment”.

**Step 4: Commit selection checkpoint**

Choose one small batch that can be validated quickly.

### Task 2: Apply the selected batch with minimal churn

**Files:**
- Modify only the files required by the chosen upstream commit(s)
- Update tests and generated artifacts if config/API shapes change

**Step 1: Write or update failing tests first where behavior changes**

Expected: a failing targeted test or a clear pre-change behavioral gap.

**Step 2: Implement the minimal alignment**

Expected: preserve custom behavior where superior, but match upstream intent where it improves safety, correctness, or maintainability.

**Step 3: Run formatters/generators required by scope**

Run: `cd codex-rs && just fmt`
Expected: formatting clean.

**Step 4: Run targeted verification**

Run the touched crate tests first.
Expected: green targeted verification.

**Step 5: Commit the batch**

Expected: one focused commit with the required Codex co-author trailer.

### Task 3: Final verification and handoff

**Files:**
- Inspect: `git status --short`, `git log --oneline -5`

**Step 1: Re-run the proof command for any success claim**

Expected: fresh evidence for tests/build relevant to the batch.

**Step 2: Ensure the worktree is clean**

Run: `git status --short`
Expected: no uncommitted changes.

**Step 3: Summarize what aligned and what was intentionally skipped**

Expected: clear handoff for the next batch.
