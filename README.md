# Nolane LocalView

> **Run your app. LocalView is already there.**

LocalView is a Rust-first, AI-native localhost visual runtime for developers and coding agents. It automatically discovers local web apps, classifies them, keeps a lightweight project/session model, provides a native Tauri/WebView preview, and exposes compact machine-facing state through a local control plane, CLI and MCP bridge.

This repository is not trying to become another browser. The product is intentionally localhost-focused: lifecycle, visual evidence, semantic structure, runtime telemetry, deterministic checks and source-oriented diagnostics.

## What exists now

The repository has moved beyond the original bootstrap and is organized as a multi-crate Rust workspace plus a Tauri 2 desktop application.

| Layer | Current implementation |
|---|---|
| Discovery | loopback listener discovery, bounded concurrent HTTP probing, frontend/API classification, framework/HMR evidence |
| Session lifecycle | project identity, port-change reconnect, disconnect grace period, cleanup |
| Observation | normalized broadcast bus + bounded recent-event history |
| Protocol | sessions, semantic nodes, geometry, state diffs, console/network issues, capabilities, budgets |
| Security | localhost-only control, bearer token, secret redaction, capability policy primitives |
| Control | Axum control API bound to loopback, pause/resume, sessions, events |
| Agent access | compact CLI and stdio MCP JSON-RPC bridge |
| Semantic | stable element refs, snapshot flattening, semantic + geometry state diff |
| Layout | overflow, overlap and alignment anomaly primitives |
| Visual | RGBA pixel diff and changed-tile localization |
| Responsive | standard viewport matrix, adaptive sweep, binary breakpoint discovery |
| Source | stack/source hint parsing and ranking |
| Capture | stable-capture orchestration and progressive visual disclosure planning |
| Engine | Tier 0–3 escalation decision model; full Chromium is not the default |
| Network | failed/slow/duplicate/large/CORS analysis |
| Console | deterministic issue grouping/deduplication |
| Accessibility | accessible-name, image alternative and effective target checks |
| Performance | long-task, layout-instability and HMR health primitives |
| Flow | interaction graph and deterministic path/replay representation |
| Design grammar | spacing/radius/type/control-scale inference and drift detection |
| Artifact store | bounded local artifact storage with content deduplication |
| Desktop | Tauri 2.11 + React 19 + Vite 8 dashboard, native system tray, close-to-tray, localhost preview windows |

## Architecture

```text
                         operating system
                               │
                   ┌───────────▼───────────┐
                   │ localhost discovery   │
                   └───────────┬───────────┘
                               │ candidates
                   ┌───────────▼───────────┐
                   │ HTTP classifier       │
                   └───────────┬───────────┘
                               │ discovered servers
                   ┌───────────▼───────────┐
                   │ session manager       │
                   └──────┬────────┬───────┘
                          │        │
               ┌──────────▼──┐  ┌──▼────────────────┐
               │ Tauri/Wry   │  │ machine runtime  │
               │ native view │  │ semantic/control │
               └──────────┬──┘  └──┬────────────────┘
                          │        │
                   ┌──────▼────────▼──────┐
                   │ observation bus      │
                   └──┬───┬───┬───┬──────┘
                      │   │   │   │
                semantic layout network visual ...
                      │   │   │   │
                   ┌──▼───▼───▼───▼──────┐
                   │ state / evidence     │
                   │ token budget layer   │
                   └─────────┬────────────┘
                             │
               ┌─────────────▼─────────────┐
               │ CLI · MCP · desktop · SDK │
               └───────────────────────────┘
```

The native desktop stack targets Tauri 2.11. On Windows that means WebView2; on macOS WKWebView; on Linux WebKitGTK through WRY. Chromium/Playwright belongs to Tier 3 and should be spawned only for browser-specific validation, exact compatibility or deep DevTools tracing.

## Repository map

```text
apps/
  cli/              # localview CLI
  daemon/           # background discovery + control runtime
  desktop/          # React/Vite UI + Tauri shell
crates/
  protocol/         # shared wire/domain schema
  core/             # runtime policy + project identity
  discovery/        # ports + HTTP classification
  sessions/         # localhost lifecycle
  observation/      # normalized event bus
  security/         # permissions + redaction
  control/          # localhost control API
  semantic/         # semantic tree + state diff
  layout/           # deterministic geometry analysis
  visual/           # visual diff primitives
  responsive/       # breakpoint/search matrix
  source-map/       # source resolution hints
  token-budget/     # compact agent packets
  framework-adapters/
  capture/
  engine/
  network/
  console/
  a11y/
  performance/
  flow/
  design-grammar/
  artifacts/
integrations/
  mcp/              # stdio MCP-compatible JSON-RPC bridge
```

## Development

Prerequisites: current stable Rust, Node.js 24+, and the platform prerequisites required by Tauri 2.

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
cargo run -p localview -- sessions
```

Desktop:

```bash
cd apps/desktop
npm install
npm run tauri dev
```

## Security model

LocalView's control API binds to `127.0.0.1`, requires a generated bearer token for agent/runtime data, and remote preview navigation is rejected by the Tauri command. Preview windows are intentionally excluded from the main Tauri command capability. Secrets are redacted before agent-facing surfaces whenever possible.

## Direction

The next major implementation blocks are live WebView instrumentation, DOM/AX extraction, real screenshot capture adapters, console/network hooks, source-map adapters for React/Vue/Svelte, state packet subscriptions, deterministic interaction execution, Storybook mode and CI/headless validation.

See `docs/ARCHITECTURE.md` and `docs/ROADMAP.md` for the expanded architecture and delivery sequence.

## License

MIT OR Apache-2.0.
