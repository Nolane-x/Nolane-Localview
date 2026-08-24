# Implementation Status

LocalView is being implemented as a sequence of independently verifiable vertical slices against the expanded product specification. The repository deliberately separates compile/contracts from runtime proof: a capability is not marked complete merely because an adapter or model exists.

## Landed architecture

- Rust workspace with protocol, discovery, sessions, observation, security and authenticated loopback control plane.
- Semantic/state diff, layout, visual, responsive, source and token-budget layers.
- Capture planning, engine escalation, network, console, accessibility, performance, flow, design grammar and artifacts.
- CLI and MCP bridge.
- Tauri 2 desktop dashboard, system tray, standalone localhost preview windows and a feature-gated native child-WebView workspace.
- In-page instrumentation and a bounded native bridge for observer events and deterministic queued page actions.
- Managed WebViews emit bounded semantic/geometry packets and support synchronous snapshot/inspect flows.
- Native viewport capture adapters exist for Windows WebView2, macOS WKWebView and Linux WebKitGTK behind an isolated platform boundary.
- Bounded local visual artifact persistence and daemon Visual evidence metadata registration are connected to the desktop capture path.
- Cross-platform CI covers the Rust core on Ubuntu, macOS and Windows plus the Linux Tauri/frontend compiler and desktop contract gates.
- A dedicated Linux GUI smoke gate launches a real WebKitGTK WebView under Xvfb, renders deterministic HTML, captures through the same snapshot helper used by production, decodes the PNG and asserts a known rendered pixel. The GUI test is ignored in ordinary headless test runs and is explicitly enabled only by the dedicated CI job.

## Native workspace safety gate

Tauri capability selectors are scoped by **WebView label** instead of parent window. `main` receives only the dashboard capability; `preview-*` and `workspace-*` receive only the loopback preview bridge capability. The `native-workspace` Cargo feature is the only path that enables `tauri/unstable`, and CI checks both default and native-workspace builds.

Compiling native child WebViews still does **not** make them the default workspace. `WorkspaceSurfaceSupport.default_mode` remains `iframe` until overlay/chrome composition, focus/input routing, DPI/bounds behavior, resize/minimize/restore, crash/reconnect cleanup and navigation policy are verified on Windows WebView2, macOS WKWebView and Linux WebKitGTK.

## Stable visual capture transaction

The current visual-capture slice wires a fail-closed private-region transaction around native pixel acquisition:

1. Validate viewport and preflight the exact session-owned managed surface.
2. Acquire a bounded **per-session** capture gate so independent sessions do not serialize globally.
3. Wait for the authenticated stable-capture settle gate.
4. Ask the managed page to freeze visual motion through a private `freeze_visuals` bridge action carrying only bounded private selectors.
5. Resolve those selectors inside the managed page to bounded viewport-relative rectangles and counts. Selector strings, DOM text, attributes and arbitrary page payload do not cross back in the freeze acknowledgement.
6. Acquire native viewport pixels through the platform adapter.
7. Restore visual state with the exact freeze token, even when native acquisition fails.
8. Revalidate the bounded mask geometry in the desktop coordinator and redact the captured PNG **in memory** using `localview-visual` before any artifact write.
9. Persist the redacted frame and register Visual evidence only after restore acknowledgement and complete mask application succeed.

The page-side freeze pauses bounded Web Animations when available and applies temporary CSS suppression for animation/transition motion. Every freeze has an 8-second fail-safe lease that attempts automatic restoration if the coordinator disappears. Private geometry resolution is bounded to 16 selectors, 4,096 unique matched elements and 256 visible rectangles inside a maximum 100,000 × 100,000 CSS-pixel viewport. Invalid selectors, malformed/non-finite geometry, budget overflow, malformed PNG data, frame-dimension mismatch, restore failure or incomplete mask application fail closed and prevent unredacted pixels from being persisted.

The final redaction boundary is native PNG processing in Rust, not a CSS overlay. `localview-visual` decodes bounded PNG input, verifies decoded dimensions against native capture metadata, validates the complete rectangle set before mutation, scales CSS coordinates to native pixels, clips to the frame, fills affected pixels opaquely and re-encodes within bounded memory/encoded-size limits.

Internal `freeze_visuals` / `restore_visuals` actions are not exposed through the generic public action-enqueue route. Private selectors travel only in the internal capture envelope. Freeze tokens remain control-flow state and are not emitted in final capture receipts or Visual evidence metadata.

## Verification state

The settle/freeze/private-geometry/restore/redaction transaction is covered by cross-platform Rust compile/test gates and the Linux Tauri/frontend contract suite. The contracts verify private/public action isolation, bounded selector transport, geometry-only freeze acknowledgements, restore-after-native-failure, restore-before-redaction, redaction-before-persistence, bounded PNG processing and fail-closed malformed geometry/dimension handling.

Linux/WebKitGTK additionally has hosted **rendered-pixel proof**: a dedicated Ubuntu GUI smoke job starts a real GTK window and WebKitGTK WebView under Xvfb, loads deterministic localhost HTML, invokes the production-shared visible-snapshot helper, fully decodes the returned PNG and verifies that the center pixel belongs to the known rendered proof region. A missing display, load failure, snapshot failure, invalid/trivial PNG or wrong rendered pixel fails the job.

The native screenshot adapter remains **Partial**, not Implemented, because equivalent real rendered-pixel GUI proof is still missing for Windows WebView2 and macOS WKWebView. Compile/contracts on those platforms are not treated as substitutes for GUI acquisition evidence.

## Next vertical slices

The strongest remaining capture/perception gates are:

1. hosted or dedicated GUI smoke for real WebView2 and WKWebView rendered-pixel capture, matching the landed WebKitGTK proof contract;
2. element/component/section native pixel execution plus changed-region scheduling;
3. true network in-flight accounting plus framework-specific live HMR signals;
4. capture → visual region diff → deterministic verification as one end-to-end loop;
5. deeper source/runtime ownership correlation and interactive point-and-select inspector wiring;
6. native workspace composition/focus/DPI/crash safety gates before promoting the child WebView surface from feature-gated to default.

The repository should not claim the complete V1/V2/V3 specification is implemented until these live adapters and later verification phases are end-to-end, not merely represented by crates or data models.
