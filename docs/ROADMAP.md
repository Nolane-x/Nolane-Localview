# LocalView Delivery Roadmap

This roadmap maps the expanded product vision to independently testable vertical slices. The repository lands shared primitives early, but a feature is counted as complete only when it is connected to the live runtime path rather than merely represented by a crate or data model.

## Wave 0 — Rust/Tauri foundation — implemented foundation

- Rust workspace and shared protocol.
- Localhost listener discovery and bounded HTTP classifier.
- Stable project/session identity and reconnect lifecycle.
- Authenticated loopback control plane.
- CLI, MCP bridge and Tauri dashboard.
- Native system tray and close-to-tray behavior.
- CI across Linux, Windows and macOS for the Rust core.
- Tiered engine policy, artifact retention, diagnostics/report primitives.

## Wave 1 — Live WebView instrumentation — late-stage active

Landed live path:

- Tauri initialization-script injection into LocalView-managed localhost WebViews.
- In-page ring buffer with bounded retention and secret/query redaction.
- Stable element fingerprints and stable-ref action targeting.
- Bounded deep DOM/ARIA semantic tree rather than interactive-only snapshots.
- Semantic role/name/description/state/attribute packets without live form-value capture.
- Fixed-property computed-style packets with bounded style sampling.
- Viewport and document-space geometry.
- Semantic added/removed/changed-ref delta plus geometry/layout delta transport.
- Bounded visibility state: viewport intersection, ancestor clipping and center-point occlusion hit-testing.
- Explicit dev source hints from `data-source` / `data-component-source` when the application exposes them.
- DOM mutation batching.
- history API, popstate and hash route observation with fresh semantic snapshots.
- focus and scroll observation.
- warning/error/exception observation.
- fetch/XHR metadata observation without response-body capture.
- long-task and layout-shift observation when supported.
- bounded native drain transport for observer events.
- normalized semantic/layout observer events through the daemon/control evidence path.
- queued deterministic click/type/key/scroll/focus/snapshot execution.
- synchronous MCP `page.snapshot` and `page.inspect` backed by fresh completed snapshot actions.
- bridge caller/session ownership validation.
- top-level navigation guard that keeps managed preview/workspace surfaces on loopback.
- capability-isolated `preview-*` / `workspace-*` WebViews.
- React `WorkspaceSurface` abstraction and feature-gated native child WebView lifecycle/bounds/navigation backend.

Current safety gate before native workspace becomes default:

- verify overlay/chrome composition and z-order on WebView2, WKWebView and WebKitGTK;
- verify focus/input routing and keyboard shortcuts;
- verify DPI/logical-pixel bounds, window resize/minimize/restore and multi-monitor movement;
- verify reconnect/crash cleanup and no orphan child WebViews;
- keep iframe fallback until those policies pass.

Remaining Wave 1 integration:

- native accessibility-tree enrichment where platform APIs materially improve over DOM/ARIA semantics;
- sourcemap consumer wired to live runtime nodes;
- React component ownership adapter, followed by Vue/Svelte ownership adapters;
- CSS declaration/specificity tracing and runtime/source correlation beyond explicit dev attributes.

**Done when:** an agent can list a bounded semantic tree, inspect one element, click/type it, and receive only relevant semantic/layout/runtime deltas through an isolated LocalView surface. The core path for that definition now exists; the remaining Wave 1 work deepens native accessibility and framework/source ownership rather than reopening the basic bridge.

## Wave 2 — Visual runtime — next major vertical slice

- Native viewport/element/region capture adapter abstraction.
- WebView2 capture backend on Windows.
- WKWebView snapshot backend on macOS.
- WebKitGTK snapshot backend on Linux.
- Stable capture transaction and settle contract.
- progressive changed-region capture.
- full-page guarded stitching.
- visual packet format with route/viewport/ref/revision provenance.
- before/after and region diff wired to evidence.
- screenshot masking for private selectors before agent exposure/persistence.

**Done when:** one button edit normally costs a crop + delta instead of a full-page screenshot, and every visual artifact can be traced to a session/revision/viewport/target.

## Wave 3 — Runtime telemetry

Foundation already landed in Wave 1:

- console warning/error/exception bridge;
- fetch/XHR request metadata and failed-request evidence;
- long-task/layout-shift observation.

Remaining integration:

- action → request → UI response correlation.
- network failure/delay/mock layer wired to live sessions.
- HMR timeline and settle detection.
- performance-lite sampling and budget packets.

## Wave 4 — Layout + responsive intelligence

- computed grid/flex data.
- overflow/occlusion/sticky collision detection beyond the bounded visibility packet.
- spacing rhythm and alignment families connected to live snapshots.
- breakpoint adaptive/binary search execution.
- responsive contact sheet.
- content stress matrix and locale expansion.

## Wave 5 — Source intelligence

Landed foundation:

- stack/data-source ranking primitives;
- source-region/dependency graph primitives;
- live explicit `data-source` / `data-component-source` propagation into semantic nodes.

Remaining integration:

- sourcemap consumer wired to live runtime nodes.
- React component ownership adapter.
- Vue/Svelte adapters.
- CSS declaration/specificity tracing.
- issue → element → component → source resolution.
- save/HMR/affected-region validation loop.

## Wave 6 — Accessibility + interaction

- axe-core bridge plus LocalView deterministic checks.
- native AX enrichment where useful and privacy-safe.
- keyboard journey.
- focus-path overlay.
- effective hitbox testing.
- dead-click and feedback-latency detection.
- interaction graph discovery and deterministic replay.

## Wave 7 — Visual critic + design grammar

- project design-scale extraction from live evidence.
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

## Later expanded-spec phases

The V2/V3 causal, proof-carrying, multi-agent, content-addressed and attested-proof phases remain future vertical slices. Existing data structures or primitives that anticipate those phases are not counted as end-to-end completion until they are connected to the live runtime, persistence and verification loop.

## Hard constraints across all waves

- No mandatory account or cloud dependency.
- No arbitrary internet browsing mode.
- No permanent Chromium process by default.
- No raw secret/cookie/token exposure to agent surfaces.
- No unbounded screenshot/history cache.
- No claim of automated accessibility completeness.
- No subjective aesthetic score presented as deterministic truth.
- Native pixel capture must use auditable platform adapters rather than silently degrading to DOM/canvas reconstruction.
