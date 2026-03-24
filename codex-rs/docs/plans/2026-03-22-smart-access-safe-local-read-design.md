# Smart Access Safe Local Read Fast Path

## Goal

Restore the original Smart Access product goal for shell-style tools:
non-sensitive local read-only commands should run silently, while truly
sensitive reads, writes, destructive actions, and external transfers should
keep the existing strict approval behavior.

## Problem

The current Smart Access flow mixes three concerns into one user-visible path:

1. Internal guardian risk analysis.
2. Smart Access permit arbitration.
3. Human approval prompts.

That design creates two behavior problems:

- Safe local reads still run through guardian review and surface
  `Automatic approval review approved ...` warnings.
- Reads outside the current workspace are too easily described as
  `sensitive_read`, which causes Smart Access to fall back to manual approval
  even when the user only asked to inspect a local markdown or config file.

The result is a mode that feels like "approve everything twice" instead of
"auto-resolve low-risk actions and escalate only real risks."

## Approved Requirements

- Any non-sensitive local read-only command in `shell` or `unified_exec`
  should be silently allowed in Smart Access.
- Silent allow means:
  - no human approval prompt
  - no `Automatic approval review approved ...` warning
  - no `Smart Access permit issued ...` UI message
- Sensitive local reads must still escalate or deny.
- Writes, deletes, moves, transfers, network access, and complex shell scripts
  must continue through the current strict path.
- The fix must be general. It must not special-case `SKILL.md`,
  `AGENTS.md`, or any single external directory.
- The implementation must prefer deterministic local rules over model-only
  interpretation.

## Design

### 1. Add a Smart Access fast path before guardian review

Introduce a deterministic pre-check in `core/src/smart_access.rs` that runs
before `review_smart_access_request(...)` performs guardian review.

This fast path only applies when all of the following are true:

- the turn is in Smart Access mode
- the request is a `Shell` or `ExecCommand`
- `sandbox_permissions` does not request additional permissions
- the request does not include deferred network access
- the command can be parsed into a known-safe local read-only shape
- every inferred read target is non-sensitive

If the request matches, Smart Access returns `ReviewDecision::Approved`
immediately and skips both guardian review and Smart Access permit tracing.

### 2. Reuse existing safe-command parsing instead of inventing new rules

The repo already has a conservative command parser and safe-command classifier:

- `shell-command/src/bash.rs`
- `shell-command/src/command_safety/is_safe_command.rs`
- `core/src/command_canonicalization.rs`

The fast path should reuse those building blocks instead of defining a second
disconnected allowlist.

The command must still be rejected from the fast path if it cannot be reduced
to one or more plain read-only commands. Complex shell structures remain on the
existing approval path.

### 3. Separate "safe command shape" from "safe read target"

`is_known_safe_command(...)` is necessary but not sufficient. The implementation
must also extract the read scope and evaluate whether the target is sensitive.

Use a small internal representation:

```rust
enum SafeLocalReadScope {
    NoPath,
    Paths(Vec<AbsolutePathBuf>),
    RepoMetadata { repo_root: AbsolutePathBuf },
}
```

Scope extraction rules:

- `cat`, `head`, `tail`, `stat`, `wc`, `nl`, `sed -n ...p`:
  use the explicit file path arguments
- `rg`, `grep`, `find`, `ls`:
  use explicit path arguments, or `cwd` if none are provided
- `pwd`, `whoami`, `id`, `uname`:
  map to `NoPath`
- read-only `git status/log/diff/show/branch --list`:
  map to `RepoMetadata`

If extraction is ambiguous, return `None` and fall back to the current path.

### 4. Add deterministic sensitive-read classification

The fast path should reject a request if any inferred target is sensitive.

Sensitivity comes from two sources:

1. `SecurityCapabilitySnapshot.sensitive_zones`
2. a small built-in fallback set for obviously secret-bearing locations when
   endpoint security is disabled or the policy does not define sensitive zones

The built-in fallback roots should be conservative and short:

- `~/.ssh`
- `~/.gnupg`
- `~/.aws`
- `~/.kube`
- `~/.config/gcloud`
- `~/Library/Keychains`
- `<codex_home>/auth.json`
- `<codex_home>/.credentials.json`

This keeps `cat ~/.ssh/id_rsa` on the strict path even when
`endpoint_security = false`, while allowing ordinary repo-adjacent docs and
config inspection to stay silent.

### 5. Make guardian review silent when Smart Access uses it internally

Smart Access still needs guardian analysis for requests that do not qualify for
the fast path. However, guardian's visible warning output is internal noise in
this mode.

Add an internal guardian review display mode:

```rust
enum GuardianReviewDisplay {
    Visible,
    Silent,
}
```

Smart Access should call the silent variant. In silent mode:

- do not emit `WarningEvent`
- do not emit user-visible guardian progress/approved/denied assessment events
- only return `GuardianReviewResult`

Non-Smart-Access approvals keep the current visible behavior.

### 6. Hide successful Smart Access permits from the UI

Today Smart Access emits visible trace events for successful permits. That is
useful for debugging but wrong for the normal UX target.

Change Smart Access trace emission so that:

- `AllowWithPermit`
- `AllowWithAmendedPermit`

remain internal only and do not produce user-visible trace output.

The UI should continue to show only:

- `Deny`
- `EscalateToHuman` / fallback to human
- `DowngradeToDefault`
- `runtime_mismatch`

This preserves observability for real risk or policy drift without turning
successful low-risk paths into approval theater.

## Affected Paths

- `core/src/smart_access.rs`
  Add the fast path, target extraction, sensitive-path classification, and
  permit visibility changes.
- `core/src/guardian.rs`
  Add a silent review mode for Smart Access internal use.
- `core/src/tools/runtimes/shell.rs`
  Invoke the fast path before Smart Access guardian review.
- `core/src/tools/runtimes/unified_exec.rs`
  Mirror the shell path.
- `core/tests/suite/approvals.rs`
  Add behavioral coverage for silent allow vs escalation.
- `tui/src/chatwidget/tests.rs`
  Assert that safe local reads do not render approval noise.
- `README.md`
  Update Smart Access wording if needed so docs match the new silent-allow
  behavior.

## Testing Strategy

Add coverage for these cases:

- `sed -n '1,20p' /path/to/SKILL.md` in Smart Access:
  silently allowed, no approval UI
- `rg -n Smart Access README.md core/src`:
  silently allowed, no approval UI
- `find .. -name '*.md'`:
  silently allowed if no sensitive root is touched
- `cat ~/.ssh/id_rsa`:
  still escalates or denies
- `rg foo ~/.ssh`:
  still escalates or denies
- `bash -lc "sed -n '1,20p' a && rg x b"`:
  eligible for the fast path
- `bash -lc "sed -n '1,20p' a && curl https://example.com"`:
  not eligible for the fast path
- successful Smart Access permit:
  no visible `WarningEvent`
- Smart Access fallback / deny / runtime mismatch:
  still visible in the UI

## Risks

- The main risk is over-broad auto-approval if the fast path tries to infer too
  much from complex shell syntax.
- The mitigation is to keep parsing conservative:
  if the command shape is not obviously safe and readable, fall back to the
  current approval path.
- The second risk is under-protecting secret-bearing files when endpoint
  security is disabled.
- The mitigation is the built-in minimal sensitive-root fallback set.

## Out of Scope

- Reclassifying every possible read-only UNIX tool
- Changing `SensitiveRead` behavior for network or MCP requests
- Reworking Endpoint Security policy format
- Adding a new end-user toggle for Smart Access verbosity
