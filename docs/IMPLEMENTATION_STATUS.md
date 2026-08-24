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
- Bounded native CSS-region execution is connected above those adapters: the coordinator acquires the real managed viewport, requires exact restore, applies private redaction, verifies live viewport consistency, crops a bounded CSS target in Rust and registers region-scoped Visual evidence without adding separate platform-specific region screenshot APIs.
- Bounded local visual artifact persistence and daemon Visual evidence metadata registration are connected to the desktop capture path.
- Cross-platform CI covers the Rust core on Ubuntu, macOS and Windows plus the Linux Tauri/frontend compiler and desktop contract gates.
- Dedicated GUI smoke gates prove real rendered pixels for Windows/WebView2, macOS/WKWebView and Linux/WebKitGTK through production-shared snapshot helpers. Windows creates a real Win32 parent window, initializes an STA COM WebView2 environment/controller, navigates to a deterministic loopback HTTP fixture with exact URI/navigation correlation, verifies live DOM/CSS/geometry, captures through production-shared `CapturePreview`, fully decodes the PNG and checks the known rendered center pixel. Linux runs a real GTK/WebKitGTK surface under Xvfb; macOS owns the real AppKit main thread, creates NSApplication + NSWindow + WKWebView, pumps NSRunLoop and verifies the same deterministic rendered-pixel contract after native snapshot/PNG decode.

## Native workspace safety gate

Tauri capability selectors are scoped by **WebView label** instead of parent window. `main` receives only the dashboard capability; `preview-*` and `workspace-*` receive only the loopback preview bridge capability. The `native-workspace` Cargo feature is the only path that enables `tauri/unstable`, and CI checks both default and native-workspace builds.

Compiling native child WebViews still does **not** make them the default workspace. `WorkspaceSurfaceSupport.default_mode` remains `iframe` until overlay/chrome composition, focus/input routing, DPI/bounds behavior, resize/minimize/restore, crash/reconnect cleanup and navigation policy are verified on Windows WebView2, macOS WKWebView and Linux WebKitGTK.

## Stable visual capture transaction

The current visual-capture slice wires a fail-closed private-region transaction around native pixel acquisition:

1. Validate the viewport and optional bounded CSS region, then preflight the exact session-owned managed surface.
2. Acquire a bounded **per-session** capture gate so independent sessions do not serialize globally.
3. Wait for the authenticated stable-capture settle gate.
4. Ask the managed page to freeze visual motion through a private `freeze_visuals` bridge action carrying only bounded private selectors.
5. Resolve those selectors inside the managed page to bounded viewport-relative rectangles and counts. Selector strings, DOM text, attributes and arbitrary page payload do not cross back in the freeze acknowledgement.
6. Acquire native **viewport** pixels through the platform adapter. Region execution intentionally reuses this auditable platform path instead of creating three independent region backends.
7. Restore visual state with the exact freeze token, even when native acquisition fails.
8. For a region target, verify that the captured frame's CSS viewport still matches the live CSS viewport reported by the freeze receipt. Resize/drift fails closed after restoration and before any artifact write.
9. Revalidate the bounded mask geometry and redact the captured viewport PNG **in memory** using `localview-visual`.
10. For a region target, crop the already-redacted viewport PNG using bounded CSS-to-native scaling, then decode the result again to verify the produced pixel dimensions.
11. Persist the processed frame and register either viewport Visual evidence or dedicated `/evidence/visual-region` metadata only after restore acknowledgement, target validation and complete private-mask application succeed.

The page-side freeze pauses bounded Web Animations when available and applies temporary CSS suppression for animation/transition motion. Every freeze has an 8-second fail-safe lease that attempts automatic restoration if the coordinator disappears. Private geometry resolution is bounded to 16 selectors, 4,096 unique matched elements and 256 visible rectangles inside a maximum 100,000 × 100,000 CSS-pixel viewport. Invalid selectors, malformed/non-finite geometry, budget overflow, malformed PNG data, frame-dimension mismatch, live viewport drift, restore failure, incomplete mask application or invalid region geometry fail closed and prevent uncertain pixels from being persisted.

The final redaction/crop boundary is native PNG processing in Rust, not a CSS overlay. `localview-visual` decodes bounded PNG input, verifies decoded dimensions against native capture metadata, validates geometry before mutation, scales CSS coordinates to native pixels, clips within bounded image memory, fills private pixels opaquely, crops requested regions only after redaction and re-encodes within bounded memory/encoded-size limits.

Internal `freeze_visuals` / `restore_visuals` actions are not exposed through the generic public action-enqueue route. Private selectors travel only in the internal capture envelope. Freeze tokens remain control-flow state and are not emitted in final capture receipts or Visual evidence metadata. Region evidence uses a separate authenticated endpoint whose schema requires `target = "region"`, a positive in-viewport CSS rectangle, nonzero pixel dimensions, a content-addressed artifact ID, an approved native backend and a loopback HTTP(S) route.

## Verification state

The settle/freeze/private-geometry/restore/redaction/region-crop transaction is covered by cross-platform Rust compile/test gates and the Linux Tauri/frontend contract suite. The contracts verify private/public action isolation, bounded selector transport, geometry-only freeze acknowledgements, restore-after-native-failure, restore-before-live-target validation, validation-before-redaction, redaction-before-crop, crop-before-persistence, bounded PNG processing, region-evidence schema isolation and fail-closed malformed geometry/dimension/target handling.

Linux/WebKitGTK has hosted **rendered-pixel proof**: a dedicated Ubuntu GUI smoke job starts a real GTK window and WebKitGTK WebView under Xvfb, loads deterministic localhost HTML, invokes the production-shared visible-snapshot helper, fully decodes the returned PNG and verifies that the center pixel belongs to the known rendered proof region. A missing display, load failure, snapshot failure, invalid/trivial PNG or wrong rendered pixel fails the job.

macOS/WKWebView has equivalent hosted **rendered-pixel proof**. A custom `harness = false` test owns the actual macOS main thread, initializes AppKit, creates a real NSWindow + WKWebView, loads the same deterministic loopback fixture, pumps NSRunLoop until the page is ready, invokes the same WKWebView snapshot helper used by the Tauri adapter, fully decodes the produced PNG and verifies the known center proof pixel. The real GUI test exposed and fixed a production bug where WKWebView's returned NSImage representations were not directly PNG-encodable by ImageIO; production now materializes the snapshot through `NSImage.TIFFRepresentation → NSBitmapImageRep → PNG` before frame validation.

Windows/WebView2 has the matching hosted **rendered-pixel proof**. A custom Windows GUI smoke creates a real Win32 parent window and installed WebView2 controller on an STA COM thread, serves a deterministic fixture from `127.0.0.1`, correlates the exact fixture URI and NavigationId, requires successful navigation, verifies `document.readyState`, route, computed proof color, viewport and proof geometry, then invokes the production-shared `ICoreWebView2::CapturePreview` path. The returned PNG is bounded, fully decoded and required to contain the known red proof region at its center. The harness now also tolerates bounded zero-byte speculative/preconnect TCP connections from WebView2 while retaining a hard deadline, per-connection read bounds and full request traces; the rendered-pixel/navigation assertions themselves are unchanged.

The viewport **Native screenshot adapter is Implemented**: all three production platform backends have live rendered-pixel acquisition evidence in dedicated hosted GUI gates, in addition to compile/contracts coverage. **Bounded CSS-region execution is also live** above that acquisition path: real native viewport pixels are restored, private-redacted, cropped, dimension-verified, persisted and registered as region Visual evidence. This does **not** make progressive capture complete: automatic element/component/section ownership resolution, changed-region scheduling and the complete capture → diff → verification loop remain Partial capabilities.

## Next vertical slices

The strongest remaining capture/perception gates are:

1. changed-region scheduling with a bounded per-session real-pixel baseline, private-redacted diff input and actual region execution;
2. automatic element/component/section target resolution wired from semantic/runtime ownership into the bounded region path;
3. true network in-flight accounting plus framework-specific live HMR signals;
4. capture → visual region diff → deterministic verification as one end-to-end loop;
5. deeper source/runtime ownership correlation and interactive point-and-select inspector wiring;
6. native workspace composition/focus/DPI/crash safety gates before promoting the child WebView surface from feature-gated to default.

The repository should not claim the complete V1/V2/V3 specification is implemented until these live adapters and later verification phases are end-to-end, not merely represented by crates or data models.
