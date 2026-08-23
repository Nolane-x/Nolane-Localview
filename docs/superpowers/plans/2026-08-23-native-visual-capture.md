# Native Visual Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first real LocalView native visual-capture vertical slice: capture the visible LocalView-managed WebView as bounded PNG bytes on Windows/macOS/Linux, persist pixels locally, and publish provenance-rich Visual evidence without introducing unsafe code into the desktop shell.

**Architecture:** Add `localview-native-capture` as the sole platform adapter allowed to contain audited unsafe. `localview-desktop` invokes it from Tauri `with_webview`, receives only owned safe Rust values, persists PNG bytes through `localview-artifacts`, and records metadata through `localview-evidence`. `localview-capture` remains the platform-independent policy/planning crate.

**Tech Stack:** Rust 2024; Tauri 2.11.5 / WRY; WebView2 via `webview2-com` 0.38 on Windows; `objc2` 0.6 + `objc2-web-kit`/`objc2-app-kit` 0.3 on macOS; GTK 0.18 + `webkit2gtk` 2.0 + Cairo on Linux; serde/thiserror/tokio; existing LocalView artifact/evidence/control layers.

**Spec:** `docs/superpowers/specs/2026-08-23-native-visual-capture-design.md`

## Global Constraints

- `apps/desktop/src-tauri`, `crates/capture`, `crates/artifacts`, `crates/evidence`, protocol and control crates keep `#![forbid(unsafe_code)]`.
- Only `crates/native-capture/src/platform/*` may contain unsafe; crate root uses `#![deny(unsafe_op_in_unsafe_fn)]`.
- Tauri stays pinned to `2.11.5`; platform binding versions match Tauri 2.11.x compatibility: `webview2-com = 0.38`, `windows = 0.61`, `gtk = 0.18`, `webkit2gtk = 2.0`, `objc2 = 0.6`, `objc2-web-kit = 0.3`, `objc2-app-kit = 0.3`.
- First slice supports `CaptureTarget::Viewport` only. Unsupported targets fail explicitly; there is no canvas/html2canvas fallback.
- Encoded PNG limit is exactly 24 MiB (`25_165_824` bytes).
- Raw PNG bytes are never inserted into observer history or JSON evidence payloads.
- All capture requests target LocalView-managed loopback preview/workspace surfaces and preserve session ownership validation.
- Coverage remains `Partial` until a real native screenshot smoke is verified; compile-only adapters are not called complete.

---

### Task 1: Safe Native Capture Contract

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/native-capture/Cargo.toml`
- Create: `crates/native-capture/src/lib.rs`
- Create: `crates/native-capture/tests/contract.rs`

**Interfaces:**
- Consumes: `localview_capture::CaptureTarget`.
- Produces: `CaptureRequest`, `ViewportMeta`, `CapturedFrame`, `NativeCaptureBackend`, `NativeCaptureError`, `validate_png`, `validate_frame_size`, `MAX_PNG_BYTES`.

- [ ] **Step 1: Write the failing contract test**

```rust
use localview_capture::CaptureTarget;
use localview_native_capture::{
    validate_frame_size, validate_png, CaptureRequest, NativeCaptureBackend,
    NativeCaptureError, ViewportMeta, MAX_PNG_BYTES,
};

#[test]
fn viewport_contract_is_bounded_and_serializable() {
    let request = CaptureRequest {
        target: CaptureTarget::Viewport,
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 820,
            device_scale_factor: 1.25,
        },
        route: "http://127.0.0.1:5173/".into(),
        revision: Some("abc123".into()),
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["viewport"]["css_width"], 1280);
    assert_eq!(MAX_PNG_BYTES, 25_165_824);
    assert_eq!(NativeCaptureBackend::WebView2.to_string(), "webview2");
}

#[test]
fn rejects_non_png_and_oversized_frames() {
    assert!(matches!(validate_png(b"not png"), Err(NativeCaptureError::InvalidImage)));
    assert!(matches!(
        validate_frame_size(MAX_PNG_BYTES + 1),
        Err(NativeCaptureError::FrameTooLarge { .. })
    ));
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test -p localview-native-capture --test contract`

Expected: FAIL because crate/types/functions do not exist.

- [ ] **Step 3: Implement the minimal safe contract**

`crates/native-capture/src/lib.rs` must begin with:

```rust
#![deny(unsafe_op_in_unsafe_fn)]

use localview_capture::CaptureTarget;
use serde::{Deserialize, Serialize};
use std::{fmt, time::{SystemTime, UNIX_EPOCH}};
use thiserror::Error;

pub const MAX_PNG_BYTES: usize = 24 * 1024 * 1024;
pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureRequest {
    pub target: CaptureTarget,
    pub viewport: ViewportMeta,
    pub route: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ViewportMeta {
    pub css_width: u32,
    pub css_height: u32,
    pub device_scale_factor: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeCaptureBackend { WebView2, WkWebView, WebKitGtk }

impl fmt::Display for NativeCaptureBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WebView2 => "webview2",
            Self::WkWebView => "wk_web_view",
            Self::WebKitGtk => "web_kit_gtk",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapturedFrame {
    pub png: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub backend: NativeCaptureBackend,
    pub viewport: ViewportMeta,
    pub route: String,
    pub revision: Option<String>,
    pub captured_at_unix_ms: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativeCaptureError {
    #[error("capture target is not supported by the native adapter")]
    UnsupportedTarget,
    #[error("native capture is not supported on this platform")]
    UnsupportedPlatform,
    #[error("webview is not ready for capture")]
    NotReady,
    #[error("native capture timed out")]
    Timeout,
    #[error("native capture platform error: {0}")]
    Platform(String),
    #[error("native capture did not return a valid PNG")]
    InvalidImage,
    #[error("native capture frame too large: {bytes} > {limit}")]
    FrameTooLarge { bytes: usize, limit: usize },
}

pub fn validate_png(bytes: &[u8]) -> Result<(), NativeCaptureError> {
    (bytes.starts_with(PNG_SIGNATURE)).then_some(()).ok_or(NativeCaptureError::InvalidImage)
}

pub fn validate_frame_size(bytes: usize) -> Result<(), NativeCaptureError> {
    (bytes <= MAX_PNG_BYTES).then_some(()).ok_or(NativeCaptureError::FrameTooLarge { bytes, limit: MAX_PNG_BYTES })
}

pub fn captured_at_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64
}
```

- [ ] **Step 4: Run contract tests GREEN**

Run: `cargo test -p localview-native-capture --test contract`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/native-capture
git commit -m "feat(capture): add safe native capture contract"
```

---

### Task 2: Safety Boundary and Platform Module Skeleton

**Files:**
- Modify: `crates/native-capture/src/lib.rs`
- Create: `crates/native-capture/src/platform/mod.rs`
- Create: `crates/native-capture/src/platform/windows.rs`
- Create: `crates/native-capture/src/platform/macos.rs`
- Create: `crates/native-capture/src/platform/linux.rs`
- Create: `crates/native-capture/tests/safety_boundary.rs`
- Modify: `crates/native-capture/Cargo.toml`

**Interfaces:**
- Consumes: Task 1 contract.
- Produces: platform-private capture functions; public `capture_webview(webview: tauri::webview::PlatformWebview, request: CaptureRequest, completion: impl FnOnce(Result<CapturedFrame, NativeCaptureError>) + Send + 'static)`.

- [ ] **Step 1: Write RED safety tests**

```rust
#[test]
fn unsafe_is_isolated_from_safe_product_crates() {
    let desktop = include_str!("../../../apps/desktop/src-tauri/src/lib.rs");
    let capture = include_str!("../../capture/src/lib.rs");
    let adapter = include_str!("../src/lib.rs");
    assert!(desktop.contains("#![forbid(unsafe_code)]"));
    assert!(capture.contains("#![forbid(unsafe_code)]"));
    assert!(adapter.contains("#![deny(unsafe_op_in_unsafe_fn)]"));
    assert!(!adapter.contains("*mut "));
    assert!(!adapter.contains("*const "));
}
```

Also assert the three platform files exist through `include_str!` and each contains `// SAFETY:` before any `unsafe {` used later.

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-native-capture --test safety_boundary`

Expected: FAIL because platform modules/public dispatch do not exist.

- [ ] **Step 3: Add target dependencies matching Tauri 2.11.5**

Use these target sections in `crates/native-capture/Cargo.toml`:

```toml
[dependencies]
localview-capture = { path = "../capture" }
serde.workspace = true
thiserror.workspace = true
tauri = { version = "2.11.5", default-features = true }

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

- [ ] **Step 4: Implement platform dispatch with explicit unsupported-target gate**

The public function must reject any `request.target != CaptureTarget::Viewport` before entering platform code and use `#[cfg]` to dispatch exactly one backend.

- [ ] **Step 5: Run safety tests GREEN and compile all portable code**

Run: `cargo test -p localview-native-capture --test safety_boundary`

Run: `cargo check -p localview-native-capture --all-targets`

Expected: PASS on the current OS; cross-platform compilation is enforced by CI in later tasks.

- [ ] **Step 6: Commit**

```bash
git add crates/native-capture
git commit -m "feat(capture): isolate platform capture boundary"
```

---

### Task 3: Windows WebView2 Viewport Backend

**Files:**
- Modify: `crates/native-capture/src/platform/windows.rs`
- Create: `crates/native-capture/tests/windows_contract.rs`

**Interfaces:**
- Consumes: Tauri `PlatformWebview`, Task 1 request/frame types.
- Produces: PNG via WebView2 `ICoreWebView2::CapturePreview` using `COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG`.

- [ ] **Step 1: Write Windows-only RED contract**

```rust
#[cfg(windows)]
#[test]
fn windows_backend_uses_native_capture_preview_png() {
    let source = include_str!("../src/platform/windows.rs");
    assert!(source.contains("CapturePreview"));
    assert!(source.contains("COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG"));
    assert!(source.contains("CreateStreamOnHGlobal"));
    assert!(source.contains("validate_png"));
    assert!(!source.contains("html2canvas"));
    assert!(!source.contains("canvas.toDataURL"));
}
```

- [ ] **Step 2: Verify RED on `windows-latest` CI**

Expected: test FAIL because native calls are absent.

- [ ] **Step 3: Implement WebView2 capture**

Inside the Tauri main-thread closure:

1. Obtain the WebView2 controller from `PlatformWebview`.
2. Resolve `ICoreWebView2` from the controller.
3. Create an in-memory COM `IStream` with `CreateStreamOnHGlobal`.
4. Create `CapturePreviewCompletedHandler` from `webview2-com` and invoke `CapturePreview(PNG, stream, handler)`.
5. On successful callback, rewind/read the stream into owned `Vec<u8>`.
6. Validate size and PNG signature.
7. Parse PNG IHDR bytes 16..24 as big-endian width/height; reject zero dimensions.
8. Construct `CapturedFrame` and invoke completion exactly once.

Every `unsafe` block must immediately follow a `// SAFETY:` comment explaining the COM object validity, main-thread origin, and ownership/lifetime of the handler/stream.

- [ ] **Step 4: Run Windows GREEN**

Run in CI: `cargo check -p localview-native-capture --all-targets`

Run in CI: `cargo test -p localview-native-capture --test windows_contract`

Expected: PASS on `windows-latest`.

- [ ] **Step 5: Commit**

```bash
git add crates/native-capture/src/platform/windows.rs crates/native-capture/tests/windows_contract.rs
git commit -m "feat(capture): add WebView2 viewport backend"
```

---

### Task 4: macOS WKWebView Viewport Backend

**Files:**
- Modify: `crates/native-capture/src/platform/macos.rs`
- Create: `crates/native-capture/tests/macos_contract.rs`

**Interfaces:**
- Produces PNG through `WKWebView::takeSnapshot` using `WKSnapshotConfiguration` with `afterScreenUpdates = true`.

- [ ] **Step 1: Write macOS-only RED contract**

```rust
#[cfg(target_os = "macos")]
#[test]
fn macos_backend_uses_wkwebview_snapshot() {
    let source = include_str!("../src/platform/macos.rs");
    assert!(source.contains("WKSnapshotConfiguration"));
    assert!(source.contains("takeSnapshot"));
    assert!(source.contains("afterScreenUpdates"));
    assert!(source.contains("NSBitmapImageRep"));
    assert!(source.contains("validate_png"));
}
```

- [ ] **Step 2: Verify RED on `macos-latest` CI**

Expected: FAIL because native implementation is absent.

- [ ] **Step 3: Implement snapshot and PNG conversion**

1. Cast the Tauri platform view to `&WKWebView` only inside the platform module.
2. Create `WKSnapshotConfiguration`; set its rect to the visible bounds and `afterScreenUpdates = true`.
3. Call the async snapshot completion API.
4. Convert returned `NSImage` to `NSBitmapImageRep`, then PNG representation data.
5. Copy bytes into owned `Vec<u8>` before leaving Objective-C ownership scope.
6. Validate size/signature/IHDR dimensions and construct `CapturedFrame`.
7. Invoke completion exactly once.

Unsafe comments must document pointer origin from Tauri/WRY and main-thread requirements.

- [ ] **Step 4: Run macOS GREEN**

Run in CI: `cargo check -p localview-native-capture --all-targets`

Run in CI: `cargo test -p localview-native-capture --test macos_contract`

Expected: PASS on `macos-latest`.

- [ ] **Step 5: Commit**

```bash
git add crates/native-capture/src/platform/macos.rs crates/native-capture/tests/macos_contract.rs
git commit -m "feat(capture): add WKWebView viewport backend"
```

---

### Task 5: Linux WebKitGTK Viewport Backend

**Files:**
- Modify: `crates/native-capture/src/platform/linux.rs`
- Create: `crates/native-capture/tests/linux_contract.rs`

**Interfaces:**
- Produces PNG using WebKitGTK visible-region snapshot and Cairo PNG encoding.

- [ ] **Step 1: Write Linux-only RED contract**

```rust
#[cfg(target_os = "linux")]
#[test]
fn linux_backend_uses_webkitgtk_snapshot() {
    let source = include_str!("../src/platform/linux.rs");
    assert!(source.contains("get_snapshot"));
    assert!(source.contains("Visible"));
    assert!(source.contains("write_to_png"));
    assert!(source.contains("validate_png"));
}
```

- [ ] **Step 2: Verify RED on Ubuntu CI**

Expected: FAIL because implementation is absent.

- [ ] **Step 3: Implement WebKitGTK snapshot**

1. Use the WebKitGTK `WebView` returned by `PlatformWebview::inner()`.
2. Request the visible snapshot asynchronously with transparent/background behavior left at WebKit default for the first slice.
3. Write the returned Cairo surface into an owned `Vec<u8>` using Cairo PNG encoding.
4. Validate size/signature/IHDR dimensions.
5. Construct `CapturedFrame` and invoke completion once.

Linux backend should remain safe if gtk-rs exposes the snapshot API safely; do not add an unsafe block unless required by Tauri handle access.

- [ ] **Step 4: Run Linux GREEN**

Run: `cargo check -p localview-native-capture --all-targets`

Run: `cargo test -p localview-native-capture --test linux_contract`

Expected: PASS on `ubuntu-latest` with existing WebKitGTK system packages.

- [ ] **Step 5: Commit**

```bash
git add crates/native-capture/src/platform/linux.rs crates/native-capture/tests/linux_contract.rs
git commit -m "feat(capture): add WebKitGTK viewport backend"
```

---

### Task 6: Desktop Capture Coordinator + Artifact/Evidence Persistence

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/native_capture_contract.rs`
- Modify: `crates/artifacts/Cargo.toml` only if needed for test helpers; avoid API churn otherwise.

**Interfaces:**
- Consumes: `localview_native_capture::capture_webview`, `ArtifactStore`, `EvidenceStore` types.
- Produces Tauri command:

```rust
#[tauri::command]
async fn capture_viewport(
    app: tauri::AppHandle,
    session_id: SessionId,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String>
```

`VisualCaptureReceipt` contains artifact id/path-safe metadata, pixel dimensions, backend, route, viewport, revision, timestamp and evidence id. It never contains PNG bytes.

- [ ] **Step 1: Write RED desktop contract**

Assert:

```rust
let lib = include_str!("../src/lib.rs");
let module = include_str!("../src/visual_capture.rs");
assert!(lib.contains("#![forbid(unsafe_code)]"));
assert!(lib.contains("capture_viewport"));
assert!(module.contains("ensure_capture_surface"));
assert!(module.contains("ArtifactStore"));
assert!(module.contains("EvidenceKind::Visual"));
assert!(!module.contains("base64"));
assert!(!module.contains("png:"));
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p localview-desktop --test native_capture_contract`

Expected: FAIL because module/command do not exist.

- [ ] **Step 3: Implement surface/session validation**

Resolve `preview-{session}` first, then `workspace-{session}`. Reject any other label. Reuse the same label policy as the live bridge rather than allowing arbitrary `WebviewWindow` capture.

- [ ] **Step 4: Implement asynchronous coordinator without blocking UI thread**

Create `tokio::sync::oneshot` channel. Call `window.with_webview(move |platform| localview_native_capture::capture_webview(platform, request, move |result| { let _ = tx.send(result); }))`. Await with `tokio::time::timeout(Duration::from_secs(3), rx)`; map expiry to `NativeCaptureError::Timeout`.

- [ ] **Step 5: Persist pixels and evidence metadata**

Use a bounded artifact root under `state_dir()/artifacts/visual` and a fixed visual artifact capacity of 256 MiB for the first slice. Store PNG as kind `visual/png`. Insert `EvidenceDraft { kind: EvidenceKind::Visual, ... }` whose JSON payload contains `artifact_id`, `pixel_width`, `pixel_height`, `backend`, `route`, `viewport`, `revision`, `target: "viewport"`; it must not contain raw PNG bytes.

- [ ] **Step 6: Run desktop GREEN**

Run: `cargo check -p localview-desktop`

Run: `cargo check -p localview-desktop --features native-workspace`

Run: `cargo test -p localview-desktop --test native_capture_contract`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src-tauri
git commit -m "feat(capture): wire native viewport capture into desktop"
```

---

### Task 7: CI Platform Gates and Coverage Ledger

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/SPEC_COVERAGE.md`

**Interfaces:**
- Produces explicit platform-native compile/contract gates on matching runners.

- [ ] **Step 1: Add CI gates**

In `rust-core`, keep existing workspace check/clippy/tests. Add a named step after tests:

```yaml
- name: Native capture platform contract
  run: cargo test -p localview-native-capture
```

This step runs on all three matrix OSes and therefore compiles the matching target module. In `desktop-linux`, add:

```yaml
- name: Native visual capture desktop contract
  run: cargo test -p localview-desktop --test native_capture_contract
```

- [ ] **Step 2: Update ledger conservatively**

`Native screenshot adapter` becomes `Partial`, with text stating real platform APIs are wired/compile-gated but runtime GUI smoke remains required before `Implemented`.

Wave 2 roadmap marks safe contract + platform adapter + desktop persistence as landed only after the full matrix is green.

- [ ] **Step 3: Run full verification**

Run on PR CI:

```text
cargo check --workspace --exclude localview-desktop --all-targets
cargo clippy --workspace --exclude localview-desktop --all-targets -- -D warnings
cargo test --workspace --exclude localview-desktop
cargo check -p localview-desktop
cargo check -p localview-desktop --features native-workspace
cargo test -p localview-desktop --test capability_isolation
cargo test -p localview-desktop --test workspace_surface_policy
cargo test -p localview-desktop --test live_semantic_bridge_contract
cargo test -p localview-desktop --test native_capture_contract
```

Expected: all jobs green on Windows/macOS/Ubuntu plus Tauri/frontend Linux job.

- [ ] **Step 4: Review before merge**

Check the full PR diff specifically for:

- unsafe outside `native-capture/platform`;
- raw pointer/platform types in public API;
- screenshot bytes in logs/evidence JSON;
- non-loopback/arbitrary window capture path;
- unbounded PNG allocation/history;
- silent canvas/Chromium fallback;
- docs overclaiming runtime smoke.

- [ ] **Step 5: Merge only after fresh head verification**

Keep PR draft through RED iterations. Mark ready and merge to `main` only after the current head—not a superseded commit—has all CI jobs green.
