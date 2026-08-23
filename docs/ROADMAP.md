# LocalView Delivery Roadmap

This roadmap maps the large product vision to independently testable vertical slices.

## Wave 0 — Rust/Tauri foundation — active

- Rust workspace and shared protocol.
- Localhost listener discovery and bounded HTTP classifier.
- Stable project/session identity and reconnect lifecycle.
- Authenticated loopback control plane.
- CLI, MCP bridge and Tauri dashboard.
- Native system tray and close-to-tray behavior.
- CI across Linux, Windows and macOS for the Rust core.

## Wave 1 — Live WebView instrumentation

- Inject observation bootstrap into preview WebViews.
- DOM mutation batches with HMR settle detection.
- Stable element ref fingerprints across re-render.
- AX/role/name extraction.
- geometry/computed-style packets.
- route/focus/scroll state.
- page snapshot v1.
- event subscription transport.

**Done when:** an agent can list interactive elements, inspect one element, click/type it and receive only state deltas.

## Wave 2 — Visual runtime

- Native viewport/element/region capture adapters.
- Stable capture transaction.
- progressive changed-region capture.
- full-page guarded stitching.
- visual packet format.
- before/after and region diff.
- screenshot masking for private selectors.

**Done when:** one button edit normally costs a crop + delta instead of a full-page screenshot.

## Wave 3 — Runtime telemetry

- WebView console bridge.
- request/response metadata and failed-request packets.
- action → request → UI response correlation.
- network failure/delay/mock layer.
- HMR timeline.
- performance-lite sampling.

## Wave 4 — Layout + responsive intelligence

- computed grid/flex data.
- overflow/occlusion/sticky collision detection.
- spacing rhythm and alignment families.
- breakpoint adaptive/binary search execution.
- responsive contact sheet.
- content stress matrix and locale expansion.

## Wave 5 — Source intelligence

- sourcemap consumer.
- React component ownership adapter.
- Vue/Svelte adapters.
- CSS declaration/specificity tracing.
- issue → element → component → source resolution.
- save/HMR/affected-region validation loop.

## Wave 6 — Accessibility + interaction

- axe-core bridge plus LocalView deterministic checks.
- keyboard journey.
- focus-path overlay.
- effective hitbox testing.
- dead-click and feedback-latency detection.
- interaction graph discovery and deterministic replay.

## Wave 7 — Visual critic + design grammar

- project design-scale extraction.
- density/balance/hierarchy features.
- deterministic / heuristic / subjective evidence classes.
- critic overlay with confidence/evidence/source hints.
- design regression baselines.

## Wave 8 — Headless/CI

- headless visual session.
- deterministic fixture/state adapters.
- report export: JSON, Markdown and HTML.
- baseline artifacts with content-addressed retention.
- Git-aware annotations and CI attestations.

## Wave 9 — Autonomous verification

- affected-state compilation.
- candidate patches in isolated shadow state.
- contract/invariant system.
- mutation challenge suite.
- predicted versus actual impact.
- proof receipts and partial revalidation.

## Hard constraints across all waves

- No mandatory account or cloud dependency.
- No arbitrary internet browsing mode.
- No permanent Chromium process by default.
- No raw secret/cookie/token exposure to agent surfaces.
- No unbounded screenshot/history cache.
- No claim of automated accessibility completeness.
- No subjective aesthetic score presented as deterministic truth.
