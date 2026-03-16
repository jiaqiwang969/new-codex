# Antigravity Gemini Model Alignment

## Goal

Align Antigravity Gemini model exposure in Codex with the official Gemini naming
scheme so the user selects one public Gemini model slug and switches
`low` / `medium` / `high` through reasoning effort instead of through
provider-specific `-high` / `-low` model IDs.

## Problem

The current Antigravity Gemini presets still expose old internal model IDs such
as:

- `antigravity/gemini-3.1-pro-high`
- `antigravity/gemini-3.1-pro-low`
- `antigravity/gemini-3-pro-high`

That design creates three issues:

1. The picker exposes provider-internal routing names instead of public Gemini
   model names.
2. Strength selection is split across both model choice and reasoning effort,
   which duplicates the same concept in two places.
3. Runtime and fallback metadata still encode the old slug convention, making
   it harder to keep picker behavior, request behavior, and tests aligned.

## Approved Requirements

- Do not preserve old Antigravity Gemini slugs as compatibility aliases.
- Expose official-style Gemini slugs for Antigravity models.
- Put `high` / `low` switching under reasoning effort, similar to GPT effort
  selection.
- Keep existing Gemini request behavior that maps reasoning effort to Gemini
  `thinkingLevel`.

## Design

### 1. Public Antigravity Gemini presets use official-style slugs

Replace the picker-visible Antigravity Gemini presets in
`core/src/models_manager/model_presets.rs` so they expose public Gemini names
instead of internal `-high` / `-low` names.

Expected direction:

- `antigravity/gemini-3.1-pro-preview`
- `antigravity/gemini-3-pro-preview`
- `antigravity/gemini-3-flash-preview`

Display names also move to the public naming style:

- `Antigravity Gemini 3.1 Pro`
- `Antigravity Gemini 3 Pro`
- `Antigravity Gemini 3 Flash`

The model picker should no longer show `High` or `Low` as part of the model
name itself.

### 2. Reasoning effort becomes the only strength selector

For Antigravity Gemini presets, strength selection is carried only by:

- `default_reasoning_effort`
- `supported_reasoning_efforts`

That means:

- Pro-family presets expose `Low`, `Medium`, `High`
- The default effort stays aligned with the intended default tier for that
  model family
- No Antigravity Gemini preset uses separate slugs just to represent a stronger
  or cheaper route

This matches existing Codex behavior for GPT families and matches the expected
Gemini public model UX.

### 3. Fallback model metadata stays family-based

`core/src/models_manager/model_info.rs` already strips provider prefixes such as
`antigravity/` and routes any normalized `gemini-*` slug through the Gemini
fallback metadata branch.

The change here is not to add a special alias layer. Instead:

- keep prefix normalization
- update tests to cover the new public Antigravity Gemini slugs
- remove tests that encode the old `-high` / `-low` public names

As a result, fallback metadata remains simple: Antigravity Gemini is still
"Gemini with a provider prefix", not a second Gemini naming system.

### 4. Runtime Gemini request behavior does not need a new mechanism

`core/src/gemini_content.rs` already converts reasoning effort into Gemini
thinking configuration:

- `high` / `xhigh` → `thinkingLevel = high`
- `medium` → `thinkingLevel = medium`
- `low` / `minimal` / `none` → `thinkingLevel = low`
- Flash models map the low end to `minimal`

That existing mapping is the correct place for high-versus-low behavior. The
public model slug change should therefore not introduce a new routing mechanism.

### 5. Old Antigravity Gemini slugs are removed, not hidden

Because the approved requirement is to remove the old naming scheme, this is a
breaking cleanup:

- old Antigravity Gemini slugs are removed from presets
- tests no longer treat the old slugs as valid public selections
- fallback metadata tests move to the new public slugs

This keeps the system internally coherent instead of supporting two parallel
public naming schemes.

## Affected Paths

- `core/src/models_manager/model_presets.rs`
  Replace old Antigravity Gemini presets with official-style public slugs and
  reasoning-effort-driven strength selection.
- `core/src/models_manager/model_info.rs`
  Update fallback-model tests and any Gemini-family metadata assumptions tied to
  old public slugs.
- `core/src/codex.rs`
  Update server-model mismatch tests or any normalized comparison tests that
  currently hard-code old Antigravity Gemini slugs.
- `core/src/gemini_content.rs`
  Likely no logic change; verify that current prefix stripping and reasoning
  mapping already support the new slugs.

## Testing Strategy

Add or update tests that prove:

- picker-visible Antigravity Gemini presets use the new public slugs
- old Antigravity Gemini `-high` / `-low` slugs are absent from presets
- Antigravity Gemini presets expose reasoning effort options instead of
  strength-specific model IDs
- fallback metadata for new Antigravity Gemini slugs reuses Gemini defaults
  without degrading to unknown fallback metadata
- server-model mismatch normalization treats provider-prefixed Gemini slugs as
  equivalent to the upstream Gemini model name

## Out of Scope

- Adding a separate alias compatibility layer for removed Antigravity Gemini
  slugs
- Changing Gemini thinking-level semantics beyond the existing effort mapping
- Reworking unrelated Anthropic, GPT, or Grok model preset behavior
