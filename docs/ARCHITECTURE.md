# LocalView Architecture

## Product boundary

LocalView is a localhost runtime, not a general-purpose browser. Every architecture choice is evaluated against four principles: zero ceremony, no developer annoyance, evidence instead of telemetry dumps, and heavy browser machinery only when necessary.

## Runtime domains

### Discovery domain

`localview-discovery` observes local TCP listeners through platform commands, normalizes endpoints, probes them concurrently, and classifies only reachable HTTP services. Classification separates frontend dev servers, Storybook, static pages, APIs and unknown HTTP endpoints. A candidate carries process metadata when the operating system exposes it.

### Identity and lifecycle domain

`localview-core` derives a project identity independently of port number. `localview-sessions` reconciles discovered servers into stable sessions, permits port changes during Vite/Next restarts, marks missing endpoints disconnected and removes them after a grace period. Preview visibility is orthogonal to the daemon lifecycle.

### Observation domain

`localview-observation` is the normalized runtime event backbone. Subsystems publish semantic events such as server detection/disconnect, HMR, route, DOM, layout, network and console events. Clients consume the same source of truth rather than maintaining parallel state models.

### Perception domain

Perception is deliberately fused rather than vision-only.

- `semantic`: stable element references and snapshot/state diff.
- `layout`: deterministic geometry constraints.
- `visual`: pixel and changed-region localization.
- `network`: request-level failure/performance patterns.
- `console`: grouped runtime diagnostics.
- `a11y`: deterministic and heuristic accessibility checks.
- `performance`: lightweight health rather than full DevTools replacement.
- `design-grammar`: inferred local design scales and drift signals.

### Capture domain

`localview-capture` represents stable capture as a transaction: wait, stabilize, optionally quiet network, freeze animation, mask sensitive/dynamic regions, capture, restore. Progressive disclosure starts at changed element context and expands to component, section, viewport or full page only when required.

### Engine domain

`localview-engine` encodes escalation as an explicit policy:

1. Tier 0 — source/static inspection.
2. Tier 1 — lightweight machine execution.
3. Tier 2 — native Tauri/WRY WebView for human rendering and platform-native capture.
4. Tier 3 — Chromium/Playwright only for browser-specific compatibility, advanced emulation or DevTools tracing.

This policy is central to LocalView's memory/RAM proposition.

### Agent domain

The Axum control plane is loopback-only and authenticated. CLI, MCP and the Tauri dashboard consume that protocol. Future SDKs should be thin clients; business logic belongs in the daemon/crates, never duplicated per interface.

## Desktop architecture

The desktop shell uses Tauri 2.11 and a React/Vite frontend. The main dashboard receives the `core:default` Tauri capability. Localhost preview windows are created from Rust only after a loopback URL check and intentionally do not receive the dashboard command capability. The main window closes to tray; the runtime survives as a separate daemon process.

## Data reduction strategy

Agent payloads are diff-first. Unchanged sections are references, not serialized again. `localview-token-budget` provides a budget-aware shaping primitive while semantic and visual subsystems localize changes before the model sees image bytes.

## Failure philosophy

Subsystem failure must degrade locally. A visual analyzer failure cannot stop preview. MCP failure cannot stop human use. WebView failure may escalate to Tier 3 only when policy permits. Discovery/session cleanup must remain independent of intelligence layers.
