# Wave 6 — Accessibility + Interaction Intelligence

Date: 2026-09-21  
Branch: \`feat/wave6-accessibility-interaction\`

## Scope

This lane closes the bounded accessibility, keyboard/focus, effective-hitbox, feedback-observation, interaction-graph, and deterministic-replay authority for Wave 6. It intentionally does not implement Wave 7 visual criticism/design grammar, Wave 8 report persistence/headless CI product surfaces, or Wave 9 mutation/shadow-patch verification.

Wave 6 reuses the existing stable \`ElementRef\`, semantic snapshot, geometry/visibility evidence, action queue, managed LocalView preview/workspace surfaces, bounded observer history, and native provider foundations. It does not rewrite Windows UIA, macOS AX, or Linux AT-SPI providers.

## Accessibility evidence model

Accessibility findings carry an explicit provenance:

- \`local_deterministic\`: a LocalView semantic invariant directly proven from the bounded semantic snapshot, such as an interactive node with no accessible name.
- \`axe_rule\`: a rule violation returned by the locally bundled axe-core engine. An axe result is rule evidence, not a claim of complete accessibility.
- \`native_ax\`: enrichment from provider-normalized native accessibility evidence.
- \`heuristic\`: bounded suspicion where delivery or platform authority is incomplete.

axe-core is pinned to 4.13.0 in the desktop package. Tauri packages \`axe.min.js\` and its upstream license as application resources. Managed preview/workspace initialization reads axe only from that application resource, with the exact local \`node_modules\` path as a development fallback. There is no CDN/runtime network loader.

The page-side scan retains only bounded rule metadata: rule id, impact, help text, LocalView stable reference when exact mapping is proven, and an explicit unresolved state otherwise. axe selectors are used transiently to attempt a unique element mapping and are not retained as selector authority. The scan does not retain arbitrary DOM HTML, form values, cookies, storage, tokens, or arbitrary page text.

## Native AX enrichment

Wave 6 consumes \`NativeSemanticNodeObservation\`, the existing provider-normalized evidence object. It does not hold UIA/AX/AT-SPI live handles.

A native node may enrich a DOM stable ref only when an explicit binding matches the entire \`ProviderElementRef\`, including:

- provider family;
- provider incarnation;
- target incarnation;
- opaque provider element id;
- acquisition cut;
- realization state.

Opaque ids, role/name similarity, semantic locator hints, or selectors never mint a stable ref. Non-current/virtualized elements are excluded.

The adapter can preserve native role, bounded name, enabled/offscreen state, and provider-declared selected/expanded/focusable state where that field is actually present. Password/sensitive nodes do not export native names. The normalized native snapshot currently has no cross-provider bounds field, so Wave 6 deliberately leaves native bounds unavailable instead of fabricating them.

DOM/native conflicts are retained as discrepancies; neither side is silently chosen as truth.

## Keyboard journey

The browser authority observes actual browser keyboard focus movement. Once armed, real \`Tab\` and \`Shift+Tab\` key events are correlated with subsequent \`focusin\` state. Wave 6 does not synthesize Enter/Space activation to discover controls.

Hard limits:

- at most 64 transitions;
- bounded candidate enumeration;
- maximum 30 second journey lease, 8 seconds by default;
- exact route and caller-supplied document generation fencing.

The receipt distinguishes body/document focus loss, repeated focus, loops, positive tabindex suspicion, hidden/offscreen focus, focus traps, route drift, and document-generation drift. A focusable candidate is called \`unreachable\` only after a keyboard cycle was actually observed; an incomplete journey reports it merely as an unvisited candidate.

## Focus-path overlay

The overlay is an isolated LocalView-owned page layer:

- \`data-localview-owned="wave6-focus-path"\`;
- pointer-events disabled;
- fixed/contained geometry so it does not participate in app layout;
- numeric markers plus stable refs only, never page text;
- excluded from LocalView semantic application content by the existing owned-node boundary;
- hidden while the existing visual-freeze attribute is active;
- removed on finish, Escape, replacement, route/generation failure, transition cap, and deadline.

No \`LocalViewShell.tsx\` changes are required. A later integrator can mount controls against this independent authority.

## Effective hitbox

Wave 6 distinguishes:

- nominal bounding rect;
- clipped effective rect;
- pointer-events state;
- browser hit-test samples from \`elementsFromPoint\`;
- bounded stable-ref occluder evidence.

Geometry-only reduction remains suspicion. Deterministic blocked-delivery claims require browser/native hit-test authority. The implementation does not equate nominal CSS width/height with actual pointer delivery.

## Feedback observation

The feedback probe observes an interaction that a trusted caller has separately classified safe. The page is not allowed to self-authorize a click through a data attribute.

Signals are bounded metadata only:

- focus changed;
- semantic fingerprint changed;
- route changed;
- resource-request count increased;
- layout-related mutation observed;
- accessible state changed.

Receipts distinguish \`observed_feedback\`, \`delayed_feedback\`, \`no_observed_feedback\`, and \`inconclusive\`. No-feedback is never renamed to “dead button.” Deadline expiry, stable-ref invalidation, generation drift, or incomplete authority remains inconclusive.

## Interaction graph and replay

\`localview-flow\` binds graph nodes to:

- route;
- document generation;
- bounded semantic fingerprint;
- optional viewport.

Edges retain action kind, stable target ref, exact pre/post state identity, safety classification, and bounded evidence refs. Discovery has explicit node, edge, route-state, and deadline budgets and rejects unknown/destructive action admission.

Replay never repairs a stale ref with a selector guess. Before each step it requires exact pre-state compatibility and a valid stable ref. The live bridge then reuses the existing deterministic action queue for Click, Focus, Tab, and Shift+Tab. Missing Key/Scroll payloads are rejected rather than invented. A replay receipt records attempted/passed steps, first failure, before/after state, evidence refs, and \`complete / failed / inconclusive\`.

## Privacy and non-claims

Wave 6 does not retain passwords, input values, cookies, tokens, storage, arbitrary HTML, AX private values, or arbitrary page text.

The following remain intentionally inconclusive when evidence is insufficient:

- axe pass does not mean accessibility complete;
- cross-origin iframe internals are not fabricated from parent-document selectors;
- native AX bounds are unavailable until a cross-provider normalized bounds authority exists;
- geometry-only occlusion does not prove pointer delivery failure;
- no observed feedback does not prove a dead click;
- keyboard candidates not visited before a proven cycle are not called unreachable;
- unknown/destructive interaction candidates are not auto-probed;
- DOM/native AX discrepancies remain disagreements, not silently reconciled truth.

## Verification

Focused gates cover:

- deterministic missing-name and image-alt checks;
- nominal/effective target distinction;
- clipped and occluded browser hit testing;
- local axe execution and unresolved target behavior;
- native AX exact-binding and sensitive-node privacy;
- real Chromium Tab/Shift+Tab/focus-trap execution;
- focus loop, body loss, offscreen focus, route drift, generation drift, and transition cap;
- overlay semantic exclusion, visual-freeze suspension, Escape/deadline cleanup;
- observed, delayed, no-observed, and inconclusive feedback;
- safe/unsafe discovery boundaries;
- replay success, state mismatch, stable-ref invalidation, and graph caps;
- cross-origin iframe and sensitive-input privacy.

The Wave 6 workflow checks out the exact PR head SHA, asserts that HEAD before tests, scopes formatting/check/clippy to affected Wave 6 surfaces, and runs a real Chromium proof.
