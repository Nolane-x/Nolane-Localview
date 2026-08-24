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

## Wave 2 — Visual runtime — active

Landed native viewport path:

- Dedicated `localview-native-capture` platform boundary with a common PNG frame contract and a 24 MiB frame limit.
- WebView2 `CapturePreview` backend on Windows.
- WKWebView native snapshot backend on macOS.
- WebKitGTK visible snapshot backend on Linux.
- No DOM/canvas screenshot reconstruction or silent Chromium fallback in the native adapter path.
- LocalView-managed surface selection only: exact session-owned preview first, feature-gated workspace child second.
- Native route is read from the managed WebView itself and must remain HTTP(S) loopback; callers cannot supply an arbitrary capture window or route.
- Three-second bounded native capture completion path.
- Desktop `capture_viewport` coordinator with a lazily opened 256 MiB local visual `ArtifactStore`.
- PNG bytes are persisted locally as `visual/png`, then dropped before daemon registration; command receipts expose metadata and IDs rather than pixel bytes or filesystem paths.
- Authenticated daemon `Visual` evidence ingestion with artifact/session/route/viewport/revision/backend provenance.
- Cross-platform compile/test contracts for native platform adapters plus a desktop integration contract.
- Deterministic stable-settle evaluator with explicit reasons for DOM, fonts, images, optional HMR signals, DOM mutation, layout and network activity.
- Privacy-safe semantic readiness metadata: document readiness, font status and image-completion counts without image URLs, response bodies, cookies or storage.
- Authenticated capture-settle endpoint that requests an exact fresh semantic snapshot action for every sample; stale observer snapshots cannot satisfy readiness.
- Fresh snapshot presence is timestamped by the daemon at evaluation time rather than trusting the page-provided action completion clock.
- DOM/layout quiet window of 200 ms and metadata-based fetch/XHR completion quiet window from capture policy (250 ms by default).
- The evaluator applies a 300 ms HMR quiet window when an HMR observer signal exists; framework-specific live HMR signal production remains Wave 3 work and is not claimed as complete here.
- Desktop managed-surface preflight followed by a five-second fail-closed settle transaction before native pixel acquisition; unstable timeout never falls through to capture.
- Settle retry is bounded to 25–100 ms, while the native three-second capture timeout remains a separate post-settle budget.
- The managed WebView route is read and loopback-validated again inside native acquisition after settle, closing the preflight/navigation race.
- Managed pages enter a bounded per-session freeze/capture/restore transaction: Web Animations are paused when available, CSS animation/transition motion is suppressed, an 8-second self-healing lease restores visual state if coordination is lost, and pixels are persisted only after exact-token restore acknowledgement succeeds.

Still required before the visual runtime is considered complete:

- hosted GUI smoke tests that render a real managed WebView and prove non-empty PNG capture on Windows, macOS and Linux runners or equivalent controlled hosts;
- screenshot masking for private selectors before agent exposure/persistence;
- true network in-flight accounting beyond the current fetch/XHR completion quiet-period heuristic;
- element/component/section capture execution beyond viewport capture;
- progressive changed-region capture and token-aware visual packet selection;
- guarded full-page stitching;
- before/after and region diff wired into evidence and verification.

**Done when:** one button edit normally costs a crop + delta instead of a full-page screenshot, and every visual artifact can be traced to a session/revision/viewport/target. Native viewport acquisition, artifact/evidence registration, a fail-closed fresh-snapshot settle gate and the live freeze/restore transaction are now present; GUI pixel smoke, masking, progressive-region execution and visual diff verification remain.

## Wave 3 — Runtime telemetry

Foundation already landed in Wave 1:

- console warning/error/exception bridge;
- fetch/XHR request metadata and failed-request evidence;
- long-task/layout-shift observation.

Remaining integration:

- action → request → UI response correlation.
- network failure/delay/mock layer wired to live sessions.
- framework-specific HMR signal production, timeline and settle detection.
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
