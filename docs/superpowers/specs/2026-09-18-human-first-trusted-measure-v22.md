# Human-First Trusted Measure V2.2

## Status

Canonical design for the Human-First Trusted Measure V2.2 wave.

Base lineage:

- `main@dc1784cc0095fd505b47edea92ef09250e85f5e9`
- Human-First UI/UX V2 merged by PR #142
- Trusted Human-First Capture V2.1 merged by PR #143

This document is the durable source of truth for the next Human-First capability wave. Later AI sessions must recover from the repository and current branch, not chat history.

The implementation may advance after this file is committed. This file defines the intended product boundary, authority model, failure semantics and merge conditions; executable tests and the exact branch head define implementation truth.

---

## 1. Goal

Make the Human-First Inspector **Measure** control a real capability without allowing React to invent geometry.

The user-facing operation is intentionally narrow:

> Measure the currently selected LocalView element.

The human surface may display:

- element width and height in CSS pixels;
- viewport-relative x/y;
- optionally document-relative x/y;
- route identity in Advanced/debug evidence only;
- a compact success/failure status.

React must never submit rectangle coordinates as authoritative input.

---

## 2. Why this wave exists

Human-First V2 intentionally left Measure disabled because the frontend did not own trusted element geometry.

The repository already contains the required lower-level primitives:

1. LocalView instrumentation assigns stable bounded element references such as `@e...`.
2. `window.__LOCALVIEW__.inspect(reference)` resolves that reference inside the exact managed page.
3. The returned semantic node contains:
   - `rect`: viewport-relative CSS geometry;
   - `documentRect`: document-relative CSS geometry;
   - viewport metadata;
   - route metadata.
4. Preview actions are session-bound and executed inside the exact LocalView-managed WebView.
5. Completed bridge actions are authenticated through the control service and can produce evidence.

The missing piece is a narrow read-only action and desktop/UI contract that converts those existing primitives into a human Measure command without exposing generic page evaluation or caller-authored geometry.

---

## 3. Product principle

A visible human control becomes enabled only when LocalView can execute it through an authority path strong enough to justify the affordance.

For Measure that means:

- user selection is represented by a LocalView stable element reference;
- the exact session-owned managed surface resolves the reference;
- geometry is read at request time from LocalView instrumentation;
- the bridge returns only bounded measurement data;
- desktop validates the result before React receives it;
- failure is explicit and humanized;
- no approximate fallback silently substitutes stale or caller-derived geometry.

---

## 4. Non-negotiable trust boundary

React may submit:

```text
session_id
element_reference
```

React must not submit:

```text
x
y
width
height
document_x
document_y
viewport_width
viewport_height
device_pixel_ratio
route
DOM selector
DOM node
CSS selector
source path
```

Those values are observed inside the LocalView-managed page and validated by trusted desktop/control code.

---

## 5. Selected architecture

The selected path is:

```text
Inspector Measure
  -> current session + latest selected LocalView @e reference
  -> desktop measure_current_selection(session_id, reference)
  -> preflight exact managed surface/session/loopback route
  -> authenticated control POST /actions
       action = Measure
       reference = @e...
  -> exact managed preview pulls queued action
  -> preview bridge executes read-only Measure
  -> LocalView instrumentation inspect(reference)
  -> preview bridge projects ONLY:
       reference
       rect
       document_rect
       viewport
       route
  -> preview completes action
  -> control validates action lineage and retains evidence
  -> desktop polls bounded result window
  -> desktop validates result/action/reference/route/geometry
  -> MeasureReceipt returned to React
  -> Inspector renders compact human status
```

This wave does not introduce arbitrary JS execution.

---

## 6. Why a dedicated Measure action

Do not expose a generic `Inspect` action directly to Human-First React.

Generic inspect payloads contain more than Measure needs:

- semantic name/description;
- safe attributes;
- style packet;
- ancestry;
- source hints.

Measure should follow least authority.

The bridge action should therefore be:

```rust
BridgeActionKind::Measure
```

The preview executor may internally call LocalView instrumentation `inspect(reference)`, but it must project only bounded geometry/route metadata before completing the action.

---

## 7. Read-only action classification

`Measure` is not a consequential UI-changing action.

Control routing currently permits `Snapshot` through the non-consequential public action path while UI-changing actions require canonical V4.3 consequential authority.

V2.2 extends the read-only allowance to:

```text
Snapshot
Measure
```

It must not broaden public access to Click, TypeText, Key, Scroll or Focus.

The test contract must explicitly prove that adding Measure does not reopen those actions through the simple queue endpoint.

---

## 8. Element reference authority

The selection reference is a LocalView instrumentation reference generated by:

```text
@e + bounded hash
```

The desktop command must reject malformed references before queueing.

Initial accepted syntax:

- starts with `@e`;
- hash portion is ASCII hexadecimal;
- total length bounded;
- no whitespace;
- no selector syntax;
- no path separators;
- no URL syntax.

The exact validator should remain simple and deterministic.

A valid-looking reference is not proof that the element still exists. The managed page must resolve it at execution time.

---

## 9. Reference freshness

No stale-geometry fallback.

If the selected element:

- was removed;
- belongs to an old route;
- belongs to an old page incarnation;
- cannot be resolved;
- resolves after route drift that violates validation;

the command fails.

The UI may ask the user to select the element again.

It must not display geometry from an older observer event as if it were current.

---

## 10. Preview-side Measure projection

The preview bridge already has session-bound action execution.

Add:

```javascript
case 'measure': {
  if (!queued.reference) throw new Error('measure requires an element reference');
  const inspected = window.__LOCALVIEW__?.inspect?.(queued.reference) ?? null;
  if (!inspected) throw new Error('measure element reference unavailable');

  return {
    reference: inspected.reference,
    rect: inspected.node?.rect ?? null,
    document_rect: inspected.node?.documentRect ?? null,
    viewport: inspected.viewport ?? null,
    route: inspected.route ?? null,
  };
}
```

Do not return:

- semantic text;
- attributes;
- style packet;
- ancestry;
- source hint;
- form values;
- private selectors.

---

## 11. Measurement receipt

Desktop exposes a narrow serializable receipt.

Suggested shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementMeasureReceipt {
    pub reference: String,
    pub rect: MeasureRect,
    pub document_rect: MeasureRect,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub route: String,
    pub measured_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasureRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
```

React receives only this receipt.

---

## 12. Geometry validation

Every numeric field must be finite.

Rectangle rules:

- width >= 0;
- height >= 0;
- x/y may be negative for partially off-viewport elements;
- width/height bounded by the existing CSS safety dimension;
- `x + width` and `y + height` finite;
- document rect width/height must agree with viewport rect within bounded tolerance;
- absurd coordinates fail closed.

Viewport rules:

- width > 0;
- height > 0;
- finite;
- below LocalView CSS safety bounds.

Do not round authoritative values to integers in desktop.

Instrumentation currently rounds to 0.1 CSS px; preserve that precision.

---

## 13. Route validation

Measurement must remain tied to the exact managed surface.

Before queueing:

- exact session surface exists;
- surface label belongs to the session;
- current URL is loopback and LocalView-allowed.

After receiving the result:

- result route parses successfully;
- result route remains LocalView-allowed;
- result route corresponds to the managed surface route under the same canonical route semantics used elsewhere;
- if the surface route changed while measurement was in flight, fail closed.

This prevents a stale selection from another navigation being presented as current geometry.

---

## 14. Session validation

The Tauri command receives `SessionId`, not a free-form URL.

The control endpoint independently verifies that the session exists.

The preview bridge independently verifies the preview caller/session binding before taking or completing actions.

V2.2 must preserve all three layers.

---

## 15. Bounded request lifecycle

The desktop command must not poll forever.

Suggested policy:

```text
measure timeout: 2,500 ms
poll interval: 40–75 ms
maximum result history read: existing bounded action result endpoint
```

Flow:

1. queue Measure;
2. retain returned action id;
3. poll action results;
4. accept only exact action id;
5. stop on success/failure;
6. timeout with a human-safe error;
7. never confuse a previous Measure result with the current request.

---

## 16. Cancellation and duplicate requests

Human-First shell owns a single in-flight Measure request.

While measuring:

- Measure button is disabled or `aria-busy=true`;
- repeated clicks do not queue another request;
- selecting another element may invalidate the pending UI result;
- V2.2 does not require backend cancellation for the short bounded read-only measurement.

If a later wave adds cancellation, it must preserve action lineage.

---

## 17. Control evidence

Successful Measure is observed layout truth and should be retained as layout evidence.

Control already inserts generic interaction evidence for completed public actions.

V2.2 should additionally retain a bounded Layout evidence object for successful Measure with:

- session id;
- region/reference;
- measurement geometry payload;
- source identifying managed-preview measurement;
- native-webview engine identity;
- observed uncertainty;
- high confidence;
- capture time from action completion.

Do not store generic full inspect payload.

---

## 18. Failure evidence

A failed Measure may remain represented through the existing sanitized Interaction evidence.

Do not store raw target-page exception text in human UI.

Control sanitization/redaction remains authoritative for retained errors.

---

## 19. Desktop command boundary

Suggested Tauri command:

```rust
#[tauri::command]
async fn measure_current_selection(
    app: tauri::AppHandle,
    session_id: SessionId,
    reference: String,
) -> Result<ElementMeasureReceipt, String>
```

It owns:

- reference syntax validation;
- managed-surface preflight;
- queue request;
- bounded wait;
- result id match;
- payload validation;
- route revalidation;
- final receipt projection.

React cannot invoke the control service directly.

---

## 20. Frontend API boundary

Suggested API:

```ts
measureElement: (sessionId: string, reference: string) =>
  invoke<ElementMeasureReceipt>(
    'measure_current_selection',
    { sessionId, reference }
  )
```

There must be no rectangle argument.

---

## 21. Selection source in Inspector

The Human-First Inspector already derives the current selection from recent LocalView focus evidence.

Measure may be enabled only when:

- active session exists;
- selected focus event exists;
- selected event has a non-empty LocalView reference;
- no measurement is currently in flight.

No selection means disabled Measure.

---

## 22. Human UI

The Measure button leaves the unavailable-action component and becomes a real action.

Other unwired actions remain fail-closed:

- Open source;
- Ask AI;
- Fix.

Capture remains real from V2.1.

Responsive remains independent.

Suggested states:

```text
idle
measuring
success
failure
```

---

## 23. Human-readable result

Compact success example:

```text
Measured 128.4 × 40 CSS px
x 24.0 · y 132.5
```

The default UI should not show:

- action id;
- raw route;
- evidence id;
- control endpoint;
- DOM ref internals unless useful in Advanced.

The stable reference may remain implicit.

---

## 24. Localization

Add canonical keys for:

- Measure;
- Measuring…;
- measured result prefix;
- CSS pixel unit if localized presentation needs it;
- x/y position labels where required;
- measure unavailable;
- measure failed;
- select an element first.

All supported locale dictionaries remain complete.

English fallback remains canonical.

---

## 25. Accessibility

The real Measure button must:

- be a semantic button;
- have localized accessible name;
- expose busy state while running;
- expose disabled state when no selection;
- not trap keyboard focus;
- announce result status through an appropriate live region without excessive verbosity.

The status should not repeatedly announce observer refreshes.

---

## 26. Error isolation

Measure failure must not collapse Inspector, Capture or the inspected workspace.

The shell catches command failure and renders a localized human-safe message.

Visible UI must not include raw strings such as:

```text
element reference not found
action_result_without_inflight_origin
reqwest error
JSON decode error
```

Those may remain in internal diagnostics/logs where appropriate.

---

## 27. Runtime race handling

A measurement can race with:

- HMR;
- route change;
- element removal;
- focus change;
- page reload;
- preview reconnect.

Fail closed when the result cannot be tied to the same trusted request and route.

The user may retry after selecting the current element.

---

## 28. No stale-result overwrite

If the user selects element B while a Measure for element A is in flight, completion of A must not overwrite B's visible measurement state as though B were measured.

The shell should associate in-flight state with the reference used for the request.

On completion:

- show result only if the selected reference still matches;
- otherwise discard it from visible Measure status.

The backend evidence may still retain the completed observation.

---

## 29. Security and privacy

Measure must not expose:

- form values;
- password content;
- arbitrary text content;
- private mask selectors;
- cookies/storage;
- arbitrary attributes;
- arbitrary JavaScript result;
- source code.

Geometry and route are sufficient.

The route must continue through existing safe URL/redaction semantics.

---

## 30. Performance

Measurement should be lightweight:

- one queued read-only action;
- one instrumentation reference resolution;
- bounded action-result polling;
- no screenshot;
- no Chromium spawn;
- no full semantic snapshot requirement;
- no full DOM serialization.

The operation should normally finish within one bridge polling cycle plus control round trips.

---

## 31. Why not reuse old observer geometry

Observer events may contain geometry from an earlier moment.

Human Measure is an explicit current-state action.

Reusing old event geometry would create ambiguity around:

- route changes;
- resize;
- scrolling;
- HMR;
- layout shifts.

Therefore V2.2 re-resolves and measures the reference at action time.

---

## 32. Why not use React getBoundingClientRect

The React shell is not the inspected page.

Even where iframe fallback exists, using frontend DOM geometry would couple Measure to presentation mode and weaken native managed-surface authority.

The measurement must originate inside the exact managed target surface.

---

## 33. Why not use screenshot-derived geometry

Visual inference is unnecessary when exact LocalView instrumentation geometry already exists.

Screenshot-derived measurement would be slower and less precise.

Visual capture remains a separate evidence channel.

---

## 34. Why not use native accessibility geometry for this first slice

LocalView has strong OS-level provider geometry infrastructure, but Human-First selection currently uses instrumentation stable refs.

Mapping an instrumentation ref to a cross-platform OS accessibility node is a separate identity problem.

V2.2 should use the exact managed-page instrumentation authority already bound to the selected ref.

A later cross-provider reconciliation wave may compare instrumentation geometry to native accessibility geometry.

---

## 35. RED → GREEN plan

### Gate A — canonical spec

Commit this document first.

### Gate B — RED contract

Require:

- `BridgeActionKind::Measure`;
- queue endpoint permits only Snapshot + Measure on the read-only path;
- preview executor projects geometry only;
- desktop command exists and accepts session + reference only;
- API accepts no geometry;
- Inspector Measure becomes real while sibling unwired actions stay unavailable;
- runtime harness contains success/failure/no-selection/in-flight/stale-selection states.

### Gate C — bridge/control GREEN

Implement Measure action and evidence semantics.

### Gate D — desktop GREEN

Implement bounded authenticated queue/wait/validate command.

### Gate E — frontend GREEN

Wire lifecycle and localized status.

### Gate F — executable runtime/visual evidence

Prove payload, state and failure boundaries.

### Gate G — broader regression

Run full CI and applicable provider/native gates.

---

## 36. Required bridge/control tests

At minimum:

1. Measure serializes as `type=measure`;
2. Measure is read-only-public;
3. Click/TypeText/Key/Scroll/Focus remain rejected from simple public queue path;
4. Measure requires reference;
5. preview bridge calls instrumentation inspect;
6. preview bridge projects geometry only;
7. completed Measure retains Layout evidence;
8. failed Measure does not create false Layout evidence;
9. sanitized error handling remains bounded.

---

## 37. Required desktop tests

At minimum:

1. reference validator accepts LocalView `@e` refs;
2. malformed/oversize references rejected;
3. finite rectangle accepted;
4. non-finite geometry rejected;
5. negative width/height rejected;
6. viewport bounds validated;
7. viewport/document sizes agree within tolerance;
8. exact action id required;
9. action failure returned as command failure;
10. timeout bounded;
11. pre/post route drift rejected;
12. non-loopback route rejected.

Where direct Tauri app testing is expensive, split pure validators from runtime contract tests.

---

## 38. Required frontend/runtime tests

At minimum:

- Measure enabled with active session + selected reference;
- Measure disabled without selection;
- Measure disabled without session;
- Capture remains independently enabled where applicable;
- one click invokes `measure_current_selection`;
- payload contains session/reference only;
- no x/y/width/height sent from React;
- success shows current dimensions;
- forced failure is humanized;
- raw backend error absent from Inspector;
- in-flight second click does not issue duplicate request;
- stale completion after selection change does not overwrite current selection state;
- non-English Measure state renders;
- narrow-width Inspector remains usable.

---

## 39. Visual evidence

Add representative Human-First screenshots:

1. Measure ready with selection;
2. Measure in progress;
3. Measure success;
4. Measure failure;
5. Measure disabled with no selection;
6. non-English Measure success/failure;
7. narrow-width measurement result.

The inspected application remains visually primary.

---

## 40. Existing capabilities that must remain green

V2.2 must not regress:

- trusted Capture V2.1;
- guarded full-page stitching;
- private visual redaction;
- managed-surface ownership;
- preview session binding;
- action cancellation lineage;
- V4.3 consequential action authority;
- observer ingestion;
- native provider suites;
- full repository CI.

---

## 41. Expected files

Likely:

- `crates/live-bridge/src/lib.rs`
- `crates/control/src/runtime.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/permissions/localview.toml`
- `apps/desktop/src/api.ts`
- `apps/desktop/src/app/LocalViewShell.tsx`
- `apps/desktop/src/features/FloatingTools.tsx`
- `apps/desktop/src/i18n.ts`
- `apps/desktop/src/styles.css`
- dedicated V2.2 tests/workflows
- `tools/human-first-ui-v2/capture.mjs`

Do not force changes into every listed file if a smaller implementation is sufficient.

---

## 42. Non-goals

V2.2 does not:

- add arbitrary JavaScript evaluation;
- expose generic inspect payload to React;
- implement Open source;
- implement AI/Fix;
- implement Responsive resize;
- edit the inspected page;
- draw persistent measurement rulers over the page;
- capture screenshots automatically;
- convert CSS pixels to physical millimeters;
- infer geometry from screenshots;
- reconcile OS accessibility geometry;
- add a generic developer-tools protocol.

---

## 43. Follow-up possibilities

After Measure closes, likely Human-First candidates are:

- Open source using trusted `sourceHint` provenance;
- Responsive using managed-surface bounds authority;
- explicit full-page capture UI;
- artifact/evidence browser;
- optional on-surface measurement overlay backed by the trusted receipt;
- cross-provider geometry reconciliation.

Each must receive an independent design and executable closure.

---

## 44. Merge readiness

V2.2 may merge only when one immutable exact final head satisfies:

1. canonical V2.2 spec committed;
2. dedicated RED/GREEN contract passes;
3. bridge/control Measure tests pass;
4. desktop geometry/reference/timeout validation tests pass;
5. frontend build passes;
6. Human-First runtime/render Measure audit passes;
7. trusted Capture V2.1 regressions remain green;
8. full repository CI passes;
9. applicable Windows/macOS/Linux provider/native gates remain green;
10. no raw Measure failure leaks into default Inspector;
11. no stale-result overwrite race remains;
12. PR body records exact final head and exact evidence;
13. no code change occurs after the evidence cited for merge.

Do not merge using green evidence from an earlier head.

---

## 45. Final invariant

The Human-First UI may ask LocalView to measure the selected element.

It may not decide what that element's geometry is.

That distinction must remain true in every implementation and follow-up commit.
