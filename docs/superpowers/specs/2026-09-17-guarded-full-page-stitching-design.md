# Guarded Full-Page Stitching Design

## Status

Approved under the standing Wave 2 LocalView roadmap direction. This slice starts from `main` merge commit `70ebd3ff54a5e8b038445ccd6651944d4be53c1c`, after Linux L05 accessibility-bus reconnect closure landed and the post-merge campaign completed successfully.

This design intentionally chooses correctness, boundedness and provenance over broad page compatibility. A page that cannot prove a coherent native full-page transaction is rejected rather than captured approximately.

## Goal

Add a native, guarded full-page visual capture path that composes multiple viewport captures from the exact LocalView-managed WebView without introducing a Chromium/Playwright default, DOM/canvas reconstruction, arbitrary script execution, or unbounded memory/storage use.

A successful full-page capture must prove all of the following:

1. the exact managed surface and loopback route remain authoritative;
2. one per-session capture gate owns the entire transaction;
3. visual motion is frozen under one exact freeze token;
4. every tile comes from the same route, viewport, device-scale context and document geometry;
5. every scroll is internal, token-bound, absolute and acknowledged;
6. private-mask geometry is refreshed after every scroll and applied before tile pixels reach the stitcher;
7. visible `position: fixed` / `position: sticky` content is rejected in the first guarded slice rather than duplicated or heuristically rewritten;
8. tile count, decoded RGBA memory, output dimensions, encoded artifact size and transaction duration remain bounded;
9. the original scroll position and visual state are restored before any final artifact/evidence is persisted;
10. intermediate tile PNG/RGBA buffers are ephemeral and never registered as independent artifacts/evidence;
11. final evidence records exact full-page transaction provenance rather than pretending to be a normal viewport capture.

If any invariant fails, the transaction fails closed and no final full-page artifact is registered.

## Architectural choice

The full-page path is a **desktop-owned native capture transaction** composed from the existing managed-surface/native viewport authority plus two new narrow internal capture actions.

It is not implemented as:

- `Page.captureScreenshot` through Chromium;
- Playwright/Puppeteer full-page capture;
- DOM serialization + canvas/html2canvas reconstruction;
- a loop of public `Scroll` actions;
- repeated independent freeze/capture/restore cycles;
- a caller-supplied list of scroll offsets or document dimensions.

The selected path is:

```text
capture_full_page
  -> validate caller viewport + exact managed surface
  -> acquire existing per-session capture gate
  -> stable-settle
  -> internal FreezeVisuals + private selectors
       -> freeze token
       -> original scroll
       -> document geometry
       -> viewport geometry
  -> validate bounded full-page plan
  -> for each planned absolute scroll Y
       -> internal CaptureScrollTo { token, y }
       -> stable-settle at the new viewport
       -> internal CaptureTileProbe { token }
            -> exact scroll position
            -> exact document/viewport geometry
            -> refreshed private mask rectangles
            -> guarded positional-content scan
       -> native viewport capture through the existing platform adapter
       -> validate native route/viewport/DSF/pixel geometry
       -> redact this tile in memory using this tile's refreshed masks
       -> decode into bounded RGBA and stitch into the in-memory output
       -> discard tile PNG/RGBA as soon as copied
  -> internal CaptureScrollTo { token, original_y }
  -> verify original scroll restoration
  -> RestoreVisuals { token }
  -> encode final stitched RGBA
  -> retained-resource admission
  -> persist one final artifact
  -> register dedicated full-page Visual evidence
```

This keeps the platform adapters viewport-only. WebView2, WKWebView and WebKitGTK continue to expose exactly one native viewport acquisition primitive; full-page composition is a higher-level Rust transaction.

## Authority model

### Desktop owns orchestration

`apps/desktop/src-tauri/src/visual_capture.rs` remains the transaction coordinator. It owns:

- the per-session capture gate;
- stable-settle sequencing;
- freeze token lifetime;
- tile planning;
- internal scroll/probe sequencing;
- native capture invocation;
- redaction-before-stitch ordering;
- final restore ordering;
- retained-resource admission;
- final artifact/evidence registration.

No daemon HTTP caller can submit a tile plan, document height, scroll offset sequence, mask rectangles, positional-content result or stitched pixel metadata as trusted authority.

### Control/LiveBridge owns internal page actions

`BridgeActionKind` gains exactly two internal capture actions:

```rust
CaptureScrollTo { token: Uuid, y: f64 },
CaptureTileProbe { token: Uuid },
```

`BridgeActionKind::is_internal_capture_action()` must classify both as internal alongside `FreezeVisuals` and `RestoreVisuals`.

The generic public action endpoint must reject them. They are available only through dedicated capture-control endpoints used by the desktop transaction.

The existing public action:

```rust
Scroll { x: f64, y: f64 }
```

remains a relative agent/user interaction and must not be reused for stitching. Full-page capture requires absolute, token-owned scroll authority.

### Page instrumentation owns token-bound scroll/probe truth

The managed WebView executor must execute full-page scroll/probe operations only when the supplied token matches the currently active LocalView visual-freeze token.

A missing, expired, auto-restored or mismatched token fails immediately.

The page never returns selector strings. Private selectors remain inside the private capture envelope and are used only to derive bounded geometry.

## Freeze receipt extension

`FreezeVisualStateReceipt` is extended for full-page planning with bounded numeric metadata:

```rust
struct FreezeVisualStateReceipt {
    token: String,
    paused_animations: u64,
    web_animations_supported: bool,
    viewport_css_width: f64,
    viewport_css_height: f64,
    masked_elements: u64,
    mask_rects: Vec<Rect>,
    lease_ms: u64,
    scroll_x: f64,
    scroll_y: f64,
    document_css_width: f64,
    document_css_height: f64,
}
```

The freeze implementation records the original scroll coordinates before any full-page scrolling begins.

The document dimensions are measured from a deterministic max of the standard layout roots needed to represent the scrollable document, bounded by the safety limits below. The exact page-side helper used for this measurement is shared with `CaptureTileProbe` so start/probe comparisons use identical semantics.

The existing viewport/region capture paths may ignore the new fields; adding them must not change their pixel semantics.

## Internal scroll contract

`CaptureScrollTo { token, y }` performs an **absolute vertical scroll** while keeping horizontal scroll at the original frozen `scroll_x`.

Page-side behavior:

1. verify the active freeze token exactly matches `token`;
2. reject non-finite `y` or `y < 0`;
3. read current bounded document/viewport geometry;
4. compute `max_scroll_y = max(document_css_height - viewport_css_height, 0)`;
5. reject a requested `y` greater than `max_scroll_y` by more than the allowed scroll quantization tolerance;
6. call an absolute auto-behavior scroll operation;
7. wait for two animation frames so layout/scroll state reaches the browser's observable position under the existing freeze stylesheet;
8. return bounded metadata only.

Receipt:

```rust
struct CaptureScrollReceipt {
    requested_y: f64,
    actual_x: f64,
    actual_y: f64,
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
}
```

The desktop accepts the scroll only when:

- `actual_x` matches the original frozen horizontal scroll within one native-pixel-equivalent CSS tolerance;
- `actual_y` matches the requested absolute offset within the same tolerance, except the final planned offset may equal the exact browser-clamped `max_scroll_y`;
- document and viewport geometry remain equal to the frozen geometry;
- all values are finite and positive where required.

The scroll receipt contains no DOM content, selectors, text, URLs, storage values or pixels.

## Tile probe contract

After the scroll acknowledgement and a fresh stable-settle pass, desktop requests `CaptureTileProbe { token }`.

The page-side probe:

1. verifies the exact active freeze token;
2. re-reads current scroll/document/viewport geometry;
3. recomputes private-mask geometry from the private selector envelope for the **current viewport**;
4. performs the guarded positional-content scan described below;
5. returns bounded metadata only.

Receipt:

```rust
struct CaptureTileProbeReceipt {
    scroll_x: f64,
    scroll_y: f64,
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
    masked_elements: u64,
    mask_rects: Vec<Rect>,
    positional_elements_scanned: u64,
    visible_fixed_or_sticky: u64,
}
```

The probe does **not** expose element identifiers, selectors, tag names, text or style values.

The desktop validates that the probe scroll/geometry is still identical to the preceding scroll receipt/frozen context before native acquisition.

## Why mask geometry must refresh per tile

The current private-redaction contract resolves selectors to viewport-relative geometry during freeze. That geometry is valid only for the viewport position at which it was measured.

Reusing the first tile's mask rectangles after scrolling could expose private pixels in later tiles or mask unrelated pixels.

Therefore full-page stitching must never reuse the initial freeze mask receipt for later tiles. Every tile receives a fresh bounded mask geometry probe after its scroll and settle. Redaction occurs before decoded tile pixels are copied into the stitched output.

The invariant is:

```text
scroll -> settle -> current mask probe -> native capture -> redact tile -> stitch
```

Never:

```text
freeze mask once -> scroll many times -> stitch raw tiles -> redact final image
```

Final-image-only redaction is explicitly prohibited because private selectors may occupy different viewport-relative rectangles at different scroll positions.

## Guarded fixed/sticky policy

The first full-page slice does not attempt heuristic de-duplication, temporary CSS rewriting, or special handling of viewport-pinned UI.

A stitched page containing visible `position: fixed` or `position: sticky` elements can otherwise duplicate those elements in multiple tiles or create inconsistent overlap.

Therefore each tile probe performs a bounded positional-content scan and the transaction is rejected when `visible_fixed_or_sticky > 0`.

The scan rules are deliberately conservative:

- inspect at most `4_096` elements;
- if the document exceeds that bounded scan authority, return a scan-budget error rather than assume safety;
- ignore elements with no visible box in the current viewport;
- classify only computed `position: fixed` or `position: sticky`;
- do not return element identity/content to desktop;
- any visible fixed/sticky element causes fail-closed rejection.

This limits initial compatibility, but it prevents visually false evidence. A later independently designed slice may add a formally tested fixed/sticky normalization strategy; this design does not pre-authorize one.

## Stable-settle semantics during stitching

The transaction runs the existing stable-settle gate before initial freeze.

After each `CaptureScrollTo`, the desktop runs stable-settle again before the tile probe/native capture. This is necessary because scrolling can trigger:

- lazy image loading;
- intersection-observer rendering;
- virtualized list materialization;
- route-local fetch/XHR work;
- DOM/layout mutations.

The full-page transaction does not chase an expanding document. If document width or height differs from the frozen geometry after any scroll/settle/probe, the entire transaction fails and discards all accumulated pixels.

This rule prefers a coherent snapshot of a finite document over an unbounded "scroll until no growth" loop.

## Tile planning

Tile planning is pure deterministic Rust and belongs in `localview-visual` rather than page JavaScript.

New types:

```rust
pub struct FullPagePlan {
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub scroll_offsets_y: Vec<f64>,
}

pub struct FullPagePolicy {
    pub max_tiles: usize,
    pub max_document_css_height: f64,
    pub max_output_rgba_bytes: usize,
    pub max_output_pixel_height: u32,
}
```

Initial policy constants:

```text
max_tiles = 32
max_document_css_height = 50_000 CSS px
max_output_rgba_bytes = 128 MiB
max_output_pixel_height = 32_768 px
```

The existing `MAX_CSS_VIEWPORT_DIMENSION = 100_000` remains an outer geometry sanity bound; the full-page policy is intentionally stricter.

Planner rules:

1. all dimensions finite and positive;
2. document width must equal viewport width within the existing viewport authority; horizontal full-page stitching is not introduced here;
3. if document height <= viewport height, plan exactly one tile at `y = original_scroll_y` only when original scroll can represent the full document; otherwise normalize through the full-page transaction to `y = 0` and restore afterward;
4. multi-tile plans start at `y = 0`;
5. intermediate offsets advance by one viewport CSS height;
6. the final offset is exactly `max(document_height - viewport_height, 0)` so the document bottom is captured;
7. duplicate offsets caused by exact divisibility or final clamping are removed;
8. offsets must be strictly increasing after de-duplication;
9. tile count must not exceed `max_tiles`.

The planner does not accept a caller-provided offset list.

## Native pixel geometry and fractional scale

The first successfully captured tile establishes native pixel geometry for the transaction:

```text
pixel_width
pixel_height
scale_x = pixel_width / viewport_css_width
scale_y = pixel_height / viewport_css_height
```

All subsequent tiles must match:

- native pixel width;
- native pixel height;
- viewport CSS width/height;
- device-scale factor;
- backend;
- canonical loopback route.

The stitched output pixel height is derived from the frozen document height and measured native vertical scale, then bounded by `max_output_pixel_height` and `max_output_rgba_bytes` before allocation.

Tile placement uses the **acknowledged actual scroll Y**, not the requested offset:

```text
top_px = round(actual_scroll_y * scale_y)
```

The tile writes into `[top_px, min(top_px + tile_pixel_height, output_height))`.

Intentional overlap on the final clamped tile is allowed. Later tile rows overwrite the exact overlapping output rows. Because motion is frozen, geometry is unchanged and visible fixed/sticky content is rejected, overlapping rows should represent the same document pixels modulo native capture rasterization.

A scroll acknowledgement is accepted only within a tolerance of one native pixel expressed in CSS units:

```text
scroll_tolerance_css = max(0.25, 1.0 / scale_y)
```

The implementation must not require integer CSS offsets or integer device-scale factors.

## Stitcher

`localview-visual` gains a pure bounded stitcher that never performs I/O or page control.

Suggested interface:

```rust
pub struct FullPageTile<'a> {
    pub actual_scroll_y: f64,
    pub image: &'a RgbaImage,
}

pub fn stitch_full_page_tile(
    output: &mut RgbaImage,
    viewport_css_height: f64,
    actual_scroll_y: f64,
    tile: &RgbaImage,
) -> Result<(), FullPageStitchError>;
```

The desktop allocates the output only after the first tile proves native pixel geometry and the projected RGBA size passes policy.

The stitcher validates:

- matching width;
- finite non-negative scroll position;
- valid output/tile image buffers;
- computed placement inside the output;
- no integer overflow in row/byte offsets.

The stitcher copies rows directly into the preallocated output and does not retain a vector of all decoded tiles.

Each tile PNG is decoded, redacted, copied, then dropped. This keeps peak memory bounded to approximately:

```text
stitched output RGBA
+ one viewport tile RGBA
+ one viewport PNG
+ encoder working memory
```

rather than `N * viewport` decoded tiles.

## Native capture transaction per tile

The platform-specific capture layer remains unchanged:

```rust
capture_webview(webview, CaptureRequest { ... })
```

Each tile uses the exact existing native viewport capture backend:

- WebView2 `CapturePreview` on Windows;
- WKWebView snapshot on macOS;
- WebKitGTK visible snapshot on Linux.

No platform adapter receives a scroll offset or full-page target.

For every tile, desktop must verify the returned `CapturedFrame` still matches the transaction's:

- canonical route;
- viewport CSS dimensions;
- device-scale factor;
- native pixel dimensions;
- backend;
- revision semantics already used by current capture receipts.

A mismatch discards the entire full-page transaction.

## Restore ordering

Full-page capture temporarily moves the page. Artifact success must imply the user's managed surface was restored.

After the final tile is stitched, desktop must:

1. issue `CaptureScrollTo { token, y: original_scroll_y }`;
2. require acknowledged `actual_x/actual_y` to match the original frozen scroll position within tolerance;
3. issue existing `RestoreVisuals { token }`;
4. require exact restore acknowledgement;
5. only then encode/persist/register the final stitched image.

If final scroll restoration fails, freeze auto-restores later but the command still fails and all stitched pixels are discarded.

If visual restore fails, the command fails and all stitched pixels are discarded.

If encoding/persistence/evidence registration fails after successful page restoration, normal artifact/evidence error handling applies; page state has already been restored.

## Freeze lease duration

The existing 8-second freeze lease is safe for a single viewport capture but can be too short for a bounded 32-tile transaction with per-tile settle/capture work.

This slice therefore makes freeze lease duration an internal bounded capture policy selected by the dedicated freeze endpoint, not a caller-controlled value.

Two modes are defined:

```text
viewport/region lease: 8_000 ms (unchanged)
full-page lease: 30_000 ms maximum
```

The page still self-restores automatically on lease expiry.

The desktop does not continuously renew the lease. A transaction that cannot complete within the single 30-second full-page lease fails closed. This prevents an indefinitely frozen managed surface.

The full-page coordinator also wraps the entire transaction in a 30-second desktop timeout aligned with the page lease.

## Resource bounds

Full-page capture must pass all of these before final persistence:

- `max_tiles = 32`;
- `document_css_height <= 50_000`;
- `output_pixel_height <= 32_768`;
- `output_rgba_bytes <= 128 MiB` using checked arithmetic;
- each native viewport frame remains under the existing native frame contract;
- final encoded PNG is admitted by the existing owner-local `ArtifactStore` retained-resource authority before mutation;
- no caller-writable retained-resource counter is introduced.

The full-page transaction reuses the existing per-session capture gate so it cannot interleave with viewport/region/changed-region/visual-packet work for the same session.

This slice does not add full-page capture as an automatic planner action. It is an explicit expensive visual operation and therefore does not silently multiply normal Active Perception cost.

## Public desktop command

Add a Tauri command:

```rust
#[tauri::command]
pub async fn capture_full_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<FullPageCaptureReceipt, String>
```

It accepts no document dimensions, tile count, scroll offsets, masks, route, backend or pixel dimensions from the caller.

Receipt:

```rust
pub struct FullPageCaptureReceipt {
    pub artifact_id: String,
    pub evidence_id: String,
    pub deduplicated: bool,
    pub backend: String,
    pub route: String,
    pub viewport: ViewportMeta,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub revision: Option<String>,
    pub captured_at_unix_ms: u64,
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub tile_count: usize,
}
```

No tile PNG bytes, filesystem paths, freeze token, private selectors, mask rectangles or internal scroll receipts are exposed.

## Dedicated full-page evidence schema

Do not overload ordinary viewport Visual evidence with `target = "full_page"` while omitting stitching provenance.

Add an authenticated dedicated endpoint:

```text
POST /v1/sessions/{id}/evidence/visual-full-page
```

Desktop sends bounded metadata only after page restoration and final artifact persistence:

```rust
struct FullPageVisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    document_css_width: f64,
    document_css_height: f64,
    tile_count: usize,
    scroll_offsets_y: Vec<f64>,
}
```

Control validates:

- existing session;
- canonical loopback route policy used by current Visual evidence;
- positive bounded dimensions;
- finite document dimensions;
- `1 <= tile_count <= 32`;
- `scroll_offsets_y.len() == tile_count`;
- finite, non-negative, strictly increasing offsets after any single-tile special case;
- first multi-tile offset is zero;
- final offset is consistent with bounded document/viewport geometry within the same scroll tolerance model;
- no arbitrary extra fields (`deny_unknown_fields`);
- artifact/session provenance is retained through the existing evidence store.

Stored evidence remains `EvidenceKind::Visual` with an explicit region/target marker such as `full_page`, plus structured payload provenance.

Intermediate tiles never become evidence records.

## Privacy and security boundaries

- Desktop remains `#![forbid(unsafe_code)]`.
- No arbitrary WebView `eval` endpoint is added.
- Public agent actions cannot invoke freeze, capture-scroll or tile-probe actions.
- Private selector strings never enter action-result/evidence stores.
- Mask geometry is refreshed per tile and redaction happens before stitching.
- Final full-page evidence contains only bounded geometry/provenance, not DOM text/content.
- Route canonicalization removes query/fragment according to the existing visual-evidence privacy rule.
- Native platform adapters remain viewport-only.
- Full-page capture never opens a new external URL or captures arbitrary windows.
- Full-page capture never creates a permanent Chromium process.
- Any uncertainty in ownership, geometry, masking, positional content, restoration or resource bounds causes fail-closed termination.

## Error taxonomy

User-visible command errors stay bounded and content-free. The implementation should preserve distinct internal/testable classes such as:

```text
full_page_invalid_geometry
full_page_document_too_tall
full_page_tile_budget_exceeded
full_page_output_memory_budget_exceeded
full_page_output_pixel_height_exceeded
full_page_scroll_token_mismatch
full_page_scroll_mismatch
full_page_document_geometry_drift
full_page_viewport_geometry_drift
full_page_route_drift
full_page_native_geometry_drift
full_page_private_mask_budget_exceeded
full_page_positional_scan_budget_exceeded
full_page_fixed_or_sticky_unsupported
full_page_tile_redaction_failed
full_page_tile_decode_failed
full_page_stitch_failed
full_page_scroll_restore_failed
full_page_visual_restore_failed
full_page_transaction_timeout
```

Errors must not include selector text, page text, response bodies, filesystem paths or captured pixels.

## Failure cleanup

The coordinator uses one cleanup path with these priorities:

1. if a freeze token was acquired, attempt original-scroll restoration while the token is still valid;
2. attempt exact visual restore;
3. drop all tile/final pixel buffers;
4. return the primary bounded transaction error, augmented only with a bounded restoration-failure class if cleanup also failed.

No artifact/evidence registration occurs unless original scroll and visual state were successfully restored.

The page-side lease remains the final safety net if desktop/control disappears entirely.

## Explicit non-goals

This slice does not implement:

- Chromium/Playwright full-page screenshot fallback;
- horizontal document stitching;
- fixed/sticky normalization or de-duplication;
- infinite-scroll crawling;
- document-height growth chasing;
- video/canvas/WebGL deterministic frame locking beyond the existing freeze guarantees;
- cross-session or multi-window stitching;
- responsive viewport sweeps/contact sheets;
- planner-selected automatic full-page capture;
- public caller control over scroll offsets, tile count, lease duration or masking;
- persisting individual tile artifacts;
- full-page changed-region baselines.

Responsive sweeps/contact sheets remain a separate roadmap slice built later on top of the same bounded capture authority.

## File boundaries

The implementation should preserve focused ownership:

- `crates/visual/src/full_page.rs`
  - pure policy, plan validation, output-size projection and row-copy stitcher;
- `crates/visual/tests/full_page_stitch_contract.rs`
  - deterministic pure planner/stitch tests including fractional scale and final overlap;
- `crates/live-bridge/src/lib.rs`
  - two new internal capture action variants and internal-action classification;
- `crates/live-bridge/tests/full_page_internal_action_contract.rs`
  - public/internal separation and bounded private capture envelope behavior;
- `crates/control/src/capture_settle.rs`
  - narrow authenticated internal capture-scroll/probe endpoints using exact action-result correlation;
- `crates/control/src/visual_full_page.rs`
  - dedicated full-page evidence ingestion/validation;
- `crates/control/tests/full_page_capture_control.rs`
  - endpoint auth/session/action-result/validation tests;
- `apps/desktop/src-tauri/src/lib.rs`
  - managed WebView executor cases for token-bound absolute scroll and bounded probe;
- `apps/desktop/src-tauri/src/visual_capture.rs`
  - full-page transaction orchestration and final receipt;
- `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`
  - ordering/fail-closed/source-contract coverage;
- `docs/ROADMAP.md`, `docs/IMPLEMENTATION_STATUS.md`, `docs/SPEC_COVERAGE.md`
  - update only after executable closure is proven.

If `visual_capture.rs` becomes materially harder to reason about during implementation, the full-page coordinator may be split into `visual_full_page.rs`, but unrelated capture code must not be refactored merely for style.

## TDD closure sequence

Implementation must proceed RED -> GREEN in this order so the riskiest authority boundaries close before orchestration claims:

### Gate 1 — Pure planner/stitcher

RED tests require:

- invalid/non-finite/zero dimensions reject;
- one-tile short document plan;
- exact-divisible multi-tile plan without duplicate final offset;
- partial final tile uses exact `max_scroll_y`;
- >32 tiles reject;
- >50,000 CSS-pixel document rejects;
- checked projected output bytes reject >128 MiB;
- output pixel height >32,768 rejects;
- fractional scale placement is deterministic;
- final overlapping tile overwrites only the computed overlap;
- width/buffer/overflow mismatch rejects.

### Gate 2 — Internal capture action authority

RED tests require:

- `CaptureScrollTo` and `CaptureTileProbe` serialize with expected tagged forms;
- both are classified internal;
- generic public action API rejects both;
- private mask selectors remain available only in the private capture envelope;
- action/result queues stay bounded.

### Gate 3 — Page executor behavior

RED tests require source/runtime contract for:

- exact token match before scroll/probe;
- absolute auto scroll, not relative `scrollBy`;
- two-frame acknowledgement after scroll;
- finite bounded document/viewport geometry;
- fresh private-mask geometry in every tile probe;
- bounded 4,096-element positional scan;
- visible fixed/sticky count without returning element content;
- no arbitrary eval or caller script;
- original public relative scroll behavior remains unchanged.

### Gate 4 — Control endpoints

RED integration tests require:

- auth + session existence;
- exact internal action enqueue/result correlation;
- mismatched action/result ids fail;
- token/geometry payload validation;
- private selectors/results remain sanitized;
- dedicated full-page evidence rejects malformed tile counts/offsets/dimensions/routes/unknown fields.

### Gate 5 — Desktop transaction

RED tests require the exact order:

```text
session gate
-> settle
-> freeze
-> plan
-> [scroll -> settle -> probe -> native -> redact -> stitch] * N
-> restore original scroll
-> restore visuals
-> encode
-> artifact admission/persistence
-> full-page evidence
```

Additional RED cases:

- route drift after any tile aborts;
- document geometry growth/shrink aborts;
- viewport/DSF/native dimension drift aborts;
- fixed/sticky detection aborts before stitching that tile;
- stale first-tile masks are never reused;
- redaction failure prevents tile copy;
- tile decode failure prevents persistence;
- final scroll restore failure prevents persistence;
- visual restore failure prevents persistence;
- transaction timeout prevents persistence;
- intermediate tile artifacts are never created;
- session gate spans the complete transaction;
- final evidence is registered only after artifact persistence and successful page restoration.

### Gate 6 — Regression and cross-platform compilation

Required regression commands include, at minimum:

```text
cargo fmt --check
cargo test -p localview-visual
cargo test -p localview-live-bridge
cargo test -p localview-control
cargo test -p localview-desktop --tests
cargo check --workspace --all-targets
```

The repository's normal Linux/Windows/macOS CI must remain green.

Existing hosted native viewport pixel smokes continue proving platform adapter truth. This slice does not claim a new hosted full-page rendered-pixel oracle until a deterministic cross-platform page fixture and transaction harness are actually landed and green.

## Documentation truth after closure

Only after exact-head executable tests/CI are green may docs move `guarded full-page stitching` out of the remaining Wave 2 list.

The update must say precisely what landed:

- explicit full-page command;
- managed-surface-only native viewport tiles;
- one freeze token across the transaction;
- absolute internal scroll authority;
- per-tile settle + private-mask refresh;
- fixed/sticky fail-closed policy;
- bounded pure Rust stitcher;
- original-scroll + visual restore before persistence;
- dedicated final full-page evidence;
- no Chromium default/fallback.

It must not claim:

- arbitrary infinite pages;
- fixed/sticky support;
- responsive sweep/contact-sheet completion;
- planner-autonomous full-page capture;
- generic Linux/Windows/macOS visual fidelity beyond the exact platform evidence already present.

## Acceptance criteria

This design is implemented only when one exact branch head proves all of the following:

1. pure planner/stitcher tests are green;
2. internal action authority tests are green;
3. page executor and control endpoint tests are green;
4. desktop orchestration fail-closed tests are green;
5. normal workspace/cross-platform CI is green;
6. no existing viewport/region/changed-region/visual-packet capture semantics regress;
7. full-page artifact/evidence cannot be created before exact original-scroll + visual restore acknowledgement;
8. every tile is redacted with geometry refreshed for that exact scroll position before stitching;
9. fixed/sticky pages are rejected rather than misrepresented;
10. no intermediate tile artifact/evidence is retained;
11. final docs are updated only after the executable evidence above exists.
