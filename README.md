<div align="center">

# LocalView

### A local-first visual runtime for humans, coding agents, and localhost applications.

**Observe the running app. Understand what actually happened. Act through bounded authority. Verify the result with evidence.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

[![CI](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml)
[![Release candidate](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/release-candidate.yml/badge.svg?branch=main)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/release-candidate.yml)
![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![React](https://img.shields.io/badge/React-19-61DAFB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## What is LocalView?

LocalView is an **AI-native localhost visual runtime** built for developers and coding agents that need to understand a running application, not merely read its source code.

It discovers local web applications, opens a controlled preview, observes semantic structure and rendered behavior, correlates runtime evidence with source ownership, exposes compact machine-facing state through a local daemon/CLI/MCP bridge, and provides bounded action + verification authority when an agent needs to interact with the app.

LocalView is deliberately **not another general-purpose browser**. Its job is narrower and more useful for software work:

> turn a running localhost application into a trustworthy, inspectable, evidence-producing environment for humans and agents.

That means LocalView cares about questions such as:

- *What is actually rendered right now?*
- *Which semantic element and source-owned region does this correspond to?*
- *Did the UI change after a fix?*
- *Did a new console/network/layout/accessibility regression appear?*
- *Can an agent click, focus, type, press a key, or scroll without receiving unlimited page authority?*
- *Can the result be verified from a fresh post-action observation instead of trusting the command that caused it?*
- *Can all of this fail closed when evidence is stale, incomplete, ambiguous, or outside the supported scope?*

---

## Why LocalView is different

| Principle | What LocalView does |
| --- | --- |
| **Runtime truth first** | Observes the live application instead of assuming source code describes the current rendered state. |
| **Local-first** | The control plane binds to loopback and focuses on localhost development workflows. |
| **Evidence before confidence** | Semantic snapshots, native visual evidence, runtime events, contracts, receipts, hashes and provenance are first-class data. |
| **Bounded agent authority** | Consequential interaction uses plan → explicit confirm → exact dispatch → fresh verification. No legacy one-step mutation shortcut is exposed. |
| **Fail-closed verification** | Stale lineage, unknown contracts, incomplete evidence, unexpected impact and unsupported authority do not silently become success. |
| **Cross-platform native proof** | Windows/WebView2, macOS/WKWebView and Linux/WebKitGTK have hosted rendered-pixel evidence paths. |
| **Resource-aware** | Heavy browser escalation is a later tier, not the default. Native/local paths are preferred when they can answer the question. |
| **Agent-friendly, not agent-only** | Desktop UI, CLI and MCP expose the same bounded runtime model to humans and coding agents. |

---

## The LocalView loop

```text
     localhost application
              │
              ▼
      discovery + session
              │
              ▼
  semantic / visual / runtime observation
              │
              ▼
    evidence + source correlation
              │
        ┌─────┴─────┐
        │           │
        ▼           ▼
      human        agent
        │           │
        └─────┬─────┘
              ▼
       bounded action plan
              │
        explicit confirmation
              │
              ▼
        exact dispatch authority
              │
              ▼
      fresh post-action observation
              │
              ▼
 contracts + mutation challenges + receipts
              │
              ▼
       verified / rejected /
          inconclusive
```

LocalView separates **doing something** from **proving what happened afterward**. That separation is one of the core security and correctness properties of the project.

---

## What the bounded V1 already proves

LocalView's current V1 claim is intentionally narrower than every research idea in the repository. The software-production boundary is defined in [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md).

### Runtime and discovery

- loopback application discovery and bounded HTTP probing;
- frontend/API classification and framework/HMR evidence;
- durable project/session identity and reconnect behavior;
- normalized observation bus with bounded retained history;
- resource-governed engine escalation.

### Semantic, layout and source intelligence

- stable semantic element references;
- semantic + geometry state snapshots and diffs;
- overflow, overlap and alignment anomaly primitives;
- responsive/adaptive viewport analysis;
- source-map and project-owned source correlation;
- React, Vue and Svelte ownership foundations;
- CSS declaration/source/cascade authority;
- point-select and affected-region verification paths.

### Visual evidence

- native rendered capture on Windows, macOS and Linux;
- changed-region localization and bounded visual evidence;
- private-region redaction before persistence;
- progressive visual targeting;
- guarded full-page stitching under bounded geometry, memory and deadline rules;
- content-addressed artifact/evidence handling.

### Runtime diagnostics

- bounded console issue grouping;
- failed/slow/duplicate/large/CORS network analysis;
- exact-session network-fault authority for localhost validation;
- performance-lite packets, long-task/layout-instability/HMR health primitives;
- accessibility naming/alternative/target checks;
- action → request → UI-response correlation.

### Consequential agent actions

Managed WebView actions support:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

The production path is deliberately two-phase:

```text
fresh semantic cut
   → plan
   → one-shot confirmation
   → payload/lineage verification
   → exact executor permit
   → durable dispatch journal
   → fresh post-dispatch observation
   → postcondition receipt
```

Payload-bearing actions keep plaintext process-local and bind it to durable HMAC commitments. Restart does not restore confirmation, payload, or dispatch authority.

CLI/MCP expose **plan → confirm → status** rather than direct one-step mutation commands.

### Trusted Fix and verification

LocalView carries human-reviewed fixes through a durable verification pipeline:

- exact candidate/revision identity;
- bounded affected-state prediction;
- source-only disposable preflight with proven repository-side-effect containment;
- fresh post-Apply deterministic observation;
- production hard-contract catalog;
- safe synthetic mutation challenges;
- actual-vs-predicted impact accounting;
- scope-explicit bounded verification receipt.

A bounded receipt may become **Verified** for the exact selected target on the current canonical route when all bounded obligations are clean.

That is **not** silently promoted into a whole-application proof.

The independent whole-impact autonomous verdict remains fail-closed while the dependency denominator or complete revalidation universe is unknown.

---

## Human + agent interfaces

### Desktop

The Tauri desktop app provides the human-facing dashboard, localhost preview, evidence surfaces, Trusted Fix/Verify flow, settings, runtime state and explicit update checking.

Technology:

- Tauri 2.11
- React 19
- Vite 8
- WebView2 on Windows
- WKWebView on macOS
- WebKitGTK through WRY on Linux

### CLI

The CLI is intended for compact local automation and agent harnesses.

Examples:

```bash
cargo run -p localview -- sessions
cargo run -p localview -- snapshot <session-id>
cargo run -p localview -- performance-lite <session-id>
```

Consequential interaction uses dedicated two-phase commands rather than the legacy generic action queue.

### MCP

`integrations/mcp` exposes a stdio MCP-compatible JSON-RPC bridge. It allows agent systems to query sessions/evidence and use the same bounded consequential authority without inventing a separate security model.

---

## Architecture

```text
                              Local machine
                                  │
                     ┌────────────▼────────────┐
                     │ localhost discovery     │
                     └────────────┬────────────┘
                                  │
                     ┌────────────▼────────────┐
                     │ session / project truth │
                     └───────┬─────────┬───────┘
                             │         │
               ┌─────────────▼──┐   ┌──▼────────────────┐
               │ Tauri desktop  │   │ daemon/control    │
               │ + preview      │   │ loopback runtime  │
               └────────┬───────┘   └────────┬──────────┘
                        │                    │
                        └──────────┬─────────┘
                                   ▼
                         observation/evidence
                  ┌────────┬────────┬────────┬────────┐
                  ▼        ▼        ▼        ▼        ▼
               semantic  visual   network  source   a11y/...
                  └────────┴────────┴────────┴────────┘
                                   │
                                   ▼
                          contracts / receipts
                                   │
                    ┌──────────────┼──────────────┐
                    ▼              ▼              ▼
                 Desktop          CLI             MCP
```

The Rust workspace keeps policy, evidence, runtime authority and platform integrations separated into focused crates. Heavy Chromium execution is Tier 3 and remains planner/resource-governor controlled rather than becoming the default observation engine.

---

## Repository map

```text
apps/
  cli/                    command-line client
  daemon/                 localhost discovery + control runtime
  desktop/                React/Vite + Tauri desktop application

crates/
  protocol/               shared domain/wire schema
  discovery/              localhost discovery + classification
  sessions/               durable runtime/session identity
  observation/            normalized retained observation bus
  control/                authenticated loopback API
  live-bridge/            managed WebView bridge/action authority
  native-capture/         native rendered capture authority
  semantic/               semantic tree and state diff
  visual/                 pixel evidence and image processing
  responsive/             responsive/adaptive analysis
  source-map/             source-map resolution
  source-graph/           bounded source/ownership graph
  network/ console/ a11y/ performance/
  contracts/ mutation/ verification/
  resource-governor/      bounded runtime resource authority
  validation-lab/         cross-platform contract/real-provider validation

integrations/
  mcp/                    stdio MCP-compatible bridge

docs/
  ARCHITECTURE.md
  IMPLEMENTATION_STATUS.md
  PRODUCTION_CLOSURE_MATRIX.md
  PRODUCTION_RELEASE.md
  ROADMAP.md
  SECURITY.md
  SPEC_COVERAGE.md
  superpowers/            active normative contracts only
```

Historical plans, evidence and superseded design material live in the private **Localview-document** research/archive repository rather than cluttering the production tree.

---

## Quick start for development

### Prerequisites

- Rust stable compatible with the workspace (`rust-version = 1.85`)
- Node.js 24+
- platform prerequisites required by Tauri 2
- WebView2 on Windows / WKWebView on macOS / WebKitGTK on Linux as required by the platform

### Rust workspace

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

The Tauri hook prepares the matching daemon sidecar automatically.

### Local release-candidate build

```bash
cd apps/desktop
npm ci
npm run tauri build
```

See [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md) before distributing a build.

---

## Security model

LocalView is designed around **bounded authority**, not blanket browser automation.

Important boundaries include:

- authenticated loopback control plane;
- no generic shell permission for dashboard WebViews;
- preview/workspace capability separation;
- stable provider/target incarnation fencing;
- one-shot consequential confirmation;
- process-local sensitive payload authority;
- durable dispatch and postcondition receipts;
- stale/restart/replay protection;
- bounded artifact retention;
- private visual redaction before persistence;
- fail-closed unknown/incomplete verification;
- exact-head production/release evidence.

See [docs/SECURITY.md](docs/SECURITY.md) for the current security model.

---

## Production and release status

### Software-production status

**Bounded V1 software-production: complete on `main`.**

The final production-closure campaign includes cross-platform CI, release-candidate bundles, clean-machine install/first-launch evidence, rollback-state policy, SBOM/provenance, security/contract gates, rendered-pixel validation and an adversarial machine-enforced production-closure truth check.

### v0.2.0 release candidate

The next distributable milestone is **v0.2.0-rc.1**.

It is intentionally an **unsigned pre-release candidate**.

The following remain external publication blockers for a signed final release:

- Windows code-signing credentials;
- macOS Developer ID signing + notarization credentials;
- production updater-signing authority;
- W10 physical mixed-DPI evidence if that physical topology is advertised as supported.

The V1 update path is **check-only**: a compile-time-pinned HTTPS channel can report a newer version, but LocalView does not automatically download/install an update without production signature-verification authority.

---

## What LocalView does *not* claim

LocalView intentionally avoids inflated claims.

Current V1 does **not** claim:

- whole-application Autonomous Verified when the affected dependency universe is incomplete;
- arbitrary internet interception or TLS MITM;
- unrestricted browser/OS automation;
- native child-WebView workspace promotion as the default before composition/focus/z-order/minimize-restore/DPI evidence is complete;
- signed public installers before signing credentials exist;
- W10 mixed-DPI physical closure without real multi-monitor hardware evidence.

Unknown evidence remains unknown.

---

## Documentation

| Document | Purpose |
| --- | --- |
| [Architecture](docs/ARCHITECTURE.md) | Current system architecture and boundaries |
| [Implementation status](docs/IMPLEMENTATION_STATUS.md) | What is implemented vs partial |
| [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md) | Exact V1 release-truth boundary |
| [Production release](docs/PRODUCTION_RELEASE.md) | Packaging, signing and publication gates |
| [Security](docs/SECURITY.md) | Security and authority model |
| [Spec coverage](docs/SPEC_COVERAGE.md) | Broader capability/spec coverage |
| [Roadmap](docs/ROADMAP.md) | Current and post-V1 direction |

---

## Project philosophy

LocalView is built around a simple rule:

> **A tool for AI coding should know the difference between “I issued an action” and “I proved the application now satisfies the intended condition.”**

That difference drives the architecture: evidence, exact lineage, bounded authority, fresh observations, explicit unknown states, and reproducible release truth.

---

## Contributing

When changing a production claim, update the corresponding implementation evidence and truth documents in the same change. Do not convert heuristics into correctness/security facts.

Historical research and completed implementation material should be archived rather than allowed to become stale product truth.

---

## License

Licensed under either of:

- MIT
- Apache License 2.0

at your option.

---

<div align="center">

**LocalView — see the running application, not just the source tree.**

</div>
