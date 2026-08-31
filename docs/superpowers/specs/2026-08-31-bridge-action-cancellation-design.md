# BridgeAction Cancellation Design

## Goal

Add exact-session, exact-action cooperative cancellation for **public** `BridgeAction` work while preserving the existing internal visual-capture authority and its mandatory restore semantics.

## Scope

This slice applies to public actions queued through `/v1/sessions/{id}/actions`: `Click`, `TypeText`, `Key`, `Scroll`, `Focus`, and public `Snapshot` work. `FreezeVisuals` and `RestoreVisuals` remain private internal-capture actions and are explicitly outside public cancellation authority.

## Architecture

`localview-live-bridge` already wraps the base bridge in `cancellable_lib.rs` to provide a serialized native-executor lifecycle authority. The BridgeAction cancellation slice extends that wrapper with a **separate page-action authority** instead of merging native and page action state into one registry. The shared invariant is `(session_id, action_id)` ownership; the lifecycle state and cleanup stay domain-specific.

The public action lifecycle is:

`pending -> inflight -> cancellation_requested -> cancelled`

Normal successful completion removes authority state. Pending cancellation becomes terminal immediately and prevents delivery. Inflight cancellation fences result claiming/completion immediately and emits a cooperative cancellation signal for the page executor. Terminal cancellation tombstones are bounded and removed on session release.

Internal capture actions never enter the public cancellation authority. Existing `FreezeVisuals` / `RestoreVisuals` delivery, claim, result sanitation, fail-safe lease, and coordinator-owned restore path remain unchanged.

## Authority Rules

1. `(session_id, action_id)` is the only cancellation ownership key.
2. Only public actions are recorded in public cancellation authority.
3. Cancellation of a queued action is acknowledged immediately and the action is filtered before public dispatch.
4. Cancellation of an inflight action changes authority to `cancellation_requested` before executor acknowledgement; from that moment a racing result cannot claim or complete the public action.
5. Repeated cancellation is idempotent.
6. Cross-session cancellation is rejected as not found.
7. Queue eviction removes stale pending authority so an evicted action cannot later be cancelled.
8. Completed non-cancelled actions are removed from cancellation authority.
9. Terminal cancelled tombstones are bounded to 256 entries globally and cleaned on session release.
10. Public cancellation endpoints cannot address internal capture actions.
11. A cancellation acknowledgement may settle the base public action origin so capacity is released, but it must never synthesize Interaction evidence as if the action had executed.
12. Public action result submission after cancellation returns conflict and stores no Interaction/Semantic/Layout evidence.

## Control API

Add authenticated session-scoped routes:

- `POST /v1/sessions/{id}/actions/cancel` with `{ "action_id": UUID }`
- `GET /v1/sessions/{id}/actions/cancellations`
- `GET /v1/sessions/{id}/actions/cancellations/{action_id}`
- `POST /v1/sessions/{id}/actions/cancellations/{action_id}/ack`

Responses mirror the native cancellation shape but use page-action terminology. Unknown actions return `404 action_not_found`; acknowledgement without a pending cancellation returns `409 action_cancellation_not_pending`.

## Executor Contract

The page executor can query exact cancellation state for the action it owns. Cancellation remains cooperative: LocalView does not force-kill JavaScript or platform WebView APIs mid-call. The executor checks the signal at safe boundaries and acknowledges when it stops the action. The authority fence prevents late/racing results from being accepted even before ACK.

## Evidence Semantics

Cancellation is control-flow state, not positive execution evidence. A cancelled action does not create Interaction evidence and cannot create Semantic/Layout evidence. If an action already completed and its result was accepted before cancellation acquired authority, cancellation returns not found because completion removed the lifecycle entry.

## Testing

TDD must prove:

- pending cancel filters delivery;
- inflight cancel fences claim/result completion before ACK;
- duplicate cancel is idempotent;
- exact-session ownership rejects cross-session IDs;
- queue eviction removes stale cancellation ownership;
- terminal tombstones are bounded;
- exact lookup works beyond a truncated cancellation listing;
- acknowledgement releases base lifecycle capacity;
- cancelled public results create no evidence;
- internal `FreezeVisuals` / `RestoreVisuals` are unreachable through public cancellation;
- existing capture restore transaction tests remain green;
- Ubuntu/macOS/Windows core gates and Tauri/native GUI smoke remain green on the exact head.

## Non-goals

- No force-abort of WebView platform APIs.
- No cancellation authority for internal capture actions.
- No unification of native-executor and public-action cancellation registries.
- No new raw page payload or secret-bearing cancellation telemetry.
