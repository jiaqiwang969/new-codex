# Antigravity Gemini Model Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace old Antigravity Gemini `-high` / `-low` public model slugs with official-style Gemini slugs and move strength selection entirely under reasoning effort.

**Architecture:** Update picker presets to expose a single public slug per Antigravity Gemini family, keep Gemini fallback metadata family-based by normalizing the provider prefix, and rely on the existing Gemini `thinkingLevel` mapping from reasoning effort instead of model-name suffixes.

**Tech Stack:** Rust (`codex-core`), static model preset metadata, fallback model metadata, unit tests.

---

### Task 1: Lock the new picker contract with failing preset tests

**Files:**
- Modify: `core/src/models_manager/model_presets.rs`

**Step 1: Write the failing test**

Add a test that asserts:

- `antigravity/gemini-3.1-pro-preview` exists
- `antigravity/gemini-3-pro-preview` exists
- old `antigravity/gemini-3.1-pro-high`
- old `antigravity/gemini-3.1-pro-low`
- old `antigravity/gemini-3-pro-high`

are absent from the preset list.

Also assert that the new presets expose `Low`, `Medium`, `High` reasoning
efforts instead of encoding strength in the slug.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-core antigravity_gemini
```

Expected: FAIL because presets still expose old `-high` / `-low` slugs.

**Step 3: Write minimal implementation**

Replace the old Antigravity Gemini preset entries in
`core/src/models_manager/model_presets.rs` with official-style public slugs and
public display names. Keep the supported reasoning efforts on the preset.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-core antigravity_gemini
```

Expected: PASS for the updated preset contract.

### Task 2: Lock fallback model metadata to the new public slugs

**Files:**
- Modify: `core/src/models_manager/model_info.rs`

**Step 1: Write the failing test**

Update the Antigravity Gemini fallback metadata test so it uses
`antigravity/gemini-3.1-pro-preview` and asserts:

- slug is preserved exactly
- provider prefix normalization still routes it through Gemini metadata
- `used_fallback_model_metadata` is `false`
- default reasoning stays `High`
- Gemini tool support remains intact

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-core antigravity_gemini_models_reuse_gemini_metadata_without_fallback
```

Expected: FAIL if the test still references the old slug or metadata assumptions
have drifted.

**Step 3: Write minimal implementation**

Adjust `core/src/models_manager/model_info.rs` tests and any related metadata
assumptions so the new public Antigravity Gemini slug is the canonical test
case.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-core antigravity_gemini_models_reuse_gemini_metadata_without_fallback
```

Expected: PASS and prove the new public slug reuses Gemini-family metadata.

### Task 3: Update runtime normalization tests that still encode old slugs

**Files:**
- Modify: `core/src/codex.rs`
- Verify: `core/src/gemini_content.rs`

**Step 1: Write the failing test**

Update the server-model mismatch normalization test so the Gemini case uses the
new public Antigravity Gemini slug and the matching upstream Gemini model slug.

If needed, add a focused assertion around `strip_model_suffix()` to confirm the
new slug format still normalizes to the correct upstream Gemini model name.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p codex-core server_model_warning_ignores_non_responses_prefix_only_differences
```

Expected: FAIL until tests reference the new public slug shape.

**Step 3: Write minimal implementation**

Update the test inputs and only touch runtime normalization code if the new
slug format exposes a real gap.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p codex-core server_model_warning_ignores_non_responses_prefix_only_differences
```

Expected: PASS with the new public slug format.

### Task 4: Run focused regression verification

**Files:**
- Modify: any files touched in Tasks 1-3

**Step 1: Run the focused models-manager tests**

Run:

```bash
cargo test -p codex-core model_presets
cargo test -p codex-core model_info
```

Expected: PASS for the changed preset and fallback metadata coverage.

**Step 2: Run the focused Gemini/runtime tests**

Run:

```bash
cargo test -p codex-core gemini
cargo test -p codex-core server_model_warning
```

Expected: PASS for Gemini request normalization and mismatch warnings.

**Step 3: Format if needed**

Run:

```bash
cargo fmt --all
```

Expected: no formatting diffs remain.

**Step 4: Commit**

```bash
git add docs/plans/2026-03-16-antigravity-gemini-model-alignment-design.md docs/plans/2026-03-16-antigravity-gemini-model-alignment.md core/src/models_manager/model_presets.rs core/src/models_manager/model_info.rs core/src/codex.rs
git commit -m "refactor: align antigravity gemini model naming"
```

Expected: a commit that contains only the Gemini model-alignment change set.
