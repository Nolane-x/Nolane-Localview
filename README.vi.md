<div align="center">

# LocalView

### Runtime trực quan local-first dành cho con người, coding agent và ứng dụng localhost.

**Quan sát ứng dụng đang chạy. Hiểu điều gì thực sự xảy ra. Hành động bằng quyền hạn có giới hạn. Xác minh kết quả bằng bằng chứng.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView là gì?

LocalView là một **runtime trực quan AI-native dành cho localhost**. Nó được xây cho developer và coding agent cần hiểu **ứng dụng đang chạy thật sự như thế nào**, chứ không chỉ đọc mã nguồn rồi suy đoán.

LocalView tự động phát hiện ứng dụng local, quản lý project/session, quan sát cấu trúc semantic và hình ảnh đã render, thu thập console/network/performance/accessibility evidence, liên kết runtime với source ownership, rồi cung cấp trạng thái gọn qua desktop, CLI và MCP.

LocalView không cố trở thành một trình duyệt đa năng khác. Mục tiêu của nó hẹp hơn nhưng hữu ích hơn cho công việc phát triển phần mềm:

> biến một ứng dụng localhost đang chạy thành một môi trường có thể quan sát, kiểm chứng và thao tác an toàn cho cả con người lẫn AI agent.

Nó tập trung trả lời những câu hỏi như:

- UI hiện tại **thực sự** đang render gì?
- Element này là ref nào và liên quan đến vùng source nào?
- Sau khi sửa code, UI có thay đổi đúng không?
- Có xuất hiện lỗi console, network, layout hay accessibility mới không?
- Agent có thể click/type/scroll mà không được trao quyền browser vô hạn không?
- Sau action, hệ thống có quan sát lại từ một snapshot mới hay chỉ tin rằng action “đã chạy”?
- Nếu bằng chứng thiếu, cũ, mơ hồ hoặc vượt ngoài phạm vi đã chứng minh thì hệ thống có **fail closed** không?

---

## Vì sao LocalView khác biệt?

| Nguyên tắc | LocalView thực hiện như thế nào |
| --- | --- |
| **Runtime truth trước** | Quan sát ứng dụng đang chạy thay vì mặc định source code phản ánh chính xác trạng thái render hiện tại. |
| **Local-first** | Control plane chạy trên loopback và tập trung vào workflow phát triển localhost. |
| **Evidence trước confidence** | Semantic snapshot, native visual evidence, runtime event, contract, receipt, hash và provenance đều là dữ liệu hạng nhất. |
| **Agent authority có giới hạn** | Action consequential đi qua plan → confirm → dispatch chính xác → quan sát mới → verification. |
| **Fail-closed** | Stale lineage, contract unknown, impact bất ngờ hoặc evidence thiếu không được tự động biến thành success. |
| **Cross-platform native proof** | Windows/WebView2, macOS/WKWebView và Linux/WebKitGTK đều có rendered-pixel evidence trong hosted CI. |
| **Nhẹ trước, nặng sau** | Chromium đầy đủ là Tier 3, không phải engine mặc định. |
| **Dành cho cả người và agent** | Desktop, CLI và MCP cùng dùng một mô hình runtime/evidence chung. |

---

## Vòng lặp của LocalView

```text
ứng dụng localhost
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
   ┌───┴────┐
   ▼        ▼
 con người  agent
   └───┬────┘
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
verified / rejected / inconclusive
```

Điểm quan trọng là LocalView tách **“đã gửi action”** khỏi **“đã chứng minh kết quả sau action”**.

---

## Những gì V1 hiện đã chứng minh

Phạm vi V1 được khóa chính xác tại [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md).

### Discovery và runtime

- phát hiện localhost + bounded HTTP probing;
- phân loại frontend/API và framework/HMR evidence;
- project/session identity bền vững;
- reconnect/disconnect grace + cleanup;
- observation bus chuẩn hóa;
- engine escalation có resource budget.

### Semantic, layout và source

- stable semantic refs;
- snapshot + semantic/geometry diff;
- overflow/overlap/alignment detection;
- responsive/adaptive viewport analysis;
- source-map và project-owned source correlation;
- React/Vue/Svelte ownership foundation;
- CSS declaration/source/cascade authority;
- point-select và affected-region verification.

### Visual evidence

- native rendered capture trên Windows/macOS/Linux;
- pixel diff + changed-region localization;
- private-region redaction trước khi persist;
- progressive visual targeting;
- guarded full-page stitching;
- bounded geometry/memory/deadline;
- content-addressed artifacts/evidence.

### Runtime diagnostics

- console grouping/dedup;
- network failed/slow/duplicate/large/CORS;
- localhost network-fault authority có lease/hit bounds;
- performance-lite packets;
- long-task/layout-instability/HMR health;
- accessibility naming/alternative/target checks;
- action → request → UI-response correlation.

---

## Agent action có kiểm soát

Managed WebView hỗ trợ:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

Pipeline production:

```text
fresh semantic cut
→ plan
→ one-shot confirmation
→ kiểm tra payload + lineage
→ exact executor permit
→ durable dispatch journal
→ fresh post-dispatch observation
→ postcondition receipt
```

Với action chứa payload nhạy cảm, plaintext được giữ process-local và durable layer chỉ lưu HMAC commitment. Restart không khôi phục confirmation/payload/dispatch authority.

CLI/MCP chỉ expose **plan → confirm → status**, không expose shortcut mutation một bước.

---

## Trusted Fix và Wave 9 verification

LocalView đưa một fix đã được người dùng review qua pipeline:

1. khóa candidate + revision chính xác;
2. dự đoán affected state trong phạm vi bounded;
3. chạy disposable source-only preflight;
4. chứng minh repository-side-effect containment;
5. Apply;
6. tạo fresh semantic/visual observation;
7. chạy production hard contracts;
8. chạy safe synthetic mutation challenges;
9. so sánh actual impact với predicted impact;
10. phát hành bounded verification receipt.

Receipt bounded có thể là **Verified** cho **target chính xác trên canonical route hiện tại** khi toàn bộ obligation trong scope này sạch.

Nhưng LocalView không đánh tráo điều đó thành “toàn ứng dụng đã được chứng minh”.

Whole-impact autonomous verdict vẫn fail-closed nếu dependency denominator hoặc revalidation universe chưa được chứng minh đầy đủ.

---

## Giao diện cho người và agent

### Desktop

Desktop app dùng:

- Tauri 2.11
- React 19
- Vite 8
- WebView2 trên Windows
- WKWebView trên macOS
- WebKitGTK/WRY trên Linux

Nó cung cấp dashboard, localhost preview, evidence, Trusted Fix/Verify, settings, runtime status và update check thủ công.

### CLI

```bash
cargo run -p localview -- sessions
cargo run -p localview -- snapshot <session-id>
cargo run -p localview -- performance-lite <session-id>
```

### MCP

`integrations/mcp` cung cấp stdio MCP-compatible JSON-RPC bridge để agent truy vấn session/evidence và dùng cùng bounded consequential authority.

---

## Kiến trúc

```text
                    Máy local
                       │
             localhost discovery
                       │
                project/session
                 ┌─────┴─────┐
                 ▼           ▼
          Tauri desktop   daemon/control
                 └─────┬─────┘
                       ▼
              observation/evidence
        ┌──────┬──────┬──────┬──────┐
        ▼      ▼      ▼      ▼      ▼
     semantic visual network source a11y...
        └──────┴──────┴──────┴──────┘
                       │
                       ▼
              contracts / receipts
                 ┌─────┼─────┐
                 ▼     ▼     ▼
              Desktop CLI   MCP
```

Chromium đầy đủ thuộc Tier 3 và phải được planner/resource-governor cấp quyền, thay vì luôn chạy nền.

---

## Bắt đầu phát triển

### Yêu cầu

- Rust stable tương thích `rust-version = 1.85`
- Node.js 24+
- dependencies nền tảng của Tauri 2

### Rust workspace

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

Tauri hook sẽ tự chuẩn bị daemon sidecar phù hợp.

### Build release candidate local

```bash
cd apps/desktop
npm ci
npm run tauri build
```

Đọc [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md) trước khi phân phối build.

---

## Security model

LocalView ưu tiên **bounded authority**, không phải browser automation vô hạn.

Các boundary chính:

- authenticated loopback control;
- không cấp generic shell permission cho dashboard WebView;
- tách preview/workspace capability;
- provider/target incarnation fencing;
- one-shot confirmation;
- process-local sensitive payload;
- durable dispatch/postcondition receipts;
- stale/restart/replay protection;
- private visual redaction trước persistence;
- bounded artifact retention;
- unknown/incomplete verification phải fail closed;
- exact-head release evidence.

Chi tiết: [docs/SECURITY.md](docs/SECURITY.md).

---

## Trạng thái production và release

### Software

**Bounded V1 software-production đã hoàn thành trên `main`.**

Closure bao gồm:

- cross-platform CI;
- Windows/macOS/Linux release-candidate bundles;
- clean-machine install + first launch;
- rendered-pixel validation;
- rollback-state policy;
- SBOM + provenance;
- security/contract gates;
- machine-enforced production truth với negative/adversarial checks.

### v0.2.0-rc.1

Mốc phân phối tiếp theo là **v0.2.0-rc.1**.

Đây là **unsigned pre-release candidate**.

Final signed public release vẫn cần:

- Windows code-signing credential;
- macOS Developer ID + notarization credential;
- production updater-signing authority;
- W10 physical mixed-DPI evidence nếu topology đó được quảng cáo là supported.

Updater V1 chỉ **check** version qua pinned HTTPS channel; nó không tự download/install khi chưa có signature-verification authority.

---

## LocalView không claim điều gì?

V1 hiện **không** claim:

- whole-app Autonomous Verified khi dependency universe chưa complete;
- arbitrary internet interception/TLS MITM;
- browser/OS automation không giới hạn;
- native child-WebView là workspace mặc định trước khi có đủ composition/focus/z-order/minimize-restore/DPI evidence;
- signed public installers trước khi có credential;
- W10 mixed-DPI closure khi chưa có real multi-monitor evidence.

Không biết thì vẫn là **unknown**, không phải success.

---

## Tài liệu chính

| Tài liệu | Mục đích |
| --- | --- |
| [ARCHITECTURE](docs/ARCHITECTURE.md) | Kiến trúc hiện tại |
| [IMPLEMENTATION_STATUS](docs/IMPLEMENTATION_STATUS.md) | Trạng thái implementation |
| [PRODUCTION_CLOSURE_MATRIX](docs/PRODUCTION_CLOSURE_MATRIX.md) | Ranh giới V1 production |
| [PRODUCTION_RELEASE](docs/PRODUCTION_RELEASE.md) | Build/sign/release gates |
| [SECURITY](docs/SECURITY.md) | Security + authority model |
| [SPEC_COVERAGE](docs/SPEC_COVERAGE.md) | Coverage rộng hơn |
| [ROADMAP](docs/ROADMAP.md) | Hướng phát triển |

Historical plans/evidence/superseded design được đưa sang kho archive riêng để product repo chỉ giữ current operational docs và active normative contracts.

---

## Triết lý

> **Một công cụ cho AI coding phải biết khác nhau giữa “tôi vừa thực hiện một action” và “tôi đã chứng minh ứng dụng hiện đáp ứng điều kiện mong muốn”.**

Đây là lý do LocalView đặt evidence, fresh observation, exact lineage, bounded authority và explicit unknown state ở trung tâm kiến trúc.

---

## License

MIT OR Apache-2.0, tùy lựa chọn.

<div align="center">

**LocalView — nhìn thấy ứng dụng đang chạy, không chỉ nhìn source tree.**

</div>
