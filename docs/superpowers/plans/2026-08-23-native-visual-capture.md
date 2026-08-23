# Native Visual Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build LocalView's first real native visual-capture vertical slice: visible managed WebView -> bounded PNG -> local artifact -> daemon Visual evidence on Windows/macOS/Linux, while desktop and core crates remain safe Rust.

**Architecture:** `localview-native-capture` is the only platform adapter allowed to contain audited unsafe. Desktop invokes it inside Tauri `with_webview`, receives an owned non-serializable `CapturedFrame`, persists the PNG through one long-lived bounded `ArtifactStore`, then posts metadata to a narrow authenticated control endpoint that inserts Visual evidence into the daemon's existing `EvidenceStore`.

**Tech Stack:** Rust 2024; Tauri 2.11.5/WRY; WebView2 `webview2-com` 0.38 + windows 0.61; objc2 0.6 + objc2-web-kit/app-kit 0.3; GTK 0.18 + webkit2gtk 2.0 + Cairo; serde/thiserror/tokio; LocalView artifact/evidence/control layers.

**Spec:** `docs/superpowers/specs/2026-08-23-native-visual-capture-design.md`

## Global Constraints

- `apps/desktop/src-tauri`, `crates/capture`, `crates/artifacts`, `crates/evidence`, `crates/control` and protocol retain `#![forbid(unsafe_code)]`.
- Only `crates/native-capture/src/platform/*` may contain explicit unsafe; crate root uses `#![deny(unsafe_op_in_unsafe_fn)]`.
- Tauri stays exactly `2.11.5`; target binding families match Tauri 2.11.x.
- First slice supports `CaptureTarget::Viewport` only; unsupported targets fail explicitly.
- PNG limit is exactly 24 MiB (`25_165_824` bytes); visual artifact store limit is 256 MiB.
- `CapturedFrame` is deliberately not `Serialize`/`Deserialize`; raw PNG never enters observer/action/evidence JSON.
- Desktop derives route from the resolved WebView, not from caller input.
- Capture is restricted to expected `preview-{session}` / `workspace-{session}` LocalView surfaces.
- No canvas/html2canvas or silent Chromium fallback.
- Coverage stays `Partial` until a real native screenshot smoke is verified.

---

### Task 1: Safe contract + PNG validation

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/native-capture/Cargo.toml`
- Create: `crates/native-capture/src/lib.rs`
- Create: `crates/native-capture/tests/contract.rs`

**Produces:** `CaptureRequest`, `ViewportMeta`, `CapturedFrame`, `NativeCaptureBackend`, `NativeCaptureError`, `MAX_PNG_BYTES`, `validate_png`, `png_dimensions`, `build_frame`.

- [ ] **Step 1: RED test**

```rust
use localview_capture::CaptureTarget;
use localview_native_capture::{png_dimensions, validate_png, CaptureRequest, NativeCaptureError, ViewportMeta, MAX_PNG_BYTES};

#[test]
fn contract_is_viewport_bounded_and_png_checked() {
    let request = CaptureRequest {
        target: CaptureTarget::Viewport,
        viewport: ViewportMeta { css_width: 1280, css_height: 820, device_scale_factor: 1.25 },
        route: "http://127.0.0.1:5173/".into(),
        revision: Some("abc123".into()),
    };
    assert_eq!(serde_json::to_value(request).unwrap()["viewport"]["css_width"], 1280);
    assert_eq!(MAX_PNG_BYTES, 25_165_824);
    assert!(matches!(validate_png(b"bad"), Err(NativeCaptureError::InvalidImage)));
    assert!(png_dimensions(b"bad").is_err());
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p localview-native-capture --test contract`
Expected: crate/type/function missing.

- [ ] **Step 3: Minimal GREEN implementation**

Use:

```rust
#![deny(unsafe_op_in_unsafe_fn)]

pub const MAX_PNG_BYTES: usize = 24 * 1024 * 1024;
pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureRequest { pub target: CaptureTarget, pub viewport: ViewportMeta, pub route: String, pub revision: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ViewportMeta { pub css_width: u32, pub css_height: u32, pub device_scale_factor: f64 }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeCaptureBackend { WebView2, WkWebView, WebKitGtk }

#[derive(Debug, PartialEq)]
pub struct CapturedFrame {
    pub png: Vec<u8>, pub pixel_width: u32, pub pixel_height: u32,
    pub backend: NativeCaptureBackend, pub viewport: ViewportMeta,
    pub route: String, pub revision: Option<String>, pub captured_at_unix_ms: u64,
}
```

`validate_png` must verify signature, encoded size and a minimally valid IHDR. `png_dimensions` reads big-endian width/height from bytes `16..24` only after validation and rejects zero dimensions. `build_frame` centralizes validation + timestamp construction so platform modules cannot bypass limits.

`crates/native-capture/Cargo.toml` has `serde`, `thiserror`, local `capture`; `[dev-dependencies] serde_json.workspace = true`.

- [ ] **Step 4: GREEN**

Run: `cargo test -p localview-native-capture --test contract`
Expected: PASS.

- [ ] **Step 5: Commit**

`feat(capture): add safe native capture contract`

---

### Task 2: Audited platform boundary

**Files:**
- Modify: `crates/native-capture/Cargo.toml`
- Modify: `crates/native-capture/src/lib.rs`
- Create: `crates/native-capture/src/platform/mod.rs`
- Create: `crates/native-capture/src/platform/windows.rs`
- Create: `crates/native-capture/src/platform/macos.rs`
- Create: `crates/native-capture/src/platform/linux.rs`
- Create: `crates/native-capture/tests/safety_boundary.rs`

**Produces:** public safe execution entrypoint accepting Tauri's safe `PlatformWebview` wrapper and a completion closure; platform FFI never appears in common request/result types.

- [ ] **Step 1: RED boundary test**

```rust
#[test]
fn unsafe_boundary_stays_out_of_product_crates() {
    let desktop = include_str!("../../../apps/desktop/src-tauri/src/lib.rs");
    let capture = include_str!("../../capture/src/lib.rs");
    let common = include_str!("../src/lib.rs");
    assert!(desktop.contains("#![forbid(unsafe_code)]"));
    assert!(capture.contains("#![forbid(unsafe_code)]"));
    assert!(common.contains("#![deny(unsafe_op_in_unsafe_fn)]"));
    assert!(!common.contains("*mut "));
    assert!(!common.contains("*const "));
}
```

Also inspect platform files and require every file containing `unsafe {` to contain `// SAFETY:`.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p localview-native-capture --test safety_boundary`
Expected: missing modules/entrypoint.

- [ ] **Step 3: Pin adapter dependencies**

```toml
[dependencies]
localview-capture = { path = "../capture" }
serde.workspace = true
thiserror.workspace = true
tauri = { version = "2.11.5", default-features = false, features = ["wry"] }

[target.'cfg(windows)'.dependencies]
webview2-com = "0.38"
windows = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Com"] }

[target.'cfg(target_os = "linux")'.dependencies]
gtk = { version = "0.18", features = ["v3_24"] }
webkit2gtk = { version = "=2.0", features = ["v2_40"] }
cairo-rs = { version = "0.18", features = ["png"] }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-foundation = { version = "0.3", default-features = false, features = ["std", "NSData", "NSDictionary"] }
objc2-app-kit = { version = "0.3", default-features = false, features = ["std", "NSImage", "NSBitmapImageRep"] }
objc2-web-kit = { version = "0.3", default-features = false, features = ["objc2-app-kit", "WKWebView", "WKSnapshotConfiguration"] }
```

- [ ] **Step 4: GREEN skeleton**

`capture_webview(platform, request, completion)` first rejects `request.target != Viewport`, then cfg-dispatches to one platform module. Unsupported desktop platforms invoke completion with `UnsupportedPlatform`. Common module has no unsafe.

- [ ] **Step 5: Verify**

Run: `cargo test -p localview-native-capture --test safety_boundary`
Run: `cargo check -p localview-native-capture --all-targets`
Expected: PASS on current target.

- [ ] **Step 6: Commit**

`feat(capture): isolate native platform boundary`

---

### Task 3: Real Windows/macOS/Linux viewport backends

**Files:**
- Modify: `crates/native-capture/src/platform/windows.rs`
- Modify: `crates/native-capture/src/platform/macos.rs`
- Modify: `crates/native-capture/src/platform/linux.rs`
- Create: `crates/native-capture/tests/platform_contract.rs`

**Produces:** native PNG capture only; all backends finish via `build_frame`.

- [ ] **Step 1: RED source contract**

On matching targets require:

```rust
#[cfg(windows)] assert!(include_str!("../src/platform/windows.rs").contains("CapturePreview"));
#[cfg(target_os="macos")] assert!(include_str!("../src/platform/macos.rs").contains("WKSnapshotConfiguration"));
#[cfg(target_os="linux")] assert!(include_str!("../src/platform/linux.rs").contains("get_snapshot"));
```

All targets assert backend source contains `build_frame` and does not contain `html2canvas`, `canvas.toDataURL`, Playwright launch code or base64 screenshot conversion.

- [ ] **Step 2: Verify RED on PR matrix**

Expected: each platform contract fails only because its native primitive is absent.

- [ ] **Step 3: Windows GREEN**

Use Tauri's platform wrapper -> WebView2 controller -> `ICoreWebView2` -> in-memory `IStream` -> `CapturePreview(PNG, stream, handler)`. Completion rewinds/reads the stream into owned bytes and calls `build_frame(..., WebView2, request)`. Every explicit unsafe block carries `// SAFETY:` for Tauri-origin handle, main-thread execution and COM lifetime.

- [ ] **Step 4: macOS GREEN**

Cast Tauri's inner handle to `WKWebView` inside the macOS module, configure visible bounds + `afterScreenUpdates`, call snapshot completion, convert returned `NSImage` through `NSBitmapImageRep` PNG representation, copy to owned bytes, call `build_frame(..., WkWebView, request)`. No Objective-C object crosses the callback boundary.

- [ ] **Step 5: Linux GREEN**

Use the safe WebKitGTK `WebView` handle, request visible snapshot, encode returned Cairo surface via `write_to_png(Vec<u8>)`, then call `build_frame(..., WebKitGtk, request)`.

- [ ] **Step 6: Verify platform compile/tests**

Run on each matrix OS:
`cargo check -p localview-native-capture --all-targets`
`cargo test -p localview-native-capture`
Expected: PASS.

- [ ] **Step 7: Commit**

`feat(capture): add native WebView viewport backends`

---

### Task 4: Shared daemon Visual evidence ingestion

**Files:**
- Modify: `crates/control/src/lib.rs`
- Create or extend tests in: `crates/control/src/lib.rs`

**Produces:** authenticated `POST /v1/sessions/{id}/evidence/visual` with a narrow body; daemon constructs `EvidenceDraft` itself.

- [ ] **Step 1: RED test**

Build a test ControlState with a real session and POST:

```json
{
  "artifact_id":"lv-123",
  "pixel_width":1280,
  "pixel_height":820,
  "backend":"webview2",
  "route":"http://127.0.0.1:5173/",
  "viewport":{"css_width":1280,"css_height":820,"device_scale_factor":1.0},
  "revision":"abc123",
  "captured_at_unix_ms":123,
  "target":"viewport"
}
```

Assert recent session evidence contains `EvidenceKind::Visual`, source `native-capture`, artifact id, and no `png`/`base64` key.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p localview-control visual_evidence`
Expected: route missing.

- [ ] **Step 3: Implement narrow request + route**

Add `VisualEvidenceRequest` as a private Deserialize type. Handler verifies bearer token + session existence, rejects `target != "viewport"`, constructs Visual `EvidenceDraft` with `UncertaintyClass::Observed`, confidence `1.0`, source `native-capture`, engine from backend, revision from request, and inserts into the existing daemon `EvidenceStore`. Return only evidence id + dedupe flag.

- [ ] **Step 4: GREEN**

Run: `cargo test -p localview-control visual_evidence`
Expected: PASS.

- [ ] **Step 5: Commit**

`feat(evidence): ingest native visual capture metadata`

---

### Task 5: Desktop managed-surface coordinator + bounded artifacts

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/native_capture_contract.rs`

**Produces:** Tauri `capture_viewport(app, state, session_id, viewport, revision) -> VisualCaptureReceipt` with artifact/evidence ids + metadata only.

- [ ] **Step 1: RED contract**

```rust
let lib = include_str!("../src/lib.rs");
let module = include_str!("../src/visual_capture.rs");
assert!(lib.contains("#![forbid(unsafe_code)]"));
assert!(lib.contains("capture_viewport"));
assert!(module.contains("ArtifactStore"));
assert!(module.contains("with_webview"));
assert!(module.contains("/evidence/visual"));
assert!(module.contains("window.url()"));
assert!(!module.contains("base64"));
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p localview-desktop --test native_capture_contract`
Expected: module/command missing.

- [ ] **Step 3: Add persistent `VisualCaptureState`**

Manage `VisualCaptureState { artifacts: tokio::sync::Mutex<Option<ArtifactStore>> }` in Tauri setup. Lazily open `state_dir()/artifacts/visual` at exactly `256 * 1024 * 1024` bytes and reuse that store for every capture.

- [ ] **Step 4: Resolve only managed surfaces and trust native route**

Resolve `preview_surface_label(session)` first, then workspace label if present. Reuse `bridge_surface_label_allowed`; reject missing/mismatched surface. Read `route = window.url()` and require existing loopback navigation policy. Do not accept route as command input.

- [ ] **Step 5: Coordinate async platform capture**

Use `tokio::sync::oneshot`. Call `window.with_webview(move |platform| localview_native_capture::capture_webview(platform, request, move |result| { let _ = tx.send(result); }))`; await with `tokio::time::timeout(Duration::from_secs(3), rx)`.

- [ ] **Step 6: Persist + post metadata**

Store PNG as `visual/png`, immediately drop the in-memory pixel vector after persistence, then POST the narrow metadata request to `/v1/sessions/{id}/evidence/visual` with bearer token. Receipt contains artifact id, evidence id, backend, route, viewport, pixel dimensions, revision, capture timestamp; never artifact filesystem path or PNG bytes.

- [ ] **Step 7: GREEN desktop checks**

Run:
`cargo check -p localview-desktop`
`cargo check -p localview-desktop --features native-workspace`
`cargo test -p localview-desktop --test native_capture_contract`
Expected: PASS.

- [ ] **Step 8: Commit**

`feat(capture): wire native viewport capture into desktop`

---

### Task 6: CI platform gates + conservative status docs

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/SPEC_COVERAGE.md`

- [ ] **Step 1: Linux matrix dependency gate**

Before the rust-core cache/check on `ubuntu-latest`, install at minimum:

```yaml
- name: Install native capture system dependencies
  if: matrix.os == 'ubuntu-latest'
  run: |
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential pkg-config
```

This is required because `localview-native-capture` now participates in the core workspace check.

- [ ] **Step 2: Add explicit native capture tests**

After workspace tests in rust-core:

```yaml
- name: Native capture platform contract
  run: cargo test -p localview-native-capture
```

In Tauri/frontend Linux job add:

```yaml
- name: Native visual capture desktop contract
  run: cargo test -p localview-desktop --test native_capture_contract
```

- [ ] **Step 3: Update docs without overclaiming**

`Native screenshot adapter` -> `Partial`: real platform API code + compile gates + desktop artifact/evidence path exist, but hosted/native GUI screenshot smoke is still a completion gate. Wave 2 lists the first slice as landed only after CI is green.

- [ ] **Step 4: Full fresh-head verification**

Require all Windows/macOS/Ubuntu Rust jobs plus Tauri/frontend Linux job green on the final head. Review diff for unsafe leakage, raw PNG in JSON/logs, arbitrary-window capture, unbounded retention, platform types in common data API, and canvas/Chromium fallback.

- [ ] **Step 5: Commit**

`ci: gate native visual capture across platforms`

---

### Task 7: PR review and merge gate

**Files:** no production changes unless review finds an issue.

- [ ] **Step 1:** Open draft PR `feat/native-visual-capture -> main` summarizing RED/GREEN evidence and remaining GUI-smoke gap.
- [ ] **Step 2:** Inspect complete diff for Critical/Important issues; fix before proceeding.
- [ ] **Step 3:** Re-run fresh current-head CI after every fix; ignore superseded/cancelled runs as completion evidence.
- [ ] **Step 4:** Mark ready and merge only when all final-head CI jobs are green.
