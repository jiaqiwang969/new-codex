# Guardian Review Replay Design

**Date:** 2026-03-31

**Goal:** Persist and replay guardian automatic approval review state through app-server v2 `thread/read`, resume, and fork flows when extended history is enabled.

## Context

Guardian approval review already emits `EventMsg::GuardianAssessment(...)` from core and rollout persistence already stores those events in extended history mode. The missing piece is app-server reconstruction: `thread_history.rs` does not consume guardian assessment events, so replayed threads lose that lifecycle state.

Today app-server compensates with temporary standalone notifications:

- `item/autoApprovalReview/started`
- `item/autoApprovalReview/completed`

Those notifications are enough for live TUI rendering, but they are not part of `ThreadItem`, so replay, resume, and fork cannot reconstruct the same UI state from persisted rollout history.

## Scope

This change only covers guardian reviews that can be attached to an existing replayable `ThreadItem` by stable id match:

- `ThreadItem::CommandExecution`
- `ThreadItem::FileChange`
- `ThreadItem::McpToolCall`

This deliberately excludes request types that do not currently have a replayable `ThreadItem` surface in app-server v2, such as network-access-only approvals and any Unix-only `Execve` case without a matching thread item.

## Recommended Approach

Attach guardian review state directly to eligible `ThreadItem` variants.

### Data model

Add an optional `guardian_review: Option<GuardianApprovalReview>` field to:

- `ThreadItem::CommandExecution`
- `ThreadItem::FileChange`
- `ThreadItem::McpToolCall`

Reuse the existing `GuardianApprovalReview` payload shape for now. Although comments mark it as temporary, reusing it avoids inventing a second review representation and keeps live notifications and replayed items aligned.

### Replay mapping

Teach `ThreadHistoryBuilder::handle_event(...)` to consume `EventMsg::GuardianAssessment`.

Mapping rule:

- `assessment.id` is the reviewed tool/request id
- `assessment.turn_id` is the owning turn id when available
- `assessment.status`, `risk_score`, `risk_level`, and `rationale` populate `GuardianApprovalReview`

The builder should upsert guardian review state onto an existing matching item when present. If the owning turn exists but the item does not yet exist, create a minimal placeholder item only when that would preserve a valid replay surface; otherwise drop with a warning. For this first pass, prefer updating already-known items and logging unmatched cases rather than synthesizing partially-known item payloads.

### Live compatibility

Keep the existing standalone guardian review notifications in app-server for now. They remain the live bridge for current clients and avoid turning this task into a broader protocol migration.

### TUI replay behavior

Update TUI app-server replay handling so that a replayed `ThreadItem` with `guardian_review` invokes the same guardian rendering flow currently used for standalone notifications. This keeps live and replayed output visually consistent without rewriting the existing history-cell logic.

## Alternatives Considered

### 1. Replay-only guardian notifications

Rejected. This would preserve the current split model where live state uses notifications and replay state uses a different API path. It does not solve the product-model problem and would need to be removed later.

### 2. Full approval timeline model

Rejected for now. A separate approval timeline could unify command, file, MCP, network, and future review types, but that is a larger protocol redesign than this continuation needs.

## Risks

### Out-of-order replay

Guardian assessment events may appear before or after the matching terminal tool event depending on persistence order and turn reconstruction. The implementation should therefore upsert onto existing items and tolerate unmatched assessments without panicking.

### Partial item coverage

Not every guardian-reviewed request type maps to a `ThreadItem`. That is acceptable for this iteration as long as documented coverage is explicit and unmatched replay is non-fatal.

### API growth

Adding `guardianReview` expands v2 schema. README and generated app-server schemas must be updated in the same change.

## Testing Strategy

1. Add protocol-level replay tests in `thread_history.rs` showing guardian assessment replay attaches to:
   - command execution
   - file change
   - MCP tool call
2. Verify unmatched guardian assessment events do not crash replay and do not invent malformed items.
3. Add TUI app-server replay coverage showing replayed `guardianReview` produces the same approved/denied rendering path as live guardian notifications.
4. Regenerate app-server schema fixtures and run targeted crate tests.

## Non-Goals

- Removing the temporary guardian standalone notifications in this change
- Introducing replay support for approval classes without a corresponding `ThreadItem`
- Redesigning the guardian review payload shape beyond reusing the existing unstable struct
