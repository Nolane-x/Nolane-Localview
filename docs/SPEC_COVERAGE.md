# Product Specification Coverage Ledger

This ledger keeps implementation claims tied to the LocalView AI-Native Localhost Runtime Product Spec. `Implemented` means the capability is connected through the live product path and has verification evidence. `Partial` means useful code exists but one or more required runtime integrations are still missing. `Planned` means the product capability is still specification-only or has only non-executing scaffolding.

| Spec capability | Status | Repository surface / next gate |
| --- | --- | --- |
| Native Rust core | Implemented | `crates/core`, daemon, CLI |
| Listener discovery | Implemented | `crates/discovery` |
| Frontend HTTP classification | Implemented | Vite/Next/Nuxt/Svelte/Astro/Angular/Webpack/Storybook evidence |
| Port-independent project identity | Implemented | `localview-core` + sessions |
| Session lifecycle / reconnect grace | Implemented | `crates/sessions` |
| Tray + close-to-tray desktop shell | Implemented | Tauri desktop |
| Lightweight native preview | Partial | Native WebView preview and feature-gated child workspace exist; iframe remains the conservative default until cross-platform composition/focus/DPI policy is validated |
| Human View zero-clutter workspace | Partial | Full-canvas workspace and floating chrome exist; native child-WebView policy gate remains |
| X-Ray / Inspector UI | Partial | Semantic/layout/source primitives and live inspect data exist; interactive point-and-select overlay is not yet fully wired |
| Stable capture policy | Partial | Exact fresh-snapshot DOM/font/image readiness plus live DOM/layout and fetch/XHR-completion quiet-window evaluation is wired through an authenticated five-second fail-closed desktop gate before native viewport capture. Managed pages enter a bounded freeze/private-geometry/restore transaction; private selectors are resolved to geometry-only receipts, native pixels are restored before processing, then private rectangles are redacted in memory before persistence. Framework-specific live HMR signal production and true network in-flight accounting remain |
| Progressive capture regions | Partial | element → component → section → viewport planning exists and native viewport execution is wired; element/component/section pixel execution and changed-region scheduling remain |
| Pixel visual diff | Partial | Live native viewport pixels can now enter bounded redacted artifacts/evidence and `crates/visual` has diff/redaction primitives; capture → region diff → verification is not yet one end-to-end loop |
| Semantic snapshot model | Implemented | Managed WebViews emit bounded deep DOM/ARIA semantic trees with states, geometry, computed-style packets, visibility/occlusion and deltas; this is not claimed to be the native OS accessibility tree |
| Stable element refs | Implemented | Injected observer fingerprints + live bridge/action paths |
| State diff | Implemented | Live semantic added/removed/changed refs plus layout deltas are transported through the observer path |
| Layout intelligence | Partial | Live document geometry and bounded visibility/occlusion are wired; broader overflow/sticky/alignment/spacing audits remain to be connected to runtime evidence |
| Responsive intelligence | Partial | Preset/adaptive/binary breakpoint algorithms exist; live resize/sweep/contact-sheet execution remains |
| Source mapping hints | Partial | Explicit `data-source` / `data-component-source` hints are carried on live semantic nodes; sourcemap consumers and framework component ownership are still pending |
| Framework awareness | Partial | Framework detection/adapters exist as primitives; React/Vue/Svelte live ownership/source correlation is not yet complete |
| Console analysis | Partial | Warning/error/exception metadata is captured, securely drained and retained; richer action/route correlation remains |
| Network analysis | Partial | fetch/XHR completion metadata and failures are captured without response bodies and securely drained; completion-event quiet periods participate in capture settling, while true in-flight accounting, mock/delay and causal correlation remain |
| Accessibility analysis | Partial | Deterministic accessible-name/image/hit-target checks exist plus DOM/ARIA semantics; axe/native AX integration and journey verification remain |
| Performance analysis | Partial | long-task/layout-shift observation is live; broader sampling/budget packets remain |
| Interaction flows | Partial | Flow graph/replay primitives and deterministic page actions exist; discovery/replay verification is not yet complete |
| Design grammar | Partial | spacing/radius/type/control inference primitives exist; live project extraction and regression baselines remain |
| Observation bus | Implemented | Bounded observer history plus authenticated native drain |
| Diagnostics fusion | Partial | Deterministic/heuristic/subjective issue assembly exists; more live visual/source evidence still needs to feed it |
| Reports | Partial | JSON/Markdown/HTML renderers exist; complete CI/headless report production is pending |
| Artifact storage | Implemented | Bounded deduplicating local store primitives are used by the native visual coordinator with a 256 MiB visual budget; private mask redaction occurs before the artifact store is reached and capture receipts expose IDs/metadata rather than artifact paths |
| Engine tier escalation | Partial | Static/lightweight/native WebView/Chromium policy exists; on-demand Tier-3 execution is not yet the default verified visual path |
| Token budgeting | Partial | Compact/deep/minimal serialization budget primitives exist; active-perception budgeting across all tools remains |
| Local permission/security model | Implemented | Bearer token, loopback control plane, caller/session ownership checks, navigation guard and redaction policy; native capture is dashboard-only, resolves only exact session-owned managed surfaces, serializes only captures belonging to the same session, fails closed on settle timeout, anchors fresh-snapshot presence to daemon evaluation time rather than the page clock, revalidates the live WebView route immediately before acquisition, keeps freeze/restore/private-selector controls off the generic public action queue, strips selector/private page payload from stored freeze results, restores exact visual state before processing pixels, and discards pixels if restoration or complete private-region redaction is not proven |
| MCP control plane | Partial | stdio MCP exposes live sessions/observer/actions plus synchronous `page.snapshot` and `page.inspect`; native visual/source/a11y/flow surfaces remain incomplete |
| AI Critic / Point-and-Ask | Partial | UI/evidence architecture exists; point-select transport and optional model-provider execution are not complete |
| Secure observer drain | Implemented | Managed preview/workspace WebViews drain bounded observer events through caller/session-validated Tauri commands into the authenticated local control plane |
| Direct click/type agent actions | Implemented | Stable-ref click/type/key/scroll/focus/snapshot actions execute inside the managed WebView through the bounded queue/result bridge; internal visual freeze/restore/private-selector actions use a separate authenticated path and cannot be enqueued through the generic action endpoint |
| Native screenshot adapter | Partial | Real WebView2/WKWebView/WebKitGTK viewport adapters, bounded desktop artifact persistence, fresh stable-settle gating, per-session freeze + private geometry → native capture → exact-token restore → in-memory redaction → persist ordering and Visual evidence metadata path exist with cross-platform compile/test contracts; hosted GUI smoke proving real rendered pixels on all three platforms remains the completion gate |
| Full replay / rich state timeline UI | Partial | Observation/action primitives exist; complete deterministic replay and desktop timeline are pending |
| Full DevTools replacement | Explicitly out of scope for v1 | Product spec says not to build it |
| Cloud accounts / collaborative remote browser | Explicitly out of scope for v1 | Local-first runtime by design |

## Completion rule

A capability is not called complete merely because a crate, panel, model or planner exists. `Implemented` requires a live execution path plus verification evidence. Pure algorithms that are valuable but not yet connected to the product remain `Partial` until their end-to-end path is real.
