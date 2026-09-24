<div align="center">

# LocalView

### A localhost visual runtime for developers and coding agents

**See what your app is doing. Act on exact evidence. Verify what changed.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

[![CI](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml)
[![Production closure](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml)
![Version](https://img.shields.io/badge/version-0.2.0-5b8def)
![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## What is LocalView?

LocalView is a **Rust-first, AI-native localhost visual runtime**.

It discovers local web applications automatically, keeps stable project/session identity across dev-server restarts, observes what the user and runtime can actually see, correlates that evidence with source/runtime state, and exposes the result to humans, CLI clients and AI coding agents through a tightly bounded local control plane.

LocalView is **not another general-purpose browser** and it is not a screenshot wrapper. Its job is narrower:

> turn a running localhost application into a compact, verifiable, agent-friendly source of visual and runtime truth.

The core runtime stays local. The control plane is loopback-only and authenticated. Heavy Chromium automation is an escalation tier, not the default.

---

## Why LocalView is different

| | LocalView focuses on |
| --- | --- |
| **Automatic localhost awareness** | Discover reachable local frontends, classify them and preserve project identity even when dev-server ports change. |
| **Evidence fusion** | Combine semantic structure, geometry, rendered pixels, console, network, accessibility, performance and source hints instead of trusting a single signal. |
| **Agent-native state** | Stable element references, bounded snapshots, diffs, compact packets, CLI and MCP access instead of dumping full DOM/browser telemetry into an LLM. |
| **Safe consequential actions** | Mutations use an explicit **plan → confirm → status** authority chain, exact surface/provider lineage, durable dispatch accounting and fresh postcondition verification. |
| **Verification after change** | Trusted Fix/Verify and Wave 9 can prove a bounded result for the exact selected target on the current route without pretending that a local proof covers the whole application. |
| **Lightweight by default** | Native WebViews and deterministic analysis are preferred. Full Chromium/Tier 3 is used only when browser-specific evidence is actually required. |
| **Production discipline** | Cross-platform CI, native rendered-pixel smoke, clean-machine install checks, rollback policy, provenance, SBOM and machine-enforced production truth. |

---

## The LocalView loop

```text
local app starts
      │
      ▼
┌───────────────┐
│   discovery   │  localhost listeners → HTTP classification
└──────┬────────┘
       ▼
┌───────────────┐
│ session model │  stable project identity, restart/reconnect handling
└──────┬────────┘
       ▼
┌─────────────────────────────────────────────────────────┐
│                    evidence fusion                      │
│ semantic · layout · pixels · network · console · a11y  │
│ performance · source correlation · runtime ownership    │
└───────────┬───────────────────────────────┬─────────────┘
            │                               │
            ▼                               ▼
     human dashboard                  CLI / MCP / agents
            │                               │
            └──────────┬────────────────────┘
                       ▼
              plan → confirm → act
                       │
                       ▼
                 fresh verification
                       │
                       ▼
              proof / report / status
```

The important part is the final step: **an action is not considered proven merely because it was dispatched**. LocalView records dispatch authority, obtains fresh post-action evidence and evaluates bounded postconditions.

---

## What LocalView can observe

LocalView's perception model is deliberately multi-signal:

- **Semantic** — stable element references, interactive state, accessible identity and semantic snapshots.
- **Layout** — geometry, overflow, overlap, alignment and responsive state.
- **Visual** — real rendered pixels, changed regions and visual-diff evidence.
- **Network** — failed, slow, duplicate, large and CORS-related request signals.
- **Console** — deterministic grouping and bounded runtime diagnostics.
- **Accessibility** — naming, alternatives, target/effective interaction checks and dedicated Wave 6 intelligence.
- **Performance** — lightweight long-task, instability, HMR and runtime health signals.
- **Source correlation** — project-contained source maps, component/source ownership and runtime/source hints.
- **Responsive behavior** — viewport matrices, adaptive sweep and breakpoint-oriented analysis.

The goal is not maximum telemetry. The goal is **the smallest evidence set that can support a useful conclusion**.

---

## Agent actions without blind automation

LocalView exposes consequential managed-WebView actions for:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

But they are intentionally not exposed as one-shot "just do it" commands.

The production authority chain is:

1. obtain fresh semantic evidence;
2. plan against an exact stable reference;
3. mint a short-lived, one-shot confirmation capability;
4. explicitly confirm;
5. verify provider/target incarnation and payload commitment;
6. dispatch through the exact managed surface;
7. durably linearize executor completion;
8. capture a new post-action observation;
9. evaluate postconditions;
10. expose a bounded proof/status receipt.

Payload-bearing actions keep plaintext process-local and bind durable metadata through keyed commitments. Restart does not restore confirmation, payload or dispatch authority.

CLI and MCP expose this as **`plan → confirm → status`**. Legacy one-step mutation shortcuts remain closed.

---

## Trusted Fix and bounded verification

LocalView separates **what was changed** from **what has actually been proven**.

The Trusted Fix/Verify path carries exact candidate and revision identity through restart-safe recovery, evaluates fresh post-Apply evidence, runs a fixed hard-contract catalog and safe synthetic mutation challenges, and produces a scope-explicit verification receipt.

For V1, the production-supported proof scope is:

`current_target_current_route`

That bounded receipt can be `verified`, `rejected` or `inconclusive`.

A separate **whole-impact autonomous verdict** exists, but LocalView deliberately keeps it fail-closed unless a future completeness-certified dependency denominator and complete revalidation universe exist.

In other words:

> LocalView will say "this exact target on this route is verified" when it can prove that — and will not silently upgrade that statement into "the entire application is verified."

---

## Security model

LocalView treats local automation as a security boundary, not as a convenience API.

- Control API binds to loopback.
- Agent/runtime access requires a generated bearer token.
- Dashboard and remote preview capabilities are separated.
- Preview surfaces do not receive the dashboard's command authority.
- Managed actions are fenced to exact session/surface/provider/target lineage.
- Payload-bearing actions use process-local plaintext plus durable HMAC commitments.
- Durable consequential journaling distinguishes admitted, prepared, dispatched/ambiguous and postcondition states.
- Fresh evidence is required for verification.
- Update checking rejects redirects and cross-origin artifacts.
- Update checking **never grants install authority** in V1.
- Automatic signed update installation remains disabled until production update-signature authority exists.

See [docs/SECURITY.md](docs/SECURITY.md) for the current model.

---

## Cross-platform runtime

LocalView targets:

| Platform | Native view / capture path |
| --- | --- |
| Windows | WebView2 + Windows UI Automation / native provider paths |
| macOS | WKWebView + macOS accessibility/native capture paths |
| Linux | WebKitGTK + AT-SPI/native capture paths |

Hosted CI exercises real rendered-pixel smoke on all three platforms. Release-candidate workflows also build platform bundles and prove clean-machine install + first launch for the unsigned candidate.

---

## Production status

The bounded V1 **software** claim is complete on `main`.

| Area | Status |
| --- | --- |
| Daemon + desktop sidecar packaging | ✅ Closed |
| Managed WebView authority | ✅ Closed |
| Consequential click/focus/type/key/scroll | ✅ Closed |
| CLI/MCP two-phase action interface | ✅ Closed |
| Restart-safe Trusted Fix/Verify recovery | ✅ Closed |
| Bounded Wave 9 post-Apply verification | ✅ Closed |
| Native rendered capture on Windows/macOS/Linux | ✅ Closed |
| Headless/reporting/attestation | ✅ Closed |
| Clean-machine install + first launch | ✅ Closed |
| Initial-release rollback policy | ✅ Closed |
| Provenance + SPDX SBOM | ✅ Closed |
| User-triggered fail-closed update check | ✅ Closed |
| Whole-app Autonomous Verified | 🧭 Post-V1 breadth |
| Native child-WebView as default workspace | 🧭 Post-V1 breadth |
| Windows code signing | ⛔ External credential |
| macOS Developer ID + notarization | ⛔ External credential |
| Signed automatic updater install | ⛔ External credential |
| W10 physical mixed-DPI topology proof | ⛔ External hardware |

**Important:** unsigned CI bundles are release candidates, not signed public production installers.

The authoritative boundary is [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md).

---

## Quick start

### Requirements

- Rust **1.85+**
- Node.js **24+**
- platform dependencies required by Tauri 2 / the native WebView stack

### Core runtime

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

In another terminal:

```bash
cargo run -p localview -- sessions
```

### Desktop

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

Production-style local candidate:

```bash
cd apps/desktop
npm ci
npm run tauri build
```

The desktop build prepares the matching `localview-daemon` sidecar automatically.

### MCP bridge

```bash
cargo run -p localview-mcp
```

The MCP bridge is a stdio JSON-RPC surface over the authenticated LocalView runtime. It is designed to give coding agents compact, bounded access to observations, verification and the two-phase consequential-action interface.

---

## Architecture

```text
┌────────────────────────── Localhost ──────────────────────────┐
│                                                              │
│  discovery → sessions → observation bus                      │
│                         │                                    │
│          ┌──────────────┼────────────────┐                   │
│          ▼              ▼                ▼                   │
│      semantic        visual/layout    runtime signals        │
│          │              │          network/console/a11y      │
│          └──────────────┼────────────────┘                   │
│                         ▼                                    │
│                 evidence + state                             │
│                         │                                    │
│            ┌────────────┼─────────────┐                      │
│            ▼            ▼             ▼                      │
│         desktop        CLI           MCP                      │
│            │            │             │                      │
│            └────────────┴──────┬──────┘                      │
│                                ▼                             │
│                     bounded action/verify                    │
└──────────────────────────────────────────────────────────────┘
```

The engine escalation policy is explicit:

1. **Tier 0** — source/static inspection.
2. **Tier 1** — lightweight machine execution.
3. **Tier 2** — native Tauri/WRY WebView and native capture.
4. **Tier 3** — Chromium only for browser-specific compatibility, emulation or deep tracing.

That policy is central to LocalView's "strong evidence without permanent heavy-browser cost" design.

---

## Repository map

```text
apps/
  daemon/        background discovery + control runtime
  cli/           localview command-line interface
  desktop/       React/Vite + Tauri desktop application

crates/
  protocol/      shared domain/wire contracts
  discovery/     localhost discovery + classification
  sessions/      stable lifecycle and identity
  observation/   normalized event backbone
  control/       authenticated loopback API
  semantic/      semantic snapshots and state diff
  native-capture/
  native-provider/
  live-bridge/   page/native action bridge
  verification/  bounded verification receipts
  planner/       bounded planning authority
  ...            visual, layout, network, a11y, source, reports, etc.

integrations/
  mcp/           stdio MCP bridge

docs/
  ARCHITECTURE.md
  SECURITY.md
  IMPLEMENTATION_STATUS.md
  PRODUCTION_CLOSURE_MATRIX.md
  PRODUCTION_RELEASE.md
  SPEC_COVERAGE.md
```

Historical plans, evidence and superseded design material are archived separately in `Nolane-x/Localview-document`. Active specs referenced by tests/workflows remain with the product.

---

## Documentation

| Document | Purpose |
| --- | --- |
| [Architecture](docs/ARCHITECTURE.md) | Product/runtime architecture |
| [Security](docs/SECURITY.md) | Authority and trust boundaries |
| [Implementation status](docs/IMPLEMENTATION_STATUS.md) | Landed capability truth |
| [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md) | V1 release boundary |
| [Production release](docs/PRODUCTION_RELEASE.md) | Packaging/signing/publication gates |
| [Spec coverage](docs/SPEC_COVERAGE.md) | Broader capability coverage and deferred breadth |
| [Roadmap](docs/ROADMAP.md) | Current and post-V1 direction |

---

## Release boundary

LocalView 0.2.0 has a complete bounded V1 software closure, but a **signed public production release** still requires external credentials:

- Windows code-signing identity;
- macOS Developer ID + notarization credentials;
- production updater-signing authority.

The W10 same-HWND cross-monitor mixed-DPI proof additionally requires real Windows multi-monitor hardware if that topology is claimed as supported.

Until those external requirements exist, published binaries should be labeled **unsigned release candidates / pre-releases**.

---

## Contributing

LocalView prefers evidence-driven contributions:

1. define the exact authority or capability being changed;
2. preserve fail-closed behavior at trust boundaries;
3. add or update deterministic contracts;
4. run the relevant cross-platform/exact-head gates;
5. update production truth only after evidence lands.

Current normative specs stay in the product repository. Historical reasoning and completed plans belong in the documentation archive.

---

## License

LocalView is dual-licensed under **MIT OR Apache-2.0**, at your option.

See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
