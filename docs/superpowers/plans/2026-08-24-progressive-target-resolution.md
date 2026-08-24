# Progressive Target Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect fresh semantic ownership/geometry to LocalView's existing audited native region-capture path so an `ElementRef` can resolve deterministically to element/component/section/viewport targets without fabricating framework ownership.

**Architecture:** `localview-capture` owns a pure deterministic resolver over `PageSnapshot`; the authenticated control plane exposes one narrow fresh-snapshot endpoint using the existing snapshot action path; the desktop coordinator binds the resolved plan to route/viewport and reuses the existing single native viewport acquisition, restore, private-redaction, crop, and evidence pipeline. Platform capture adapters remain viewport-only.

**Tech Stack:** Rust 2024 workspace, Axum loopback control plane, Tauri 2 desktop, existing LiveBridge snapshot actions, `localview-capture`, `localview-protocol`, GitHub Actions cross-platform CI.

**Spec:** `docs/superpowers/specs/2026-08-24-progressive-target-resolution-design.md`

## Global Constraints

- Do not implement framework-specific React/Vue/Svelte ownership in this slice.
- Do not fabricate component ownership from tag/class/depth heuristics.
- Platform capture adapters must remain `CaptureTarget::Viewport` only.
- Target resolution uses semantic metadata; private pixel processing remains restore → validation → redaction → crop → persistence.
- Missing/invalid ownership, geometry, route, or viewport fails closed.
- Existing 120 CSS-pixel element expansion semantics remain authoritative.
- Final completion requires exact-head cross-platform CI plus all three real rendered-pixel GUI smoke gates.

---

### Task 1: Deterministic progressive target resolver

**Files:**
- Modify: `crates/capture/src/lib.rs`
- Create: `crates/capture/tests/progressive_target_resolution.rs`

**Interfaces:**
- Consumes: `PageSnapshot`, `SemanticNode`, `ElementRef`, `Rect` from `localview-protocol`.
- Produces: `ProgressiveTargetKind`, `ProgressiveTargetProvenance`, `ProgressiveResolvedTarget`, `ProgressiveTargetPlan`, `ProgressiveTargetError`, and `resolve_progressive_targets(snapshot: &PageSnapshot, reference: &str) -> Result<ProgressiveTargetPlan, ProgressiveTargetError>`.

- [ ] **Step 1: Write failing behavioral tests**

Add tests that construct semantic trees and assert: stable-ref lookup; 120px expanded/clamped element target; nearest same-`source.component` ancestor wins; missing source component emits no component target; nearest explicit section/landmark ancestor wins; duplicate rects are removed in target order; invalid/fully-offscreen geometry and missing refs fail closed.

- [ ] **Step 2: Run resolver test to verify RED**

Run: `cargo test -p localview-capture --test progressive_target_resolution`
Expected: compile failure because resolver types/functions do not exist.

- [ ] **Step 3: Implement minimal pure resolver**

Implement a root→target ancestry walk. Validate viewport and target rect. Build element rect using existing 120px expansion/clamp behavior. Resolve component only from corroborated `source.component` on an ancestor. Resolve section only from accepted semantic tags/roles. Append viewport fallback and stable-deduplicate equal rectangles.

- [ ] **Step 4: Run resolver tests and crate checks**

Run: `cargo test -p localview-capture --test progressive_target_resolution && cargo clippy -p localview-capture --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: resolve progressive semantic capture targets`

### Task 2: Authenticated fresh semantic snapshot endpoint

**Files:**
- Modify: `crates/control/src/runtime.rs` or add a focused control module if that keeps the route isolated.
- Modify: `crates/control/src/lib.rs` only if a new module/router is introduced.
- Create: `crates/control/tests/fresh_semantic_snapshot.rs`

**Interfaces:**
- Consumes: existing LiveBridge snapshot action queue/result path.
- Produces: authenticated `GET /v1/sessions/{id}/semantic-snapshot/fresh` returning one bounded `PageSnapshot`-compatible semantic snapshot payload from a newly completed snapshot action.

- [ ] **Step 1: Write failing control tests**

Tests must prove: no bearer token → 401; unknown session → 404; endpoint enqueues a new snapshot action for the requested session; stale observer history alone cannot satisfy the request; only the matching completed action result is accepted; malformed/missing snapshot payload fails closed.

- [ ] **Step 2: Run control test to verify RED**

Run: `cargo test -p localview-control --test fresh_semantic_snapshot`
Expected: route/API missing.

- [ ] **Step 3: Implement narrow endpoint using existing action path**

Reuse existing action queue/claim/result semantics rather than creating a second snapshot transport. Bound waiting time and payload parsing. Keep route authenticated/session-scoped.

- [ ] **Step 4: Run control tests and clippy**

Run: `cargo test -p localview-control --test fresh_semantic_snapshot && cargo clippy -p localview-control --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: expose fresh semantic snapshot control gate`

### Task 3: Live desktop progressive target capture

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/progressive_target_capture_contract.rs`

**Interfaces:**
- Consumes: `resolve_progressive_targets`, fresh snapshot endpoint, existing `capture_redacted_viewport_after_gate`, `RequestedCaptureTarget`, `persist_and_register`.
- Produces: Tauri command `capture_progressive_target(app, state, session_id, reference, viewport, revision, level)` and a serialized receipt containing the selected target kind/provenance plus underlying visual evidence receipt.

- [ ] **Step 1: Write failing desktop contract**

Contract must assert: command registration; fresh snapshot fetch before target resolution; caller viewport equals snapshot viewport; requested level is explicit and cannot silently widen; one call to shared native acquisition helper; platform request remains `CaptureTarget::Viewport`; exact restore occurs before live viewport validation/redaction; redaction occurs before crop/persistence; region evidence carries resolved rect.

- [ ] **Step 2: Run desktop contract to verify RED**

Run: `cargo test -p localview-desktop --test progressive_target_capture_contract`
Expected: missing command/live path.

- [ ] **Step 3: Implement desktop command**

Add bounded level enum (`element|component|section|viewport`), fetch/parse fresh snapshot, call deterministic resolver, require snapshot viewport to match the caller viewport, select exact requested target or fail closed, then reuse the existing shared redacted native transaction exactly once and persist as viewport/region evidence.

- [ ] **Step 4: Run desktop compiler/contracts**

Run: `cargo check -p localview-desktop && cargo check -p localview-desktop --features native-workspace && cargo test -p localview-desktop --test progressive_target_capture_contract && cargo test -p localview-desktop --test visual_freeze_capture_contract && cargo test -p localview-desktop --test changed_region_schedule_contract`
Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: capture progressive semantic targets`

### Task 4: CI authority and adversarial coverage

**Files:**
- Modify: `.github/workflows/ci.yml`
- Extend: `crates/capture/tests/progressive_target_resolution.rs`
- Extend: `crates/control/tests/fresh_semantic_snapshot.rs`
- Extend: `apps/desktop/src-tauri/tests/progressive_target_capture_contract.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: explicit CI gates preventing regression of ownership provenance, freshness, one-acquisition semantics, and privacy ordering.

- [ ] **Step 1: Add adversarial tests**

Cover NaN/infinite/zero/offscreen geometry; target source component with mismatched ancestor component; same-rect component/section dedupe; route/viewport mismatch; malformed snapshot action result; requested component/section missing; duplicate native acquisition forbidden by contract.

- [ ] **Step 2: Add CI commands**

Add `cargo test -p localview-capture --test progressive_target_resolution`, `cargo test -p localview-control --test fresh_semantic_snapshot`, and `cargo test -p localview-desktop --test progressive_target_capture_contract` to the existing authority jobs without removing old contracts.

- [ ] **Step 3: Run full local-equivalent targeted gate**

Run all tests from Tasks 1–3 plus `cargo fmt --all -- --check` and relevant clippy commands.
Expected: PASS.

- [ ] **Step 4: Commit**

Commit message: `test: gate progressive target capture`

### Task 5: Status/coverage alignment and final PR verification

**Files:**
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/SPEC_COVERAGE.md`
- Modify: `docs/ROADMAP.md`

**Interfaces:**
- Consumes: verified live path from Tasks 1–4.
- Produces: documentation that distinguishes live ownership-driven targeting from still-missing token-aware policy/framework-specific ownership/verification loop.

- [ ] **Step 1: Update docs without overclaiming**

Record that semantic element + evidence-backed component + semantic section + viewport target resolution is live. Keep `Progressive capture regions` Partial because token-aware visual packet selection, framework-specific ownership depth, guarded stitching, and complete verification remain.

- [ ] **Step 2: Open/update Draft PR and run exact-head GitHub CI**

Require success for three Rust core jobs, Tauri/frontend including all desktop contracts, and real GUI smoke on WebView2/WKWebView/WebKitGTK.

- [ ] **Step 3: Fresh review gate**

Review complete diff; list PR comments/reviews/threads; fix all Critical/Important issues; confirm `main` base has not drifted or rebase/reverify if it has.

- [ ] **Step 4: Mark Ready and merge with expected head SHA**

Only after exact final head has green CI. Use expected-head guard.

- [ ] **Step 5: Verify merged main tree**

Confirm `main` points to merge commit and merge tree matches the exact tested PR head tree, or run post-merge CI if the tree differs.
