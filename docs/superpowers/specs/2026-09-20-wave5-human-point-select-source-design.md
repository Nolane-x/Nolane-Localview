# Wave 5 Human Point Selection → Stable ElementRef → Existing Source Authority

Date: 2026-09-20  
Branch: `feat/wave5-point-select-source`

## Goal

Close the direct-human-selection gap without creating another source resolver.

The only new authority introduced here is the one-shot mapping:

`human pointer in an exact LocalView-managed WebView → existing instrumentation fingerprint → stable ElementRef`.

After that point, the desktop Inspector continues to use the already-landed semantic/component/source authority. Open Source, Measure, Ask AI, Fix, Verify, framework ownership and Source Map resolution are intentionally not reimplemented.

## Authority chain

1. The desktop UI creates an opaque point-select request token. It never creates an element reference.
2. Tauri binds that token to the exact session and the current canonical managed-surface route.
3. Tauri arms point selection only inside the LocalView-managed preview WebView (or the feature-gated native workspace child when it is the managed surface).
4. Managed-page instrumentation uses `document.elementFromPoint(clientX, clientY)` at pointer time. This preserves browser hit-testing semantics for nesting, SVG, pointer-events and app overlays. Cross-origin frame interiors are never traversed.
5. The selected element is converted by the existing `refFor(element)` fingerprint authority. CSS selectors and DOM paths are never accepted as selection authority.
6. The bridge returns only request token, canonical route, terminal status, stable reference when selected, bounded reason and bridge generation.
7. Desktop rejects stale token/session/route/generation/ref outcomes.
8. React sets the point-selected stable reference as selection authority ahead of legacy focus evidence.
9. Existing Open Source / Measure / Ask AI / Fix / Verify flows consume that same `selectedReference` and perform their existing fresh semantic/source checks.

## One-shot interaction ownership

Active mode installs capture-phase listeners only for its lifetime.

- `pointermove` updates the transient highlight.
- left `pointerdown`, mouse activation, click, auxiliary click and context menu are suppressed so selection cannot activate the application.
- selection completes on the captured click, after a fresh hit-test.
- `Escape` cancels the request.
- completion, cancellation, route drift and errors remove every listener, observer and overlay.
- outside active mode LocalView installs no point-select keyboard or click suppression.

A new request replaces the prior desktop token. Therefore completion A cannot overwrite request B.

## Hover/click race rule

Hover is advisory; click is authoritative.

The click path requires the target to remain connected and to match the currently highlighted/down target under a fresh `elementFromPoint` query. If the element disappears or another element takes the point between hover and click, the request fails closed rather than returning the stale hover reference.

## Transient highlight

The highlight is an in-page LocalView-owned overlay because the point target exists inside the managed WebView.

It is:

- stored in a private `WeakSet` of LocalView-owned nodes;
- `pointer-events:none`;
- bounded to the current viewport;
- excluded from semantic-tree traversal, interactive snapshots, ref resolution and occlusion ownership;
- removed exactly on every terminal path;
- hidden while the existing visual-freeze attribute is active, so visual capture does not treat it as application pixels.

The `data-localview-owned="point-select"` attribute is diagnostic only; ownership is the private WeakSet, not a caller-writable DOM attribute.

## Route and session races

Desktop status is keyed by `(session_id, request_token)`.

The begin route is read from the existing managed-surface canonical-route authority. Completion must come from an allowed LocalView bridge surface, with the same canonical caller route. The bridge contributes its document generation to the receipt.

SPA route signals terminate active point selection immediately. Desktop polling independently rechecks the managed route and fails closed on drift. Session change increments the UI generation, cancels the old token best-effort, clears point selection and prevents stale async completion from writing state.

## Focus evidence precedence

Legacy focus evidence remains a fallback only.

`selectedReference = pointSelectedReference ?? focusSelectedReference`

Focus fallback is limited to focus observations after the latest route event. Once a human point selection succeeds, later focus evidence cannot overwrite it. Route/session drift clears point authority.

## Privacy

Point-select transport never retains or sends:

- `innerHTML`;
- full `textContent`;
- input/password values;
- cookies or storage;
- arbitrary attributes;
- framework props/state/context/hooks.

The receipt schema contains only bounded control metadata and the stable ref.

## Failure policy

Fail closed for:

- no managed surface;
- invalid/oversized token;
- invalid stable ref;
- target removed/changed;
- route drift;
- session mismatch;
- stale token;
- invalid bridge generation;
- malformed completion;
- expired one-shot lease.

There is no selector/tag/class/index fallback.

## Browser proof

The deterministic Playwright fixture proves:

- bounded hover highlight;
- semantic exclusion of LocalView overlay;
- overlay suspension during visual freeze;
- nested child click → exact stable `ElementRef`;
- click suppression;
- one-shot listener/overlay cleanup;
- Escape cancellation;
- route-drift cleanup;
- removed element never returns the stale ref;
- password input value is absent from receipt;
- SVG exact hit testing;
- keyboard handling returns to the app after mode completion.

## Known boundary

The ordinary fallback iframe in the dashboard is not treated as point-selection authority because it is not the exact Tauri-managed/instrumented WebView. If no managed surface exists, the Human Inspector opens the existing managed LocalView preview and arms selection there. Cross-origin iframe content remains opaque; the frame element itself may be hit, but LocalView does not traverse its document.

Same-URL hard reload is bounded by document-generation receipts and token ownership, but a reload that destroys the active page before it can emit a terminal completion is ultimately cleared by desktop lifecycle/status authority rather than by recovering selection from the new document. No ref is synthesized across that boundary.
