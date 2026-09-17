# Guarded Full-Page Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement bounded, fail-closed native full-page capture over the existing LocalView-managed viewport capture authority, with token-bound absolute scrolling, per-tile privacy redaction, deterministic row stitching, exact restoration, and dedicated full-page evidence provenance.

**Architecture:** Keep WebView2/WKWebView/WebKitGTK adapters viewport-only. Add pure planner/stitcher logic to `localview-visual` using its existing `RgbaImage { width, height, data }` type, add two internal capture actions to `localview-live-bridge`, extend the managed WebView executor and `crates/control/src/capture_settle.rs`, then orchestrate the transaction in desktop `visual_capture.rs` under one session gate and one freeze token. Every production change follows RED -> verify RED -> GREEN -> verify GREEN.

**Tech Stack:** Rust workspace, existing `localview-visual::RgbaImage`, existing `png` dependency, Tauri desktop coordinator, Axum control plane, LocalView live bridge/instrumentation JavaScript, existing native capture adapters, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`

## Global Constraints

- No Chromium/Playwright/Puppeteer full-page fallback.
- Native platform adapters remain viewport-only.
- No new image crate dependency: stitching uses existing `localview_visual::RgbaImage`.
- One per-session capture gate owns the complete transaction.
- One visual-freeze token owns all internal scroll/probe actions.
- Public relative `Scroll` is never reused for stitching.
- Private-mask geometry is refreshed after every scroll and applied before tile pixels enter the stitcher.
- Any visible `position: fixed` or `position: sticky` element fails closed in this slice.
- Any route, viewport, document geometry, device-scale, backend, native pixel geometry, token, or action-result drift fails closed.
- `max_tiles = 32`.
- `max_document_css_height = 50_000` CSS px.
- `max_output_rgba_bytes = 128 MiB`.
- `max_output_pixel_height = 32_768` px.
- Positional scan authority is bounded to 4,096 elements.
- Original scroll position and visual state must be restored before final artifact/evidence persistence.
- Intermediate tile PNG/RGBA buffers are ephemeral and never independently persisted or registered.
- Production code never precedes its failing test.

---

## File Map

- Create `crates/visual/src/full_page.rs`: deterministic plan, scale/tolerance, projected output bounds, row-copy stitching.
- Modify `crates/visual/src/lib.rs`: expose full-page module/types; reuse existing `RgbaImage`.
- Create `crates/visual/tests/full_page_stitching.rs`.
- Modify `crates/live-bridge/src/lib.rs`.
- Create `crates/live-bridge/tests/full_page_capture_actions.rs`.
- Modify `apps/desktop/src-tauri/src/lib.rs`: managed WebView executor cases and page helpers.
- Create `apps/desktop/src-tauri/tests/full_page_bridge_contract.rs`.
- Modify `crates/control/src/capture_settle.rs`: freeze receipt extension + internal capture-scroll/probe routes and validators.
- Extend `crates/control/tests/capture_visual_state.rs` and create `crates/control/tests/full_page_capture_control.rs` where isolation improves reviewability.
- Modify `crates/control/src/runtime.rs`: dedicated full-page visual evidence ingestion/validation, following existing visual-evidence ownership.
- Create `crates/control/tests/full_page_visual_evidence.rs`.
- Modify `apps/desktop/src-tauri/src/visual_capture.rs`: transaction orchestration.
- Create `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`.
- Update `docs/IMPLEMENTATION_STATUS.md`, `docs/ROADMAP.md`, `docs/SPEC_COVERAGE.md` only after executable exact-head GREEN.

---

### Task 1: Pure Full-Page Planner and Stitcher

**Files:**
- Create `crates/visual/src/full_page.rs`
- Modify `crates/visual/src/lib.rs`
- Create `crates/visual/tests/full_page_stitching.rs`

**Interfaces:**

```rust
pub struct FullPagePolicy {
    pub max_tiles: usize,
    pub max_document_css_height: f64,
    pub max_output_rgba_bytes: usize,
    pub max_output_pixel_height: u32,
}

pub struct FullPagePlan {
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub scroll_offsets_y: Vec<f64>,
}

pub fn plan_full_page(
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
    original_scroll_y: f64,
    policy: FullPagePolicy,
) -> Result<FullPagePlan, FullPagePlanError>;

pub fn project_output_height_px(
    document_css_height: f64,
    viewport_css_height: f64,
    tile_pixel_height: u32,
    policy: FullPagePolicy,
) -> Result<u32, FullPagePlanError>;

pub fn scroll_tolerance_css(scale_y: f64) -> Result<f64, FullPagePlanError>;

pub fn stitch_full_page_tile(
    output: &mut RgbaImage,
    actual_scroll_y: f64,
    scale_y: f64,
    tile: &RgbaImage,
) -> Result<(), FullPageStitchError>;
```

- [ ] Write RED planner tests for one tile, `[0,1000,1500]` bottom clamp on 2500/1000 geometry, exact divisibility de-duplication, 32-tile boundary, 33-tile rejection, non-finite/zero dimensions, width mismatch, original-scroll validation and 50k CSS-height bound.
- [ ] Run `cargo test -p localview-visual --test full_page_stitching planner_ -- --nocapture`; expected RED is unresolved full-page API only.
- [ ] Implement minimal policy/planner and export it from `lib.rs`.
- [ ] Re-run targeted planner tests to GREEN.
- [ ] Write RED projection/stitch tests using direct `RgbaImage` construction, e.g. `RgbaImage { width: 4, height: 2, data: vec![7; 32] }`; cover fractional scale, final overlap overwrite, width mismatch, negative/non-finite placement, 32768px height, 128MiB RGBA and checked arithmetic.
- [ ] Run targeted tests and confirm RED for missing stitch/projection behavior.
- [ ] Implement row-copy stitching directly over `RgbaImage.data` using checked offsets; retain no vector of decoded tiles.
- [ ] Run `cargo test -p localview-visual` and `cargo clippy -p localview-visual --all-targets -- -D warnings`.
- [ ] Commit `feat(visual): add bounded full-page planner and stitcher`.

### Task 2: Internal Capture Action Authority

**Files:**
- Modify `crates/live-bridge/src/lib.rs`
- Create `crates/live-bridge/tests/full_page_capture_actions.rs`

**Interfaces:**

```rust
CaptureScrollTo { token: Uuid, y: f64 },
CaptureTileProbe { token: Uuid },
```

Both are internal capture actions alongside `FreezeVisuals` and `RestoreVisuals`.

- [ ] RED: serialization, internal classification, generic public action rejection, private-selector envelope availability only on capture queue, bounded queue behavior.
- [ ] Verify RED: `cargo test -p localview-live-bridge --test full_page_capture_actions -- --nocapture`.
- [ ] GREEN: add variants/classification and minimal private-capture queue plumbing; do not alter public `Scroll` behavior.
- [ ] Verify full package + clippy.
- [ ] Commit `feat(live-bridge): add internal full-page capture actions`.

### Task 3: Managed WebView Token-Bound Scroll and Probe

**Files:**
- Modify `apps/desktop/src-tauri/src/lib.rs`
- Create `apps/desktop/src-tauri/tests/full_page_bridge_contract.rs`

**Required behavior:**
- `CaptureScrollTo`: exact active-freeze token match, finite non-negative absolute Y, preserve original frozen X, `window.scrollTo`, two animation frames before acknowledgement, bounded geometry-only receipt.
- `CaptureTileProbe`: exact token match, shared document-geometry helper, current scroll/viewport/document geometry, current private-mask rectangles, bounded positional scan.
- At most 4,096 elements are scanned; overflow is an error, not truncation.
- Visible computed `fixed`/`sticky` positions increment only counts; no identity/text/style values escape.

- [ ] RED source/behavior contract proving absolute scroll, token check before operation, unchanged public `scrollBy`, two-frame acknowledgement and no selector leakage.
- [ ] GREEN executor cases next to existing freeze/restore handling.
- [ ] RED positional-scan and per-tile mask-refresh contracts.
- [ ] GREEN bounded scan + reuse of existing `privateMaskGeometry` after every scroll.
- [ ] Run new tests plus `live_semantic_bridge_contract` and `visual_freeze_capture_contract`.
- [ ] Commit `feat(desktop): execute guarded full-page bridge actions`.

### Task 4: Control-Plane Full-Page Capture Contracts

**Files:**
- Modify `crates/control/src/capture_settle.rs`
- Extend `crates/control/tests/capture_visual_state.rs`
- Create `crates/control/tests/full_page_capture_control.rs`

**Interfaces:**
- Freeze receipt gains finite bounded `scroll_x`, `scroll_y`, `document_css_width`, `document_css_height` without changing viewport/region pixel semantics.
- Add authenticated narrow routes for capture-scroll and capture-tile-probe.
- Scroll accepts only token + Y; probe accepts token and private selectors only through the private capture envelope.
- Both wait for exact action id/result correlation with existing bounded timeout.

- [ ] RED freeze compatibility/validation tests.
- [ ] GREEN freeze receipt extension.
- [ ] RED scroll endpoint tests: unauthenticated, missing session, non-finite Y, wrong action id, mismatched token, timeout, sanitized success.
- [ ] GREEN scroll endpoint using only `CaptureScrollTo`.
- [ ] RED probe tests: auth/session/correlation, fixed/sticky count, scan-budget error, mask-budget error, forbidden selector/text/URL/storage leakage.
- [ ] GREEN probe endpoint using only `CaptureTileProbe`.
- [ ] Run `cargo test -p localview-control` and clippy.
- [ ] Commit `feat(control): add guarded full-page capture controls`.

### Task 5: Dedicated Full-Page Evidence

**Files:**
- Modify `crates/control/src/runtime.rs`
- Create `crates/control/tests/full_page_visual_evidence.rs`

**Contract:** dedicated full-page evidence records final artifact reference, canonical route, document/viewport CSS geometry, final native pixel dimensions, scale/backend provenance, tile count, acknowledged offsets and transaction timing. It excludes freeze token, selectors, masks, intermediate bytes and filesystem paths.

- [ ] RED strict-schema tests for valid provenance and rejection of unknown fields, zero/>32 tile count, non-monotonic offsets, non-finite geometry, malformed route/provenance, and forbidden internal fields.
- [ ] GREEN strict request type/validation and evidence insertion under existing bearer/session authority.
- [ ] Run full existing visual evidence/diff/region/control tests.
- [ ] Commit `feat(control): add full-page visual evidence provenance`.

### Task 6: Desktop Transaction Orchestration

**Files:**
- Modify `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify Tauri command registration where existing capture commands are registered.
- Create `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`

**Exact success order:**

```text
preflight exact managed surface
-> acquire session gate
-> stable-settle
-> freeze + original scroll/document geometry
-> pure plan
-> for each offset:
     token scroll
     stable-settle
     tile probe
     reject fixed/sticky/drift
     native viewport capture
     route/viewport/DSF/backend/native-pixel validation
     private redaction in tile memory
     stitch rows
     drop tile buffers
-> token scroll to original Y
-> verify original scroll
-> restore visuals
-> encode final RGBA
-> retained-resource admission
-> persist one artifact
-> register dedicated full-page evidence
```

- [ ] RED source/order contract requiring restore before persistence.
- [ ] GREEN minimal command/transaction structure without premature artifact creation.
- [ ] RED happy-path 2/3-tile transaction proving one gate, one freeze token, per-tile mask refresh and exactly one final artifact/evidence.
- [ ] GREEN scroll/settle/probe/native/redact/stitch loop.
- [ ] RED fail-closed cases: route drift, viewport drift, document growth/shrink, DSF drift, native dimensions/backend mismatch, wrong offset, token expiry, fixed/sticky, scan budget, tile/output-memory/output-height budget, native capture error, encode/admission error.
- [ ] GREEN explicit guards; no heuristic recovery.
- [ ] RED cleanup cases: mid-loop failure, original-scroll restore failure, visual restore failure, capture+restore failure; every case asserts zero final artifact/evidence.
- [ ] GREEN single cleanup/result path that always attempts token-owned restoration after freeze and persists only after both restorations succeed.
- [ ] Run all desktop visual capture/freeze/private-redaction/resource-retention tests.
- [ ] Commit `feat(desktop): orchestrate guarded full-page capture`.

### Task 7: Exact-Head Closure

**Files:**
- Modify `docs/IMPLEMENTATION_STATUS.md`
- Modify `docs/ROADMAP.md`
- Modify `docs/SPEC_COVERAGE.md`
- Workflow files only if current CI lacks required package/OS execution.

- [ ] Run formatting, all targeted package suites and repository-standard workspace CI commands.
- [ ] Every newly discovered implementation bug starts with a reproducing RED test.
- [ ] Update docs only after executable tests are GREEN; explicitly retain unsupported fixed/sticky, expanding/infinite documents, horizontal stitching and Chromium fallback limitations.
- [ ] Commit docs closure.
- [ ] Require exact-head GitHub Actions with no failure/cancelled/timed_out/action_required/startup_failure/queued/in_progress/null conclusion.
- [ ] Review diff against every spec invariant; scan for `TODO`, `TBD`, unbounded retention, leaked tokens/selectors and accidental platform full-page API usage.
- [ ] Merge only with expected-head SHA guard.

## Self-Review Result

- Spec coverage maps to Tasks 1–7.
- No dependency on an `image` crate; tests/implementation use the repository's existing `RgbaImage` representation.
- Control ownership is exact: current freeze/restore routes live in `crates/control/src/capture_settle.rs`.
- `CaptureScrollTo { token, y }` and `CaptureTileProbe { token }` are the only new bridge actions.
- Desktop remains orchestration authority; control remains action/evidence authority; platform adapters remain viewport-only.
- No placeholder implementation steps are authorized.

## Execution Mode

Use **Inline Execution** with `superpowers:executing-plans`. GitHub feature branch `feat/guarded-full-page-stitching` is the isolated workspace for this connector-driven session. Start with Task 1 RED tests only and capture failing CI/test evidence before modifying production planner/stitcher code.
