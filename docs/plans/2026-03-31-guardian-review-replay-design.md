# Guardian Review Replay Design

**Date:** 2026-03-31

> **Status (2026-03-31): Implemented on `main`.**
> The design below was landed as `452f8e13e2`
> (`feat: replay guardian approval reviews`).
> Keep this note as implementation rationale, not as a pending design review.

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

Add a replayable `ThreadItem::GuardianApprovalReview` item that reconstructs guardian review state from persisted rollout events.

### Data model

Add a new replay-only `ThreadItem` variant:

- `GuardianApprovalReview { id, target_item_id, review, action }`

Where:

- `id` is a stable synthetic thread item id derived from the reviewed request id
- `target_item_id` is the reviewed request/tool id from `GuardianAssessmentEvent.id`
- `review` reuses the existing `GuardianApprovalReview` payload shape
- `action` carries the guardian action summary payload from core when available

This keeps replay state durable without requiring us to synthesize incomplete command/file/MCP items for denied reviews.

### Replay mapping

Teach `ThreadHistoryBuilder::handle_event(...)` to consume `EventMsg::GuardianAssessment`.

Mapping rule:

- `assessment.id` is the reviewed tool/request id
- `assessment.turn_id` is the owning turn id when available
- `assessment.status`, `risk_score`, `risk_level`, and `rationale` populate `GuardianApprovalReview`
- `assessment.action` is copied onto the replay item so clients can render the same approval summary they use for live notifications

The builder should upsert by synthetic guardian-review item id so `inProgress -> approved/denied/aborted` transitions collapse into one final replay item.

### Live compatibility

Keep the existing standalone guardian review notifications in app-server for now. They remain the live bridge for current clients and avoid turning this task into a broader protocol migration.

### TUI replay behavior

Update TUI app-server replay handling so that a replayed `ThreadItem::GuardianApprovalReview` invokes the same guardian rendering flow currently used for standalone notifications. This keeps live and replayed output visually consistent without rewriting the existing history-cell logic.

## Alternatives Considered

### 1. Attaching guardian state to existing command/file/MCP items

Rejected for this iteration. Denied reviews often do not emit a subsequent command execution, patch apply, or MCP completion item, so replay would still need synthetic placeholder items. A dedicated replay item gets us complete history reconstruction with lower risk.

### 2. Full approval timeline model

Rejected for now. A separate approval timeline could unify command, file, MCP, network, and future review types, but that is a larger protocol redesign than this continuation needs.

## Risks

### Out-of-order replay

Guardian assessment events may appear before or after the matching terminal tool event depending on persistence order and turn reconstruction. The implementation should therefore upsert onto existing items and tolerate unmatched assessments without panicking.

### Product model mismatch

The replay item is a pragmatic bridge, not the final guardian lifecycle model. A future protocol revision could still fold this state into richer approval-request items if the live app-server API grows that surface.

### API growth

Adding `guardianReview` expands v2 schema. README and generated app-server schemas must be updated in the same change.

## Testing Strategy

1. Add protocol-level replay tests in `thread_history.rs` showing guardian assessment replay becomes a `guardianApprovalReview` thread item for command, patch, and MCP review actions.
2. Verify repeated guardian assessment lifecycle updates upsert into one replay item.
3. Add TUI app-server replay coverage showing replayed `guardianApprovalReview` produces the same approved/denied rendering path as live guardian notifications.
4. Regenerate app-server schema fixtures and run targeted crate tests.

## Non-Goals

- Removing the temporary guardian standalone notifications in this change
- Introducing replay support for approval classes without a corresponding `ThreadItem`
- Redesigning the guardian review payload shape beyond reusing the existing unstable struct
