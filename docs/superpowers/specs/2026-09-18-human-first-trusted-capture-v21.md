# Human-First Trusted Capture V2.1

## Status

Canonical design specification for the first post-V2 Human-First primary-action wiring wave.

This specification starts from `main` merge commit:

`ac1474ff365b0a8bfe82fcd3f57bb3fbac54878b`

which merged PR #142 (Human-First LocalView UI/UX V2) after exact-head closure at `3724132ea6cd52cc81ad209842723a322b1ec0d0`.

The V2 merge tree and exact GREEN PR head tree are identical:

`4c63ad8371ffa38bddfaeae68bd673a380cc3d64`

This file is the durable source of truth for V2.1. Future AI must inspect the latest branch head and executable tests before assuming implementation status.

---

## 1. Why V2.1 exists

Human-First V2 deliberately left Inspector primary actions disabled when the frontend did not own a trustworthy execution path.

That was correct.

The next step is not to make every disabled button appear functional. The next step is to wire one action end-to-end without weakening LocalView's authority model.

The first action is **Capture**.

Capture is selected because LocalView already has a mature native visual-capture substrate:

- managed-surface ownership checks;
- loopback route checks;
- stable-settle;
- visual freeze/restore;
- private-mask redaction;
- native WebView capture;
- bounded artifact storage;
- visual evidence registration;
- per-session capture serialization;
- guarded full-page capture as an independent stronger path.

The missing piece is a human-facing entry point whose geometry authority is owned by desktop/native code rather than caller-supplied React state.

---

## 2. Product goal

When a human opens Inspector and presses **Capture**, LocalView should capture the currently managed viewport through the existing native capture pipeline and return a small human-readable success state.

The user should not need to understand:

- `ViewportMeta`;
- device scale factor;
- freeze tokens;
- evidence endpoints;
- artifact stores;
- capture gates;
- platform adapter details.

The UI should answer only:

1. did the capture succeed;
2. what was captured;
3. is there evidence identity available;
4. can the user safely continue working.

---

## 3. Core invariant

> The human-facing Capture action must never trust React to declare the authoritative viewport geometry.

React may request:

> capture the current managed viewport for this session.

React must not be allowed to authoritatively submit:

- CSS viewport width;
- CSS viewport height;
- device scale factor;
- native pixel dimensions;
- route;
- managed-surface identity;
- private-mask geometry;
- artifact identity;
- evidence identity.

Those values must be derived or verified inside the existing trusted desktop/native transaction.

---

## 4. Existing command is not sufficient as the human API

The existing Tauri command:

```rust
capture_viewport(
    app,
    state,
    session_id,
    viewport: ViewportMeta,
    revision: Option<String>,
)
```

is useful for trusted/internal callers, tests and existing visual workflows.

It accepts a `ViewportMeta` from the caller.

Human-First V2.1 must not simply expose that shape directly through `api.ts` and fill it using browser values such as:

```ts
window.innerWidth
window.innerHeight
window.devicePixelRatio
```

That would move geometry authority into the presentation layer.

V2.1 therefore introduces a narrower desktop-owned command.

---

## 5. New command

Suggested command name:

```text
capture_current_viewport
```

Suggested Rust contract:

```rust
#[tauri::command]
pub async fn capture_current_viewport(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String>
```

The caller supplies only:

- session identity;
- optional revision correlation.

The caller does not supply viewport geometry.

---

## 6. Authority derivation

### 6.1 Session and surface ownership

The command must first run the existing managed-surface preflight.

It must refuse capture when:

- no LocalView-managed surface exists for the session;
- surface label/session ownership does not match;
- route is not loopback/allowed;
- surface is not the LocalView-owned preview/workspace surface.

### 6.2 CSS viewport authority

CSS viewport width/height must come from the page-side freeze receipt already produced by the trusted internal capture-control path.

The command must run stable-settle, then freeze visuals, and read:

- `viewport_css_width`;
- `viewport_css_height`.

These values must be:

- finite;
- positive;
- within LocalView's existing viewport safety bounds;
- convertible to `u32` without overflow;
- sufficiently close to integral CSS pixel dimensions for the current native capture contract.

If the page reports impossible or unsupported geometry, fail closed.

Do not silently clamp arbitrary dimensions.

### 6.3 Device scale factor authority

Device scale factor must be read from the managed native surface/window, not from React.

Use the Tauri/native surface scale factor exposed by the exact managed surface being captured.

The value must be:

- finite;
- > 0;
- <= the existing visual capture safety maximum;
- obtained from the same surface/session that passes ownership preflight.

### 6.4 Route authority

Route remains owned by the managed native surface and existing capture path.

React must never send the route as capture authority.

### 6.5 Private redaction authority

Private selectors/mask geometry remain inside the existing freeze/private capture envelope.

The Human-First command must reuse current redaction-before-persistence behavior.

No raw private pixels may be persisted merely because Capture is human-triggered.

---

## 7. Transaction ordering

The trusted human capture transaction is:

```text
Capture button
  -> api.captureCurrentViewport(sessionId)
  -> Tauri capture_current_viewport
      -> validate managed surface ownership + loopback route
      -> acquire existing per-session capture gate
      -> stable-settle
      -> FreezeVisuals
          -> trusted CSS viewport dimensions
          -> private mask geometry
      -> read native managed-surface scale factor
      -> construct ViewportMeta internally
      -> native viewport capture
      -> require capture/frame viewport coherence
      -> restore visuals
      -> redact private pixels
      -> persist bounded visual artifact
      -> register visual evidence
      -> return VisualCaptureReceipt
  -> Inspector shows human-readable success
```

Failure at any trust/restore/redaction/persistence/evidence step fails the command.

---

## 8. Restore-before-success invariant

A successful Human-First Capture result must imply visual freeze has been restored.

Do not show success merely because native pixels were obtained.

The existing rule remains:

> page visual state restoration is part of capture success.

If restore acknowledgement fails:

- discard pixels;
- do not expose a successful receipt;
- show a humanized failure state.

---

## 9. Native adapter boundary

V2.1 must not modify platform adapters merely to wire the UI.

The existing native viewport adapters remain authoritative:

- WebView2 on Windows;
- WKWebView on macOS;
- WebKitGTK on Linux.

The new command is orchestration above those adapters.

---

## 10. Reuse instead of duplicate capture logic

Do not fork a second implementation of:

- artifact storage;
- evidence registration;
- private redaction;
- native capture;
- capture gate management;
- stable settle.

Refactor current internal helpers only where needed so both:

- existing `capture_viewport`; and
- new `capture_current_viewport`

share the same core transaction semantics.

The new command should primarily replace caller-owned viewport construction with desktop-owned derivation.

---

## 11. Viewport conversion policy

The freeze receipt currently carries CSS dimensions as floating-point values while `ViewportMeta` uses `u32`.

V2.1 must define conversion explicitly.

Required policy:

1. value finite;
2. value > 0;
3. value <= existing maximum CSS viewport dimension;
4. nearest integer conversion is checked, not blindly cast;
5. absolute difference between reported value and rounded integer must be within a tiny browser quantization tolerance;
6. integer must fit `u32`;
7. otherwise fail closed with a stable capture error.

Suggested tolerance:

```text
<= 0.01 CSS px
```

Do not use a broad tolerance that would hide geometry mismatch.

---

## 12. Native scale-factor helper

Introduce one narrow helper that reads the exact managed surface scale factor for a session.

Suggested conceptual contract:

```rust
fn managed_surface_scale_factor(
    app: &tauri::AppHandle,
    session_id: SessionId,
) -> Result<f64, String>
```

It must follow the same preview-first/native-workspace fallback and ownership rules as current managed-surface route/capture helpers.

Do not choose an arbitrary app window's scale factor.

---

## 13. Coherence validation

After native capture, require:

- frame CSS width equals derived CSS width;
- frame CSS height equals derived CSS height;
- frame DSF equals the internally derived managed-surface DSF;
- route remains canonical and session-owned;
- native pixel dimensions are positive;
- revision semantics remain unchanged.

Viewport capture should not have a weaker coherence check merely because the UI initiated it.

If current shared helpers do not validate all of these for viewport target, V2.1 should strengthen the shared trusted-current-viewport path.

---

## 14. Frontend API

Add a narrow API method.

Suggested TypeScript types:

```ts
export interface VisualCaptureReceipt {
  artifact_id: string;
  evidence_id: string;
  deduplicated: boolean;
  backend: string;
  route: string;
  viewport: {
    css_width: number;
    css_height: number;
    device_scale_factor: number;
  };
  pixel_width: number;
  pixel_height: number;
  revision?: string | null;
  captured_at_unix_ms: number;
  target: string;
  region?: Rect | null;
}
```

Suggested call:

```ts
api.captureCurrentViewport(sessionId)
```

It should invoke:

```text
capture_current_viewport
```

with no caller viewport.

---

## 15. Inspector behavior

Capture becomes enabled only when a current session exists.

Other previously fail-closed actions remain unchanged unless independently wired with trusted authority.

Specifically, V2.1 does not automatically enable:

- Open source;
- Measure;
- Ask AI;
- Fix;
- Responsive presets.

Capture must not be used as an excuse to make sibling buttons fake-functional.

---

## 16. Human UI states

Inspector Capture needs explicit state.

Minimum states:

### Idle

Button label:

`Capture`

### Capturing

Button disabled while request is active.

Useful visible text can be:

`Capturing…`

Avoid duplicate concurrent user captures from repeated clicking.

Backend per-session capture gate remains the final concurrency authority.

### Success

Show compact human-readable confirmation such as:

`Captured current viewport`

Optionally show:

- pixel dimensions;
- backend;
- evidence identity in a copyable/secondary diagnostic disclosure.

Do not expose raw artifact filesystem paths.

### Failure

Show humanized failure copy.

Do not dump raw backend error strings into the default Inspector.

Advanced/diagnostics may retain technical detail later through an explicit path.

---

## 17. Success lifetime

Capture success state is ephemeral workspace state.

It does not need to survive app restart.

Suggested behavior:

- remain visible until the next capture or panel close;
- or auto-clear after a bounded period.

Do not persist capture UI state in preferences.

The artifact/evidence itself is already persisted through backend authority.

---

## 18. Accessibility

Capture must be keyboard operable.

Requirements:

- real `button`;
- accessible name follows active locale;
- disabled state is semantic;
- capturing state does not remove the accessible name;
- status result uses an appropriate live/status region without excessive announcements;
- focus remains predictable after success/failure.

---

## 19. Localization

Add keys for at least:

- capture in progress;
- capture success;
- capture unavailable;
- capture failed;
- optional evidence label.

English must be canonical.

All registered locale dictionaries must remain structurally complete.

Fallback behavior remains deterministic.

---

## 20. Error isolation

Capture failure must not collapse:

- Inspector;
- current session;
- workspace surface;
- tool rail;
- target bar;
- Console/Network/Advanced.

A failed capture is an auxiliary-tool failure.

The workspace remains usable.

---

## 21. Error taxonomy

Backend may keep stable machine error strings.

The default human UI should map them to bounded categories.

Examples:

- no managed surface -> `Open the preview before capturing.`
- geometry unavailable -> `Current viewport could not be verified.`
- capture busy/resource denial -> `Capture is temporarily unavailable.`
- native capture failure -> `Could not capture the current viewport.`
- restore/evidence failure -> `Capture could not be completed safely.`

Do not surface internal tokens, paths, selector data or raw transport traces.

---

## 22. Security/privacy

Human-triggered Capture must preserve all current privacy behavior.

Required:

- per-tile/per-viewport private masking remains before persistence;
- no selector strings returned to React;
- no page DOM dump returned to React;
- no raw PNG bytes returned through the human command;
- only bounded receipt metadata returns;
- artifact/evidence identifiers remain opaque;
- non-loopback managed surface capture remains refused.

---

## 23. Resource policy

No new unbounded storage path.

Reuse:

- visual artifact storage budget;
- retained-resource governor;
- capture serialization;
- existing PNG encoding/storage.

The frontend does not cache image bytes.

---

## 24. Full-page capture relationship

V2.1 Capture means **current viewport**, not full page.

Do not silently map one click to full-page stitching.

Full-page capture has stricter compatibility and time/resource semantics.

A future UI may expose `Capture full page` as an explicit secondary action with its own UX/error model.

This wave does not add it.

---

## 25. Revision semantics

The human action may omit revision when no trustworthy revision is available in current frontend state.

Do not fabricate a Git revision from display text.

If later a trusted revision is available, it may be passed through unchanged.

---

## 26. RED -> GREEN implementation sequence

### Gate A — canonical design

Commit this specification before implementation.

### Gate B — RED source/contract tests

Add a dedicated V2.1 contract requiring:

- new Tauri command registered;
- command takes session + optional revision but no caller viewport;
- managed surface scale factor helper;
- freeze-derived viewport construction;
- strict geometry conversion;
- shared private-redacted capture path;
- frontend API method without viewport argument;
- Capture button enabled only with session;
- sibling unwired actions remain fail-closed.

### Gate C — Rust GREEN

Implement trusted desktop command and helper/refactor.

Run focused Rust tests.

### Gate D — frontend GREEN

Wire Inspector Capture.

Add capturing/success/failure state.

Keep error humanized.

### Gate E — executable UI/runtime audit

Extend render/runtime harness so it proves:

- Capture enabled with active session;
- Capture disabled/no-op without a session;
- command invocation payload contains session but no viewport;
- success state is rendered;
- forced capture failure remains isolated;
- no raw backend error leaks.

### Gate F — broader regression

Run:

- frontend build;
- dedicated Human-First/V2.1 contract;
- relevant visual capture Rust tests;
- full repository CI;
- native/provider smoke required by repository policy.

---

## 27. Required Rust tests

At minimum add tests for:

1. freeze CSS viewport -> valid `ViewportMeta`;
2. fractional/non-integral CSS dimension outside tolerance rejected;
3. zero/negative/non-finite dimensions rejected;
4. scale factor non-finite/zero/too-large rejected;
5. exact session-owned surface scale factor selected;
6. command does not accept caller viewport;
7. restore failure does not return success;
8. capture receipt preserves artifact/evidence registration;
9. private redaction path remains shared;
10. route/surface preflight remains fail-closed.

Where direct Tauri runtime testing is impractical, combine pure helper tests with contract tests and existing native capture integration coverage.

---

## 28. Required frontend/runtime tests

At minimum:

- Capture button is not the old `UnavailableInspectorAction` when a current session exists;
- button is disabled when no session exists;
- repeated click during in-flight request cannot issue duplicate UI request;
- API invocation is `capture_current_viewport`;
- payload has `sessionId` and no `viewport`;
- success status is localized/human-readable;
- forced backend rejection renders failure without pageerror/unhandled rejection;
- raw backend failure string is absent from visible Inspector text.

---

## 29. Visual evidence

Capture these representative states:

1. Inspector before capture;
2. capture in progress if deterministically capturable;
3. capture success;
4. capture failure;
5. non-English capture success or failure;
6. narrow-width Inspector with capture state.

The captured inspected app should remain visually primary.

---

## 30. Files expected to change

Likely:

- `apps/desktop/src-tauri/src/visual_capture.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src/api.ts`
- `apps/desktop/src/features/FloatingTools.tsx`
- `apps/desktop/src/app/LocalViewShell.tsx`
- `apps/desktop/src/i18n.ts`
- V2.1 tests/workflows
- Human-First render harness

Do not treat this list as exhaustive if implementation proves another file is required.

---

## 31. Non-goals

V2.1 does not:

- redesign the native capture backend;
- change guarded full-page stitching;
- implement screenshot editing;
- return raw image bytes to React;
- add filesystem save-as UI;
- enable fake Measure/Open source/Responsive behavior;
- expose private selectors;
- trust DOM/browser window dimensions as capture authority;
- weaken evidence registration.

---

## 32. Merge readiness

V2.1 may merge only when one exact final head satisfies:

1. this spec committed;
2. dedicated V2.1 source/behavior contract GREEN;
3. trusted current-viewport Rust tests GREEN;
4. Human-First render/runtime capture states GREEN;
5. frontend build GREEN;
6. relevant visual capture regressions GREEN;
7. full repository CI GREEN;
8. applicable native/provider smoke GREEN;
9. no raw failure leakage in default Inspector;
10. PR body records exact head and evidence.

Any code change after closure invalidates prior exact-head evidence.

---

## 33. Future continuation

After trusted Capture is closed, likely Human-First follow-up candidates are:

- Measure using existing trusted geometry authority;
- Open source using observer/source provenance;
- Responsive using managed-surface bounds authority;
- explicit full-page capture UI;
- evidence/artifact browser.

Each must preserve the same principle:

> A visible human control becomes enabled only when LocalView can execute it through an authority path strong enough to justify the affordance.
