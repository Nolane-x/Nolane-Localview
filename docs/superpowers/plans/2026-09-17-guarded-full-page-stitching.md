# Guarded Full-Page Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a bounded, fail-closed native full-page capture transaction over the existing LocalView-managed viewport capture authority, with token-bound absolute scrolling, per-tile privacy redaction, deterministic stitching, exact restoration, and dedicated provenance evidence.

**Architecture:** Keep WebView2/WKWebView/WebKitGTK adapters viewport-only. Add pure planner/stitcher logic in `localview-visual`, two internal capture actions in `localview-live-bridge`, page-side execution and narrow control endpoints, then orchestrate the whole transaction in desktop `visual_capture.rs` under one session gate and one freeze token. Every behavior change follows RED -> verify RED -> GREEN -> verify GREEN; any drift, budget overflow, fixed/sticky content, restore failure, or privacy uncertainty aborts without persisting full-page evidence.

**Tech Stack:** Rust workspace, Tauri desktop coordinator, Axum control plane, LocalView live bridge/instrumentation JavaScript, `image` RGBA/PNG handling, existing native capture adapters, GitHub Actions cross-platform CI.

**Spec:** `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`

## Global Constraints

- No Chromium/Playwright/Puppeteer full-page fallback.
- Native platform adapters remain viewport-only.
- One per-session capture gate owns the complete full-page transaction.
- One visual-freeze token owns all internal scroll/probe actions.
- Generic public `Scroll` is never reused for stitching.
- Private-mask geometry is refreshed after every scroll and applied before tile pixels enter the stitcher.
- Any visible `position: fixed` or `position: sticky` element fails closed in this first slice.
- Any route, viewport, document geometry, device-scale, backend, native pixel geometry, token, or action-result correlation drift fails closed.
- `max_tiles = 32`.
- `max_document_css_height = 50_000` CSS px.
- `max_output_rgba_bytes = 128 MiB`.
- `max_output_pixel_height = 32_768` px.
- Positional scan authority is bounded to 4,096 elements.
- Original scroll position and visual state must be restored before final artifact/evidence persistence.
- Intermediate tile PNG/RGBA buffers are ephemeral and never independently persisted or registered as evidence.
- Production code must never precede its failing test.

---

## File Structure

- `crates/visual/src/lib.rs`: expose full-page planner/stitcher API.
- `crates/visual/src/full_page.rs`: pure deterministic planning, output-budget validation, scroll tolerance, row-copy stitching.
- `crates/visual/tests/full_page_stitching.rs`: planner/stitcher RED/GREEN contract tests.
- `crates/live-bridge/src/lib.rs`: add internal `CaptureScrollTo` / `CaptureTileProbe` action kinds and internal-action classification.
- `crates/live-bridge/tests/full_page_capture_actions.rs`: serialization, internal-only authority, bounded private envelope/result behavior.
- `apps/desktop/src-tauri/src/lib.rs`: managed-WebView executor support for token-bound absolute scroll, current geometry/private-mask probe, positional scan.
- `apps/desktop/src-tauri/tests/full_page_bridge_contract.rs`: source/behavior contract around executor authority and sanitization.
- `crates/control/src/capture_visual_state.rs` or existing capture-state module used by the current freeze endpoints: extend receipts and add narrow scroll/probe endpoints without exposing arbitrary script.
- `crates/control/tests/full_page_capture_control.rs`: auth/session/action-correlation/payload validation tests.
- `apps/desktop/src-tauri/src/visual_capture.rs`: full transaction coordinator and Tauri command.
- `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`: orchestration order, restore-before-persist, no-artifact-on-failure, per-tile mask refresh, budget/fixed-sticky rejection.
- `crates/control/src/runtime.rs` plus evidence tests only if dedicated full-page evidence ingestion is owned there in the current tree.
- `docs/IMPLEMENTATION_STATUS.md`, `docs/ROADMAP.md`, `docs/SPEC_COVERAGE.md`: update only after exact-head executable evidence is green.

---

### Task 1: Pure deterministic full-page planner and stitcher

**Files:**
- Create: `crates/visual/src/full_page.rs`
- Modify: `crates/visual/src/lib.rs`
- Create: `crates/visual/tests/full_page_stitching.rs`

**Interfaces:**
- Produces `FullPagePolicy`, `FullPagePlan`, `FullPagePlanError`, `FullPageStitchError`.
- Produces `plan_full_page(document_css_width, document_css_height, viewport_css_width, viewport_css_height, original_scroll_y, policy) -> Result<FullPagePlan, FullPagePlanError>`.
- Produces `project_output_height_px(document_css_height, viewport_css_height, tile_pixel_height, policy) -> Result<u32, FullPagePlanError>`.
- Produces `scroll_tolerance_css(scale_y: f64) -> Result<f64, FullPagePlanError>`.
- Produces `stitch_full_page_tile(output: &mut RgbaImage, actual_scroll_y: f64, scale_y: f64, tile: &RgbaImage) -> Result<(), FullPageStitchError>`.

- [ ] **Step 1: Write RED planner tests** for one-tile pages, multi-tile pages, exact divisibility, clamped final offset, 32-tile boundary, 33-tile rejection, non-finite dimensions, width mismatch, `50_000` CSS-px boundary and overflow rejection.

```rust
#[test]
fn planner_adds_exact_bottom_clamped_tile_without_duplicate_offset() {
    let policy = FullPagePolicy::default();
    let plan = plan_full_page(1200.0, 2500.0, 1200.0, 1000.0, 0.0, policy).unwrap();
    assert_eq!(plan.scroll_offsets_y, vec![0.0, 1000.0, 1500.0]);
}
```

- [ ] **Step 2: Verify RED** with `cargo test -p localview-visual --test full_page_stitching planner_ -- --nocapture`; expected failure is unresolved full-page symbols, not compile errors unrelated to the test.
- [ ] **Step 3: Implement minimal planner/policy** in `full_page.rs`; reject caller-supplied offset lists by not exposing such an API.
- [ ] **Step 4: Verify planner GREEN** with the same targeted test command.
- [ ] **Step 5: Write RED pixel-budget/stitch tests** for fractional scale, partial final tile, exact overlap overwrite, width mismatch, out-of-range placement, `32_768` output height, `128 MiB` RGBA boundary, and checked arithmetic overflow.

```rust
#[test]
fn stitcher_places_fractional_scale_tile_from_acknowledged_scroll() {
    let mut output = RgbaImage::new(4, 6);
    let tile = RgbaImage::from_pixel(4, 2, image::Rgba([1, 2, 3, 255]));
    stitch_full_page_tile(&mut output, 2.0, 1.5, &tile).unwrap();
    assert_eq!(output.get_pixel(0, 3).0, [1, 2, 3, 255]);
}
```

- [ ] **Step 6: Verify RED**, then implement bounded row-copy stitching using checked row/byte arithmetic and no tile vector retention.
- [ ] **Step 7: Run `cargo test -p localview-visual`** and `cargo clippy -p localview-visual --all-targets -- -D warnings`.
- [ ] **Step 8: Commit** `feat(visual): add bounded full-page planner and stitcher`.

### Task 2: Internal capture action authority

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Create: `crates/live-bridge/tests/full_page_capture_actions.rs`

**Interfaces:**
- Add `BridgeActionKind::CaptureScrollTo { token: Uuid, y: f64 }`.
- Add `BridgeActionKind::CaptureTileProbe { token: Uuid }`.
- Both must return `true` from `is_internal_capture_action()`.
- Reuse `PrivateCaptureActionData { mask_selectors }` only through the private capture queue; selectors never appear in public `BridgeActionResult` payloads.

- [ ] **Step 1: Write RED serialization/internal-classification tests** including snake_case tagged JSON and generic/public queue rejection behavior.
- [ ] **Step 2: Verify RED** via `cargo test -p localview-live-bridge --test full_page_capture_actions -- --nocapture`.
- [ ] **Step 3: Add the two enum variants and internal classification only; do not alter public `Scroll` semantics.**
- [ ] **Step 4: Verify GREEN** targeted and then `cargo test -p localview-live-bridge`.
- [ ] **Step 5: Add RED queue-bound tests** proving private selector envelopes remain bounded/sanitized for tile probes and internal result queues obey existing capacities.
- [ ] **Step 6: Implement the minimal queue plumbing needed by those tests; no new unbounded queue/map.**
- [ ] **Step 7: Run clippy for `localview-live-bridge`.**
- [ ] **Step 8: Commit** `feat(live-bridge): add internal full-page capture actions`.

### Task 3: Managed WebView executor for token-bound scroll and probe

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/full_page_bridge_contract.rs`

**Interfaces:**
- `capture_scroll_to` executor path validates active freeze token, finite non-negative Y, preserves original frozen X, performs absolute `window.scrollTo`, waits two animation frames, and returns bounded geometry metadata.
- `capture_tile_probe` executor path validates token, returns current scroll/document/viewport geometry, recomputes private masks for the current viewport, and returns only counts + rectangles + positional scan counts.
- Shared page-side document geometry helper is used by freeze and every probe.

- [ ] **Step 1: Write RED source/contract tests** proving `window.scrollTo` is absolute, token matching occurs before scroll/probe, public `scrollBy` remains unchanged, two animation-frame acknowledgement exists, and selector strings are not returned.
- [ ] **Step 2: Verify RED** with `cargo test -p localview-desktop --test full_page_bridge_contract -- --nocapture` using the package name already used by existing desktop tests.
- [ ] **Step 3: Implement token-bound executor cases** next to existing `freeze_visuals`/`restore_visuals` handling.
- [ ] **Step 4: Add RED positional-scan tests** requiring at most 4,096 inspected elements, visible-box filtering, `position: fixed|sticky` counting, and scan-budget failure rather than silent truncation.
- [ ] **Step 5: Implement the bounded scan and shared document-geometry helper.**
- [ ] **Step 6: Add RED per-tile privacy tests** proving masks are recomputed after current scroll and `MAX_PRIVATE_MASK_RECTS` / `MAX_MASKED_ELEMENTS` failures are preserved.
- [ ] **Step 7: Implement only the minimal probe wiring to existing `privateMaskGeometry`.**
- [ ] **Step 8: Run desktop targeted tests plus the existing `live_semantic_bridge_contract` and `visual_freeze_capture_contract`.**
- [ ] **Step 9: Commit** `feat(desktop): execute guarded full-page bridge actions`.

### Task 4: Control-plane receipts and narrow internal endpoints

**Files:**
- Modify the existing module that owns `/capture-freeze` and `/capture-restore` receipts/routes.
- Modify router registration where those routes are mounted.
- Create: `crates/control/tests/full_page_capture_control.rs`

**Interfaces:**
- Extend freeze receipt with `scroll_x`, `scroll_y`, `document_css_width`, `document_css_height` while preserving existing viewport/region callers.
- Add authenticated narrow endpoints for exact internal scroll and probe operations, accepting only token + requested Y for scroll and token for probe.
- Return bounded typed receipts; reject unknown/malformed/non-finite fields.
- Wait for exact action id/result correlation with existing bounded acknowledgement timeout.

- [ ] **Step 1: Write RED freeze-receipt compatibility tests** showing new numeric fields are required/validated for full-page use while existing viewport capture remains behaviorally unchanged.
- [ ] **Step 2: Verify RED** with the targeted control test.
- [ ] **Step 3: Extend receipt parsing/validation without weakening existing mask/viewport limits.**
- [ ] **Step 4: Write RED scroll endpoint tests** for auth failure, missing session, wrong result id, mismatched token, non-finite Y, stale result, timeout, and sanitized success payload.
- [ ] **Step 5: Implement scroll endpoint using only `CaptureScrollTo`.**
- [ ] **Step 6: Write RED probe endpoint tests** for auth/session/correlation, fixed-sticky count, scan-budget error, mask-budget error and absence of selectors/text/URL/storage data.
- [ ] **Step 7: Implement probe endpoint using only `CaptureTileProbe` with private selector envelope.**
- [ ] **Step 8: Run `cargo test -p localview-control` and clippy.**
- [ ] **Step 9: Commit** `feat(control): add full-page capture control contracts`.

### Task 5: Dedicated full-page evidence contract

**Files:**
- Modify: `crates/control/src/runtime.rs` or the current visual-evidence owner module.
- Create: `crates/control/tests/full_page_visual_evidence.rs`.

**Interfaces:**
- Add a dedicated full-page visual evidence ingestion type/route rather than reusing ordinary viewport evidence with only `target = "full_page"`.
- Payload includes final artifact id/reference, canonical route, document/viewport CSS geometry, native pixel output dimensions, device scale/backend provenance, tile count, acknowledged scroll offsets and capture transaction timing metadata.
- Payload excludes freeze token, private selectors, mask rectangles, tile bytes and filesystem paths.

- [ ] **Step 1: Write RED evidence schema tests** for valid provenance and rejection of unknown fields, zero/excess tile count, non-monotonic offsets, non-finite geometry, route mismatch shape, and leaked forbidden fields.
- [ ] **Step 2: Verify RED** with `cargo test -p localview-control --test full_page_visual_evidence -- --nocapture`.
- [ ] **Step 3: Implement strict request type, validation and evidence insertion using existing bearer/session authority.**
- [ ] **Step 4: Verify targeted GREEN plus existing visual evidence/diff/region tests.**
- [ ] **Step 5: Commit** `feat(control): add full-page visual evidence provenance`.

### Task 6: Desktop full-page transaction orchestration

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify Tauri command registration only where required.
- Create: `apps/desktop/src-tauri/tests/full_page_capture_contract.rs`

**Interfaces:**
- Add `capture_full_page(...)` Tauri command mirroring existing authenticated managed-surface capture entry conventions.
- Transaction order is exactly: preflight -> session gate -> settle -> freeze -> plan -> for each offset: scroll -> settle -> probe -> fixed/sticky check -> native capture -> geometry check -> redact -> stitch -> restore original scroll -> verify scroll -> restore visuals -> encode final -> retained-resource admission -> persist one artifact -> register dedicated evidence.
- Cleanup path always attempts original-scroll restoration and visual restore after freeze, but persistence requires both to succeed.

- [ ] **Step 1: Write RED ordering contract test** asserting source/behavior order and that persistence occurs only after successful scroll restoration and `RestoreVisuals` acknowledgement.
- [ ] **Step 2: Verify RED** targeted.
- [ ] **Step 3: Add minimal `capture_full_page` skeleton sufficient to satisfy only transaction-order construction tests, without yet persisting on missing tile execution.**
- [ ] **Step 4: Write RED happy-path transaction test** with a deterministic two/three-tile fake managed capture path proving per-tile mask refresh, one freeze token, one session gate, ephemeral tiles and one final artifact/evidence record.
- [ ] **Step 5: Implement planner invocation, scroll/settle/probe loop, native capture, redaction and streaming stitch.**
- [ ] **Step 6: Write RED fail-closed tests** separately for route drift, viewport drift, document growth/shrink, DSF drift, native pixel mismatch, backend mismatch, wrong offset ack, token expiry, fixed/sticky detection, scan-budget failure, tile budget, output-memory budget, native capture failure and final encode/resource-admission failure.
- [ ] **Step 7: Implement minimal fail-closed guards for each failing test; no heuristic recovery.**
- [ ] **Step 8: Write RED restoration tests** for mid-loop failure, final scroll-restore failure, visual-restore failure, and both capture+restore failure; assert zero final artifact/evidence persistence on every failure.
- [ ] **Step 9: Implement a single cleanup/result path that preserves the primary bounded error while still attempting token-owned restoration.**
- [ ] **Step 10: Run targeted desktop test plus all existing visual capture/freeze/private-redaction/resource-retention tests.**
- [ ] **Step 11: Commit** `feat(desktop): orchestrate guarded full-page capture`.

### Task 7: Regression, cross-platform build authority, and documentation closure

**Files:**
- Modify only after executable GREEN: `docs/IMPLEMENTATION_STATUS.md`, `docs/ROADMAP.md`, `docs/SPEC_COVERAGE.md`.
- Add a focused workflow only if existing CI does not exercise the new package/tests on all required desktop OS targets; otherwise reuse existing CI.

**Interfaces:**
- No documentation may say full-page stitching is complete until exact-head CI proves Linux/macOS/Windows compile/test coverage appropriate to the existing native capture matrix.

- [ ] **Step 1: Run local/workspace verification**: `cargo fmt --all -- --check`, targeted package tests from Tasks 1–6, `cargo test --workspace` where supported by repository CI conventions, and `cargo clippy --workspace --all-targets -- -D warnings` where supported.
- [ ] **Step 2: Fix only failures introduced by this wave; every bug fix starts with a reproducing RED test.**
- [ ] **Step 3: Update docs** to distinguish shipped guarded full-page stitching from unsupported fixed/sticky pages, expanding/infinite documents, horizontal stitching and Chromium fallback.
- [ ] **Step 4: Commit** `docs: record guarded full-page stitching closure`.
- [ ] **Step 5: Push exact head and require GitHub Actions completion with no `failure`, `cancelled`, `timed_out`, `action_required`, `startup_failure`, `queued`, `in_progress`, or null conclusion before marking the implementation PR ready.**
- [ ] **Step 6: Review the final diff against the spec**: every spec invariant maps to code + test; scan for `TODO`, `TBD`, unbounded collections, public exposure of internal tokens/selectors, and accidental platform full-page APIs.
- [ ] **Step 7: Merge only with expected-head SHA guard after exact-head CI is green.**

## Self-Review Result

- Spec coverage: planner, scrolling, freeze-token authority, per-tile masks, fixed/sticky rejection, stable-settle, fractional scale, memory/tile bounds, restoration, dedicated evidence, native viewport-only adapters and failure behavior all map to explicit tasks/tests.
- Placeholder scan: no `TBD`, `TODO`, `implement later`, or unspecified error-handling steps are permitted in execution; the plan names concrete failure cases and commands.
- Type consistency: `CaptureScrollTo { token, y }` and `CaptureTileProbe { token }` are the only new bridge actions; `FullPagePolicy`/`FullPagePlan` are owned by `localview-visual`; desktop remains orchestration authority; evidence registration is control-plane owned.

## Execution Handoff

Recommended execution for this environment: **Inline Execution** using `superpowers:executing-plans`, because the available GitHub workflow can preserve exact RED/green commit evidence task-by-task and no independent subagent runtime is exposed in this chat. Start with Task 1 RED tests only; do not write planner production code until the failing CI/test evidence is captured.
