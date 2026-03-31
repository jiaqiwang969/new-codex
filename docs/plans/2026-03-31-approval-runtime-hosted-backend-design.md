# Approval Runtime Hosted Backend Design

**Date:** 2026-03-31

**Status:** Proposed

**Goal:** Preserve the valuable stateful runtime ideas from the old `phase2b`
Smart Access residue by adding a hosted backend to `approval_runtime`, without
reviving the removed `smart_access` product stack.

## Context

`main` already landed the narrow `approval_runtime` abstraction:

- session and child leases
- destructive preflight / postflight hooks
- fail-closed runtime decisions
- runtime warning rendering in `tui_app_server`

What is still missing is a real runtime backend. Today
`default_runtime_client()` returns an in-memory client, which is sufficient for
tests and call-flow integration but does not preserve runtime state across
instances or exercise lock/recovery behavior.

The old `feature-smart-access-phase2b-control-plane` worktree still contains
backend experiments under `security_runtime/{bootstrap,hosted,remote,legacy}`,
but those files are entangled with removed `smart_access`, `security_host`, and
legacy TUI paths. Replaying that stack would re-open the product surface that
`main` explicitly deleted.

## Recommendation

Add a hosted backend inside `approval_runtime` and make it the default runtime
factory for normal session startup.

This backend should be a narrow translation of the useful parts of the old
hosted runtime:

- file-backed lease state under `codex_home`
- file-lock coordination
- stale-lock recovery surfaced as `RuntimeHealth::Recovery`
- parent/child lease derivation and descendant invalidation

Do not import the old naming or product model. The new code should live entirely
under `approval_runtime`.

## Alternatives Considered

### 1. Keep in-memory only

Safest, but it leaves the runtime layer mostly as a testing scaffold and loses
the only durable value left in the old `phase2b` residue.

### 2. Add hosted backend only

Recommended. It captures persistence and recovery behavior while keeping the
surface area small and compatible with current `main`.

### 3. Port hosted + remote + bootstrap stack together

Rejected for the first slice. That would drag old control-plane handshakes and
fallback behavior back into `main` before the new seam has proven stable.

## Architecture

### Runtime client split

Keep the existing `ApprovalRuntimeClient` trait and add:

- `InMemoryApprovalRuntimeClient`
- `HostedApprovalRuntimeClient`

`ApprovalRuntime` remains the orchestration layer over the trait, so existing
destructive tool runtimes do not need behavioral changes.

### Factory

Replace the current zero-argument `default_runtime_client()` helper with a small
factory that can build the default backend from `codex_home`.

Initial policy:

- root sessions default to hosted backend
- delegated sessions inherit the parent runtime client as they do today
- tests can still inject custom runtimes directly

### Hosted state model

Use the existing runtime types rather than reintroducing the larger
`Security*` family:

- `RuntimeLease`
- `RuntimeLeaseRegistration`
- `RuntimeChildLeaseRequest`
- `RuntimePreflight`
- `RuntimeFinishObservation`

Persist only the state needed by current `approval_runtime` semantics:

- active leases
- parent/child lease graph
- next action sequence
- backend health / recovery notes

Do not port permit catalogs, endpoint-security event batches, or old Smart
Access trace payloads in this slice.

## Failure Semantics

Hosted backend behavior should map onto existing runtime decisions:

- stale or recovered lock -> `RuntimeHealth::Recovery`
- missing or revoked lease -> `FallbackToHuman`
- revoked parent causing invalid child lease -> `FallbackToHuman`

The first slice should not introduce new runtime decision variants.

## Testing Strategy

Add targeted unit tests for the hosted backend:

- lease registration persists across client instances for the same state root
- child lease derivation persists parent linkage
- revoking a parent lease invalidates descendants
- stale lock cleanup surfaces a `Recovery` summary on the next preflight

Then re-run existing integration coverage already proving the outer runtime flow:

- `cargo test -p codex-core approval_runtime`
- `cargo test -p codex-core --test all suite::unified_exec::runtime_`
- `cargo test -p codex-core --test all suite::approvals::runtime_`

## Non-Goals

- restoring `smart_access.rs`
- porting `security_runtime/remote.rs` in the first slice
- reviving old `security_host` or `es_daemon` plumbing
- restoring legacy `codex-rs/tui` runtime warning paths
