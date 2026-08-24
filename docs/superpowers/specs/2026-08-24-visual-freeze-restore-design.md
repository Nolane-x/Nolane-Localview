# Visual Freeze / Restore Capture Transaction Design

## Status

Approved under the standing Wave 2 LocalView roadmap direction. This slice starts from `main` merge commit `e573448d1de4561c91bb4bf916020ad0e4cca92f`, after the stable-settle transaction landed.

## Goal

Make native viewport screenshots deterministic in the presence of CSS animations, CSS transitions and Web Animations API activity without introducing a browser-reconstruction fallback, global JavaScript timer patching or a permanent page mutation.

A visual capture must either:

1. settle;
2. enter a bounded visual-freeze lease on the exact managed WebView;
3. capture native pixels;
4. restore the page state;
5. only then persist/register the artifact;

or fail closed.

The page must self-heal if the native/control path disappears after freeze.

## Architectural choice

Visual freeze is a **bounded internal bridge action**, not an arbitrary `eval()` from desktop and not a public agent action.

Data/control path:

```text
desktop capture_viewport
  -> per-session capture gate
  -> stable-settle endpoint
  -> internal capture-freeze endpoint
      -> LiveBridge internal FreezeVisuals action
      -> managed WebView instrumentation freeze lease
      -> exact action result
  -> native viewport capture
  -> internal capture-restore endpoint
      -> LiveBridge internal RestoreVisuals action
      -> exact token-owned restore
  -> artifact persistence + Visual evidence registration
```

Reasons:

- the existing bridge already validates session/surface ownership;
- exact action results provide an acknowledgement boundary before native capture;
- the control plane remains the action authority;
- desktop remains safe Rust and does not inject arbitrary script;
- the page can enforce lease/token semantics and automatic recovery;
- internal actions can be rejected from the generic `/actions` endpoint so an agent cannot intentionally leave the UI frozen.

## Internal action contract

`BridgeActionKind` gains:

```rust
FreezeVisuals,
RestoreVisuals { token: Uuid },
```

`FreezeVisuals` uses its queued action id as the freeze token. The WebView executor calls:

```js
window.__LOCALVIEW__.freezeVisuals(queued.id)
```

`RestoreVisuals` carries the original freeze action id and calls:

```js
window.__LOCALVIEW__.restoreVisuals(action.token)
```

The generic authenticated action queue rejects both variants. Only the dedicated capture transaction endpoints can enqueue them.

Result payloads are bounded metadata only: token, paused animation count, capability flags and auto-restore deadline. No DOM text, selector content, URLs, form values, pixels or storage values are returned.

## Page-side freeze semantics

Instrumentation owns one active visual-freeze lease at a time.

On freeze:

1. Reject a different active token instead of silently stealing ownership.
2. Record the token and monotonic lease generation.
3. Enumerate `document.getAnimations()` when supported.
4. Record only each animation object reference plus whether it was running/pending before freeze.
5. Pause running/pending animations defensively.
6. Inject exactly one LocalView-owned `<style>` node that:
   - pauses CSS animation playback;
   - disables new transition motion during the lease;
   - hides caret blinking;
   - forces auto scroll behavior for capture determinism.
7. Add a LocalView-owned root attribute containing only the current token.
8. Wait one animation frame before acknowledging freeze so the style/paused state is observable to native pixels.
9. Arm an 8-second auto-restore timer.

The injected stylesheet is intentionally narrow to motion/caret behavior. It must not change visibility, colors, layout dimensions, fonts or content.

### Transition behavior

Active transitions exposed through Web Animations are paused at their current visual time. New transitions started after the freeze begins are disabled for the lease, producing their destination state immediately rather than allowing motion during screenshot acquisition. This favors deterministic pixels over preserving an arbitrary transition midpoint.

### Unsupported animation APIs

If `document.getAnimations` is unavailable, the CSS freeze stylesheet still pauses CSS animations and prevents new transitions. The result reports `web_animations_supported: false`; this slice does not pretend to freeze arbitrary JavaScript-driven canvas/game loops.

## Restore semantics

Restore is token-owned and idempotent for the matching active lease.

On restore:

1. Reject a non-matching token.
2. Cancel the auto-restore timer.
3. Remove only the LocalView-owned style node for that token.
4. Remove the root token attribute only if it still matches.
5. Resume only animations that LocalView observed as running/pending before freeze.
6. Do not resume animations that were already paused, idle or finished before capture.
7. Clear all retained animation references and lease metadata.

Detached/cancelled animations are tolerated with bounded `try/catch` handling so restore cannot cascade into an unhandled page exception.

## Auto-recovery lease

A freeze acknowledgement can be lost after the WebView executes it. Therefore every freeze has an 8-second page-side lease.

If explicit restore never arrives, the timer executes the same token-owned restore logic automatically. This prevents a daemon restart, desktop timeout or transport failure from leaving a managed preview visibly frozen indefinitely.

The lease duration is longer than the existing native-capture 3-second timeout plus bridge/control overhead, but short enough to self-heal promptly.

## Per-session capture serialization

`VisualCaptureState` gains a bounded per-session capture gate so concurrent requests for the same managed WebView cannot interleave:

```text
capture A: settle -> freeze A -> pixels -> restore A
capture B: waits until A releases session gate
```

Different LocalView sessions may still capture concurrently.

The gate registry is bounded/cleaned as capture guards finish; this slice must not create an unbounded map keyed by historical sessions.

## Control-plane endpoints

Add authenticated narrow endpoints:

```text
POST /v1/sessions/{id}/capture-freeze
POST /v1/sessions/{id}/capture-restore
```

Freeze returns a bounded receipt:

```json
{
  "token": "<freeze-action-uuid>",
  "paused_animations": 3,
  "web_animations_supported": true,
  "lease_ms": 8000
}
```

Restore accepts only:

```json
{ "token": "<freeze-action-uuid>" }
```

and returns `204 No Content` on exact successful acknowledgement.

Both endpoints:

- require bearer authentication;
- require the session to exist;
- enqueue an internal bridge action;
- wait for the exact action result id;
- use a bounded acknowledgement timeout;
- fail closed if the managed WebView does not acknowledge.

No caller-controlled script, selector or style text is accepted.

## Desktop transaction

`capture_viewport` becomes:

1. validate viewport;
2. acquire per-session capture gate;
3. preflight exact managed surface + loopback route;
4. run existing 5-second stable-settle transaction;
5. request visual freeze and receive token;
6. run native capture; native path re-reads/revalidates route as it already does;
7. always attempt matching restore before leaving the pixel-acquisition phase;
8. if native capture failed, return capture error after restore attempt;
9. if restore failed, fail the command and do **not** persist/register captured pixels;
10. only after successful restore persist PNG bytes and register Visual evidence.

This ordering keeps the page mutation lifetime short and ensures artifact/evidence success implies the page was restored successfully.

If both native capture and restore fail, return a bounded combined error without route/body/DOM leakage.

## Security and privacy boundaries

- Desktop remains `#![forbid(unsafe_code)]`.
- Native platform adapters are unchanged.
- No arbitrary browser evaluation endpoint is added.
- Generic agent actions cannot queue freeze/restore.
- Freeze/restore payloads contain no user content.
- No response body, cookie, storage value, canvas buffer or screenshot bytes enter the bridge action result store.
- The stylesheet/token is LocalView-owned and removed by token match only.
- Capture receipt does not expose freeze token; it is transaction-internal metadata.

## Explicit non-goals

This slice does not implement:

- freezing arbitrary `requestAnimationFrame` application loops;
- monkey-patching `setTimeout`, `setInterval`, Date or performance clocks;
- canvas/video/WebGL frame locking;
- private-selector masking;
- true network in-flight accounting;
- element/region/full-page capture;
- hosted GUI pixel smoke;
- visual diff execution.

These remain later Wave 2 work.

## Testing strategy

### Instrumentation contract

RED -> GREEN tests require:

- `freezeVisuals` and `restoreVisuals` on `window.__LOCALVIEW__`;
- `document.getAnimations()` usage when supported;
- LocalView-owned style/token markers;
- 8-second auto-restore lease;
- matching-token restore;
- pre-existing paused animations are not resumed;
- no timer/Date/performance monkey patching;
- no canvas screenshot path or content extraction added.

### Live bridge/control contract

Tests require:

- serialization of the two new internal action kinds;
- generic `/actions` rejects freeze/restore;
- capture-freeze queues exact internal action and returns only bounded metadata;
- capture-restore requires the matching token and exact action result;
- unauthorized/missing-session requests fail;
- action results remain bounded and sanitized.

### Desktop contract

Tests require source/behavior invariants:

- session gate acquired before settle/freeze;
- freeze occurs after settle and before native capture;
- restore occurs after native capture attempt and before persistence;
- capture success + restore failure never reaches artifact persistence;
- native failure still attempts restore;
- no arbitrary `eval()` path is introduced;
- no global capture mutex serializes unrelated sessions.

### Cross-platform verification

Final head must pass the existing full CI matrix:

- Rust workspace check + Clippy `-D warnings` + tests on Ubuntu, macOS and Windows;
- native-capture platform contract on all three OSes;
- frontend build;
- stable Tauri backend compile;
- `native-workspace` backend compile;
- all existing desktop capability/workspace/semantic/native-capture/stable-settle contracts;
- new visual-freeze desktop contract.

## Completion criteria

This slice is complete when every successful native viewport artifact is captured inside a token-owned bounded motion-freeze lease, restore acknowledgement succeeds before artifact persistence, the page self-recovers from lost restore transport, generic agent actions cannot abuse the mechanism, and the final branch head passes the complete cross-platform CI matrix.
