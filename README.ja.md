<div align="center">

# LocalView

### 開発者・コーディングエージェント・localhost アプリのための local-first ビジュアルランタイム。

**実行中のアプリを観測し、実際に起きたことを理解し、限定された権限で操作し、証拠によって結果を検証する。**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## LocalView とは

LocalView は、ソースコードを読むだけではなく、**実際に動いているアプリケーションを理解する**ための AI-native localhost ビジュアルランタイムです。

ローカルアプリを検出し、project/session を管理し、semantic 構造・レンダリング結果・console/network/performance/accessibility evidence を収集します。さらに runtime evidence と source ownership を関連付け、Desktop・CLI・MCP から人間とエージェントに同じ bounded runtime model を提供します。

LocalView は一般用途のブラウザを作り直すプロジェクトではありません。

> 実行中の localhost アプリを、観測可能・検証可能・安全に操作可能な開発環境に変えることが目的です。

LocalView が答えようとするのは、たとえば次のような問いです。

- UI は今、本当に何を描画しているのか。
- この element はどの stable ref と source-owned region に対応しているのか。
- 修正後に、意図した UI 変更が実際に起きたのか。
- 新しい console/network/layout/accessibility regression は発生していないか。
- エージェントに無制限なブラウザ権限を与えずに click/type/scroll を許可できるか。
- action 実行後に fresh observation で結果を検証しているか。
- evidence が stale / incomplete / ambiguous の場合に fail closed できるか。

---

## LocalView の特徴

| 原則 | LocalView のアプローチ |
| --- | --- |
| **Runtime truth first** | 現在の UI をソースから推測するのではなく、実行中のアプリを観測します。 |
| **Local-first** | Control plane は loopback にバインドされ、localhost 開発に集中します。 |
| **Evidence-first** | semantic snapshot、native visual evidence、runtime event、contract、receipt、hash、provenance を第一級データとして扱います。 |
| **限定された agent authority** | consequential action は plan → confirm → exact dispatch → fresh verification を通ります。 |
| **Fail closed** | stale lineage、unknown contract、unexpected impact、不完全な evidence は success に昇格しません。 |
| **Cross-platform native proof** | Windows/WebView2、macOS/WKWebView、Linux/WebKitGTK に hosted rendered-pixel evidence があります。 |
| **Resource-aware** | フル Chromium は Tier 3 であり、デフォルトではありません。 |
| **Human + Agent** | Desktop、CLI、MCP が同じ runtime/evidence model を共有します。 |

---

## LocalView のループ

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

LocalView は **「action を実行した」ことと「その後の状態を証明した」ことを分離**します。

---

## V1 で証明されている範囲

正確な production boundary は [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md) にあります。

### Runtime / discovery

- localhost discovery と bounded HTTP probing
- frontend/API classification、framework/HMR evidence
- durable project/session identity
- reconnect / disconnect grace / cleanup
- normalized observation bus
- resource-governed engine escalation

### Semantic / layout / source

- stable semantic refs
- semantic + geometry snapshot/diff
- overflow / overlap / alignment primitives
- responsive/adaptive viewport analysis
- source-map と project-owned source correlation
- React/Vue/Svelte ownership foundation
- CSS declaration/source/cascade authority
- point-select と affected-region verification

### Visual evidence

- Windows/macOS/Linux の native rendered capture
- pixel diff と changed-region localization
- persistence 前の private-region redaction
- progressive visual targeting
- guarded full-page stitching
- bounded geometry/memory/deadline
- content-addressed artifact/evidence

### Runtime diagnostics

- console grouping/dedup
- failed/slow/duplicate/large/CORS network analysis
- exact-session localhost network-fault authority
- performance-lite / long-task / layout-instability / HMR health
- accessibility checks
- action → request → UI-response correlation

---

## Consequential action の安全モデル

Managed WebView は次をサポートします。

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

Production path:

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

機密 payload の plaintext は process-local に留まり、durable layer には HMAC commitment が保存されます。再起動によって confirmation/payload/dispatch authority が復元されることはありません。

CLI/MCP は **plan → confirm → status** のみを公開し、旧式の one-step mutation shortcut は公開しません。

---

## Trusted Fix / Wave 9

ユーザーが review した fix は、次の bounded verification pipeline を通ります。

1. exact candidate/revision identity
2. bounded affected-state prediction
3. disposable source-only preflight
4. repository-side-effect containment の証明
5. Apply
6. fresh semantic/visual observation
7. production hard contracts
8. safe synthetic mutation challenges
9. actual-vs-predicted impact
10. bounded verification receipt

現在の canonical route 上の exact target について bounded obligation がすべて clean なら、receipt は **Verified** になれます。

ただし、それを whole-app proof として扱うことはありません。

whole-impact autonomous verdict は completeness-certified dependency denominator と完全な revalidation universe がない限り fail closed のままです。

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

`integrations/mcp` は stdio MCP-compatible JSON-RPC bridge を提供し、Desktop と同じ bounded evidence/authority model を agent system から利用できます。

---

## 開発を始める

必要環境:

- `rust-version = 1.85` と互換性のある Rust stable
- Node.js 24+
- Tauri 2 のプラットフォーム依存パッケージ

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

別ターミナル:

```bash
cargo run -p localview -- sessions
```

Desktop:

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

Release candidate build:

```bash
cd apps/desktop
npm ci
npm run tauri build
```

配布前に [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md) を確認してください。

---

## Security

LocalView は無制限のブラウザ自動化ではなく、**bounded authority** を中心に設計されています。

主な境界:

- authenticated loopback control
- dashboard WebView に generic shell permission を与えない
- preview/workspace capability separation
- provider/target incarnation fencing
- one-shot confirmation
- process-local sensitive payload authority
- durable dispatch/postcondition receipts
- stale/restart/replay protection
- private visual redaction
- bounded artifact retention
- unknown/incomplete verification は fail closed
- exact-head release evidence

詳細: [docs/SECURITY.md](docs/SECURITY.md)

---

## Production / Release

**Bounded V1 software-production は `main` 上で完了しています。**

Cross-platform CI、release-candidate bundles、clean-machine install/first launch、rendered-pixel validation、rollback-state policy、SBOM/provenance、security/contract gates、adversarial production-truth checks が closure に含まれます。

次の配布マイルストーン:

**v0.2.0-rc.1 — unsigned pre-release candidate**

Signed final public release にはまだ以下が必要です。

- Windows code-signing credentials
- macOS Developer ID + notarization
- production updater-signing authority
- その topology を supported と宣言する場合の W10 physical mixed-DPI evidence

V1 updater は pinned HTTPS channel を check するだけで、signature-verification authority がない状態では自動 download/install しません。

---

## LocalView が現在 claim しないもの

- dependency universe が不完全な状態での whole-app Autonomous Verified
- arbitrary internet interception / TLS MITM
- 無制限の browser/OS automation
- composition/focus/z-order/minimize-restore/DPI evidence が揃う前の native child-WebView default promotion
- signing credential がない状態での signed public installer
- real multi-monitor evidence がない状態での W10 mixed-DPI closure

Unknown は unknown のままです。

---

## ドキュメント

- [Architecture](docs/ARCHITECTURE.md)
- [Implementation status](docs/IMPLEMENTATION_STATUS.md)
- [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md)
- [Production release](docs/PRODUCTION_RELEASE.md)
- [Security](docs/SECURITY.md)
- [Spec coverage](docs/SPEC_COVERAGE.md)
- [Roadmap](docs/ROADMAP.md)

historical plan / evidence / superseded design は別 archive に移し、product repository には current operational docs と active normative contract のみを残します。

---

## Philosophy

> **AI coding tool は「action を実行した」と「アプリが目標状態になったことを証明した」の違いを理解しなければならない。**

この考え方が evidence、fresh observation、exact lineage、bounded authority、explicit unknown state という LocalView の設計を決めています。

---

## License

MIT OR Apache-2.0。

<div align="center">

**LocalView — source tree だけでなく、実行中のアプリを見る。**

</div>
