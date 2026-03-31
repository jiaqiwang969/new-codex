# Provider Family / Utility Routing Merge Analysis

**Date:** 2026-03-24
**Branch state:** `0fa92816c`
**Baseline:** `upstream/main` at `9dbe09834`

> **Status (2026-03-31): Historical merge analysis.**
> The upstream merge has since landed on `main`.
> This file captures why this capability mattered during merge work, but it is
> not a substitute for checking the current provider-routing code directly.

> **Goal of this note:** explain what the local provider-family expansion and
> utility-routing layer actually does, why it is more than a few extra model
> aliases, and where the future upstream merge will keep colliding with it.

## Bottom Line

This is a real local architecture layer, not merge residue.

The current branch adds:

1. dedicated Anthropic request/stream adapters
2. dedicated Gemini/Gemma request/stream adapters
3. a local model capability/normalization matrix
4. provider-family-aware utility routing for internal tasks

`upstream/main` currently does not contain this layer as same-name files or as
an equivalent standalone abstraction. That means future merge work should
preserve the semantics, but it should not assume the current file layout is the
right long-term shape.

## Current upstream confirmation (`upstream/main` at `9dbe09834`)

Rechecked directly against the current upstream baseline:

- `codex-rs/core/src/anthropic_content.rs`
- `codex-rs/core/src/anthropic_streaming.rs`
- `codex-rs/core/src/gemini_content.rs`
- `codex-rs/core/src/gemini_streaming.rs`
- `codex-rs/core/src/model_compat.rs`
- `codex-rs/core/src/utility_model.rs`

All of the above are absent in `upstream/main`.

What upstream does have instead:

- the general provider/config baseline
- collaboration mode plumbing
- large TUI model switching surfaces

What upstream does **not** currently have as a first-class layer:

- provider-family-aware utility routing separate from the leader model
- local-only Anthropic/Gemini transport shaping modules
- the local compatibility matrix for Grok/Gemma/Claude/antigravity slugs

## What The Local Branch Adds

### 1. Anthropic transport shaping

Representative files:

- `codex-rs/core/src/anthropic_content.rs`
- `codex-rs/core/src/anthropic_streaming.rs`

Behavior:

- normalizes Anthropic-compatible base URLs
- builds Anthropic message/tool payloads from Codex response items
- parses image inputs for Anthropic requests
- separates reasoning-like markup from answer text during streaming
- provides explicit no-content fallback diagnostics for endpoint/model mismatch

Why it matters:

- the branch is not only choosing a different provider ID
- it is translating Codex tool/content semantics onto Anthropic-specific wire
  behavior

### 2. Gemini / Gemma / antigravity transport shaping

Representative files:

- `codex-rs/core/src/gemini_content.rs`
- `codex-rs/core/src/gemini_streaming.rs`

Behavior:

- normalizes Gemini-compatible base URLs
- builds Gemini thinking/tool/content payloads
- handles reference images and inline image data
- preserves provider-specific tool restrictions for Gemini vs Gemma
- maps Gemini grounding/search/tool outputs into Codex response events
- strips synthetic thought signatures when crossing provider boundaries

Why it matters:

- this is where a lot of the mixed-provider workflow actually becomes usable
- losing this layer would keep the model names while silently breaking the
  request/response contract

### 3. Local compatibility matrix

Representative file:

- `codex-rs/core/src/model_compat.rs`

Behavior:

- normalizes namespaced and bare slugs for Grok / Gemma / Claude / OpenAI
- maps legacy Gemini selectors onto current preview/image models
- gates web search, reasoning effort, image inputs, data-url image inputs, and
  memory trace summarization by model family

Why it matters:

- the branch is carrying a local model-behavior policy matrix, not just aliases
- memory, image, web-search, and utility-model decisions all depend on it

### 4. Utility routing separate from the leader model

Representative file:

- `codex-rs/core/src/utility_model.rs`

Behavior:

- resolves provider family from the target model slug
- preserves the active or user-configured custom provider when it already
  matches the needed family
- routes Responses-only utility work through `model_sub_responses` when needed
- starts utility requests from the logical provider's first `account_pool`
  entry instead of mutating global config

Useful direct evidence from local tests:

- OpenAI utility work prefers a custom Responses provider over built-in
  upstream defaults
- Claude utility work prefers the active custom Anthropic provider
- Responses-only utility work ignores non-Responses `model_sub` and falls back
  to `model_sub_responses` / OpenAI defaults
- utility routing starts from `account_pool[0]`

## What Problem The Local Author Was Solving

The local branch is optimizing for heterogeneous model teams:

- leader model on one provider family
- utility/sub-agent work on another family
- memory or internal unary work forced onto a Responses-compatible path
- custom endpoints and account-pool rotation still applying underneath

In short:

- upstream mostly optimizes around choosing a session model cleanly
- local branch optimizes around keeping multiple model families interoperable
  inside one workflow

## What Must Be Preserved

Based on current user direction and current runtime behavior, these semantics
are the important ones:

1. model-family-aware routing for OpenAI, Anthropic, Gemini, Gemma, Grok, and
   antigravity variants
2. utility routing must respect local custom provider IDs instead of forcing
   built-in provider IDs
3. Responses-only internal tasks must keep a Responses-compatible path
4. utility routing must stay compatible with account-pool and custom endpoints
5. Gemini/Gemma/Grok capability gating must remain coherent with local model
   usage
6. provider-specific content shaping and stream parsing must survive for the
   non-OpenAI families that the branch actively uses

## What Can Change

These are implementation details, not architecture requirements:

- exact module boundaries
- exact helper/function names
- exact streaming parser structure
- exact tool allowlist heuristics for Gemini/Gemma
- exact placement of compatibility helpers

## Main Merge Risks

### Risk 1: keeping `model_sub` but losing provider-family routing

If the branch keeps `model_sub` config but drops the utility-routing layer:

- child/internal tasks may silently use the wrong provider family
- custom endpoints may stop applying
- account-pool semantics may disappear from utility tasks

### Risk 2: keeping model names but losing transport shaping

If local Gemini / Anthropic model slugs remain visible but the dedicated
request/stream shaping disappears:

- tool semantics can drift
- image handling can regress
- stream parsing can become incompatible with current local endpoints

### Risk 3: preserving old patches too literally

These files are currently local-only, which is good for isolation, but it also
means a future upstream refactor can strand them if integration points move.

The safe strategy is:

- preserve behavior
- reattach to newer upstream provider/config/runtime surfaces

not:

- replay the old patch shape verbatim

## Highest Future Conflict Magnets For This Domain

Even though several core modules are local-only, the merge pain will still
concentrate in shared files where this layer attaches:

- `codex-rs/core/src/config/mod.rs`
  - model-family auto-switching and configured-provider preservation
- `codex-rs/core/src/codex.rs`
  - turn-time provider restoration / switching
- `codex-rs/core/src/client.rs`
  - request dispatch and provider-specific auth/transport behavior
- `codex-rs/core/src/agent/role.rs`
  - child model/provider inheritance
- `codex-rs/core/src/memories/phase2.rs`
  - memory-worker model/provider compatibility

## Practical Merge Judgment

This layer should be treated as a preserved local capability block.

The right sequence is:

1. preserve account-pool / provider routing
2. preserve provider-family-aware utility routing
3. only then preserve `model_sub` and related higher-level workflow features

If steps 1 and 2 are not kept together, the branch can retain the knobs while
losing the behavior that made those knobs useful.
