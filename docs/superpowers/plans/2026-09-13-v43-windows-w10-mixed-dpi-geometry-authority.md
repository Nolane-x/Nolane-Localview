# V4.3 Windows W10 Mixed-DPI Geometry Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a lineage-bound Windows UIA physical-geometry observation path and close W10 only when a real distinct-DPI topology is actually observed.

**Architecture:** Preserve the generic semantic snapshot schema. Add a Windows-specific exact-element geometry request/receipt on the existing MTA worker, extend the test-only WPF seed with independent physical geometry/DPI oracle facts, and make the real W10 campaign capability-bound rather than simulated.

**Tech Stack:** Rust, windows-rs, Windows UI Automation, Win32 User32, WPF/.NET 8, Tokio, serde/serde_json, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-13-v43-windows-w10-mixed-dpi-geometry-authority-design.md`

## Global Constraints

- UIA BoundingRectangle remains physical screen pixels; never scale it by target DPI.
- No geometry string convention inside `NativeSemanticNodeObservation.attributes`.
- W10 pass requires two nonzero, distinct effective DPI values observed on real Windows placements.
- If distinct DPI is unavailable, W10 remains explicitly unmeasured and the existing 11-case campaign is unchanged.
- Production crates never depend on the WPF seed/oracle harness.
- No pointer input, monitor-setting mutation, global DPI-awareness mutation, retry-until-green, or broad `SUPPORTED` claim.
- Every production change follows RED -> minimal GREEN -> regression verification.

---

### Task 1: Define pure W10 Lab semantics

**Files:**
- Modify: `crates/validation-lab/src/real_provider_adapter.rs`
- Create: `crates/validation-lab/tests/real_provider_w10.rs`

**Interfaces:**
- Produces `RealProviderCaseKind::W10MixedDpiGeometry`.
- Reuses RPOMR behavior unchanged.

- [ ] **Step 1: Write RED tests** for a measured pass with distinct DPI, explicit physical coordinate space, two oracle matches, no double scaling; add counterexamples for equal DPI, missing coordinate-space authority, either rectangle mismatch, and double scaling.
- [ ] **Step 2: Run** `cargo test -p localview-validation-lab --test real_provider_w10 -- --nocapture` and require compile RED because the W10 variant is absent.
- [ ] **Step 3: Add minimal enum variant**:

```rust
W10MixedDpiGeometry {
    distinct_effective_dpi_observed: bool,
    coordinate_space_explicit: bool,
    first_rect_matches_oracle: bool,
    second_rect_matches_oracle: bool,
    double_scaling_observed: bool,
},
```

Reducer counterexample condition:

```rust
!distinct_effective_dpi_observed
    || !coordinate_space_explicit
    || !first_rect_matches_oracle
    || !second_rect_matches_oracle
    || double_scaling_observed
```

- [ ] **Step 4: Run W10 and full adapter tests** and require GREEN.
- [ ] **Step 5: Commit** `test(v43): define W10 mixed-DPI Lab semantics`.

### Task 2: Add typed Windows geometry contracts

**Files:**
- Create: `crates/windows-uia-provider/src/geometry.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Create: `crates/windows-uia-provider/tests/geometry_contract.rs`

**Interfaces:**
- Produces `WindowsUiaCoordinateSpace::PhysicalScreenPixels`.
- Produces validated `WindowsUiaPhysicalRect`.
- Produces `WindowsUiaGeometryRequest::new(snapshot_cut_ref, element_ref)`.
- Produces `WindowsUiaGeometryReceipt`.

- [ ] **Step 1: Write RED contract tests** requiring nonempty cut, exact element ref retention, inverted-rect rejection, and coordinate-space serialization.
- [ ] **Step 2: Run** `cargo test -p localview-windows-uia-provider --test geometry_contract -- --nocapture` and require compile RED.
- [ ] **Step 3: Implement the pure types** in `geometry.rs`; `WindowsUiaPhysicalRect::new(left, top, right, bottom)` rejects `right < left || bottom < top` and exposes width/height as checked differences.
- [ ] **Step 4: Re-export the module** from the provider crate without exposing any mutation or input API.
- [ ] **Step 5: Run geometry contract plus existing provider contract tests** and require GREEN.
- [ ] **Step 6: Commit** `feat(v43): add typed Windows UIA geometry contracts`.

### Task 3: Bind geometry observation to the exact retained UIA element

**Files:**
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Test: `crates/windows-uia-provider/tests/windows_geometry_worker_contract.rs`

**Interfaces:**
- Adds `WindowsUiaWorker::observe_geometry(&WindowsUiaAttachment, WindowsUiaGeometryRequest) -> Result<WindowsUiaGeometryReceipt, WindowsUiaWorkerError>`.
- Adds worker command `ObserveGeometry`.

- [ ] **Step 1: Write RED source/Windows contract** proving stale cut/element lineage cannot be used and non-Windows stub returns `UnsupportedPlatform`.
- [ ] **Step 2: Add `ObserveGeometry` command** to the same worker MTA. Reuse `exact_retained_element`; do not reacquire by AutomationId/name.
- [ ] **Step 3: Read `CurrentBoundingRectangle()`** from the retained `IUIAutomationElement`; map its native RECT directly into `WindowsUiaPhysicalRect`.
- [ ] **Step 4: Read `GetDpiForWindow(exact attachment HWND)`**; `0` is `ProviderFailure` and cannot mint a receipt.
- [ ] **Step 5: Mint receipt** with exact provider/target/cut/element lineage and `PhysicalScreenPixels`.
- [ ] **Step 6: Run focused provider tests plus Windows UIA compile/check** and require GREEN.
- [ ] **Step 7: Commit** `feat(v43): bind UIA physical geometry to exact retained element`.

### Task 4: Extend WPF edge seed with independent geometry/DPI oracle

**Files:**
- Modify: `tools/provider-seeds/windows-uia-edge-seed/EdgeWindow.cs`
- Modify: `tools/provider-seeds/windows-uia-edge-seed/OracleProtocol.cs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_seed.rs` or split a focused `v43_geometry_seed.rs` support module.

**Interfaces:**
- Adds deterministic geometry target AutomationId `LocalViewW10GeometryTarget`.
- Adds oracle state for target physical rectangle, HWND, effective window DPI, monitor identity, and available monitor/DPI catalog.
- Adds bounded move-to-monitor command.

- [ ] **Step 1: Add RED harness registration** that references the new oracle commands and geometry target.
- [ ] **Step 2: Add a visible WPF geometry target** with stable size/layout and AutomationId.
- [ ] **Step 3: Derive physical target rectangle independently** from WPF screen projection/device transform; do not call LocalView/UIA to establish oracle truth.
- [ ] **Step 4: Enumerate monitor placements and effective DPI values** through seed-owned Win32 calls; retain only non-sensitive topology facts.
- [ ] **Step 5: Add a move command** that places the seed wholly on a selected monitor and waits for WPF layout/render settling using an observed condition rather than a blind long sleep.
- [ ] **Step 6: Build the WPF seed and compile the harness**.
- [ ] **Step 7: Commit** `test(v43): add W10 physical geometry oracle fixture`.

### Task 5: Prove real W10 when heterogeneous DPI exists

**Files:**
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w10.rs`
- Create/modify focused support for W10 records.

**Interfaces:**
- Produces a measured `RealProviderLabRecord` only when two distinct DPI placements exist.
- Produces a capability result when they do not.

- [ ] **Step 1: Write real W10 test**: enumerate topology; select A/B with distinct DPI; move to A; fresh attach/snapshot; observe exact target geometry; compare physical rect to oracle; move to B; take a new cut; repeat; require `dpi_a != dpi_b` and no double scaling.
- [ ] **Step 2: If no distinct-DPI pair exists, return an explicit capability-unavailable outcome from the harness** and do not call `adapt_real_provider_case`.
- [ ] **Step 3: Run on Windows** with exact single test. A topology with heterogeneous DPI must GREEN; a homogeneous hosted runner must report unmeasured without fabricating pass.
- [ ] **Step 4: Commit** `test(v43): prove W10 only on real mixed-DPI topology`.

### Task 6: Integrate capability evidence into permanent Windows CI

**Files:**
- Modify: `.github/workflows/windows-real-provider-seeds.yml`
- Modify harness artifact writer/support.

**Interfaces:**
- Produces `W10-CAPABILITY.json` on every Windows real-provider run.
- Existing 11-case artifact remains unchanged when W10 is unavailable.

- [ ] **Step 1: Add W10 capability/proof step** after W09 and before campaign finalization.
- [ ] **Step 2: Persist exact candidate SHA, monitor-count, effective-DPI set, `measured` boolean, and reason when unmeasured. Do not record invented topology metadata.**
- [ ] **Step 3: Keep the required campaign as W01-W09+W11+W12 unless W10 produced a real measured record.**
- [ ] **Step 4: Add source-level guard that a homogeneous capability result cannot be labeled `RealProviderIntegrationPass`.**
- [ ] **Step 5: Run permanent Windows Real Provider Seeds and inspect artifact.
- [ ] **Step 6: Commit** `ci(v43): probe real Windows W10 mixed-DPI capability`.

### Task 7: Promote campaign to W01-W12 only on measured W10 topology

**Files:**
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_campaign.rs`
- Modify campaign support modules and `.github/workflows/windows-real-provider-seeds.yml` only when a genuine W10 environment is available.

**Interfaces:**
- Measured topology: 12 records, RPOMR `(0,12)`, 12 observation digests, exact W01-W12 case set.
- Unmeasured topology: existing 11 records, RPOMR `(0,11)`, W10 explicitly unmeasured.

- [ ] **Step 1: Add conditional campaign assembly driven only by the typed W10 measured record, never an environment string alone.**
- [ ] **Step 2: Add assertions for both lawful campaign shapes and reject any `12` claim without the W10 record.**
- [ ] **Step 3: On a genuine mixed-DPI runner, execute the 12-case campaign and inspect artifact lineage.**
- [ ] **Step 4: Commit** `test(v43): promote L7 campaign when W10 is genuinely measured`.

### Task 8: Exact-head audit and integration

**Files:** no feature changes unless a verified failure reveals a root cause.

- [ ] **Step 1: Audit scope**: no pointer dispatch, no generic snapshot geometry churn, no shipping seed dependency, no W13/W14/W15 claim, no forced 12-case artifact.
- [ ] **Step 2: Run exact-head `CI`, `Windows UIA Observe`, `Windows Real Provider Seeds`, and `V4.3 Green Proof`.**
- [ ] **Step 3: Inspect bounded artifact**. On homogeneous runner require explicit W10 unmeasured capability evidence and intact 11-case pass; on heterogeneous runner require W10 geometry proof and 12-case RPOMR `0/12`.
- [ ] **Step 4: Mark PR ready and merge with expected head SHA only after all required gates succeed.**
- [ ] **Step 5: Verify post-merge `main` and do not call W10 real-provider-closure complete unless the merged exact head has genuine mixed-DPI evidence.