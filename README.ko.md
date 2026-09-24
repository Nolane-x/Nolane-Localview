<div align="center">

# LocalView

### 개발자, 코딩 에이전트, localhost 애플리케이션을 위한 local-first 시각 런타임.

**실행 중인 앱을 관찰하고, 실제로 무엇이 일어났는지 이해하고, 제한된 권한으로 행동하고, 증거로 결과를 검증합니다.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView란?

LocalView는 소스 코드를 읽는 것만으로 끝나지 않고 **실제로 실행 중인 애플리케이션을 이해하기 위한 AI-native localhost 시각 런타임**입니다.

로컬 앱을 탐지하고 project/session을 유지하며 semantic 구조와 실제 렌더링 결과를 관찰합니다. console/network/performance/accessibility evidence를 수집하고 runtime evidence를 source ownership과 연결한 뒤 Desktop, CLI, MCP를 통해 사람과 에이전트가 동일한 bounded runtime model을 사용하도록 합니다.

LocalView의 목적은 범용 브라우저를 다시 만드는 것이 아닙니다.

> 실행 중인 localhost 앱을 신뢰할 수 있고 관찰 가능하며 검증 가능한 개발 환경으로 바꾸는 것이 목적입니다.

LocalView는 다음 질문에 답하려고 합니다.

- 지금 UI가 실제로 무엇을 렌더링하고 있는가?
- 이 element는 어떤 stable ref와 source 영역에 대응하는가?
- 수정 후 의도한 UI 변화가 정말 발생했는가?
- 새로운 console/network/layout/accessibility regression이 생겼는가?
- 에이전트에게 무제한 브라우저 권한을 주지 않고 click/type/scroll을 허용할 수 있는가?
- action 이후 fresh observation으로 결과를 검증하는가?
- evidence가 stale/incomplete/ambiguous일 때 fail closed 하는가?

---

## LocalView가 다른 이유

| 원칙 | LocalView 방식 |
| --- | --- |
| **Runtime truth first** | 소스에서 현재 UI를 추측하지 않고 실행 중인 앱을 관찰합니다. |
| **Local-first** | Control plane은 loopback에 바인딩되고 localhost 개발 흐름에 집중합니다. |
| **Evidence-first** | semantic snapshot, native visual evidence, runtime event, contract, receipt, hash, provenance를 1급 데이터로 취급합니다. |
| **Bounded agent authority** | consequential action은 plan → confirm → exact dispatch → fresh verification을 거칩니다. |
| **Fail closed** | stale lineage, unknown contract, unexpected impact, 불완전 evidence는 success로 승격되지 않습니다. |
| **Cross-platform proof** | Windows/WebView2, macOS/WKWebView, Linux/WebKitGTK에 hosted rendered-pixel evidence가 있습니다. |
| **Resource-aware** | 전체 Chromium은 Tier 3이며 기본 경로가 아닙니다. |
| **Human + Agent** | Desktop, CLI, MCP가 같은 runtime/evidence model을 공유합니다. |

---

## LocalView 루프

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

LocalView는 **“action을 실행했다”**와 **“action 이후 상태를 증명했다”**를 분리합니다.

---

## V1에서 검증된 범위

정확한 production boundary는 [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md)에 정의되어 있습니다.

### Runtime / discovery

- localhost discovery와 bounded HTTP probing
- frontend/API classification과 framework/HMR evidence
- durable project/session identity
- reconnect/disconnect grace/cleanup
- normalized observation bus
- resource-governed engine escalation

### Semantic / layout / source

- stable semantic refs
- semantic + geometry snapshot/diff
- overflow/overlap/alignment 분석
- responsive/adaptive viewport analysis
- source-map과 project-owned source correlation
- React/Vue/Svelte ownership foundation
- CSS declaration/source/cascade authority
- point-select와 affected-region verification

### Visual evidence

- Windows/macOS/Linux native rendered capture
- pixel diff와 changed-region localization
- persistence 전 private-region redaction
- progressive visual targeting
- guarded full-page stitching
- bounded geometry/memory/deadline
- content-addressed artifact/evidence

### Runtime diagnostics

- console grouping/dedup
- failed/slow/duplicate/large/CORS network analysis
- exact-session localhost network-fault authority
- performance-lite, long-task, layout-instability, HMR health
- accessibility checks
- action → request → UI-response correlation

---

## 제한된 consequential actions

Managed WebView 지원:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

Production chain:

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

민감한 payload plaintext는 process-local로 유지되고 durable layer에는 HMAC commitment만 기록됩니다. restart는 confirmation/payload/dispatch authority를 복구하지 않습니다.

CLI/MCP는 **plan → confirm → status**만 노출합니다.

---

## Trusted Fix / Wave 9

사용자가 review한 fix는 다음 단계를 거칩니다.

1. exact candidate/revision identity
2. bounded affected-state prediction
3. disposable source-only preflight
4. repository-side-effect containment proof
5. Apply
6. fresh semantic/visual observation
7. production hard contracts
8. safe synthetic mutation challenges
9. actual-vs-predicted impact
10. bounded verification receipt

현재 canonical route의 exact target이 모든 bounded obligation을 만족하면 receipt가 **Verified**가 될 수 있습니다.

하지만 이것을 whole-app proof로 표현하지 않습니다.

whole-impact autonomous verdict는 completeness-certified dependency denominator와 complete revalidation universe가 없으면 fail closed 상태를 유지합니다.

---

## Desktop / CLI / MCP

### Desktop

- Tauri 2.11
- React 19
- Vite 8
- Windows: WebView2
- macOS: WKWebView
- Linux: WebKitGTK / WRY

### CLI

```bash
cargo run -p localview -- sessions
cargo run -p localview -- snapshot <session-id>
cargo run -p localview -- performance-lite <session-id>
```

### MCP

`integrations/mcp`는 stdio MCP-compatible JSON-RPC bridge를 제공합니다. 에이전트는 Desktop과 동일한 bounded evidence/authority model을 사용할 수 있습니다.

---

## 개발 시작

요구사항:

- `rust-version = 1.85`와 호환되는 Rust stable
- Node.js 24+
- Tauri 2 플랫폼 의존성

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

다른 터미널:

```bash
cargo run -p localview -- sessions
```

Desktop:

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

Release candidate:

```bash
cd apps/desktop
npm ci
npm run tauri build
```

배포 전에 [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md)를 확인하세요.

---

## Security model

LocalView는 무제한 browser automation보다 **bounded authority**를 선택합니다.

핵심 경계:

- authenticated loopback control
- dashboard WebView에 generic shell permission 없음
- preview/workspace capability separation
- provider/target incarnation fencing
- one-shot confirmation
- process-local sensitive payload authority
- durable dispatch/postcondition receipts
- stale/restart/replay protection
- private visual redaction
- bounded artifact retention
- unknown/incomplete verification은 fail closed
- exact-head release evidence

자세한 내용: [docs/SECURITY.md](docs/SECURITY.md)

---

## Production / Release 상태

**Bounded V1 software-production은 `main`에서 완료되었습니다.**

closure에는 cross-platform CI, release-candidate bundle, clean-machine install/first launch, rendered-pixel validation, rollback-state policy, SBOM/provenance, security/contract gates, adversarial production-truth checks가 포함됩니다.

다음 배포 마일스톤:

**v0.2.0-rc.1 — unsigned pre-release candidate**

Signed final public release에는 다음이 추가로 필요합니다.

- Windows code-signing credentials
- macOS Developer ID + notarization
- production updater-signing authority
- 해당 topology를 지원한다고 명시할 경우 W10 physical mixed-DPI evidence

V1 updater는 pinned HTTPS channel을 확인할 뿐, signature-verification authority 없이 자동 다운로드/설치를 수행하지 않습니다.

---

## LocalView가 현재 주장하지 않는 것

- incomplete dependency universe에서 whole-app Autonomous Verified
- arbitrary internet interception / TLS MITM
- unrestricted browser/OS automation
- composition/focus/z-order/minimize-restore/DPI evidence 없이 native child-WebView를 기본 workspace로 승격
- credential 없는 signed public installer
- real multi-monitor evidence 없는 W10 mixed-DPI closure

Unknown은 success가 아니라 unknown으로 남습니다.

---

## 문서

- [Architecture](docs/ARCHITECTURE.md)
- [Implementation status](docs/IMPLEMENTATION_STATUS.md)
- [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md)
- [Production release](docs/PRODUCTION_RELEASE.md)
- [Security](docs/SECURITY.md)
- [Spec coverage](docs/SPEC_COVERAGE.md)
- [Roadmap](docs/ROADMAP.md)

과거 plan/evidence/superseded design은 별도 archive로 이동하고 product repository에는 current operational docs와 active normative contracts만 유지합니다.

---

## Philosophy

> **AI coding tool은 “action을 실행했다”와 “애플리케이션이 목표 상태임을 증명했다”의 차이를 알아야 합니다.**

이 원칙이 LocalView의 evidence, fresh observation, exact lineage, bounded authority, explicit unknown state 설계를 이끕니다.

---

## License

MIT OR Apache-2.0.

<div align="center">

**LocalView — 소스 트리뿐 아니라 실행 중인 애플리케이션을 보세요.**

</div>
