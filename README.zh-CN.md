<div align="center">

# LocalView

### 面向开发者、编码智能体与 localhost 应用的 local-first 可视化运行时。

**观察真实运行的应用。理解实际发生了什么。以受限权限执行操作。用证据验证结果。**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView 是什么？

LocalView 是一个 **AI-native、面向 localhost 的可视化运行时**。它服务于需要理解“应用实际正在如何运行”的开发者和编码智能体，而不是只读取源码后进行猜测。

LocalView 可以发现本地应用、维护项目与会话、观察语义结构和真实渲染结果、收集 console/network/performance/accessibility 证据、关联源码归属，并通过桌面端、CLI 与 MCP 向人类和智能体暴露紧凑而可验证的运行时状态。

它并不试图成为另一个通用浏览器。它的目标更明确：

> 将正在运行的 localhost 应用转化为一个可观察、可验证、可安全操作的开发环境。

LocalView 关心的是：

- 页面现在真正渲染了什么？
- 某个元素对应哪个稳定引用和源码区域？
- 修改后 UI 是否真的发生了预期变化？
- 是否新增了 console、network、layout 或 accessibility 回归？
- 智能体能否 click/type/scroll，同时又不拥有无限页面权限？
- action 之后是否基于**新的观察**验证结果，而不是相信“命令已执行”？
- 当证据陈旧、缺失、歧义或超出支持范围时，系统能否 fail closed？

---

## LocalView 的核心差异

| 原则 | 实现方式 |
| --- | --- |
| **运行时事实优先** | 直接观察运行中的应用，而不是假设源码等于当前 UI。 |
| **Local-first** | 控制面绑定 loopback，专注本地开发工作流。 |
| **Evidence-first** | semantic snapshot、native visual evidence、runtime event、contract、receipt、hash 与 provenance 都是一等数据。 |
| **受限智能体权限** | consequential action 使用 plan → confirm → exact dispatch → fresh verification。 |
| **Fail closed** | stale lineage、unknown contract、意外影响与不完整证据不会被静默升级为成功。 |
| **跨平台原生证据** | Windows/WebView2、macOS/WKWebView、Linux/WebKitGTK 均拥有 hosted rendered-pixel 证据路径。 |
| **资源感知** | 完整 Chromium 是 Tier 3，而不是默认引擎。 |
| **人类与智能体共享模型** | Desktop、CLI、MCP 共用同一套 runtime/evidence 边界。 |

---

## LocalView 的工作闭环

```text
localhost app
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
 human / agent
    │
    ▼
bounded action plan
    │
explicit confirmation
    │
    ▼
exact dispatch
    │
    ▼
fresh post-action observation
    │
    ▼
contracts + mutation challenges + receipts
    │
    ▼
verified / rejected / inconclusive
```

LocalView 明确区分 **“执行了操作”** 和 **“证明操作后的状态满足目标”**。

---

## V1 已经证明的能力

精确的 V1 production 边界见 [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md)。

### 运行时与发现

- localhost discovery 与 bounded HTTP probing；
- frontend/API classification、framework/HMR evidence；
- durable project/session identity；
- reconnect、disconnect grace 与 cleanup；
- normalized observation bus；
- resource-governed engine escalation。

### Semantic / layout / source

- stable semantic refs；
- semantic + geometry snapshots/diffs；
- overflow、overlap、alignment primitives；
- responsive/adaptive viewport analysis；
- source-map 与 project-owned source correlation；
- React/Vue/Svelte ownership foundation；
- CSS declaration/source/cascade authority；
- point-select 与 affected-region verification。

### Visual evidence

- Windows/macOS/Linux 原生 rendered capture；
- pixel diff 与 changed-region localization；
- persistence 前 private-region redaction；
- progressive visual targeting；
- guarded full-page stitching；
- bounded geometry/memory/deadline；
- content-addressed artifacts/evidence。

### 运行时诊断

- console grouping/dedup；
- network failed/slow/duplicate/large/CORS；
- exact-session localhost network-fault authority；
- performance-lite、long-task、layout-instability、HMR health；
- accessibility checks；
- action → request → UI-response correlation。

---

## 受控 consequential actions

Managed WebView 支持：

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

生产路径：

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

敏感 payload 的明文只保留在 process-local 内存中，持久层使用 HMAC commitment。重启不会恢复 confirmation、payload 或 dispatch authority。

CLI/MCP 仅暴露 **plan → confirm → status**，不提供旧式一步 mutation shortcut。

---

## Trusted Fix 与 Wave 9

一个用户审核过的修复会经过：

1. 精确 candidate/revision identity；
2. bounded affected-state prediction；
3. disposable source-only preflight；
4. repository-side-effect containment 证明；
5. Apply；
6. fresh semantic/visual observation；
7. production hard contracts；
8. safe synthetic mutation challenges；
9. actual-vs-predicted impact；
10. bounded verification receipt。

当当前 canonical route 上的精确目标满足所有 bounded obligation 时，receipt 可以得到 **Verified**。

但这不会被冒充成“整个应用都已验证”。

whole-impact autonomous verdict 仍要求 completeness-certified dependency denominator 与完整 revalidation universe，否则保持 Inconclusive。

---

## Desktop / CLI / MCP

### Desktop

Tauri 2.11 + React 19 + Vite 8。

平台 WebView：

- Windows：WebView2
- macOS：WKWebView
- Linux：WebKitGTK/WRY

### CLI

```bash
cargo run -p localview -- sessions
cargo run -p localview -- snapshot <session-id>
cargo run -p localview -- performance-lite <session-id>
```

### MCP

`integrations/mcp` 提供 stdio MCP-compatible JSON-RPC bridge，使智能体能够使用与桌面端相同的 evidence 和 bounded authority 模型。

---

## 开发快速开始

要求：

- Rust stable，兼容 `rust-version = 1.85`
- Node.js 24+
- Tauri 2 对应平台依赖

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

另一个终端：

```bash
cargo run -p localview -- sessions
```

桌面端：

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

本地 release candidate：

```bash
cd apps/desktop
npm ci
npm run tauri build
```

发布前请阅读 [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md)。

---

## 安全模型

LocalView 选择 **bounded authority**，而不是无限制浏览器自动化。

核心边界包括：

- authenticated loopback control；
- dashboard WebView 不拥有 generic shell permission；
- preview/workspace capability separation；
- provider/target incarnation fencing；
- one-shot confirmation；
- process-local sensitive payload authority；
- durable dispatch/postcondition receipts；
- restart/replay/stale protection；
- private visual redaction；
- bounded artifact retention；
- unknown/incomplete verification 必须 fail closed；
- exact-head release evidence。

详见 [docs/SECURITY.md](docs/SECURITY.md)。

---

## Production 与发布状态

**Bounded V1 software-production 已在 `main` 完成。**

最终 closure 包含跨平台 CI、release-candidate bundles、clean-machine install/first launch、rendered-pixel validation、rollback-state policy、SBOM/provenance、security/contract gates 与 adversarial production-truth checks。

下一发布里程碑：

**v0.2.0-rc.1 — unsigned pre-release candidate**

签名版 final public release 仍需要：

- Windows code-signing credentials；
- macOS Developer ID + notarization；
- production updater-signing authority；
- 如果宣称支持该拓扑，则需要 W10 physical mixed-DPI evidence。

V1 updater 只检查 pinned HTTPS channel，不会在缺少 signature-verification authority 时自动下载安装。

---

## LocalView 不宣称什么

V1 不宣称：

- dependency universe 不完整时的 whole-app Autonomous Verified；
- arbitrary internet interception / TLS MITM；
- 无限制浏览器/操作系统自动化；
- 缺乏完整 composition/focus/z-order/minimize-restore/DPI evidence 时将 native child-WebView 设为默认 workspace；
- 没有证书时的 signed public installers；
- 没有真实多显示器证据时的 W10 mixed-DPI closure。

未知就是未知，不会自动变成成功。

---

## 文档

- [Architecture](docs/ARCHITECTURE.md)
- [Implementation status](docs/IMPLEMENTATION_STATUS.md)
- [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md)
- [Production release](docs/PRODUCTION_RELEASE.md)
- [Security](docs/SECURITY.md)
- [Spec coverage](docs/SPEC_COVERAGE.md)
- [Roadmap](docs/ROADMAP.md)

历史 plans、evidence 与 superseded design 被迁移至独立 archive，使产品仓库只保留当前 operational 文档和 active normative contracts。

---

## 项目理念

> **AI coding 工具必须知道“我执行了一个 action”和“我已经证明应用满足目标状态”之间的区别。**

LocalView 的 evidence、fresh observation、exact lineage、bounded authority 与 explicit unknown state 都来自这一原则。

---

## License

MIT OR Apache-2.0。

<div align="center">

**LocalView — 看见正在运行的应用，而不仅仅是源码树。**

</div>
