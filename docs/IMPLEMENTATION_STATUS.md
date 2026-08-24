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

## Native workspace safety gate

Tauri capability selectors are scoped by **WebView label** instead of parent window. `main` receives only the dashboard capability; `preview-*` and `workspace-*` receive only the loopback preview bridge capability. The `native-workspace` Cargo feature is the only path that enables `tauri/unstable`, and CI checks both default and native-workspace builds.

Compiling native child WebViews still does **not** make them the default workspace. `WorkspaceSurfaceSupport.default_mode` remains `iframe` until overlay/chrome composition, focus/input routing, DPI/bounds behavior, resize/minimize/restore, crash/reconnect cleanup and navigation policy are verified on Windows WebView2, macOS WKWebView and Linux WebKitGTK.

## Stable visual capture transaction

The current visual-capture slice now wires the managed WebView state transition around native pixel acquisition:

1. Validate viewport and preflight exact session-owned managed surface.
2. Acquire a bounded **per-session** capture gate so independent sessions do not serialize globally.
3. Wait for the authenticated stable-capture settle gate.
4. Ask the managed page to freeze visual motion through a private `freeze_visuals` bridge action.
5. Acquire native viewport pixels through the platform adapter.
6. Restore visual state with the exact freeze token, even when native acquisition fails.
7. Persist the frame and register Visual evidence only after restore acknowledgement succeeds.

The page-side freeze pauses bounded Web Animations when available and applies temporary CSS suppression for animation/transition motion. Every freeze has an 8-second fail-safe lease that attempts automatic restoration if the coordinator disappears. The desktop rejects malformed or over-budget freeze receipts, and restore acknowledgement failure discards captured pixels rather than persisting evidence from an uncertain page state.

Internal `freeze_visuals` / `restore_visuals` actions are not exposed through the generic public action-enqueue route. Freeze tokens remain control-flow state and are not emitted in final capture receipts or Visual evidence metadata.

## Verification state

The freeze/restore transaction has cross-platform Rust compile/test coverage and a Linux Tauri/frontend contract gate covering bridge wiring, stable-settle ordering, session-scoped serialization, restore-after-native-failure and restore-before-persist behavior.

The native screenshot adapter remains **Partial**, not Implemented, because hosted CI currently proves compilation and contracts rather than real rendered-pixel acquisition in a GUI session on all three native WebView backends. Cross-platform GUI smoke remains the completion gate for that capability.

## Next vertical slices

The strongest remaining capture/perception gates are:

1. hosted or dedicated GUI smoke for real WebView2/WKWebView/WebKitGTK rendered-pixel capture;
2. element/component/section native pixel execution and changed-region scheduling;
3. private-selector masking/redaction in the capture transaction;
4. true network in-flight accounting and framework-specific live HMR signals;
5. capture → visual region diff → deterministic verification as one end-to-end loop;
6. deeper source/runtime ownership correlation and interactive point-and-select inspector wiring.

The repository should not claim the complete V1/V2/V3 specification is implemented until these live adapters and later verification phases are end-to-end, not merely represented by crates or data models.
