<div align="center">

# LocalView

### 面向开发者与 AI 编程代理的 localhost 可视化运行时

**看见应用真实发生了什么。基于精确证据执行操作。验证实际改变。**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

[![CI](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml)
[![Production closure](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml)
![Version](https://img.shields.io/badge/version-0.2.0-5b8def)
![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView 是什么？

LocalView 是一个 **Rust-first、AI-native 的 localhost 可视化运行时**。

它会自动发现本机正在运行的 Web 应用，在开发服务器重启或端口变化时保持稳定的项目/会话身份，采集用户与运行时真实可见的证据，并通过受限的本地控制面把这些信息提供给桌面 UI、CLI、MCP 和 AI 编程代理。

LocalView **不是通用浏览器，也不是简单的截图包装器**。

它的目标很明确：

> 把正在运行的 localhost 应用转换成紧凑、可验证、适合代理消费的视觉与运行时事实来源。

核心运行时保持本地。控制 API 仅绑定 loopback，并要求认证。完整 Chromium 只是按需升级层，而不是默认成本。

---

## LocalView 的核心差异

| 能力 | LocalView 的重点 |
| --- | --- |
| **自动理解 localhost** | 自动发现本地前端、分类端点，并在端口变化时保持项目身份。 |
| **多证据融合** | 结合语义、几何、真实像素、console、network、accessibility、performance 与 source correlation，而不是相信单一信号。 |
| **为 Agent 原生设计** | 稳定元素引用、受限 snapshot、diff-first 状态、token-budget packet、CLI 与 MCP，避免把整个 DOM/浏览器遥测直接塞给 LLM。 |
| **有权限边界的操作** | 采用 **plan → confirm → status**，绑定精确 surface/provider lineage，记录持久化 dispatch，并用新的 postcondition evidence 验证结果。 |
| **修改后必须验证** | Trusted Fix/Verify 与 Wave 9 可以验证当前路由上的精确目标，而不会把局部证明夸大为“整个应用都已验证”。 |
| **默认轻量** | 优先 native WebView 与确定性分析；仅在真正需要浏览器特定证据时升级到 Chromium/Tier 3。 |
| **生产级纪律** | 跨平台 CI、真实 rendered-pixel smoke、clean install、rollback policy、provenance、SBOM 与机器强制的 production truth。 |

---

## 工作流程

```text
本地应用启动
    │
    ▼
 discovery
    │
    ▼
 stable session
    │
    ▼
 semantic + layout + visual + network + console + a11y
    │
    ▼
 evidence / bounded state
   │               │
 desktop        CLI / MCP / agent
   │               │
   └──── plan → confirm → act
                    │
                    ▼
             fresh verification
                    │
                    ▼
               proof / status
```

关键在最后：**操作被发送并不意味着操作被证明成功**。LocalView 会记录操作权限，重新采集 post-action evidence，并基于新的观察评估 postcondition。

---

## LocalView 能观察什么？

- **Semantic** — 稳定元素引用、交互状态、可访问名称与语义快照。
- **Layout** — 几何、overflow、overlap、alignment 与 responsive state。
- **Visual** — 真实渲染像素、变化区域与 visual diff。
- **Network** — 失败、慢、重复、大请求与 CORS 信号。
- **Console** — 确定性分组与受限运行时诊断。
- **Accessibility** — accessible name、图片替代文本、有效交互目标与 Wave 6 intelligence。
- **Performance** — long task、layout instability、HMR health 与轻量运行时健康信息。
- **Source correlation** — 项目内 source map、component/source ownership 与运行时源码提示。
- **Responsive** — viewport matrix、adaptive sweep 与 breakpoint 分析。

LocalView 追求的不是“遥测越多越好”，而是 **足以支持结论的最小证据集**。

---

## Agent 操作，而不是盲目自动化

LocalView 支持以下 managed-WebView consequential actions：

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

它们不会作为一次性“直接执行”接口暴露。

生产链路：

1. 获取新的 semantic evidence；
2. 针对精确 stable ref 规划；
3. 生成短生命周期、one-shot confirmation capability；
4. 显式确认；
5. 校验 provider/target incarnation 与 payload commitment；
6. 在精确 managed surface 上 dispatch；
7. 持久化 executor completion；
8. 获取新的 post-action observation；
9. 评估 postcondition；
10. 返回 bounded proof/status。

带 payload 的操作把明文保留在进程内，持久化元数据只保存 keyed commitment。重启不会恢复 confirmation、payload 或 dispatch authority。

CLI/MCP 只暴露 **`plan → confirm → status`**。

---

## Trusted Fix 与 bounded verification

LocalView 会严格区分：

- “我们改了什么？”
- “我们真正证明了什么？”

Trusted Fix/Verify 会保存精确 revision/candidate identity，使用新的 post-Apply evidence，执行固定 hard-contract catalog 与安全的 synthetic mutation challenges。

V1 支持的生产证明范围是：

`current_target_current_route`

bounded receipt 可以是 `verified`、`rejected` 或 `inconclusive`。

独立的 whole-impact autonomous verdict 仍然存在，但只有在未来具备可证明完整的 dependency denominator 和 complete revalidation universe 时才允许通过。

> LocalView 能证明“这个路由上的这个目标正确”时，就只说这一点；不会自动声称“整个应用都正确”。

---

## 安全模型

- 控制 API 仅绑定 loopback。
- Agent/runtime 访问需要生成的 bearer token。
- Dashboard 与 preview capability 分离。
- Preview surface 不获得 dashboard command authority。
- Managed action 绑定精确 session/surface/provider/target lineage。
- Payload action 使用进程内明文 + 持久 HMAC commitment。
- Consequential journal 区分 admitted/prepared/dispatched-or-ambiguous/postcondition。
- Verification 必须使用 fresh evidence。
- Update check 拒绝 redirect 与 cross-origin artifact。
- V1 updater **永远不会授予 install authority**。
- 自动签名更新在生产 signing authority 存在前保持关闭。

详见 [docs/SECURITY.md](docs/SECURITY.md)。

---

## 跨平台运行时

| 平台 | Native 路径 |
| --- | --- |
| Windows | WebView2 + Windows UI Automation / native provider |
| macOS | WKWebView + macOS accessibility/native capture |
| Linux | WebKitGTK + AT-SPI/native capture |

Hosted CI 在三大平台上执行真实 rendered-pixel smoke。Release candidate 也验证 clean-machine install + first launch。

---

## Production 状态

bounded V1 **software production** 已在 `main` 完成。

| 项目 | 状态 |
| --- | --- |
| Daemon + desktop sidecar | ✅ Closed |
| Managed WebView authority | ✅ Closed |
| click/focus/type/key/scroll | ✅ Closed |
| CLI/MCP two-phase action | ✅ Closed |
| Restart-safe Trusted Fix/Verify | ✅ Closed |
| Wave 9 bounded verification | ✅ Closed |
| 三平台 native rendered capture | ✅ Closed |
| Headless/reporting/attestation | ✅ Closed |
| Clean-machine install + first launch | ✅ Closed |
| Initial-release rollback policy | ✅ Closed |
| Provenance + SPDX SBOM | ✅ Closed |
| Fail-closed update check | ✅ Closed |
| Whole-app Autonomous Verified | 🧭 Post-V1 |
| Native child-WebView 默认 workspace | 🧭 Post-V1 |
| Windows code signing | ⛔ 外部凭据 |
| macOS Developer ID + notarization | ⛔ 外部凭据 |
| Signed automatic updater | ⛔ 外部 signing authority |
| W10 mixed-DPI physical proof | ⛔ 需要真实硬件 |

**Unsigned CI bundle 是 release candidate，不是已签名的 public production installer。**

权威边界见 [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md)。

---

## 快速开始

要求：

- Rust **1.85+**
- Node.js **24+**
- Tauri 2 / 平台 Native WebView 所需系统依赖

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

另一个终端：

```bash
cargo run -p localview -- sessions
```

Desktop：

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

构建本地候选：

```bash
npm run tauri build
```

MCP：

```bash
cargo run -p localview-mcp
```

---

## 架构原则

LocalView 使用明确的 engine escalation：

1. **Tier 0** — source/static inspection
2. **Tier 1** — lightweight machine execution
3. **Tier 2** — native Tauri/WRY WebView + native capture
4. **Tier 3** — 仅在浏览器特定 compatibility/emulation/deep tracing 时使用 Chromium

因此强证据能力不必永久承担重型浏览器成本。

---

## 文档

- [Architecture](docs/ARCHITECTURE.md)
- [Security](docs/SECURITY.md)
- [Implementation status](docs/IMPLEMENTATION_STATUS.md)
- [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md)
- [Production release](docs/PRODUCTION_RELEASE.md)
- [Spec coverage](docs/SPEC_COVERAGE.md)
- [Roadmap](docs/ROADMAP.md)

历史计划、证据与已被替代的设计文档存放在 `Nolane-x/Localview-document`。仍被测试/工作流引用的 active specs 保留在产品仓库。

---

## Release 边界

LocalView 0.2.0 已完成 bounded V1 software closure，但正式的 **signed public production release** 仍需要：

- Windows code-signing identity；
- macOS Developer ID + notarization；
- production updater-signing authority。

如果要声明支持 W10 的同 HWND 跨屏 mixed-DPI topology，还需要真实 Windows 多屏硬件证明。

在这些外部条件满足之前，公开二进制应标记为 **unsigned release candidate / pre-release**。

---

## License

LocalView 按 **MIT OR Apache-2.0** 双许可证发布。

见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)。
