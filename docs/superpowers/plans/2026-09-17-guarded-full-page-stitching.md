# Guarded Full-Page Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded native full-page capture by vertically stitching redacted viewport captures from the existing managed WebView authority.

**Architecture:** Keep platform adapters viewport-only. Add a pure planner/streaming stitcher in `localview-visual`, exact-freeze-token probe/scroll actions in the live bridge/instrumentation path, and one desktop coordinator that owns settle → freeze → tile scroll/probe/capture/redact/stitch → original-scroll restore → visual restore → persistence → evidence. All drift fails closed.

**Tech Stack:** Rust, Tauri 2, existing `png` crate, LocalView live bridge/instrumentation, Axum control plane, GitHub Actions cross-platform CI.

**Spec:** `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`

## Global Constraints

- `MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT = 100_000.0`.
- `MAX_FULL_PAGE_TILES = 128`.
- `MAX_FULL_PAGE_PIXELS = 64_000_000`.
- `MAX_FULL_PAGE_PNG_BYTES = 128 * 1024 * 1024`.
- Existing viewport DSF bound remains `0 < dsf <= 8`.
- Full-page capture is vertical-only and rejects horizontal document overflow beyond `0.5 CSS px`.
- Fixed or sticky rendered content is rejected in this slice.
- Every tile must be redacted before entering the stitcher.
- Every native tile must be bracketed by exact-token pre/post probes with stable mask geometry.
- No intermediate tile artifact may be persisted.
- Original scroll and visual state must restore successfully before final persistence.
- No Chromium/Playwright full-page fallback.

---

## Task 1: Pure deterministic full-page planner and streaming compositor

**Files:**
- Create: `crates/visual/src/full_page.rs`
- Modify: `crates/visual/src/lib.rs`
- Create: `crates/visual/tests/full_page_stitching.rs`

**Interfaces:**
- Produces: `MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64`, `MAX_FULL_PAGE_TILES: usize`, `MAX_FULL_PAGE_PIXELS: u64`, `MAX_FULL_PAGE_PNG_BYTES: usize`.
- Produces: `FullPageTilePlan { scroll_y_css, source_start_y_css, contribution_height_css, destination_start_y_css }`.
- Produces: `FullPagePlan { document_css_width, document_css_height, viewport_css_width, viewport_css_height, tiles }`.
- Produces: `plan_full_page(document_css: (f64, f64), viewport_css: (f64, f64)) -> Result<FullPagePlan, VisualError>`.
- Produces: `FullPageStitcher::new(plan, native_pixel_width, native_pixel_height) -> Result<Self, VisualError>`.
- Produces: `FullPageStitcher::push_tile(index, &RgbaImage) -> Result<(), VisualError>` and `finish() -> Result<Vec<u8>, VisualError>`.

- [ ] **Step 1: Write planner RED tests**

Create tests for shorter-than-viewport, exact viewport multiples, clamped overlapping final scroll, 100,000 CSS-px bound, 129 tiles, horizontal overflow, and non-finite geometry.

Representative expected plan for `H=2500,V=1000`:

```rust
assert_eq!(plan.tiles.len(), 3);
assert_eq!(plan.tiles[0].scroll_y_css, 0.0);
assert_eq!(plan.tiles[1].scroll_y_css, 1000.0);
assert_eq!(plan.tiles[2].scroll_y_css, 1500.0);
assert_eq!(plan.tiles[2].source_start_y_css, 500.0);
assert_eq!(plan.tiles[2].destination_start_y_css, 2000.0);
assert_eq!(plan.tiles[2].contribution_height_css, 500.0);
```

- [ ] **Step 2: Commit the RED planner tests and require GitHub CI to fail for missing API**

Expected failure: unresolved imports/types from `localview_visual`.

- [ ] **Step 3: Implement the minimal deterministic planner**

Use checked finite arithmetic, explicit `max_scroll = (H - V).max(0.0)`, monotonic offsets, exact inclusion of `max_scroll`, and contribution intervals derived from prior covered end.

- [ ] **Step 4: Add streaming compositor RED tests**

Use small synthetic RGBA tiles with distinct row values. Prove overlapping source rows are discarded, destination rows appear exactly once, fractional CSS→pixel boundaries are contiguous, wrong tile order/dimensions fail, and output cap errors.

- [ ] **Step 5: Implement bounded streaming PNG output**

Use `png::Encoder` + stream writer over a custom bounded `Write` sink. Never allocate the full stitched RGBA image. For each tile contribution, derive source/destination pixel boundaries from cumulative CSS coordinates using one stable scale from actual native pixel height / viewport CSS height, then stream only contributing RGBA rows. Reject total pixel count above `64_000_000` and encoded output above `128 MiB`.

- [ ] **Step 6: Run exact crate tests through GitHub CI and commit GREEN**

Required command in CI/local-capable environment:

```bash
cargo test -p localview-visual --test full_page_stitching
cargo test -p localview-visual
```

---

## Task 2: Freeze-token stitch probe/scroll action contracts

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Modify: `crates/live-bridge/src/action_envelope.rs`
- Modify: `crates/live-bridge/tests/visual_freeze_contract.rs`
- Modify: `crates/control/src/runtime.rs`
- Modify: `crates/control/tests/capture_visual_state.rs`

**Interfaces:**
- Produces internal variants:

```rust
BridgeActionKind::StitchProbe { token: Uuid }
BridgeActionKind::StitchScroll { token: Uuid, y: f64 }
```

- Both variants return `None` from public action-envelope kind projection and `true` from `is_internal_capture_action()`.

- [ ] **Step 1: Add RED live-bridge tests** proving both variants are internal and cannot be represented as a public `PageActionKind`.
- [ ] **Step 2: Implement enum/envelope classification and runtime summaries** using stable names `stitch_probe` and `stitch_scroll`.
- [ ] **Step 3: Extend control capture-state tests** so internal action enqueue/claim/result plumbing accepts the two new variants without granting public cancellation authority.
- [ ] **Step 4: Run focused tests and commit GREEN**:

```bash
cargo test -p localview-live-bridge visual_freeze_contract
cargo test -p localview-control capture_visual_state
```

---

## Task 3: Page-side exact-token stitch state and privacy-safe geometry receipts

**Files:**
- Modify: `crates/instrumentation/src/lib.rs`
- Modify: `crates/instrumentation/tests/visual_freeze_contract.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Add focused contract test under `apps/desktop/src-tauri/tests/` if existing source-string contract style is the repository convention.

**Interfaces:**
- Freeze state retains original scroll coordinates and the already-private selector list keyed only by the active freeze token.
- Page-side functions:

```text
stitchProbe(token)
stitchScroll(token, y)
```

- Receipt fields: `scroll_x`, `scroll_y`, `viewport_css_width`, `viewport_css_height`, `document_css_width`, `document_css_height`, `fixed_or_sticky_count`, `mask_rects`, `lease_ms`.

- [ ] **Step 1: Add RED instrumentation contract tests** requiring exact-token rejection, lease renewal, refreshed masks after scroll, fixed/sticky counting, and absence of serialized selector strings.
- [ ] **Step 2: Implement private active-freeze state** so selectors remain page-local and are cleared on restore/lease expiry.
- [ ] **Step 3: Implement `stitchProbe`** with finite geometry normalization, rendered fixed/sticky scan, current viewport-relative mask geometry, and lease renewal.
- [ ] **Step 4: Implement `stitchScroll`** preserving original horizontal scroll, using `window.scrollTo`, reading actual clamped offsets, then delegating receipt production to the same probe authority.
- [ ] **Step 5: Wire desktop queued-action JS execution cases** for `stitch_probe` and `stitch_scroll`; validate token and finite `y` before page invocation.
- [ ] **Step 6: Run instrumentation + desktop source-contract tests and commit GREEN**.

---

## Task 4: Full-page daemon evidence schema

**Files:**
- Create: `crates/control/src/visual_full_page.rs`
- Modify: `crates/control/src/lib.rs` or router aggregation file that currently mounts `visual_region`.
- Create: `crates/control/tests/full_page_visual_evidence.rs`

**Interfaces:**
- Route: `POST /v1/sessions/{id}/evidence/visual-full-page`.
- Request fields exactly match spec provenance, including `target = "full_page"`, document dimensions, tile count, DSF, scroll offsets, original scroll coordinates, `fixed_or_sticky_count = 0`, and `redaction_applied = true`.
- Response: existing evidence ingestion shape `{ evidence_id, deduplicated }`.

- [ ] **Step 1: Add RED endpoint tests** for unknown fields, NaN/non-finite numeric representations where JSON permits, invalid tile count, non-monotonic offsets, fixed/sticky nonzero, `redaction_applied=false`, non-loopback/noncanonical route behavior matching existing visual contracts, and session/artifact provenance failures.
- [ ] **Step 2: Implement fail-closed request validation and evidence creation** following `visual_region.rs` conventions; do not accept caller verdicts.
- [ ] **Step 3: Mount the router and run control tests GREEN**:

```bash
cargo test -p localview-control full_page_visual_evidence
cargo test -p localview-control visual_evidence
```

---

## Task 5: Desktop guarded full-page transaction

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`

**Interfaces:**
- Produces Tauri command:

```rust
capture_full_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<FullPageCaptureReceipt, String>
```

- `FullPageCaptureReceipt` references only the final artifact/evidence and trusted full-page provenance.

- [ ] **Step 1: Add RED transaction/source-contract tests** locking command registration, one session gate, settle/freeze order, exact-token scroll/probe calls, pre/post probe bracket, redaction-before-stitch, original scroll restore, visual restore, final-only persistence, and full-page evidence endpoint.
- [ ] **Step 2: Add typed stitch receipt parsing** with `serde(deny_unknown_fields)` and finite/range validation.
- [ ] **Step 3: Implement helper to enqueue/correlate internal stitch actions** using the same exact-session action-result authority as freeze/restore.
- [ ] **Step 4: Implement guarded tile loop** with drift checks, per-tile settle, fixed/sticky zero check, pre/post mask equality, native viewport capture reuse, immediate redaction, decode, and stitch push.
- [ ] **Step 5: Implement restoration guard** so every post-freeze exit attempts original scroll restoration and exact visual restore; persistence remains unreachable unless both acknowledgements succeed.
- [ ] **Step 6: Implement final retained-resource admission/persistence and full-page evidence registration**. Preserve truth that an evidence-registration error after successful `ArtifactStore::put` fails the command but does not claim nonexistent rollback.
- [ ] **Step 7: Run focused desktop tests GREEN**.

---

## Task 6: Regression, docs truth, and exact-head CI closure

**Files:**
- Modify after implementation gates are green: `docs/ROADMAP.md`
- Modify after implementation gates are green: `docs/IMPLEMENTATION_STATUS.md`
- Modify after implementation gates are green: `docs/SPEC_COVERAGE.md`

- [ ] **Step 1: Run/require core Rust regression**:

```bash
cargo test --workspace
```

- [ ] **Step 2: Require existing native rendered-pixel workflows** for Linux/WebKitGTK, macOS/WKWebView and Windows/WebView2 to remain GREEN. No platform adapter gets a full-page API.
- [ ] **Step 3: Update docs truth** to mark guarded full-page stitching landed while preserving explicit limits: vertical-only, fixed/sticky rejection, bounded document/tile/pixel sizes, no Chromium fallback.
- [ ] **Step 4: Re-run exact-head CI after docs commit** and require no failing/queued/in-progress required check before integration.
- [ ] **Step 5: Review diff for privacy/authority regressions** and verify no public endpoint accepts selectors, mask rectangles, tile offsets, document dimensions, evidence verdicts or trusted IDs from callers.
- [ ] **Step 6: Use `superpowers:verification-before-completion`, then `superpowers:finishing-a-development-branch` before marking the wave complete.**
