# Guarded Full-Page Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded native full-page capture command that composes redacted viewport captures from the exact LocalView-managed WebView while preserving capture, privacy, restoration and retained-resource authority.

**Architecture:** Keep WebView2/WKWebView/WebKitGTK adapters viewport-only. Put deterministic planning/output geometry/row-copy stitching in `localview-visual`; add exact freeze-token `CaptureScrollTo` and `CaptureTileProbe` internal bridge actions; expose narrow authenticated control endpoints; and let the desktop coordinator own one session-gated settle -> freeze -> tile loop -> original-scroll restore -> visual restore -> final persistence/evidence transaction. Any authority, geometry, privacy, resource or restoration uncertainty fails closed.

**Tech Stack:** Rust 1.85+, Tauri 2, existing `png`/visual utilities, LocalView LiveBridge/instrumentation, Axum control plane, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`

## Global Constraints

- `max_tiles = 32`.
- `max_document_css_height = 50_000 CSS px`.
- `max_output_rgba_bytes = 128 MiB`.
- `max_output_pixel_height = 32_768 px`.
- Existing viewport/region freeze lease remains `8_000 ms`; full-page uses one internal `30_000 ms` lease and one matching desktop transaction timeout. No lease-renewal loop.
- Full-page stitching is vertical-only. Document CSS width must remain equal to the frozen viewport width within the existing geometry tolerance.
- Visible `position: fixed` or `position: sticky` content fails closed in this slice.
- Every tile gets fresh private-mask geometry after its exact scroll/settle and is redacted before entering the stitcher.
- Intermediate tile PNG/RGBA buffers are ephemeral and never persisted or registered as evidence.
- Original scroll and exact visual state must be restored before final encode/persistence/evidence registration.
- No Chromium/Playwright/Puppeteer/html2canvas/DOM reconstruction fallback.
- No caller-provided tile offsets, document dimensions, masks, backend, route or evidence verdicts.

---

### Task 1: Move the pure planner/stitcher into `localview-visual`

**Files:**
- Create: `crates/visual/src/full_page.rs`
- Modify: `crates/visual/src/lib.rs`
- Create: `crates/visual/tests/full_page_stitch_contract.rs`
- Modify: `Cargo.toml` to remove the temporary `crates/full-page` workspace member
- Delete: `crates/full-page/Cargo.toml`
- Delete: `crates/full-page/src/lib.rs`
- Delete: `crates/full-page/tests/full_page_stitching.rs`

**Interfaces:**

```rust
pub const MAX_FULL_PAGE_TILES: usize = 32;
pub const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;
pub const MAX_FULL_PAGE_OUTPUT_RGBA_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT: u32 = 32_768;

#[derive(Debug, Clone, PartialEq)]
pub struct FullPagePlan {
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub scroll_offsets_y: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullPageOutputGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba_bytes: usize,
}

pub fn plan_full_page(
    document_css: (f64, f64),
    viewport_css: (f64, f64),
    original_scroll_y: f64,
) -> Result<FullPagePlan, FullPageError>;

pub fn project_full_page_output(
    plan: &FullPagePlan,
    native_viewport: (u32, u32),
) -> Result<FullPageOutputGeometry, FullPageError>;

pub fn stitch_full_page_tile(
    output: &mut RgbaImage,
    viewport_css_height: f64,
    actual_scroll_y: f64,
    tile: &RgbaImage,
) -> Result<(), FullPageError>;
```

Do not add an unused `FullPageTile` wrapper; the direct-argument stitcher above is the canonical interface.

- [ ] **Step 1: RED planner tests.** Add tests that import the interfaces above and require non-finite/zero geometry rejection, one-tile short document, exact-divisible plan with no duplicate final offset, partial final tile at exact `max_scroll_y`, `>32` tile rejection and `>50_000` CSS-px rejection.

Representative assertion:

```rust
let plan = plan_full_page((800.0, 2500.0), (800.0, 1000.0), 250.0).unwrap();
assert_eq!(plan.scroll_offsets_y, vec![0.0, 1000.0, 1500.0]);
```

- [ ] **Step 2: Run RED.**

```bash
cargo test -p localview-visual --test full_page_stitch_contract
```

Expected: compile failure because the approved `localview_visual` full-page API does not exist yet.

- [ ] **Step 3: Minimal planner GREEN.** Implement finite/positive validation, vertical-only width equality, deterministic offsets, final exact clamp, duplicate removal and strict monotonicity. No caller-supplied offset list.

- [ ] **Step 4: RED output/stitch tests.** Require output height `round(document_css_height * scale_y)`, reject height `>32768`, reject checked RGBA bytes `>128 MiB`, support fractional scale, reject width/buffer mismatch, and prove the final overlapping tile overwrites only the rows mapped from acknowledged `actual_scroll_y`.

- [ ] **Step 5: Minimal stitcher GREEN.** Allocate only the one final `RgbaImage`; copy rows directly from one already-redacted tile and use checked arithmetic for all pixel/byte offsets.

- [ ] **Step 6: Verify Gate 1.**

```bash
cargo fmt --check
cargo test -p localview-visual --test full_page_stitch_contract
cargo test -p localview-visual
```

Commit only after the focused and crate regressions are GREEN.

---

### Task 2: Add exact internal capture action authority

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Modify: `crates/live-bridge/src/action_envelope.rs` if public action projection requires an exhaustive update
- Create: `crates/live-bridge/tests/full_page_internal_action_contract.rs`

**Interfaces:**

```rust
BridgeActionKind::CaptureScrollTo { token: Uuid, y: f64 }
BridgeActionKind::CaptureTileProbe { token: Uuid }
```

`is_internal_capture_action()` must return true for both. Public action projection/cancellation must never authorize them. `CaptureTileProbe` carries the same private selector envelope used by capture freeze so selector strings stay private while page-side geometry can be refreshed.

- [ ] **Step 1: RED tests.** Prove tagged serialization, internal classification, generic-public-action rejection, private selector envelope retention for tile probes, queue pressure isolation and public cancellation denial.
- [ ] **Step 2: Run RED.**

```bash
cargo test -p localview-live-bridge --test full_page_internal_action_contract
```

- [ ] **Step 3: Implement minimal variants/enqueue helpers** following `enqueue_capture_freeze`/capture-private queue ownership rather than the public queue.
- [ ] **Step 4: GREEN + regression.**

```bash
cargo test -p localview-live-bridge --test full_page_internal_action_contract
cargo test -p localview-live-bridge
```

---

### Task 3: Execute token-bound absolute scroll/probe in the managed WebView

**Files:**
- Modify: `crates/instrumentation/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create or extend focused tests under `crates/instrumentation/tests/` and `apps/desktop/src-tauri/tests/`

**Page-side contract:**

```text
CaptureScrollTo(token, y)
  -> exact active freeze-token check
  -> finite y >= 0
  -> absolute window.scrollTo({left: original_scroll_x, top: y, behavior: 'auto'})
  -> wait two animation frames
  -> bounded geometry receipt

CaptureTileProbe(token)
  -> exact active freeze-token check
  -> current scroll/document/viewport geometry
  -> current private mask rectangles
  -> bounded visible fixed/sticky scan
```

The shared document-geometry helper uses identical semantics during freeze and every tile probe. Scan at most 4,096 elements; budget exhaustion is an error, not a safe assumption. No selector/tag/text/style value leaves the private executor.

- [ ] **Step 1: RED tests** for wrong/expired token, absolute (not relative) scroll, two-frame acknowledgement, finite bounded geometry, fresh mask geometry, 4,096-element scan cap, visible fixed/sticky count and unchanged public relative `Scroll` behavior.
- [ ] **Step 2: Extend freeze receipt** with `scroll_x`, `scroll_y`, `document_css_width`, `document_css_height` while preserving existing viewport/region semantics.
- [ ] **Step 3: Implement executor cases** in the managed WebView action switch; no arbitrary eval/caller script endpoint.
- [ ] **Step 4: Verify instrumentation/desktop executor contracts GREEN.**

---

### Task 4: Add narrow authenticated control endpoints and dedicated evidence

**Files:**
- Modify: `crates/control/src/capture_settle.rs`
- Create: `crates/control/src/visual_full_page.rs`
- Modify the control router aggregation module
- Create: `crates/control/tests/full_page_capture_control.rs`

**Routes:**

```text
POST /v1/sessions/{id}/capture-scroll
POST /v1/sessions/{id}/capture-tile-probe
POST /v1/sessions/{id}/evidence/visual-full-page
```

Scroll/probe endpoints accept only token/scroll inputs required by the internal action. The evidence request uses `#[serde(deny_unknown_fields)]` and contains bounded provenance only: final artifact id, canonical route, backend, viewport, revision, timestamp, final pixels, document CSS dimensions, tile count and ordered actual scroll offsets.

- [ ] **Step 1: RED integration tests** for bearer auth, session existence, exact action/result id correlation, malformed/non-finite geometry, token mismatch, selector leakage, unknown evidence fields, invalid tile count/offset monotonicity/final offset and noncanonical route.
- [ ] **Step 2: Implement scroll/probe endpoints** by reusing capture-internal LiveBridge queues and the existing bounded result wait.
- [ ] **Step 3: Implement `/visual-full-page`** using existing evidence-store/session/route authority. Stored kind remains `EvidenceKind::Visual` with explicit full-page provenance; intermediate tiles never become evidence.
- [ ] **Step 4: GREEN + control regression.**

```bash
cargo test -p localview-control --test full_page_capture_control
cargo test -p localview-control
```

---

### Task 5: Implement the desktop full-page transaction

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`

**Public interface:**

```rust
#[tauri::command]
pub async fn capture_full_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<FullPageCaptureReceipt, String>;
```

Successful order is fixed:

```text
managed-surface preflight
-> session capture gate
-> settle
-> full-page freeze (30s internal lease)
-> plan
-> [CaptureScrollTo -> settle -> CaptureTileProbe -> native viewport -> redact -> decode -> stitch] * N
-> CaptureScrollTo(original_y) + verify original x/y
-> RestoreVisuals(exact token)
-> encode final image
-> retained-resource admission + one artifact persistence
-> one dedicated full-page evidence record
```

- [ ] **Step 1: RED source/behavior tests** lock the exact order above and prove the session gate spans the whole transaction.
- [ ] **Step 2: Add strict typed receipts** for freeze extension, scroll and probe; reject unknown fields, non-finite dimensions, geometry drift, fixed/sticky count, mask-budget overflow and stale action ids.
- [ ] **Step 3: Implement one bounded 30-second coordinator timeout** and a cleanup path that attempts original-scroll restoration then exact visual restore on every post-freeze failure.
- [ ] **Step 4: Implement tile loop.** Every native tile must match canonical route, viewport CSS dimensions, DSF, native dimensions and backend. Use only that tile's refreshed masks; redaction must finish before `decode_png_rgba`/stitch copy.
- [ ] **Step 5: Implement final restoration/persistence gate.** Any scroll-restore or visual-restore failure makes persistence unreachable. Intermediate frames are dropped after copy.
- [ ] **Step 6: Register final evidence only after one final artifact is persisted.** If evidence registration fails after a real artifact put, fail the command without inventing rollback; retained accounting must remain truthful.
- [ ] **Step 7: GREEN desktop tests.**

```bash
cargo test -p localview-desktop --test full_page_capture_contract
cargo test -p localview-desktop --tests
```

---

### Task 6: Exact-head regression, documentation truth and integration

**Files:**
- Modify only after Gates 1-5 are GREEN: `docs/ROADMAP.md`
- Modify only after Gates 1-5 are GREEN: `docs/IMPLEMENTATION_STATUS.md`
- Modify only after Gates 1-5 are GREEN: `docs/SPEC_COVERAGE.md`

- [ ] **Step 1: Run repository regression.**

```bash
cargo fmt --check
cargo test -p localview-visual
cargo test -p localview-live-bridge
cargo test -p localview-control
cargo test -p localview-desktop --tests
cargo check --workspace --all-targets
```

- [ ] **Step 2: Require the normal Linux/Windows/macOS CI and existing native rendered-pixel viewport proofs to remain GREEN.** No platform adapter receives a full-page API.
- [ ] **Step 3: Update docs only to proven truth:** explicit guarded full-page command, vertical viewport-width stitching, managed native tiles, 32/50k/128MiB/32768 bounds, one freeze token, per-tile mask refresh, fixed/sticky rejection, restore-before-persist, dedicated evidence, no Chromium fallback.
- [ ] **Step 4: Verify one immutable exact head** has no failing/cancelled/timed-out/queued/in-progress required checks.
- [ ] **Step 5: Audit privacy/authority:** no public selector/mask/tile/document-authority inputs, no intermediate artifact, no stale-mask reuse, no evidence before restoration.
- [ ] **Step 6: Use `superpowers:verification-before-completion`, then `superpowers:finishing-a-development-branch` before marking ready/merging.**
