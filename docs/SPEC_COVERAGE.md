# Product Specification Coverage Ledger

This ledger keeps implementation claims tied to the LocalView AI-Native Localhost Runtime Product Spec. `Implemented` means code exists in the repository. `Partial` means the subsystem exists but lacks one or more live integrations. `Planned` means it is still specification-only.

| Spec capability | Status | Repository surface / next gate |
| --- | --- | --- |
| Native Rust core | Implemented | `crates/core`, daemon, CLI |
| Listener discovery | Implemented | `crates/discovery` |
| Frontend HTTP classification | Implemented | Vite/Next/Nuxt/Svelte/Astro/Angular/Webpack/Storybook evidence |
| Port-independent project identity | Implemented | `localview-core` + sessions |
| Session lifecycle / reconnect grace | Implemented | `crates/sessions` |
| Tray + close-to-tray desktop shell | Implemented | Tauri desktop |
| Lightweight native preview | Partial | Native WebView window implemented; flagship shell now also presents inline workspace when framing is permitted |
| Human View zero-clutter workspace | Implemented | Full-canvas workspace + floating chrome |
| X-Ray / Inspector UI | Partial | Floating surface and semantic/layout/source subsystems exist; live element-selection transport still to connect |
| Stable capture policy | Implemented | `crates/capture` |
| Progressive capture regions | Implemented | element → component → section → viewport planning |
| Pixel visual diff | Implemented | `crates/visual` |
| Semantic snapshot model | Implemented | protocol + semantic crate |
| Stable element refs | Implemented | semantic + injected observer |
| State diff | Implemented | semantic/state delta models |
| Layout intelligence | Implemented | overflow, zero-area, overlap, drift, spacing inference |
| Responsive intelligence | Implemented | presets + adaptive/binary breakpoint sweep |
| Source mapping hints | Implemented | stack/source ranking |
| Framework awareness | Implemented | framework adapter crate |
| Console analysis | Partial | analyzer + injected observer capture; secure native drain pending |
| Network analysis | Partial | analyzer implemented; live request bridge pending |
| Accessibility analysis | Implemented | accessible-name/image/hit-target checks |
| Performance analysis | Partial | analyzer + observer long-task/layout-shift capture; native drain pending |
| Interaction flows | Implemented | flow graph + shortest path primitives |
| Design grammar | Implemented | spacing/radius/type/control scale inference |
| Observation bus | Implemented | bounded broadcast/history |
| Diagnostics fusion | Implemented | deterministic/heuristic/subjective issue assembly |
| Reports | Implemented | JSON/Markdown/HTML renderers |
| Artifact storage | Implemented | bounded deduplicating local store |
| Engine tier escalation | Implemented | static/lightweight/native WebView/Chromium policy |
| Token budgeting | Implemented | compact/deep/minimal serialization budget |
| Local permission/security model | Implemented | bearer token, localhost control plane, redaction policy |
| MCP control plane | Partial | stdio bridge and core tools implemented; protocol/version compatibility must continue to be verified |
| AI Critic / Point-and-Ask | Partial | UI and evidence architecture defined; model-provider execution intentionally not hard-wired |
| Secure observer drain | Planned | next highest-priority integration |
| Native screenshot adapter | Planned | capture planner exists; OS/WebView execution adapter still required |
| Direct click/type agent actions | Planned | protocol surface and flow primitives exist; secure WebView action bridge required |
| Full replay / rich state timeline UI | Planned | observation primitives exist; desktop timeline surface still to implement |
| Full DevTools replacement | Explicitly out of scope for v1 | Product spec says not to build it |
| Cloud accounts / collaborative remote browser | Explicitly out of scope for v1 | Local-first runtime by design |

## Completion rule

This file must be updated whenever a capability crosses `Planned → Partial → Implemented`. A feature is not called complete merely because a panel exists; the backing runtime path and verification evidence must exist too.
