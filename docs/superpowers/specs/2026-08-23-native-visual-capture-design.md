# Native Visual Capture Design

## Status

Approved architecture for Wave 2 of LocalView. This design starts from `main` commit `482085756e7ee8bb6155d9fbe33ee7ce497d4f6e` and extends the already-live semantic/action bridge with real platform WebView pixels.

## Goal

Provide real, privacy-bounded native viewport capture for LocalView-managed WebViews on Windows, macOS and Linux without weakening the existing `#![forbid(unsafe_code)]` boundary in the desktop shell, capture planner, protocol or evidence crates.

The first vertical slice is intentionally narrow:

`managed WebView -> native platform snapshot -> PNG bytes -> provenance-rich capture packet -> bounded local artifact/evidence path`

Element/region capture, stable-settle orchestration, progressive changed-region capture, masking, stitching and responsive contact sheets build on this slice after viewport capture is verified.

## Non-goals for the first slice

- Full-page stitching.
- DOM/canvas reconstruction or html2canvas fallback.
- Chromium/Playwright as the default capture engine.
- Arbitrary external-page screenshotting.
- Reading cookies, storage, response bodies or other secrets.
- Making the native child-WebView workspace the default before its existing cross-platform composition gate is complete.

## Architectural boundary

Create a new `crates/native-capture` crate. It owns all platform-specific WebView access and is the only LocalView crate allowed to contain audited `unsafe` required by platform handles. Consumers receive only safe Rust values.

```text
localview-desktop (safe)
        |
        | WebviewWindow::with_webview(main-thread closure)
        v
localview-native-capture
  safe public API
  +-- windows backend: WebView2 CapturePreview
  +-- macOS backend: WKWebView takeSnapshot
  +-- Linux backend: WebKitGTK get_snapshot
        |
        v
CapturedFrame { png, metadata }
        |
        +--> localview-artifacts
        +--> localview-evidence (visual evidence metadata)
```

`localview-capture` remains the platform-independent transaction/planning layer. `localview-native-capture` is an execution adapter, not a replacement for capture policy.

## Why a separate crate

Tauri 2.11.5 exposes `WebviewWindow::with_webview`, executing a closure on the main thread and providing a platform-specific handle. Tauri explicitly notes that WebView2/WebKitGTK/objc2 bindings may move across minor versions, so the desktop dependency is already pinned to `2.11.5` and the adapter must keep platform bindings tightly scoped.

Official platform primitives:

- Windows: `ICoreWebView2::CapturePreview` writes PNG/JPEG into an `IStream` and completes asynchronously.
- macOS: `WKWebView::takeSnapshot` with `WKSnapshotConfiguration` returns a platform-native image asynchronously.
- Linux: `webkit_web_view_get_snapshot` / finish returns a WebKitGTK snapshot asynchronously.

No DOM screenshot emulation is needed.

## Public safe API

The crate exposes data and error types that contain no raw platform handles:

```rust
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeCaptureBackend {
    WebView2,
    WkWebView,
    WebKitGtk,
}
```

The first slice only accepts `CaptureTarget::Viewport`. Unsupported targets return an explicit `UnsupportedTarget` error; they must never silently degrade to a different capture method.

## Platform backend contract

### Windows

Use the WebView2 handle obtained from Tauri's platform WebView. Request PNG through `CapturePreview`. Wait for completion without blocking the UI thread indefinitely. Convert the resulting `IStream` to owned bytes before returning to safe code.

Requirements:

- Do not capture before WebView content is ready; surface an explicit not-ready/platform error.
- Enforce a hard timeout at the coordinator boundary.
- Validate the PNG signature before a frame is accepted.
- Release all COM resources on every path.

### macOS

Use the `WKWebView` pointer exposed through Tauri and `WKSnapshotConfiguration` covering the visible viewport. Request a native snapshot after pending screen updates and encode the resulting image as PNG.

Requirements:

- All AppKit/WebKit interaction stays on the main thread.
- Callback state must be lifetime-safe across the async completion.
- Return owned PNG bytes; no Objective-C object escapes the crate boundary.

### Linux

Use WebKitGTK snapshot APIs against the WebView handle. For the first slice capture the visible region only, convert the returned Cairo surface to PNG bytes, and return owned bytes.

Requirements:

- Run GTK/WebKit calls on the UI thread.
- Correctly own/unref GObject/Cairo resources on success and error paths.
- No fallback to browser-side canvas.

## Safety policy

- `localview-desktop`, `localview-capture`, `localview-artifacts`, `localview-evidence` and protocol crates keep `#![forbid(unsafe_code)]`.
- `localview-native-capture` uses `#![deny(unsafe_op_in_unsafe_fn)]`.
- Platform code lives in `platform/windows.rs`, `platform/macos.rs`, `platform/linux.rs`; common modules remain safe.
- Every unsafe block must have a `// SAFETY:` comment describing lifetime/thread/ownership invariants.
- No raw pointer or platform object appears in the public API.
- No `unsafe` is introduced into the desktop Tauri crate.

## Coordinator and provenance

Desktop owns the WebView window and therefore starts capture. The coordinator validates that the caller targets a LocalView-managed `preview-*` or `workspace-*` WebView whose session matches the request.

The capture result must preserve:

- LocalView session id outside the raw adapter result.
- Route.
- Viewport CSS size.
- device scale factor.
- Pixel width/height.
- Backend.
- Optional source revision.
- Capture timestamp.
- Target (`viewport` in this slice).
- Artifact id after persistence.

Pixels are stored in `localview-artifacts`; `localview-evidence` receives metadata/provenance and the artifact id, not a base64 copy of the PNG.

## Privacy and resource limits

- Capture only LocalView-managed loopback preview/workspace surfaces.
- Do not expose screenshot bytes through observer event history.
- Default per-frame encoded PNG limit: 24 MiB. Larger frames fail closed.
- No unbounded screenshot history; persistence remains governed by the artifact store capacity.
- No screenshot body is written to logs.
- Private-selector masking is a later transaction stage; until masking exists, this capture tool is local-only and must not claim masked-safe export.

## Error model

Use a typed `NativeCaptureError` with stable categories:

- `UnsupportedTarget`
- `UnsupportedPlatform`
- `NotReady`
- `Timeout`
- `Platform(String)`
- `InvalidImage`
- `FrameTooLarge { bytes, limit }`

Public callers can convert these to user-facing strings, but tests match the typed category.

## Testing strategy

### Portable tests

Run on all CI OSes and require no real GUI:

- capture request/metadata serde round-trip;
- viewport-only target enforcement;
- PNG signature validation;
- frame-size bound;
- provenance construction;
- safety-boundary contract ensuring desktop/capture crates retain `forbid(unsafe_code)` and the adapter contains no public raw pointer type.

### Platform compile gates

CI must compile the real backend on its matching runner:

- Windows backend on `windows-latest`;
- macOS backend on `macos-latest`;
- Linux backend on `ubuntu-latest` with existing Tauri/WebKitGTK system packages.

### Runtime smoke test

After compile-safe adapters exist, add one platform smoke harness that opens a local fixture in a managed WebView and verifies the returned bytes decode as PNG with non-zero dimensions. This may be gated where CI GUI availability is insufficient; a missing GUI smoke must remain documented as an integration gap rather than being called implemented.

## Delivery slices

1. Safe contract + validation + CI boundary tests.
2. Platform backend compilation through Tauri `with_webview`.
3. Desktop viewport capture command and caller/session ownership validation.
4. Artifact persistence and Visual evidence metadata.
5. MCP/CLI read path for capture metadata/artifact reference.
6. Stable-capture settle transaction.
7. Region/element capture and progressive changed-region capture.
8. Privacy masking, visual diff integration and guarded full-page stitching.

## Completion criteria for the first native-capture vertical slice

The slice is complete only when all of the following are true:

- `localview-native-capture` exists with a safe public API and audited platform modules.
- Desktop still forbids unsafe code.
- Windows/macOS/Linux platform backends compile in CI.
- A managed preview/workspace can request a viewport capture through the native adapter.
- Returned bytes are validated as PNG and bounded by size.
- Pixels can be stored through the bounded artifact store.
- Visual evidence records artifact id + route/viewport/backend/revision provenance without embedding PNG bytes.
- No browser-side canvas fallback exists.
- Coverage ledger says `Partial`, not `Implemented`, until a real native screenshot smoke is verified on supported platforms.
