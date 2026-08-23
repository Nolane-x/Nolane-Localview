# Implementation Status

LocalView is being implemented as a sequence of independently verifiable vertical slices against the expanded product specification. This branch is the native-workspace integration slice stacked on the Rust/Tauri verification branch.

## Landed architecture

- Rust workspace with protocol, discovery, sessions, observation, security and authenticated loopback control plane.
- Semantic/state diff, layout, visual, responsive, source and token-budget layers.
- Capture planning, engine escalation, network, console, accessibility, performance, flow, design grammar and artifacts.
- CLI and MCP bridge.
- Tauri 2 desktop dashboard, system tray and standalone localhost preview windows.
- In-page instrumentation and a bounded native bridge for observer events and queued page actions.
- Cross-platform CI for the Rust core plus a Linux Tauri/frontend compiler gate.

## Native workspace surface wave

Implemented in this branch:

- Tauri capability selectors are scoped by **WebView label** instead of parent window, preventing a child WebView from inheriting the bundled dashboard capability.
- `main` receives only the dashboard capability; `preview-*` and `workspace-*` receive only the loopback `previewbridge` capability.
- Regression tests enforce selector and permission isolation and keep remote capability URLs loopback-only.
- Cargo feature `native-workspace` is the only path that enables `tauri/unstable`.
- Feature-gated child WebView backend with bounded open, resize/reposition, loopback navigation and close commands.
- Workspace WebViews reuse the existing instrumentation/native bridge while bridge caller validation binds the WebView label to the exact session id.
- `dashboard_state` publishes workspace-surface support/policy to the React shell.
- React now renders through a `WorkspaceSurface` abstraction with lifecycle and bounds synchronization plus deterministic iframe fallback.
- CI checks both the normal Tauri backend and `--features native-workspace` so unstable APIs cannot leak accidentally into the default build.

## Deliberate safety gate

Compiling native child WebViews does **not** make them the default workspace yet. `WorkspaceSurfaceSupport.default_mode` remains `iframe` until LocalView has verified overlay/chrome composition, focus and input routing, DPI/bounds behavior, window resize/minimize/restore, crash/reconnect cleanup and navigation policy on Windows WebView2, macOS WKWebView and Linux WebKitGTK.

This is intentional: the product specification prioritizes native rendering and low overhead, but capability isolation and deterministic lifecycle are correctness boundaries rather than cosmetic implementation details.

## Next vertical slice

After this compiler/capability gate is green, Wave 1 should continue with:

1. deeper semantic DOM/AX packet extraction beyond interactive-only snapshots;
2. bounded computed-style and geometry packets;
3. geometry/state delta subscription over the existing native bridge;
4. source/runtime correlation hooks;
5. native screenshot/region capture adapters and stable-capture transactions.

The repository should not claim the complete V1/V2/V3 specification is implemented until these live adapters and the later verification phases are end-to-end, not merely represented by crates or data models.
