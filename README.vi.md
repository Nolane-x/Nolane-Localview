<div align="center">

# LocalView

### Runtime trực quan cho localhost dành cho developer và AI coding agent

**Nhìn thấy ứng dụng đang thực sự làm gì. Hành động trên bằng chứng chính xác. Xác minh điều đã thay đổi.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [Español](README.es.md) · [Français](README.fr.md)

[![CI](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/ci.yml)
[![Production closure](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml/badge.svg)](https://github.com/Nolane-x/Nolane-Localview/actions/workflows/software-production-closure.yml)
![Version](https://img.shields.io/badge/version-0.2.0-5b8def)
![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView là gì?

LocalView là một **localhost visual runtime viết theo hướng Rust-first và agent-native**.

Nó tự phát hiện ứng dụng web đang chạy trên máy, duy trì danh tính project/session ổn định khi dev server restart hoặc đổi port, quan sát bằng chứng mà người dùng và runtime thực sự nhìn thấy, rồi cung cấp dữ liệu đó cho dashboard, CLI, MCP và AI coding agent thông qua một control plane cục bộ có giới hạn rõ ràng.

LocalView **không cố trở thành một trình duyệt đa năng** và cũng không chỉ là công cụ chụp screenshot.

Mục tiêu của nó hẹp và rõ:

> biến một ứng dụng localhost đang chạy thành một nguồn sự thật trực quan + runtime nhỏ gọn, có thể kiểm chứng và phù hợp cho AI agent.

Core runtime hoạt động cục bộ. Control API chỉ bind loopback và có xác thực. Chromium đầy đủ chỉ là tầng escalation khi thật sự cần.

---

## Điều làm LocalView khác biệt

| Điểm | LocalView làm gì |
| --- | --- |
| **Tự hiểu localhost** | Phát hiện local frontend đang chạy, phân loại endpoint và giữ project identity ngay cả khi port thay đổi. |
| **Hợp nhất nhiều loại bằng chứng** | Semantic, geometry, pixel thật, console, network, accessibility, performance và source correlation được kết hợp thay vì tin vào một tín hiệu duy nhất. |
| **Thiết kế cho AI agent từ đầu** | Stable element ref, snapshot có giới hạn, diff-first state, token-budget packet, CLI và MCP thay vì đổ toàn bộ DOM/browser telemetry vào LLM. |
| **Action có authority rõ ràng** | Action dùng chuỗi **plan → confirm → status**, exact surface/provider lineage, durable dispatch accounting và fresh postcondition verification. |
| **Xác minh sau khi sửa** | Trusted Fix/Verify và Wave 9 có thể chứng minh bounded result cho đúng target trên route hiện tại mà không giả vờ đã chứng minh toàn ứng dụng. |
| **Nhẹ mặc định** | Ưu tiên native WebView + deterministic analysis; chỉ nâng lên Chromium/Tier 3 khi evidence browser-specific thật sự cần. |
| **Production discipline** | Cross-platform CI, rendered-pixel smoke, clean install, rollback policy, provenance, SBOM và machine-enforced production truth. |

---

## Vòng đời của LocalView

```text
ứng dụng local chạy
        │
        ▼
   discovery
        │
        ▼
 session model
        │
        ▼
 semantic + layout + visual + network + console + a11y
        │
        ▼
  evidence / state
     │        │
 dashboard  CLI / MCP / agent
     │        │
     └── plan → confirm → act
                     │
                     ▼
              fresh verification
                     │
                     ▼
               proof / status
```

Điểm quan trọng nhất là bước cuối: **LocalView không coi action là đúng chỉ vì action đã được gửi đi**. Nó ghi nhận authority, quan sát lại sau action và kiểm tra postcondition trên evidence mới.

---

## LocalView nhìn thấy những gì?

LocalView dùng mô hình perception đa tín hiệu:

- **Semantic** — stable element refs, trạng thái interactive, accessible identity, semantic snapshot.
- **Layout** — geometry, overflow, overlap, alignment, responsive state.
- **Visual** — rendered pixels thật, changed region và visual diff.
- **Network** — request fail/chậm/trùng/lớn/CORS.
- **Console** — nhóm lỗi runtime một cách deterministic và bounded.
- **Accessibility** — accessible name, image alternative, target/effective interaction và Wave 6 intelligence.
- **Performance** — long task, instability, HMR health và lightweight runtime health.
- **Source correlation** — project-contained source maps, component/source ownership và runtime/source hint.
- **Responsive** — viewport matrix, adaptive sweep và breakpoint analysis.

Mục tiêu không phải thu càng nhiều telemetry càng tốt. Mục tiêu là **tìm bộ evidence nhỏ nhất nhưng đủ để kết luận có trách nhiệm**.

---

## Cho agent hành động mà không "nhắm mắt click"

LocalView hỗ trợ managed-WebView consequential actions:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

Nhưng chúng không được expose như lệnh one-shot.

Production chain:

1. lấy fresh semantic evidence;
2. plan trên exact stable ref;
3. mint confirmation capability ngắn hạn, one-shot;
4. confirm rõ ràng;
5. kiểm provider/target incarnation và payload commitment;
6. dispatch qua đúng managed surface;
7. ghi nhận executor completion bền vững;
8. lấy post-action observation mới;
9. đánh giá postcondition;
10. trả về proof/status bounded.

Với action có payload, plaintext chỉ nằm process-local; durable metadata dùng keyed commitment. Restart không khôi phục confirmation, payload hay dispatch authority.

CLI/MCP chỉ expose giao diện **`plan → confirm → status`**. Shortcut mutation one-step cũ vẫn đóng.

---

## Trusted Fix và bounded verification

LocalView tách riêng hai câu hỏi:

1. **Ta đã thay đổi gì?**
2. **Ta thực sự chứng minh được điều gì?**

Trusted Fix/Verify mang exact revision/candidate identity qua recovery sau restart, dùng fresh post-Apply evidence, fixed hard-contract catalog và safe synthetic mutation challenges.

Scope production V1 được hỗ trợ là:

`current_target_current_route`

Receipt bounded có thể là:

- `verified`
- `rejected`
- `inconclusive`

Một whole-impact autonomous verdict riêng vẫn tồn tại, nhưng LocalView cố tình fail-closed nếu chưa có dependency denominator được chứng minh đầy đủ và complete revalidation universe.

Nói cách khác:

> Nếu LocalView chỉ chứng minh được "target này trên route này đã đúng", nó sẽ nói đúng như vậy — không tự nâng thành "toàn ứng dụng đã đúng".

---

## Security model

Local automation được coi là security boundary.

- Control API chỉ bind loopback.
- Agent/runtime access cần bearer token được tạo riêng.
- Dashboard capability và preview capability tách biệt.
- Preview surface không nhận command authority của dashboard.
- Managed action bị fence theo exact session/surface/provider/target lineage.
- Payload action dùng process-local plaintext + durable HMAC commitment.
- Durable journal phân biệt admitted/prepared/dispatched-or-ambiguous/postcondition.
- Verification luôn cần fresh evidence.
- Update check không follow redirect và reject cross-origin artifact.
- V1 **không cấp install authority** cho updater.
- Signed automatic update vẫn bị khóa cho tới khi có production signing authority.

Chi tiết: [docs/SECURITY.md](docs/SECURITY.md).

---

## Cross-platform

| Nền tảng | Native path |
| --- | --- |
| Windows | WebView2 + Windows UI Automation / native provider |
| macOS | WKWebView + macOS accessibility/native capture |
| Linux | WebKitGTK + AT-SPI/native capture |

Hosted CI chạy rendered-pixel smoke thật trên cả ba nền tảng. Release-candidate workflow cũng build bundle và chứng minh fresh install + first launch cho unsigned candidate trên Windows/macOS/Linux.

---

## Trạng thái production

Bounded V1 **software** hiện đã complete trên `main`.

| Hạng mục | Trạng thái |
| --- | --- |
| Daemon + desktop sidecar | ✅ Closed |
| Managed WebView authority | ✅ Closed |
| click/focus/type/key/scroll | ✅ Closed |
| CLI/MCP two-phase action | ✅ Closed |
| Restart-safe Trusted Fix/Verify | ✅ Closed |
| Wave 9 bounded verification | ✅ Closed |
| Native rendered capture 3 nền tảng | ✅ Closed |
| Headless/reporting/attestation | ✅ Closed |
| Clean-machine install + first launch | ✅ Closed |
| Initial-release rollback policy | ✅ Closed |
| Provenance + SPDX SBOM | ✅ Closed |
| Fail-closed update check | ✅ Closed |
| Whole-app Autonomous Verified | 🧭 Post-V1 |
| Native child-WebView làm workspace mặc định | 🧭 Post-V1 |
| Windows code signing | ⛔ Thiếu credential ngoài hệ thống |
| macOS Developer ID + notarization | ⛔ Thiếu credential ngoài hệ thống |
| Signed automatic updater | ⛔ Thiếu signing authority |
| W10 mixed-DPI physical proof | ⛔ Cần hardware thật |

**Unsigned CI bundle là release candidate, không phải signed public production installer.**

Nguồn sự thật: [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md).

---

## Bắt đầu nhanh

### Yêu cầu

- Rust **1.85+**
- Node.js **24+**
- dependency hệ thống cần bởi Tauri 2 / native WebView của nền tảng

### Core runtime

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

Terminal khác:

```bash
cargo run -p localview -- sessions
```

### Desktop

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

Build candidate:

```bash
cd apps/desktop
npm ci
npm run tauri build
```

Desktop build tự chuẩn bị đúng `localview-daemon` sidecar.

### MCP bridge

```bash
cargo run -p localview-mcp
```

MCP bridge dùng stdio JSON-RPC và tiêu thụ LocalView runtime đã được xác thực. Nó hướng tới packet nhỏ gọn, bounded observation, verification và two-phase consequential actions.

---

## Kiến trúc

```text
localhost discovery
       │
       ▼
stable sessions
       │
       ▼
observation bus
       │
       ├── semantic
       ├── layout / visual
       ├── network / console
       ├── accessibility / performance
       └── source correlation
       │
       ▼
 evidence + bounded state
       │
  ┌────┼────┐
  ▼    ▼    ▼
 UI   CLI   MCP
  └────┬────┘
       ▼
 plan / confirm / verify
```

Engine escalation:

1. **Tier 0** — source/static inspection.
2. **Tier 1** — lightweight machine execution.
3. **Tier 2** — native Tauri/WRY WebView + native capture.
4. **Tier 3** — Chromium cho browser-specific compatibility/emulation/deep tracing.

Đây là lý do LocalView có thể mạnh mà không biến Chromium nặng thành chi phí mặc định.

---

## Cấu trúc repository

```text
apps/
  daemon/        discovery + control runtime
  cli/           CLI
  desktop/       React/Vite + Tauri

crates/
  protocol/
  discovery/
  sessions/
  observation/
  control/
  semantic/
  native-capture/
  native-provider/
  live-bridge/
  verification/
  planner/
  ...

integrations/
  mcp/

docs/
  ARCHITECTURE.md
  SECURITY.md
  IMPLEMENTATION_STATUS.md
  PRODUCTION_CLOSURE_MATRIX.md
  PRODUCTION_RELEASE.md
  SPEC_COVERAGE.md
```

Historical plan/evidence/superseded design được chuyển sang `Nolane-x/Localview-document`. Active spec đang được tests/workflows dùng vẫn ở product repo.

---

## Tài liệu quan trọng

| Tài liệu | Nội dung |
| --- | --- |
| [Architecture](docs/ARCHITECTURE.md) | Kiến trúc runtime |
| [Security](docs/SECURITY.md) | Trust/authority boundary |
| [Implementation status](docs/IMPLEMENTATION_STATUS.md) | Năng lực đã landed |
| [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md) | Boundary V1 |
| [Production release](docs/PRODUCTION_RELEASE.md) | Packaging/signing/release gates |
| [Spec coverage](docs/SPEC_COVERAGE.md) | Capability rộng hơn và deferred breadth |
| [Roadmap](docs/ROADMAP.md) | Hướng phát triển |

---

## Release boundary

LocalView 0.2.0 đã complete bounded V1 software closure, nhưng **signed public production release** vẫn cần:

- Windows code-signing identity;
- macOS Developer ID + notarization;
- production updater-signing authority.

W10 mixed-DPI còn cần Windows hardware nhiều màn hình thật nếu muốn claim topology đó là supported.

Trước khi có các external requirement trên, binary phát hành phải được ghi rõ là **unsigned release candidate / pre-release**.

---

## Đóng góp

LocalView ưu tiên contribution theo evidence:

1. xác định authority/capability chính xác đang thay đổi;
2. giữ fail-closed ở trust boundary;
3. thêm deterministic contract;
4. chạy gate cross-platform/exact-head liên quan;
5. chỉ cập nhật production truth sau khi evidence đã landed.

---

## License

LocalView dual-license theo **MIT OR Apache-2.0**, tùy lựa chọn của bạn.

Xem [LICENSE-MIT](LICENSE-MIT) và [LICENSE-APACHE](LICENSE-APACHE).
