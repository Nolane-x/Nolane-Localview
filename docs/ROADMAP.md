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
- Exact-session, exact-action cooperative cancellation for public `BridgeAction` work. Pending cancellation filters delivery; inflight cancellation fences result authority before acknowledgement; cancelled actions cannot create Interaction/Semantic/Layout evidence; `FreezeVisuals` / `RestoreVisuals` remain outside public cancellation authority.
- The desktop keeps ownership of actions already taken from the daemon until cancellation ACK or result publication is terminal. Cancellation/ACK/result transport retries retain `executed` and `cancellationSeen` state, so transient failure cannot orphan the action or execute an already-applied DOM side effect twice. Cancellation remains cooperative and does not force-abort a synchronous WebView call already in progress.
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
- Vue ownership remains to be connected beyond the landed bounded React + Svelte ownership paths;
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
- Capture-storage and changed-region baseline-cache limits are now enforced by owner-local retained-resource authority. `ArtifactStore` and `VisualBaselineCache` compute their own deterministic projected/actual retained usage, desktop holds each owner mutex across reconcile → project → admit → mutate → reconcile, and no caller-writable retained-resource counter or HTTP mutation endpoint is introduced. Artifact deletion failures cannot be counted as reclaimed bytes; baseline projection remains side-effect-free and follows the cache's real replacement/LRU policy.
- Changed-region planning is deterministic and bounded: an unchanged frame emits no new visual artifact; a missing compatible baseline emits one viewport `baseline_reset`; localized change emits bounded CSS regions; broad or excessively fragmented change falls back to one viewport packet.
- Multiple changed regions are cropped from the same decoded private-redacted frame, so region count does not multiply native acquisition or PNG decode cost. The baseline advances only after the entire selected evidence emission succeeds; partial evidence failure leaves the prior baseline authoritative.
- `localview-token-budget` contains a deterministic, model-agnostic visual packet selector. Changed-region and progressive semantic candidates are scored by bounded information-gain × confidence × relevance / normalized-cost utility, invalid geometry/scores fail closed, highly overlapping nested evidence is suppressed, and an explicit `image_regions` budget bounds selected visual regions.
- Desktop `capture_visual_packet` connects that selector to the live runtime without creating another capture authority. An optional stable ref is resolved from a fresh semantic snapshot while holding the same per-session capture gate; a positive image budget then performs exactly one shared settle → freeze → native viewport acquisition → restore → private redaction transaction, computes changed-region candidates from the already-redacted frame, selects evidence, crops/persists only selected redacted regions, and commits the private baseline only after evidence succeeds. `image_regions = 0` returns explicit metadata-only output before native acquisition.
- The V3 Perception Budget Contract is represented by the exact four specification dimensions: `latency_ms`, `text_tokens`, `image_regions` and `chromium_spawns`. Deterministic evaluation returns `within_budget`, fails closed on an overrun without an allowed reason, or returns `escalated` while preserving one of the four explicit reasons: `critical_issue`, `explicit_deep_mode`, `insufficient_evidence`, or `browser_specific_suspicion`.
- `capture_visual_packet` consumes that full contract. It derives the bounded text/image selector budget, records measured pre-persistence latency, packet text-token estimate, selected image-region count and `chromium_spawns = 0` for the native path, then evaluates the contract before any selected visual artifact is persisted or the private baseline advances. The budget decision is returned separately from the token-counted packet so budget accounting is not circular.
- Active Perception budget authority is connected beyond the desktop packet path: `localview-planner` chooses one next observation under the same four-dimensional contract and owns escalation reasons, while Tier-3 engine admission consumes the authorized plan. Chromium cannot be selected merely because a caller asks for deep mode; browser-specific suspicion is required.
- Authenticated `POST /v1/sessions/{id}/perception/plan` derives diagnosis, planner signals, the next budgeted action and engine admission from retained live state. Public callers cannot inject `budget_escalation_reason` or a pre-authorized plan.
- Authenticated `POST /v1/sessions/{id}/perception/step` re-plans internally on every request. It executes the selected `SemanticSnapshot` through the exact fresh-snapshot action/result authority, treats an empty plan as a no-op, and fails closed for action kinds that are not supported by that endpoint rather than silently converting them into generic page commands.
- The semantic execution loop is now closed: the authenticated action-result path retains successful native snapshot payloads as Semantic + Layout evidence before result publication; the following planner cycle consumes only trusted observed untainted `native-semantic-snapshot` / `native-webview` evidence when the live observer window lacks those facts. Arbitrary retained Semantic/Layout evidence is not allowed to suppress a required observation.
- Whole-cycle budget authority is connected through `POST /v1/sessions/{id}/perception/cycle`. `localview-planner` evaluates cumulative `spent + next` against the original contract with saturating arithmetic and scores against remaining budget. The bounded coordinator repeatedly re-plans from retained evidence, carries cumulative text/image/Chromium reservations, replaces forecast latency with measured elapsed wall-clock time at execution/completion boundaries and re-evaluates using only the planner-owned escalation reason. Caller-supplied `spent`, serialized plans and escalation reasons are rejected.
- Planner-selected native visual work has a dedicated executor boundary. The live bridge carries a bounded native executor request; the desktop worker executes `VisualPacket` through the existing settle → freeze → native acquisition → restore → private-redaction/baseline authority rather than a second screenshot path, and the control plane correlates only the exact request result and retained evidence.
- Planner-authorized Chromium work has a real bounded process executor. `ChromiumEscalation` is admitted only through browser-specific planner authority, uses an ephemeral process/profile execution path, retains bounded contract evidence, and does not require a permanent Chromium process.
- Deterministic visual verification is connected end to end. Desktop `capture_changed_regions` publishes bounded `native-visual-diff` Contract evidence; `VisualDiffCapture` reuses that exact transaction; authenticated `/v1/sessions/{id}/verify/visual/capture` enqueues the native request, correlates the exact result/evidence and computes PASS/FAIL/INCONCLUSIVE server-side from retained observed evidence. Callers cannot submit the changed ratio, verdict or trusted evidence id.
- Native capture+verify is admitted by the existing Runtime Resource Governor before it can cross the native-executor boundary. High-pressure denial returns before enqueue, and the RAII reservation covers native execution/wait while releasing automatically on success, timeout, failure or cancellation. The broader Runtime Resource Governor program still has remaining enforcement work listed below.
- CPU/RAM are intentionally not fields of the Perception Budget Contract. They belong to the separate Runtime Resource Governor in the expanded specification. Capture-storage and visual baseline-cache limits now have owner-local enforcement; browser-process, hidden-surface and analysis-concurrency enforcement remain broader governor work.
- PNG bytes are persisted locally as `visual/png`, then dropped before daemon registration; command receipts expose metadata and IDs rather than pixel bytes or filesystem paths.
- Authenticated daemon `Visual` evidence ingestion with artifact/session/route/viewport/revision/backend provenance, plus a separate fail-closed `/evidence/visual-region` schema for bounded region metadata.
- Cross-platform compile/test contracts for native platform adapters plus desktop transaction, target-ordering, region-evidence, fresh-snapshot, progressive-target, changed-region scheduling, visual-packet selection, Perception Budget enforcement, planner/Tier-3 authority, live perception planning/execution, retained feedback, whole-cycle budget accounting, native visual-diff execution, deterministic live visual verification and governed capture verification.
- Progressive resolver adversarial tests cover missing refs, NaN/infinite/zero/offscreen geometry, invalid viewport, mismatched source ownership and duplicate component/section rectangles. Desktop authority locks exact-level selection, one native acquisition, route/viewport drift rejection and restore → redaction → crop → persistence ordering; visual-packet contracts additionally lock deterministic budget selection, zero-image short-circuit, full budget admission before persistence and baseline commit-after-evidence ordering.
- Dedicated hosted Linux GUI smoke: Ubuntu/Xvfb starts a real GTK window and WebKitGTK WebView, renders deterministic localhost HTML, captures through the same visible-snapshot helper used by production, fully decodes the PNG and asserts the known center proof pixel. Ordinary headless test runs keep this test ignored; the dedicated GUI job explicitly enables it.
- Dedicated hosted macOS GUI smoke: a custom harness owns the real AppKit main thread, initializes NSApplication, creates a real NSWindow + WKWebView, loads deterministic loopback HTML, pumps NSRunLoop, captures through the same WKWebView snapshot helper used by production, fully decodes the PNG and asserts the same known center proof pixel. This proof exposed a production ImageIO bug in direct NSImage representation encoding; the adapter now materializes snapshot pixels through TIFF → NSBitmapImageRep → PNG before frame validation.
- Dedicated hosted Windows GUI smoke: a real Win32 parent window and installed WebView2 controller run on an STA COM thread, navigate to a deterministic `127.0.0.1` HTTP fixture with exact URI/NavigationId correlation, verify DOM/CSS/geometry and capture through production-shared `CapturePreview`. The fixture server is bounded but tolerates WebView2 speculative zero-byte preconnects; successful navigation, full PNG decode and the known center rendered pixel are still mandatory.
- Deterministic stable-settle evaluator with explicit reasons for DOM, fonts, images, optional HMR signals, DOM mutation, layout and network activity.
- Privacy-safe semantic readiness metadata: document readiness, font status and image-completion counts without image URLs, response bodies, cookies or storage.
- Authenticated capture-settle endpoint that requests an exact fresh semantic snapshot action for every sample; stale observer snapshots cannot satisfy readiness.
- Fresh snapshot presence is timestamped by the daemon at evaluation time rather than trusting the page-provided action completion clock.
- DOM/layout quiet window of 200 ms and metadata-based fetch/XHR completion quiet window from capture policy (250 ms by default).
- Stable capture now combines that completion quiet window with a privacy-safe fresh aggregate fetch/XHR in-flight count. Active requests block with `network_inflight`; missing or malformed state fails closed with `network_state_unknown` while the network gate is enabled. Readiness exports only the aggregate count, and repeated/rejected XHR `send()` attempts cannot steal another request's counter, timing metadata or completion-listener ownership.
- The evaluator applies a 300 ms HMR quiet window when an HMR observer signal exists. Managed-page instrumentation now produces bounded loopback-only HMR telemetry for strongly classified Vite, Next.js and webpack development transports; those events feed the existing daemon-owned settle gate while unrelated or remote WebSockets remain outside HMR observation authority.
- Desktop managed-surface preflight followed by a five-second fail-closed settle transaction before native pixel acquisition; unstable timeout never falls through to capture.
- Settle retry is bounded to 25–100 ms, while the native three-second capture timeout remains a separate post-settle budget.
- The managed WebView route is read and loopback-validated again inside native acquisition after settle, closing the preflight/navigation race.
- Managed pages enter a bounded per-session freeze/capture/restore transaction: Web Animations are paused when available, CSS animation/transition motion is suppressed, an 8-second self-healing lease restores visual state if coordination is lost, and pixels continue only after exact-token restore acknowledgement succeeds.
- Default private selectors travel only through the private capture-action envelope and are resolved inside the managed page to geometry-only receipts. The live bridge strips selectors and arbitrary page payload before daemon storage.
- Private-region resolution is bounded to 16 selectors, 4,096 unique elements, 256 visible rectangles and a 100,000 × 100,000 CSS-pixel viewport; invalid selector/geometry/budget paths fail the capture instead of silently persisting an uncertain frame.
- After exact restore, the desktop revalidates live target geometry and uses `localview-visual` to redact the native viewport PNG in memory before the artifact store or changed-region baseline is reachable. Region crops occur only after that redaction. PNG decode/encode budgets, native dimension checks, whole-mask validation and crop verification make malformed/incomplete processing fail closed.
- Explicit guarded full-page capture is now connected through `capture_full_page` without adding a platform full-page adapter. One per-session gate owns settle → one non-renewed 30-second freeze lease → bounded pure-Rust planning (32 tiles, 50,000 CSS px document height, 128 MiB RGBA, 32,768 output-pixel height) → exact-token absolute scroll → fresh settle/probe/private-mask geometry → the existing native viewport acquisition → per-tile redaction before decode/stitch. Visible fixed/sticky content fails closed before native copy. The original scroll and exact visual state are restored before final encoding, artifact persistence or the dedicated `/evidence/visual-full-page` registration; no intermediate tile artifact/evidence is persisted and there is no Chromium/Playwright fallback.
- Trusted responsive sweep/contact-sheet execution is now connected through `capture_responsive_sweep`. The request carries only a session ID plus 1–4 canonical preset IDs; desktop verifies the exact LocalView-owned preview/registry owner, rejects maximized/fullscreen authority, removes the preview minimum only for the bounded transaction, converges each canonical size, settles/freezes/captures/redacts each viewport, restores the original physical preview size and canonical minimum, settles and revalidates route, then persists exactly one contact-sheet artifact and dedicated responsive Visual evidence record. Arbitrary caller width/height, device emulation and Chromium/Playwright fallback are not claimed.

Still required before the visual/runtime Active Perception path is considered complete:

- Vue/CSS ownership depth beyond the landed bounded React + Svelte ownership paths;
- extend the separate Runtime Resource Governor with analysis-concurrency enforcement when a concrete concurrent analysis owner exists;

**Done when:** one button edit normally costs an evidence-backed crop + delta instead of a full-page screenshot, and every visual artifact can be traced to a session/revision/viewport/target. Native viewport acquisition, all three hosted rendered-pixel proofs, artifact/evidence registration, fail-closed fresh-snapshot settling with true aggregate network in-flight accounting, live freeze/restore, pre-persistence private-region redaction, bounded CSS-region execution, evidence-backed progressive semantic targeting, baseline-driven changed-region scheduling, token-aware visual packet selection, planner-owned four-dimensional Perception Budget authority, native visual execution, planner-authorized Chromium execution, retained semantic feedback, single-request whole-cycle budget accounting, cooperative public-action cancellation and the capture → diff → retained evidence → deterministic verification loop are now present. Runtime Resource Governor capture-storage/cache, Chromium-process and hidden-surface ownership enforcement are landed; analysis-concurrency enforcement and Vue/Svelte/CSS ownership depth remain. Guarded full-page stitching and canonical responsive preset/contact-sheet execution are now present as explicit bounded operations; adaptive/binary responsive execution, content/locale stress, infinite-page crawling and fixed/sticky normalization are not claimed. Hard force-abort inside an already-running synchronous WebView/platform action is intentionally not claimed by the cooperative cancellation protocol.

## Wave 3 — Runtime telemetry

Foundation already landed in Wave 1:

- console warning/error/exception bridge;
- fetch/XHR request metadata and failed-request evidence;
- long-task/layout-shift observation.

Landed live integration:

- canonical action → request → UI-response correlation is connected to the live session path. Correlation is anchored to canonical V4.3 action IDs/receipts, derives bounded temporal/causal evidence from trusted observer/network/runtime signals, deduplicates repeated derived evidence and preserves stale-session isolation rather than reviving the legacy direct action route.
- bounded live network fault authority is connected for exact LocalView-managed loopback sessions. Canonical rules support fetch/XHR fail, bounded delay and empty-body status mock effects under finite leases and hit budgets; authenticated control-plane install/get/clear operations are exact-session and exact-managed-surface scoped, private bridge state is sanitized, install acknowledgement failures compensate/clear fail-closed, and real Chromium proof covers fail/delay/mock, hit exhaustion, expiry, explicit clear, observation metadata, zero final in-flight debt, unrelated loopback pass-through and real HTTP non-loopback pass-through.
- aggregate fetch/XHR in-flight accounting remains exactly-once and privacy-safe and is shared with capture settling; network fault observations expose only bounded rule/effect metadata rather than response bodies or secrets.
- framework-specific HMR signal production is connected to the live managed-page path for strongly classified loopback Vite, Next.js and webpack transports. Retained HMR packets contain only bounded framework/phase/update-count metadata, the existing observer timeline carries them, and the existing daemon-owned 300 ms HMR quiet evaluator consumes them without retaining raw WebSocket payloads, module paths or query tokens.
- bounded performance-lite packets are connected to the existing long-task/layout-shift observer path. `localview-performance` computes truthful full-window long-task count/total/max while retaining only the deterministic longest-duration sample under a canonical default budget of 8 and hard cap of 16, rejects malformed/negative measurements, carries finite non-negative cumulative layout shift only, and exposes the packet through authenticated exact-session `GET /v1/sessions/{id}/performance-lite`. The live analysis response uses the same packet authority, and packet schemas contain no route, raw observer payload, source path, token or arbitrary application text.

The bounded Wave 3 runtime-telemetry scope listed above is now connected end-to-end. This does not claim CPU profiling, JavaScript flamegraphs, heap snapshots/allocation profiling, Core Web Vitals completeness, remote telemetry, or root-cause proof from performance telemetry alone.

The Wave 3 fault layer is intentionally not a general proxy: arbitrary internet interception, TLS MITM, response/body/header fixtures, WebSocket interception and a permanent interception backend remain unclaimed.

## Wave 4 — Layout + responsive intelligence

Software closure landed:

- live bounded layout projection now carries computed grid/flex, overflow and position evidence into deterministic layout analysis;
- overflow/container clipping, sibling overlap and bounded fixed/sticky collision checks are connected to live snapshots with fail-closed authority when geometry or stacking evidence is insufficient;
- spacing rhythm and local alignment families are connected to live evidence with explicit heuristic/deterministic classification and subpixel tolerance;
- canonical four-preset responsive sweep/contact-sheet execution remains the native-pixel evidence baseline with exact preview restoration before persistence;
- adaptive responsive execution is live with at most 12 unique widths, bounded candidate downsampling, fixed route/state/height authority, observed transition bracketing and bounded binary refinement;
- responsive issue evidence covers overflow, clipping, disappearance, collisions, large layout jumps, out-of-viewport controls/text, breakpoint-local regression and nearby instability without claiming aesthetic quality;
- transactional content/locale stress is live through four bounded synthetic profiles on the exact managed surface, with exact session/route/document-generation authority, privacy-safe mutation, deterministic overflow/collision/disappearance comparison, serialized capture ownership and mandatory restoration proof.

Truth boundaries:

- adaptive results are observed responsive transitions, not claimed CSS media-query/source breakpoints;
- contradictory, truncated or non-monotonic responsive evidence is inconclusive;
- content-stress profiles are synthetic expansion/locale-shape probes, not translations and not retained source text;
- W10 mixed-DPI physical hardware proof remains an independent deferred hardware gate and is not part of Wave 4 software closure.

Closure evidence for Waves 4–5 is accepted only from exact-head/live-path tests; these software closures do not imply that later Waves 6–9 or the deferred W10 hardware proof are complete.

## Wave 5 — Source intelligence

Software closure landed:

- stack/data-source ranking, source-region/dependency graph primitives and explicit `data-source` / `data-component-source` propagation remain the highest explicit source authority;
- the bounded Source Map v3 consumer and authenticated exact-session project-owned Source Map runtime resolve only contained project sources and retain no `sourcesContent`;
- trusted retained RuntimeError positions correlate through the same project-owned authority from caller-supplied `event_seq` only;
- coordinate-independent `ComponentOwnership` is live across bounded React, Svelte and Vue exact-element evidence with explicit-source precedence and no retained props/state/context/hooks;
- Vue compiler paths are canonicalized under backend-owned project authority before retention;
- bounded CSS declaration trace and fail-closed author-cascade evidence are live; supported author winners remain explicitly narrower than final browser-computed provenance;
- CSS source authority can promote a same-origin stylesheet hint to verified project file and exact declaration line/column only when a unique bounded parse or exact project-owned Source Map segment proves it;
- direct human point-select is live on the exact managed WebView: pointer hit-testing yields a stable LocalView `ElementRef`, suppresses the selection click, cleans up one-shot overlay/listeners, fences route/session/document-generation drift and feeds the existing Measure/Open Source/Ask AI/Fix/Verify flows;
- trusted Fix now reaches HMR-aware settle, fresh semantic/source validation, bounded affected-region evidence and deterministic Verify receipts.

Truth boundaries:

- LocalView never fabricates file/line/column, component ownership, selector authority or root cause;
- ambiguous CSS declarations, unsafe/remote source maps, cross-origin content and unsupported runtime contexts fail closed;
- the bounded author-cascade subset is not claimed to be the browser's complete cascade winner model;
- direct point selection requires the exact LocalView-managed/instrumented surface; opaque cross-origin frame contents are not traversed.

## Wave 6 — Accessibility + interaction — software scope closed

Software closure landed:

- bundled/local axe-core bridge plus LocalView deterministic checks;
- privacy-safe native AX enrichment with discrepancy evidence instead of silent precedence;
- bounded keyboard/focus journey with route/document-generation authority;
- transient focus-path overlay isolated from semantic/capture evidence;
- effective hitbox evidence that distinguishes nominal geometry from clipping/occlusion uncertainty;
- bounded dead-click/feedback-latency observation with observed/delayed/no-feedback/inconclusive states;
- live interaction-graph discovery and deterministic replay receipts bound to stable refs and exact state identity.

Truth boundaries:

- axe/native AX evidence does not claim automated accessibility completeness;
- unsafe/destructive interaction discovery remains fail-closed;
- geometry-only suspicion is not promoted to deterministic pointer-delivery truth.

Closure PR: #195. Exact-head dedicated Wave 6 and full repository CI were green before merge.

## Wave 7 — Visual critic + design grammar — software scope closed

Software closure landed:

- live project design-scale extraction from bounded semantic/layout/style evidence;
- density, balance and hierarchy feature extraction;
- explicit deterministic / heuristic / subjective evidence classes;
- structured critic findings with stable refs, measured evidence, confidence and optional source hints;
- isolated critic overlay model;
- serializable design-regression baselines and bounded diff logic.

Truth boundaries:

- inferred clusters are observed project patterns, not automatically official design tokens;
- subjective aesthetic findings do not fail CI or masquerade as deterministic truth;
- missing style/source authority stays unavailable/inconclusive rather than guessed.

Closure PR: #194. Exact-head dedicated Wave 7 and full repository CI were green before merge.

## Wave 8 — Headless/CI — software scope closed

Software closure landed:

- real headless LocalView execution through the existing authenticated/session-owned runtime;
- deterministic bounded fixture/state adapters and explicit exit policy;
- complete JSON / Markdown / HTML report production;
- bounded baseline artifacts with content-addressed identity and retention;
- Git-aware local annotations without remote GitHub API dependence;
- CI annotations/policy that distinguish hard deterministic failures from heuristic/subjective findings;
- bounded digest attestation over report/state/evidence identity without claiming an unproven trusted signature.

Truth boundaries:

- headless execution does not bypass daemon auth, runtime resource governance or local-session authority;
- heuristic/subjective findings do not fail CI by default without explicit policy;
- digest attestation is not called cryptographically signed unless a real signer exists.

Closure PR: #196. Exact-head dedicated Wave 8 and full repository CI were green before merge.

## Wave 9 — Autonomous verification — production closure reopened

The 2026-09-22 production audit disproved the earlier end-to-end closure claim. PR #198 landed substantial Wave 9 libraries and contracts, but the live Trusted Fix/Verify call graph did not execute the complete autonomous pipeline. In particular, `ShadowWorkspace::prepare` and autonomous receipt construction were not reachable from the human Apply path before this audit.

Production wiring now present on this branch:

- human `apply_fix_proposal` reaches `FixProposalStore::begin_apply`;
- `begin_apply` derives the exact repository revision, binds the pending reviewed proposal to a disposable `SemanticOnly` candidate, and executes `run_production_candidate_preflight`;
- production preflight calls `ShadowWorkspace::prepare -> proof -> cleanup` before the existing real-file Apply transaction;
- the shadow worktree is source-only: LocalView materializes only validated candidate files from exact committed blobs, applies the bounded patch there, and never checks out or launches the project as part of this preflight;
- Git commands issued by the shadow layer disable repository hooks/fsmonitor inheritance and external-diff inheritance;
- cleanup and real-worktree equality remain explicit proof obligations;
- external side-effect containment is represented as `not_proven` unless an actual isolation authority proves otherwise;
- production preflight has no `Verified` state. With current platform authority, a clean preflight is truthfully `Inconclusive`; identity, real-worktree, revision or cleanup failures reject/fail closed;
- the existing Trusted Fix transaction remains the only human-reviewed real-file write authority;
- the existing Trusted Verify path still performs fresh semantic/source/visual partial revalidation after Apply.

Library capability that is implemented but **not yet production-orchestrated end-to-end**:

- affected-state compilation;
- execution of the applicable hard/soft contract set for the real candidate;
- mutation challenges against that production candidate;
- predicted-versus-actual affected-state comparison;
- issuance of a production `AutonomousVerificationReceipt` backed by fresh evidence;
- complete partial/escalated revalidation accounting tied into that receipt.

Truth boundaries:

- a candidate is never called autonomous-`Verified` merely because Wave 9 libraries exist;
- `external_side_effect_containment = not_proven` blocks the autonomous `Verified` verdict and the verified handoff;
- a temp worktree, loopback address or source-only patch does **not** prove network/process/filesystem containment outside the shadow root;
- unsupported executable isolation remains Inconclusive rather than being promoted to success;
- the simpler human Trusted Verify receipt is not re-labelled as an autonomous proof receipt;
- no root-cause claim is invented from correlation-only evidence.

Historical integration: PR #198 merged as `bded849d7fdb4a640b4cd12c802381b783bc42c2` after its exact head passed the then-current CI. That CI evidence remains evidence for the implemented Wave 9 library surface; it is not evidence that the complete pipeline was production-reachable.

## Wave 1–9 roadmap status

Waves 1–8 retain their bounded software closures. Wave 9 is **Partial at the live-product level** until the remaining production orchestration and isolation/evidence gates above are satisfied. The repository must not use the former “Waves 1–9 live closure” wording as a production fact.

Independently open:

- Wave 9 full production orchestration and real isolation authority;
- native workspace composition/focus/crash/DPI policy before promotion to the default surface;
- V4.3 W10 physical mixed-DPI proof on PR #116, which remains deferred and unmeasured on hosted CI;
- broader Partial capabilities explicitly retained in `docs/SPEC_COVERAGE.md`;
- analysis-concurrency authority only when a concrete concurrent owner exists;
- security/production hardening and adversarial audit work;
- later expanded causal, proof-carrying, multi-agent, content-addressed and attested-proof vertical slices.

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
