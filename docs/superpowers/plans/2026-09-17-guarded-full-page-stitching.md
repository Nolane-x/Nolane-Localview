# Guarded Full-Page Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement bounded native full-page screenshot stitching over the exact LocalView-managed WebView while preserving per-tile privacy redaction, exact page restoration, owner-local resource limits, and dedicated evidence provenance.

**Architecture:** The desktop owns one serialized full-page transaction. The control/live-bridge path supplies token-bound internal scroll/probe authority, instrumentation owns the active visual-freeze lease and document truth, and `localview-visual` owns pure deterministic planning/output projection/stitching. Native WebView platform adapters remain viewport-only. No final artifact/evidence is created until original scroll and visual state are restored successfully.

**Tech Stack:** Rust workspace, Tokio, Axum, Serde/serde_json, Tauri 2, LocalView LiveBridge, LocalView instrumentation JavaScript embedded from Rust, native WebView capture adapters, custom `localview_visual::RgbaImage`, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`

## Global Constraints

- Start implementation from `main@91f1ddb205fabadad589b9b3ad3bc7d3654ec761` on `feat/guarded-full-page-stitching-impl`.
- Desktop and Rust capture code remain `#![forbid(unsafe_code)]`.
- Native platform adapters remain viewport-only; do not add Chromium/Playwright, DOM reconstruction, canvas reconstruction, or arbitrary `eval()` fallback.
- One per-session capture gate owns the complete full-page transaction.
- One exact freeze token owns all full-page scroll/probe work; an expired, auto-restored, or mismatched token fails closed.
- Viewport/region freeze lease stays exactly `8_000 ms`; full-page freeze lease is server-selected and capped at exactly `30_000 ms`, never caller-controlled.
- Full-page bounds are exactly: `max_tiles = 32`, `max_document_css_height = 50_000.0`, `max_output_rgba_bytes = 128 * 1024 * 1024`, `max_output_pixel_height = 32_768`.
- Positional-content scan authority is at most `4_096` elements. A 4,097th candidate fails closed; visible `position: fixed` or `position: sticky` content fails closed.
- Every tile follows `absolute scroll -> settle -> fresh private-mask probe -> native capture -> redact -> decode -> stitch`; never reuse first-tile mask geometry.
- Route, document geometry, viewport geometry, backend, DSF, native pixel dimensions, and revision semantics must remain coherent across all tiles.
- Original scroll and exact visual state restore must succeed before final PNG encoding, artifact mutation, or evidence registration.
- Intermediate tile PNG/RGBA buffers are ephemeral and never become artifacts/evidence.
- Full-page evidence uses a dedicated endpoint and records bounded transaction provenance.
- User-visible failures remain bounded/content-free and never include selector strings, page text, response bodies, filesystem paths, or captured pixels.
- Documentation truth is updated only after exact-head executable closure and normal cross-platform CI are green.

---

## File map

- Create `crates/visual/src/full_page.rs`: full-page policy, deterministic scroll plan, output projection, and row-copy stitcher.
- Modify `crates/visual/src/lib.rs`: export the full-page API and errors without changing existing viewport/region behavior.
- Create `crates/visual/tests/full_page_stitch_contract.rs`: pure planner/projection/stitch tests.
- Modify `crates/live-bridge/src/lib.rs`: new internal actions, private capture lease/mask envelope, exact result sanitizer.
- Modify `crates/live-bridge/src/action_envelope.rs`: treat the new actions as internal-only/non-canonical public actions.
- Create `crates/live-bridge/tests/full_page_internal_action_contract.rs`: internal/public separation, private envelope, bounded sanitization.
- Modify `crates/instrumentation/src/lib.rs`: active-lease document geometry, exact-token absolute scroll, tile probe, bounded positional scan, 8s/30s lease allowlist.
- Modify `apps/desktop/src-tauri/src/lib.rs`: execute internal full-page actions through the managed WebView bridge and register `capture_full_page` in Tauri.
- Create `apps/desktop/src-tauri/tests/full_page_page_executor_contract.rs`: embedded-page executor contract tests.
- Modify `crates/control/src/capture_settle.rs`: dedicated full-page freeze, capture-scroll, and tile-probe endpoints with exact action-result correlation.
- Modify `crates/control/src/runtime.rs`: keep all new internal actions rejected by the public action API and exhaustive matches content-free.
- Create `crates/control/src/visual_full_page.rs`: dedicated full-page visual evidence endpoint/validation.
- Modify `crates/control/src/lib.rs`: mount `visual_full_page` router.
- Create `crates/control/tests/full_page_capture_control.rs`: endpoint/evidence integration tests.
- Modify `apps/desktop/src-tauri/src/visual_capture.rs`: full-page orchestration, cleanup, output persistence, final receipt/evidence.
- Create `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`: ordering and fail-closed transaction contract tests.
- Modify `docs/ROADMAP.md`, `docs/IMPLEMENTATION_STATUS.md`, `docs/SPEC_COVERAGE.md`: closure wording only after executable evidence is green.

---

### Task 1: Pure full-page planner, output projection, and stitcher

**Files:**
- Create: `crates/visual/src/full_page.rs`
- Modify: `crates/visual/src/lib.rs`
- Create: `crates/visual/tests/full_page_stitch_contract.rs`

**Interfaces:**
- Produces `FullPagePolicy`, `FullPagePlan`, `FullPageOutputGeometry`, `FullPageError`.
- Produces `plan_full_page(document_css_width, document_css_height, viewport_css_width, viewport_css_height, original_scroll_y, policy) -> Result<FullPagePlan, FullPageError>`.
- Produces `project_full_page_output(plan, tile_pixel_width, tile_pixel_height, policy) -> Result<FullPageOutputGeometry, FullPageError>`.
- Produces `stitch_full_page_tile(output, viewport_css_height, actual_scroll_y, tile) -> Result<(), FullPageError>`.
- Consumes existing `RgbaImage` from `localview-visual`; no I/O, WebView, artifact, or control dependency.

- [ ] **Step 1: Write RED planner/projection/stitch tests**

Add tests that compile against these exact signatures:

```rust
use localview_visual::{
    plan_full_page, project_full_page_output, stitch_full_page_tile,
    FullPageError, FullPagePolicy, RgbaImage,
};

#[test]
fn partial_final_tile_uses_exact_max_scroll_y() {
    let plan = plan_full_page(800.0, 1_450.0, 800.0, 600.0, 0.0, FullPagePolicy::default()).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 600.0, 850.0]);
}

#[test]
fn exact_division_does_not_duplicate_final_offset() {
    let plan = plan_full_page(800.0, 1_200.0, 800.0, 600.0, 0.0, FullPagePolicy::default()).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 600.0]);
}

#[test]
fn document_over_tile_budget_is_rejected() {
    let error = plan_full_page(800.0, 20_000.0, 800.0, 600.0, 0.0, FullPagePolicy { max_tiles: 2, ..Default::default() }).unwrap_err();
    assert_eq!(error, FullPageError::TileBudgetExceeded);
}
```

Cover non-finite/zero dimensions, width mismatch, single-tile document, >50,000 CSS px, >32 tiles, >128 MiB output, >32,768 output height, fractional `scale_y`, exact final overlap overwrite, width mismatch, invalid buffer, and checked arithmetic overflow.

- [ ] **Step 2: Run the RED test file**

Run:

```text
cargo test -p localview-visual --test full_page_stitch_contract
```

Expected: compilation/test failure because the full-page module/types/functions do not exist.

- [ ] **Step 3: Implement the minimal pure policy and planner**

Create the exact public data model:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FullPagePolicy {
    pub max_tiles: usize,
    pub max_document_css_height: f64,
    pub max_output_rgba_bytes: usize,
    pub max_output_pixel_height: u32,
}

impl Default for FullPagePolicy {
    fn default() -> Self {
        Self {
            max_tiles: 32,
            max_document_css_height: 50_000.0,
            max_output_rgba_bytes: 128 * 1024 * 1024,
            max_output_pixel_height: 32_768,
        }
    }
}

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
```

Use checked integer arithmetic for byte projection. Multi-tile plans start at zero, advance by exactly one viewport CSS height, append exact `max_scroll_y`, and de-duplicate equality with deterministic floating comparison derived from the exact input arithmetic rather than an unbounded epsilon loop.

- [ ] **Step 4: Implement output projection and in-place row-copy stitcher**

`project_full_page_output` derives vertical scale from the first tile:

```rust
let scale_y = tile_pixel_height as f64 / plan.viewport_css_height;
let projected_height = (plan.document_css_height * scale_y).round();
```

Reject non-finite/out-of-range projection before allocation. `stitch_full_page_tile` validates both images, calculates:

```rust
let scale_y = tile.height as f64 / viewport_css_height;
let top_px = (actual_scroll_y * scale_y).round();
```

then copies only rows intersecting the preallocated output, with later tile rows overwriting intentional final overlap.

- [ ] **Step 5: Export the module and run GREEN tests**

Add to `crates/visual/src/lib.rs`:

```rust
mod full_page;
pub use full_page::{
    plan_full_page, project_full_page_output, stitch_full_page_tile,
    FullPageError, FullPageOutputGeometry, FullPagePlan, FullPagePolicy,
};
```

Run:

```text
cargo test -p localview-visual --test full_page_stitch_contract
cargo test -p localview-visual
```

Expected: both commands pass.

- [ ] **Step 6: Commit Gate 1**

```text
git add crates/visual/src/full_page.rs crates/visual/src/lib.rs crates/visual/tests/full_page_stitch_contract.rs
git commit -m "feat(visual): add bounded full-page planner and stitcher"
```

---

### Task 2: Internal full-page action authority and private result sanitization

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Modify: `crates/live-bridge/src/action_envelope.rs`
- Create: `crates/live-bridge/tests/full_page_internal_action_contract.rs`

**Interfaces:**
- Extends `BridgeActionKind` with `CaptureScrollTo { token: Uuid, y: f64 }` and `CaptureTileProbe { token: Uuid }`.
- Extends private capture envelope so the bridge, not a public caller, can carry `mask_selectors` and an optional server-selected `visual_freeze_lease_ms`.
- Produces `enqueue_full_page_capture_freeze(session_id, mask_selectors) -> BridgeAction` with hard-coded 30,000 ms private lease.
- Produces `enqueue_capture_tile_probe(session_id, token, mask_selectors) -> BridgeAction` so every tile receives fresh private-selector authority.
- Result sanitizer stores only approved numeric/boolean/rect metadata for freeze/scroll/probe; selector strings and arbitrary page payload never persist.

- [ ] **Step 1: Write RED LiveBridge tests**

Add tests asserting:

```rust
assert!(BridgeActionKind::CaptureScrollTo { token, y: 100.5 }.is_internal_capture_action());
assert!(BridgeActionKind::CaptureTileProbe { token }.is_internal_capture_action());
```

Verify `take_public_actions()` never returns these actions, `take_internal_capture_actions()` does, full-page freeze carries exactly `visual_freeze_lease_ms = Some(30_000)`, normal freeze remains `Some(8_000)` or the existing compatibility representation, and tile probe receives `mask_selectors` only in `PrivateBridgeAction.private_capture`.

Complete a tile probe with a payload containing valid geometry plus `mask_selectors`, `innerText`, and a fake secret. Assert only the bounded geometry/count fields survive in `recent_internal_capture_results`.

- [ ] **Step 2: Run RED bridge tests**

```text
cargo test -p localview-live-bridge --test full_page_internal_action_contract
```

Expected: compile/test failure because new actions/enqueue helpers do not exist.

- [ ] **Step 3: Extend the internal action and private envelope model**

Use this exact shape:

```rust
CaptureScrollTo { token: Uuid, y: f64 },
CaptureTileProbe { token: Uuid },
```

and extend private data without making it public action input:

```rust
pub struct PrivateCaptureActionData {
    pub mask_selectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_freeze_lease_ms: Option<u64>,
}
```

Existing freeze enqueue must explicitly use `Some(8_000)`; full-page freeze must use `Some(30_000)`. Tile probe carries selectors and no caller-selected lease.

- [ ] **Step 4: Extend exact storage sanitization**

For `CaptureScrollTo`, retain only `requested_y`, `actual_x`, `actual_y`, document CSS dimensions, and viewport CSS dimensions after finite/bounded validation. For `CaptureTileProbe`, retain only scroll/document/viewport dimensions, `masked_elements`, sanitized `mask_rects`, `positional_elements_scanned`, and `visible_fixed_or_sticky` after the same bounds used by control.

On invalid metadata set `ok = false`, `payload = Value::Null`, and a bounded internal error code. Never store selector strings or arbitrary page-supplied keys.

- [ ] **Step 5: Update canonical-action exhaustive matching**

In `action_envelope.rs`, group all four internal capture actions together as unsupported for canonical public action envelopes:

```rust
BridgeActionKind::FreezeVisuals
| BridgeActionKind::RestoreVisuals { .. }
| BridgeActionKind::CaptureScrollTo { .. }
| BridgeActionKind::CaptureTileProbe { .. } => None,
```

- [ ] **Step 6: Run GREEN bridge regression**

```text
cargo test -p localview-live-bridge --test full_page_internal_action_contract
cargo test -p localview-live-bridge
```

Expected: pass.

- [ ] **Step 7: Commit Gate 2**

```text
git add crates/live-bridge/src/lib.rs crates/live-bridge/src/action_envelope.rs crates/live-bridge/tests/full_page_internal_action_contract.rs
git commit -m "feat(bridge): add guarded full-page internal actions"
```

---

### Task 3: Instrumentation-owned lease, absolute scroll, and bounded tile probe

**Files:**
- Modify: `crates/instrumentation/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/full_page_page_executor_contract.rs`

**Interfaces:**
- `window.__LOCALVIEW__.freezeVisuals(token, leaseMs)` accepts only `8_000` or `30_000` and stores original scroll coordinates in the closure-private lease.
- `window.__LOCALVIEW__.captureScrollTo(token, y)` checks the active closure-private lease token, uses absolute auto scroll, waits two animation frames, and returns bounded geometry.
- `window.__LOCALVIEW__.captureTileProbe(token)` checks the same lease and returns geometry plus bounded positional scan counts only.
- Desktop preview bridge combines `captureTileProbe` with `privateMaskGeometry(queued.private_capture?.mask_selectors || [])` for the current viewport.

- [ ] **Step 1: Write RED page-executor contract tests**

Assert source contains exact-token checks against closure-private `visualFreezeLease`, allowed lease constants `8000` and `30000`, `window.scrollTo` with original horizontal scroll, two nested/requested animation-frame waits, shared document geometry helper, maximum 4,096 positional scans, and no selector/text return from tile probe.

Assert the existing public relative action still contains `window.scrollBy` only in the public `'scroll'` case.

- [ ] **Step 2: Run RED desktop executor test**

```text
cargo test -p localview-desktop --test full_page_page_executor_contract
```

Expected: fail because new page methods/cases are absent.

- [ ] **Step 3: Extend the instrumentation lease without making it spoofable**

Inside the same closure that owns `visualFreezeLease`, add:

```javascript
const VIEWPORT_VISUAL_FREEZE_LEASE_MS = 8000;
const FULL_PAGE_VISUAL_FREEZE_LEASE_MS = 30000;
const MAX_POSITIONAL_SCAN_ELEMENTS = 4096;
```

`freezeVisuals(token, leaseMs = VIEWPORT_VISUAL_FREEZE_LEASE_MS)` rejects any lease other than the two constants, captures `originalScrollX`/`originalScrollY`, and arms the existing auto-restore using the selected bounded lease. Do not infer authority from the DOM root attribute.

- [ ] **Step 4: Add shared bounded document geometry and token-bound absolute scroll**

Document geometry must use one helper for freeze and probes and return only finite numeric values. `captureScrollTo(token, y)` must reject a non-active token/non-finite negative `y`, clamp only to the measured `maxScrollY`, call absolute `window.scrollTo({ left: lease.originalScrollX, top: y, behavior: 'auto' })`, wait two animation frames, re-check the lease token, then return requested/actual/geometry metadata.

- [ ] **Step 5: Add bounded positional tile probe**

Use a bounded traversal that detects a 4,097th element and throws `full_page_positional_scan_budget_exceeded` rather than silently truncating. For each inspected element, count visible viewport boxes whose computed `position` is exactly `fixed` or `sticky`. Return counts only; do not return tag names, ids, selectors, text, computed style objects, or nodes.

- [ ] **Step 6: Wire managed-WebView executor cases**

Add preview bridge cases equivalent to:

```javascript
case 'capture_scroll_to':
  return await window.__LOCALVIEW__?.captureScrollTo?.(action.token, action.y);
case 'capture_tile_probe': {
  const probe = await window.__LOCALVIEW__?.captureTileProbe?.(action.token);
  const geometry = privateMaskGeometry(queued.private_capture?.mask_selectors || []);
  return { ...probe, ...geometry };
}
```

The `freeze_visuals` case must pass only the private server-selected allowlisted lease to `freezeVisuals`.

- [ ] **Step 7: Run GREEN instrumentation/desktop executor regressions**

```text
cargo test -p localview-instrumentation
cargo test -p localview-desktop --test full_page_page_executor_contract
cargo test -p localview-desktop --test live_semantic_bridge_contract
cargo test -p localview-desktop --test visual_freeze_capture_contract
```

Expected: pass; viewport freeze semantics remain 8 seconds.

- [ ] **Step 8: Commit Gate 3**

```text
git add crates/instrumentation/src/lib.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/tests/full_page_page_executor_contract.rs
git commit -m "feat(capture): add token-bound full-page page executor"
```

---

### Task 4: Authenticated internal full-page control endpoints

**Files:**
- Modify: `crates/control/src/capture_settle.rs`
- Modify: `crates/control/src/runtime.rs`
- Create: `crates/control/tests/full_page_capture_control.rs`

**Interfaces:**
- `POST /v1/sessions/{id}/capture-freeze-full-page` has no caller-controlled lease body and queues `enqueue_full_page_capture_freeze`.
- `POST /v1/sessions/{id}/capture-scroll` accepts only `{ "token": Uuid, "y": f64 }`.
- `POST /v1/sessions/{id}/capture-tile-probe` accepts only `{ "token": Uuid }`, queues a probe with `StableCapturePolicy::default().mask_selectors` privately.
- All endpoints wait for the exact action id in the internal result scope and return bounded validated receipts only.

- [ ] **Step 1: Add RED endpoint integration tests**

Test unauthorized and missing-session requests, unknown JSON fields, non-finite/out-of-range geometry, exact result-id correlation, timeout, mismatched token result, probe mask budget, positional scan >4,096, and content sanitization.

Test that posting new internal action JSON through the existing public `/actions` endpoint still returns `internal_capture_action_not_public`.

- [ ] **Step 2: Run RED control tests**

```text
cargo test -p localview-control --test full_page_capture_control
```

Expected: fail because the routes do not exist.

- [ ] **Step 3: Add request/receipt types with deny-unknown-fields**

Use:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureScrollRequest { token: Uuid, y: f64 }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureTileProbeRequest { token: Uuid }
```

Validate all geometry against `MAX_CSS_VIEWPORT_DIMENSION`, count fields against existing mask/element bounds, and positional scan against exactly 4,096.

- [ ] **Step 4: Implement full-page freeze/scroll/probe routes using existing exact-result polling**

Reuse `wait_for_action_result(..., ActionResultScope::InternalCapture)`. The full-page freeze endpoint returns the extended freeze receipt with original scroll/document geometry and `lease_ms = 30_000`. Scroll/probe return only sanitized numeric/rect/count metadata.

- [ ] **Step 5: Update public runtime exhaustive matches**

Any exhaustive `BridgeActionKind` match in `runtime.rs` must classify new variants as internal/non-public without creating public action summaries or evidence from their page-returned payloads.

- [ ] **Step 6: Run GREEN control regressions**

```text
cargo test -p localview-control --test full_page_capture_control
cargo test -p localview-control
```

Expected: pass.

- [ ] **Step 7: Commit Gate 4**

```text
git add crates/control/src/capture_settle.rs crates/control/src/runtime.rs crates/control/tests/full_page_capture_control.rs
git commit -m "feat(control): add guarded full-page capture endpoints"
```

---

### Task 5: Dedicated full-page visual evidence contract

**Files:**
- Create: `crates/control/src/visual_full_page.rs`
- Modify: `crates/control/src/lib.rs`
- Extend: `crates/control/tests/full_page_capture_control.rs`

**Interfaces:**
- `POST /v1/sessions/{id}/evidence/visual-full-page`.
- Consumes exact bounded metadata: artifact id, pixel dimensions, backend, canonical loopback route, viewport, revision, timestamp, document dimensions, tile count, scroll offsets.
- Produces `{ evidence_id, deduplicated }` through the existing `EvidenceKind::Visual` store with `region: Some("full_page")` and `source: "native-capture"`.

- [ ] **Step 1: Add RED evidence validation tests**

Include valid two-tile evidence plus malformed artifact id/backend/route, non-finite dimensions, zero dimensions, tile count 0/33, length mismatch, duplicate/decreasing offsets, non-zero first multi-tile offset, impossible final offset, unknown fields, unauthorized request, and missing session.

- [ ] **Step 2: Implement strict evidence request validation**

Mirror existing `visual_region.rs` privacy rules. Strip query/fragment through the same canonical route policy used by desktop before send, accept only the three current native backend names, and never accept tile pixels/masks/tokens/selector strings.

- [ ] **Step 3: Mount the router**

Add `mod visual_full_page;` and `.merge(visual_full_page::router(state.clone()))` before the final consuming merge in `crates/control/src/lib.rs`.

- [ ] **Step 4: Run GREEN evidence/control tests**

```text
cargo test -p localview-control --test full_page_capture_control
cargo test -p localview-control
```

Expected: pass.

- [ ] **Step 5: Commit Gate 5**

```text
git add crates/control/src/visual_full_page.rs crates/control/src/lib.rs crates/control/tests/full_page_capture_control.rs
git commit -m "feat(evidence): add full-page visual provenance"
```

---

### Task 6: Desktop full-page transaction and fail-closed cleanup

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`

**Interfaces:**
- Public Tauri command exactly:

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

- Private desktop HTTP helpers: full-page freeze, absolute capture scroll, tile probe, full-page evidence registration.
- Produces `FullPageCaptureReceipt` fields exactly as approved by the spec; no freeze token/masks/internal tile metadata exposed.

- [ ] **Step 1: Write RED transaction-order/failure contract tests**

Assert the coordinator's executable ordering is:

```text
session_capture_gate
wait_for_capture_settle
freeze_full_page_visual_state
plan_full_page
capture_scroll_to -> wait_for_capture_settle -> capture_tile_probe -> capture_managed_surface -> redact -> decode -> stitch
restore original capture_scroll_to
restore_visual_state
encode_png_rgba
artifact admission/persistence
register_full_page_visual_evidence
```

Add source/behavior assertions for route drift, document drift, viewport/DSF/native dimensions/backend drift, fixed/sticky rejection before native copy, per-tile `probe.mask_rects` redaction, redaction failure, decode failure, stitch failure, original-scroll restore failure, visual restore failure, transaction timeout, no intermediate artifact calls, and capture gate spanning the complete transaction.

- [ ] **Step 2: Run RED desktop transaction tests**

```text
cargo test -p localview-desktop --test full_page_capture_contract
```

Expected: fail because the coordinator/receipt/helpers do not exist.

- [ ] **Step 3: Add extended private receipt types and exact validators**

Extend the full-page freeze receipt with original scroll/document dimensions. Add `CaptureScrollReceipt` and `CaptureTileProbeReceipt` matching the spec. Validators reject non-finite values, wrong geometry, stale route, wrong backend/DSF/native dimensions, mask/count overflow, and fixed/sticky presence with bounded error classes.

- [ ] **Step 4: Implement one full-page transaction under the existing session gate**

The coordinator must acquire the gate before initial settle and hold it through cleanup and final persistence/evidence. It must preflight the exact managed surface before work and use the existing `capture_managed_surface` for every tile.

After the first tile proves native pixel geometry, call `project_full_page_output`, allocate exactly one bounded output `RgbaImage`, then for each tile:

```rust
let scroll = capture_scroll_to(...).await?;
wait_for_capture_settle(session_id).await?;
let probe = capture_tile_probe(...).await?;
let frame = capture_managed_surface(...).await?;
let redacted_png = redact_png_css_rects(
    &frame.png,
    (frame.pixel_width, frame.pixel_height),
    (probe.viewport_css_width, probe.viewport_css_height),
    &probe.mask_rects,
)?;
let tile = decode_png_rgba(&redacted_png)?;
stitch_full_page_tile(&mut output, probe.viewport_css_height, scroll.actual_y, &tile)?;
```

Do not retain a vector of tile PNGs/images.

- [ ] **Step 5: Implement bounded cleanup and 30-second transaction deadline**

Track the primary bounded failure separately from cleanup. If a freeze token exists, always attempt original-scroll restoration while the token is live, then exact visual restore. Use one absolute 30-second transaction deadline; do not renew the page lease. Prefer cleanup when work approaches the deadline so the transaction does not intentionally consume the complete lease before restoration. If cleanup fails, return the primary bounded error augmented only by `full_page_scroll_restore_failed` and/or `full_page_visual_restore_failed` class, never page content.

- [ ] **Step 6: Persist only after exact restoration**

Only after both restoration acknowledgements succeed: encode the final output, perform existing owner-local retained-resource admission, write one artifact, then call `/evidence/visual-full-page`. No intermediate tile artifact/evidence mutation is permitted.

- [ ] **Step 7: Register the Tauri command**

Add `visual_capture::capture_full_page` to the existing `tauri::generate_handler![...]` list without changing current commands.

- [ ] **Step 8: Run GREEN desktop regressions**

```text
cargo test -p localview-desktop --test full_page_capture_contract
cargo test -p localview-desktop --test full_page_page_executor_contract
cargo test -p localview-desktop --test visual_freeze_capture_contract
cargo test -p localview-desktop --tests
```

Expected: pass.

- [ ] **Step 9: Commit Gate 6**

```text
git add apps/desktop/src-tauri/src/visual_capture.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/tests/full_page_capture_contract.rs
git commit -m "feat(desktop): orchestrate guarded full-page stitching"
```

---

### Task 7: Exact-head regression closure before documentation claims

**Files:**
- No documentation truth changes yet.

**Interfaces:**
- Consumes all Gate 1-6 code.
- Produces one exact implementation head that passes local workspace verification before documentation status changes.

- [ ] **Step 1: Run formatting and focused package suites**

```text
cargo fmt --check
cargo test -p localview-visual
cargo test -p localview-live-bridge
cargo test -p localview-instrumentation
cargo test -p localview-control
cargo test -p localview-desktop --tests
```

Expected: every command exits 0.

- [ ] **Step 2: Run all-target workspace compilation**

```text
cargo check --workspace --all-targets
```

Expected: exit 0 with no exhaustive-match or target-gated capture regressions.

- [ ] **Step 3: Inspect the exact diff for forbidden scope creep**

The diff must contain no Chromium/Playwright full-page implementation, platform-adapter scroll offsets, public caller tile plans, arbitrary eval endpoint, fixed/sticky rewrite, infinite-scroll loop, or intermediate tile persistence.

- [ ] **Step 4: Push the exact implementation head and require GitHub CI**

Record the resulting branch SHA. Do not update roadmap/status docs while any check is queued/in-progress/failing. Require the repository's normal CI and applicable Linux/Windows/macOS checks to complete successfully for that exact SHA.

---

### Task 8: Documentation truth, final exact-head CI, and merge gate

**Files:**
- Modify: `docs/ROADMAP.md`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/SPEC_COVERAGE.md`

**Interfaces:**
- Documentation may describe only behavior proven by Gate 7 and the exact implementation branch CI.
- Must preserve the separate unresolved physical mixed-DPI blocker on PR #116; full-page closure does not close W10 physical evidence.

- [ ] **Step 1: Update docs only after Gate 7 is green**

Document exactly: explicit full-page command, managed-surface native viewport tiles, one freeze token, internal absolute scroll authority, per-tile settle/private mask refresh, fixed/sticky fail-closed, bounded pure Rust stitcher, exact scroll+visual restoration before persistence, dedicated final evidence, no Chromium fallback.

Keep explicit non-claims: infinite pages, fixed/sticky support, responsive sweep/contact-sheet completion, planner-autonomous full-page capture, and generic visual-fidelity claims beyond existing platform evidence.

- [ ] **Step 2: Run documentation-sensitive and complete regression suite again**

```text
cargo fmt --check
cargo test -p localview-visual
cargo test -p localview-live-bridge
cargo test -p localview-instrumentation
cargo test -p localview-control
cargo test -p localview-desktop --tests
cargo check --workspace --all-targets
```

Expected: all exit 0 on the final documented head.

- [ ] **Step 3: Commit documentation closure**

```text
git add docs/ROADMAP.md docs/IMPLEMENTATION_STATUS.md docs/SPEC_COVERAGE.md
git commit -m "docs: record guarded full-page stitching closure"
```

- [ ] **Step 4: Require final exact-head GitHub checks before merge**

Fetch check-runs for the final commit SHA. Require every applicable check to be `completed` with an acceptable successful conclusion and require zero `queued`, `in_progress`, `failure`, `cancelled`, `timed_out`, `action_required`, `startup_failure`, or null conclusions. Do not merge from a stale earlier green SHA.

- [ ] **Step 5: Merge with expected-head guard only after final evidence**

Merge the implementation PR using the exact final head SHA guard. After merge, fetch `main` and its post-merge checks; do not call the wave closed until post-merge evidence is green.

---

## Self-review record

### Spec coverage

- Pure planner, exact offsets, fractional scale, output bounds, and final overlap: Task 1.
- Internal action authority, private lease/selectors, bounded result store: Task 2.
- Closure-private token authority, 8s/30s allowlist, absolute scroll, two-frame ACK, fresh probe, 4,096 fixed/sticky scan: Task 3.
- Auth/session/result correlation and bounded scroll/probe endpoints: Task 4.
- Dedicated full-page evidence provenance and validation: Task 5.
- One session gate, per-tile redaction-before-stitch, immutable geometry, cleanup-before-persistence, 30s transaction bound, ephemeral tiles: Task 6.
- Focused/workspace/cross-platform verification: Task 7.
- Truthful docs, exact-head CI, expected-head merge, post-merge verification: Task 8.

### Type consistency

- New action names are consistently `CaptureScrollTo { token, y }` and `CaptureTileProbe { token }` from LiveBridge through control and desktop executor.
- Full-page policy fields match the approved spec exactly.
- Tile placement consumes acknowledged `actual_y`; requested offsets are evidence/planning provenance only.
- Private selectors travel only through `PrivateCaptureActionData`; they never enter public action input or final evidence.
- `localview-live-bridge` compiles through `src/v43_lib.rs -> src/cancellable_lib.rs -> src/lib.rs`; edits to `src/lib.rs` are therefore live and tests must import the public crate.

### Placeholder scan

The plan contains no deferred implementation markers. Every task names concrete files, interfaces, RED/GREEN commands, expected outcomes, and commit boundaries.