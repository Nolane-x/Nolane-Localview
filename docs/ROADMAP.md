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

Landed native visual path:

- Dedicated `localview-native-capture` platform boundary with a common PNG frame contract and a 24 MiB frame limit.
- WebView2 `CapturePreview` backend on Windows.
- WKWebView native snapshot backend on macOS.
- WebKitGTK visible snapshot backend on Linux.
- No DOM/canvas screenshot reconstruction or silent Chromium fallback in the native adapter path.
- LocalView-managed surface selection only: exact session-owned preview first, feature-gated workspace child second.
- Native route is read from the managed WebView itself and must remain HTTP(S) loopback; callers cannot supply an arbitrary capture window or route.
- Three-second bounded native capture completion path.
- Desktop `capture_viewport` coordinator with a lazily opened 256 MiB local visual `ArtifactStore`.
- Desktop `capture_region` reuses the exact native viewport acquisition path and performs bounded Rust-side region processing only after exact restore and private redaction; it does not introduce separate platform-specific region screenshot APIs.
- Region targets require finite positive in-viewport CSS geometry and are revalidated against the live CSS viewport reported during freeze. A resize/drift race discards pixels after restore and before persistence.
- Authenticated `GET /v1/sessions/{id}/semantic-snapshot/fresh` requests a newly completed snapshot action for the exact session, accepts only the matching action result and projects it into a bounded `PageSnapshot`. Stale observer history, unrelated action results and malformed payloads cannot satisfy the request.
- Pure `localview-capture` progressive resolution now turns one stable `ElementRef` from that fresh snapshot into ordered evidence-backed `element → component → section → viewport` targets. Element geometry is expanded by the existing 120 CSS-pixel policy and clamped; component ownership requires corroborated explicit `source.component` evidence on an ancestor; section ownership requires an explicit semantic section/landmark ancestor; equal intermediate rectangles are deduplicated while the viewport remains an explicit final fallback.
- Desktop `capture_progressive_target` executes one exact caller-requested target level. It acquires the per-session gate before the fresh snapshot, rejects caller/snapshot viewport mismatch and missing component/section levels rather than silently widening, then reuses one shared settle → freeze → native viewport acquisition → restore → private redaction transaction. After acquisition it rejects live route/viewport drift, crops only the already-redacted image for non-viewport levels and returns provenance/confidence/snapshot version/route with the visual evidence receipt.
- Platform adapters remain viewport-only for progressive targeting; no WebView2/WKWebView/WebKitGTK element/component/section capture APIs were added.
- Desktop `capture_changed_regions` uses that same auditable viewport transaction once per scheduling pass: settle → private freeze → one native viewport acquisition → exact restore → private redaction → one RGBA decode → baseline comparison → bounded region/viewport evidence emission.
- Changed-region baselines are already-private-redacted `Arc<RgbaImage>` frames held only in a deterministic 96 MiB / 32-entry LRU cache. Compatibility is bound to route, CSS viewport, device-scale factor and native pixel dimensions; incompatible contexts are invalidated rather than diffed.
- Changed-region planning is deterministic and bounded: an unchanged frame emits no new visual artifact; a missing compatible baseline emits one viewport `baseline_reset`; localized change emits bounded CSS regions; broad or excessively fragmented change falls back to one viewport packet.
- Multiple changed regions are cropped from the same decoded private-redacted frame, so region count does not multiply native acquisition or PNG decode cost. The baseline advances only after the entire selected evidence emission succeeds; partial evidence failure leaves the prior baseline authoritative.
- `localview-token-budget` now contains a deterministic, model-agnostic visual packet selector. Changed-region and progressive semantic candidates are scored by bounded information-gain × confidence × relevance / normalized-cost utility, invalid geometry/scores fail closed, highly overlapping nested evidence is suppressed, and an explicit `image_regions` budget bounds selected visual regions.
- Desktop `capture_visual_packet` connects that selector to the live runtime without creating another capture authority. An optional stable ref is resolved from a fresh semantic snapshot while holding the same per-session capture gate; a positive image budget then performs exactly one shared settle → freeze → native viewport acquisition → restore → private redaction transaction, computes changed-region candidates from the already-redacted frame, selects evidence, crops/persists only selected redacted regions, and commits the private baseline only after evidence succeeds. `image_regions = 0` returns explicit metadata-only output before native acquisition.
- Visual packet text metadata reuses the existing bounded token serializer. The current slice intentionally budgets text plus visual-region count and deterministic visual cost; it does not yet claim the full V3 latency/CPU/memory/Chromium-spawn Perception Budget Contract.
- PNG bytes are persisted locally as `visual/png`, then dropped before daemon registration; command receipts expose metadata and IDs rather than pixel bytes or filesystem paths.
- Authenticated daemon `Visual` evidence ingestion with artifact/session/route/viewport/revision/backend provenance, plus a separate fail-closed `/evidence/visual-region` schema for bounded region metadata.
- Cross-platform compile/test contracts for native platform adapters plus desktop transaction, target-ordering, region-evidence, fresh-snapshot, progressive-target, changed-region scheduling and visual-packet integration contracts.
- Progressive resolver adversarial tests cover missing refs, NaN/infinite/zero/offscreen geometry, invalid viewport, mismatched source ownership and duplicate component/section rectangles. Desktop authority locks exact-level selection, one native acquisition, route/viewport drift rejection and restore → redaction → crop → persistence ordering; visual-packet contracts additionally lock deterministic budget selection, zero-image short-circuit and baseline commit-after-evidence ordering.
- Dedicated hosted Linux GUI smoke: Ubuntu/Xvfb starts a real GTK window and WebKitGTK WebView, renders deterministic localhost HTML, captures through the same visible-snapshot helper used by production, fully decodes the PNG and asserts the known center proof pixel. Ordinary headless test runs keep this test ignored; the dedicated GUI job explicitly enables it.
- Dedicated hosted macOS GUI smoke: a custom harness owns the real AppKit main thread, initializes NSApplication, creates a real NSWindow + WKWebView, loads deterministic loopback HTML, pumps NSRunLoop, captures through the same WKWebView snapshot helper used by production, fully decodes the PNG and asserts the same known center proof pixel. This proof exposed a production ImageIO bug in direct NSImage representation encoding; the adapter now materializes snapshot pixels through TIFF → NSBitmapImageRep → PNG before frame validation.
- Dedicated hosted Windows GUI smoke: a real Win32 parent window and installed WebView2 controller run on an STA COM thread, navigate to a deterministic `127.0.0.1` HTTP fixture with exact URI/NavigationId correlation, verify DOM/CSS/geometry and capture through production-shared `CapturePreview`. The fixture server is bounded but tolerates WebView2 speculative zero-byte preconnects; successful navigation, full PNG decode and the known center rendered pixel are still mandatory.
- Deterministic stable-settle evaluator with explicit reasons for DOM, fonts, images, optional HMR signals, DOM mutation, layout and network activity.
- Privacy-safe semantic readiness metadata: document readiness, font status and image-completion counts without image URLs, response bodies, cookies or storage.
- Authenticated capture-settle endpoint that requests an exact fresh semantic snapshot action for every sample; stale observer snapshots cannot satisfy readiness.
- Fresh snapshot presence is timestamped by the daemon at evaluation time rather than trusting the page-provided action completion clock.
- DOM/layout quiet window of 200 ms and metadata-based fetch/XHR completion quiet window from capture policy (250 ms by default).
- The evaluator applies a 300 ms HMR quiet window when an HMR observer signal exists; framework-specific live HMR signal production remains Wave 3 work and is not claimed as complete here.
- Desktop managed-surface preflight followed by a five-second fail-closed settle transaction before native pixel acquisition; unstable timeout never falls through to capture.
- Settle retry is bounded to 25–100 ms, while the native three-second capture timeout remains a separate post-settle budget.
- The managed WebView route is read and loopback-validated again inside native acquisition after settle, closing the preflight/navigation race.
- Managed pages enter a bounded per-session freeze/capture/restore transaction: Web Animations are paused when available, CSS animation/transition motion is suppressed, an 8-second self-healing lease restores visual state if coordination is lost, and pixels continue only after exact-token restore acknowledgement succeeds.
- Default private selectors travel only through the private capture-action envelope and are resolved inside the managed page to geometry-only receipts. The live bridge strips selectors and arbitrary page payload before daemon storage.
- Private-region resolution is bounded to 16 selectors, 4,096 unique elements, 256 visible rectangles and a 100,000 × 100,000 CSS-pixel viewport; invalid selector/geometry/budget paths fail the capture instead of silently persisting an uncertain frame.
- After exact restore, the desktop revalidates live target geometry and uses `localview-visual` to redact the native viewport PNG in memory before the artifact store or changed-region baseline is reachable. Region crops occur only after that redaction. PNG decode/encode budgets, native dimension checks, whole-mask validation and crop verification make malformed/incomplete processing fail closed.

Still required before the visual runtime is considered complete:

- true network in-flight accounting beyond the current fetch/XHR completion quiet-period heuristic;
- framework-specific/sourcemap-backed ownership beyond current explicit source evidence;
- generalization from the current text + image-region visual-packet budget to the full V3 Perception Budget Contract, including latency/CPU/memory/Chromium-spawn accounting and explicit escalation/overrun reasons;
- guarded full-page stitching;
- before/after and region diff wired through evidence into deterministic verification;
- responsive sweep/contact-sheet execution over the same bounded capture authority.

**Done when:** one button edit normally costs an evidence-backed crop + delta instead of a full-page screenshot, and every visual artifact can be traced to a session/revision/viewport/target. Native viewport acquisition, all three hosted rendered-pixel proofs, artifact/evidence registration, fail-closed fresh-snapshot settling, live freeze/restore, pre-persistence private-region redaction, bounded CSS-region execution, evidence-backed progressive semantic targeting, baseline-driven changed-region scheduling and the first live token-aware visual packet selection policy are now present. Full Perception Budget accounting, deeper framework/source ownership, guarded stitching, responsive execution and the capture → diff → deterministic verification loop remain.

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
- live explicit `data-source` / `data-component-source` propagation into semantic nodes;
- progressive component targeting consumes corroborated explicit `source.component` ancestry without fabricating ownership from tag/class/depth heuristics.

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
