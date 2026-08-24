# Native Visual Capture Design

## Status

Approved architecture for Wave 2 of LocalView. This design starts from `main` commit `482085756e7ee8bb6155d9fbe33ee7ce497d4f6e` and extends the already-live semantic/action bridge with real platform WebView pixels.

## Goal

Provide real, privacy-bounded native viewport capture for LocalView-managed WebViews on Windows, macOS and Linux without weakening the existing `#![forbid(unsafe_code)]` boundary in the desktop shell, capture planner, protocol, control, artifact or evidence crates.

The first vertical slice is intentionally narrow:

`managed WebView -> native platform snapshot -> owned PNG bytes -> bounded local artifact -> daemon Visual evidence metadata`

Element/region capture, stable-settle orchestration, progressive changed-region capture, masking, stitching and responsive contact sheets build on this slice only after viewport capture is verified.

## Non-goals for the first slice

- Full-page stitching.
- DOM/canvas reconstruction or html2canvas fallback.
- Chromium/Playwright as the default capture engine.
- Arbitrary external-page screenshotting.
- Reading cookies, storage, response bodies or other secrets.
- Making the native child-WebView workspace the default before its existing cross-platform composition gate is complete.

## Architectural boundary

Create `crates/native-capture`. It owns platform-specific WebView execution and is the only LocalView crate allowed to contain audited `unsafe` required by platform handles.

```text
localview-desktop (safe)
        |
        | WebviewWindow::with_webview(main-thread closure)
        v
localview-native-capture
  safe data/error API
  safe entrypoint accepting Tauri PlatformWebview wrapper
  +-- windows backend: WebView2 CapturePreview
  +-- macOS backend: WKWebView takeSnapshot
  +-- Linux backend: WebKitGTK get_snapshot
        |
        v
CapturedFrame { owned png, metadata }   <-- deliberately NOT Serialize
        |
        +--> localview-artifacts (desktop-owned bounded store)
        |
        +--> authenticated control endpoint
                    |
                    v
              daemon EvidenceStore
              Visual metadata only
```

`localview-capture` remains the platform-independent transaction/planning layer. `localview-native-capture` is an execution adapter, not a replacement for capture policy.

## Platform compatibility anchor

Tauri is pinned to `2.11.5`. `WebviewWindow::with_webview` runs the platform closure on the main thread and provides a safe Tauri `PlatformWebview` wrapper. Tauri notes that direct WebView2/WebKitGTK/objc2 bindings may change across minor releases, so the adapter pins matching dependency families and keeps their use inside target-specific modules.

Platform primitives used by the design:

- Windows: `ICoreWebView2::CapturePreview` -> PNG in `IStream`.
- macOS: `WKWebView::takeSnapshot` + `WKSnapshotConfiguration` -> native image -> PNG.
- Linux: WebKitGTK visible-region snapshot -> Cairo surface -> PNG.

No browser-side screenshot emulation is permitted.

## Safe contract

Serializable request/metadata types contain no raw handles:

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeCaptureBackend {
    WebView2,
    WkWebView,
    WebKitGtk,
}
```

The pixel-bearing frame is intentionally not serializable:

```rust
#[derive(Debug, PartialEq)]
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
```

The execution entrypoint may accept Tauri's safe `PlatformWebview` wrapper because desktop receives that wrapper from `with_webview`. No raw pointer, COM interface, Objective-C object, GTK object or platform FFI type may appear in request/result/error types or cross back into desktop.

The first slice accepts only `CaptureTarget::Viewport`. Any other target returns `UnsupportedTarget`; no silent fallback is allowed.

## Platform backend contract

### Windows

Use the WebView2 handle held by Tauri's platform wrapper. Request PNG with `CapturePreview`, collect the async `IStream` result into owned bytes, validate it and return it through the safe completion boundary.

Requirements:

- Capture before content readiness fails explicitly.
- The desktop coordinator enforces a hard timeout.
- PNG signature, encoded byte size and non-zero IHDR dimensions are validated.
- COM stream/callback lifetime is contained entirely within the Windows module.

### macOS

Use `WKWebView` and `WKSnapshotConfiguration` for the visible viewport with pending screen updates included. Convert the returned native image to PNG inside the macOS module.

Requirements:

- AppKit/WebKit interaction stays on the main thread.
- Callback state remains valid until completion.
- No Objective-C object escapes the adapter.
- PNG signature, size and dimensions are validated.

### Linux

Use the WebKitGTK visible-region snapshot API against the Tauri-owned `WebView`, then encode the returned Cairo surface to PNG.

Requirements:

- GTK/WebKit calls stay on the UI thread.
- GObject/Cairo ownership remains inside the platform module.
- No canvas fallback.
- PNG signature, size and dimensions are validated.

## Safety policy

- `localview-desktop`, `localview-capture`, `localview-artifacts`, `localview-evidence`, `localview-control` and protocol crates retain `#![forbid(unsafe_code)]`.
- `localview-native-capture` uses `#![deny(unsafe_op_in_unsafe_fn)]`.
- Platform implementation lives only in `src/platform/windows.rs`, `macos.rs`, `linux.rs`.
- Every explicit unsafe block must have a neighboring `// SAFETY:` explanation of thread, lifetime and ownership invariants.
- Crate root/common modules expose no raw pointers or platform FFI objects.
- No unsafe code is introduced into the Tauri desktop crate.

## Desktop coordinator and trusted provenance

Desktop owns the managed WebView and starts capture. It resolves only the expected `preview-{session}` or `workspace-{session}` surface and reuses the existing label/session ownership policy.

Provenance rules:

- Route is read from the resolved WebView itself; a caller-supplied route is never trusted.
- Session id is the command/session identity already checked against the surface label.
- Viewport metadata is provided by the LocalView surface state for the first slice and is recorded alongside pixel dimensions; later work can add stricter native viewport cross-checking.
- Optional revision is carried from project state when available.
- Backend and capture timestamp come from the adapter result.
- Artifact id comes from bounded local persistence.

The command receipt exposes artifact id and metadata only; it never returns filesystem path or PNG bytes.

## Artifact and evidence ownership

Desktop keeps a long-lived `VisualCaptureState` managed by Tauri. It lazily opens one `ArtifactStore` under `state_dir()/artifacts/visual` with a first-slice capacity of 256 MiB. Reusing the store instance is required so in-memory accounting/LRU remains meaningful across captures.

After storing `visual/png`, desktop posts a narrow `VisualEvidenceRequest` to the authenticated daemon control plane. The control route verifies the session exists and constructs `EvidenceDraft { kind: Visual, ... }` itself. The daemon remains the single owner of `EvidenceStore`, so visual evidence becomes visible to existing `/evidence/recent`, verification and MCP paths without duplicating evidence state in desktop.

The Visual evidence payload contains only:

- artifact id;
- pixel width/height;
- backend;
- route;
- viewport metadata;
- revision;
- target `viewport`;
- capture timestamp.

It never contains raw PNG or base64.

## Privacy and resource limits

- Capture only LocalView-managed loopback preview/workspace surfaces.
- Do not expose screenshot bytes through observer/action histories.
- Encoded PNG limit is exactly 24 MiB (`25_165_824` bytes); larger frames fail closed.
- Artifact retention is bounded to 256 MiB for this first desktop visual store.
- No screenshot body is logged.
- Private-selector masking is a later capture-transaction stage. Until masking exists, the feature remains local-only and must not claim masked-safe export.

## Error model

`NativeCaptureError` has stable categories:

- `UnsupportedTarget`
- `UnsupportedPlatform`
- `NotReady`
- `Timeout`
- `Platform(String)`
- `InvalidImage`
- `FrameTooLarge { bytes, limit }`

Callers convert these to UI strings only at the outer boundary; tests match typed categories.

## Testing strategy

### Portable tests

All CI OSes verify:

- request/metadata serde round-trip;
- `CapturedFrame` is kept out of serialization paths by design and contract tests inspect evidence payloads instead;
- viewport-only target enforcement;
- PNG signature/IHDR parsing;
- 24 MiB limit;
- desktop/capture/control/evidence/artifact crates retain unsafe prohibition;
- native-capture common/public files expose no raw pointer declarations.

### Platform compile gates

- Windows backend on `windows-latest`.
- macOS backend on `macos-latest`.
- Linux backend on Ubuntu with WebKitGTK development packages installed before the native-capture workspace check.

### Runtime smoke

After compile-safe adapters exist, add a managed local fixture smoke that verifies returned bytes decode as PNG with non-zero dimensions. If a hosted CI runner cannot provide the required GUI session, that is recorded as an integration gap rather than relabeled as completion.

## Delivery slices

1. Safe contract + PNG/dimension/size validation.
2. Audited platform adapter boundary and compile gates.
3. Windows/macOS/Linux native viewport implementations.
4. Desktop managed-surface coordinator + bounded artifact store.
5. Authenticated daemon Visual evidence ingestion.
6. MCP/CLI metadata/artifact-reference read surface.
7. Stable-settle transaction.
8. Element/region + progressive changed-region capture.
9. Masking + visual diff + guarded full-page stitching.

## Completion criteria for the first native-capture vertical slice

The slice is complete only when:

- `localview-native-capture` has safe common types and audited platform modules.
- Desktop/control/capture/artifact/evidence crates still forbid unsafe.
- Windows/macOS/Linux backend code compiles on matching CI runners.
- Managed preview/workspace capture goes through the native adapter only.
- PNG bytes pass signature, dimension and size validation.
- Pixels are retained through one bounded desktop artifact store.
- Visual evidence is inserted into the daemon's existing EvidenceStore using artifact id + provenance only.
- No canvas/browser reconstruction exists.
- Coverage ledger remains `Partial` until real native screenshot smoke evidence exists on supported platforms.
