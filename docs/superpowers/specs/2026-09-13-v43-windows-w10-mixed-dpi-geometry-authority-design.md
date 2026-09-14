# V4.3 Windows W10 Mixed-DPI Geometry Authority — Design

Date: 2026-09-13

Status: Approved for implementation by continuation directive

Base: `main@7d371755b9c4de85c63ea271737dd2b647171188`

Branch: `feat/v43-l7-windows-w10-mixed-dpi-authority`

## 1. Purpose

Close seed W10 — mixed DPI — without broadening the existing semantic snapshot into an ambiguous geometry container and without calling simulated scale arithmetic real-provider evidence.

W10 belongs to the geometry/accessibility/privacy family. W07/W08/W09/W11/W12 already closed the input/action-race family. W13/W14/W15 remain out of scope.

The current `NativeSemanticNodeObservation` contains semantic identity/capability facts but no rectangle or coordinate-space authority. The Windows UIA tree reader likewise does not call `CurrentBoundingRectangle`. W10 therefore requires an explicit geometry observation boundary rather than an untyped attribute convention.

## 2. External coordinate semantics

Windows UI Automation `BoundingRectangle` is treated as **physical screen coordinates**. LocalView must preserve those coordinates exactly as observed. It must not multiply or divide the UIA rectangle by the target DPI.

The target HWND DPI is a separate environment fact obtained with `GetDpiForWindow`. It explains the display context and permits independent conversion of logical/DIP oracle facts when a test fixture needs that conversion, but it is not a scaling instruction for the already-physical UIA rectangle.

The geometry receipt therefore states the coordinate space explicitly.

## 3. Selected architecture

Add a Windows-specific, read-only geometry receipt to the existing `WindowsUiaWorker`.

```text
existing snapshot cut
  -> exact retained element lease
  -> WindowsUiaGeometryRequest
  -> worker-owned MTA reads CurrentBoundingRectangle
  -> worker reads GetDpiForWindow(exact attached HWND)
  -> WindowsUiaGeometryReceipt
       - exact provider incarnation
       - exact target incarnation
       - exact element ref
       - exact snapshot cut
       - physical screen rectangle
       - coordinate_space = PhysicalScreenPixels
       - target_window_dpi
```

This is not added to `NativeSemanticNodeObservation`. Geometry stays opt-in and action/verification consumers must request it against an exact current retained element.

## 4. Types

Add pure Windows geometry contracts:

```rust
pub enum WindowsUiaCoordinateSpace {
    PhysicalScreenPixels,
}

pub struct WindowsUiaPhysicalRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub struct WindowsUiaGeometryRequest {
    snapshot_cut_ref: String,
    element_ref: ProviderElementRef,
}

pub struct WindowsUiaGeometryReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub coordinate_space: WindowsUiaCoordinateSpace,
    pub bounding_rect: WindowsUiaPhysicalRect,
    pub target_window_dpi: u32,
}
```

`WindowsUiaPhysicalRect` must reject inverted rectangles. Zero-area rectangles may be observed for non-displayed UIA elements but cannot be used for a successful W10 measured case.

`WindowsUiaGeometryRequest` is minted only from an exact snapshot cut and exact retained `ProviderElementRef`. The worker reuses `exact_retained_element`, so stale provider/target/cut/element lineage fails before geometry is read.

## 5. Why a separate receipt

Adding rectangle fields to the generic semantic node would silently alter every provider and every existing snapshot constructor. Encoding geometry into `attributes: BTreeMap<String, String>` would make coordinate space, numeric validation, and lineage parsing conventions rather than types.

The separate receipt keeps the authority narrow and lets future platforms define their own geometry evidence without pretending all provider coordinate systems are identical.

## 6. W10 seed and independent oracle

Extend the non-shipping Windows WPF edge seed with a deterministic geometry target and oracle commands.

The oracle exposes only test facts:

```text
window HWND
window DPI
monitor identity token suitable only for test evidence
geometry target physical screen rectangle
```

The oracle must derive its physical target rectangle independently from LocalView UIA observation. WPF layout values alone are not sufficient; the oracle must convert the target's screen points through the seed's own window/device transform and retain the resulting physical rectangle.

Add a command to move the seed window to a selected monitor. The harness enumerates available monitors/DPI profiles and selects two displays with distinct effective DPI when such a topology exists.

## 7. Real mixed-DPI capability rule

A real W10 pass requires:

1. two real monitor placements A and B;
2. `dpi_a != dpi_b` and both nonzero;
3. a fresh LocalView snapshot cut after each placement;
4. a geometry receipt for the same logical target at each cut;
5. `coordinate_space == PhysicalScreenPixels` at both cuts;
6. the LocalView UIA physical rectangle equals the independent oracle physical rectangle at A and B;
7. no additional DPI scaling is applied to the UIA rectangle;
8. the exact candidate SHA and topology evidence are bound into the Lab artifact.

If the environment cannot provide two distinct effective DPIs, W10 is **not measured**. It is not a pass and not a counterexample. The existing W01-W09+W11+W12 campaign remains authoritative and continues to say W10 is unmeasured.

A pure deterministic conversion test may verify arithmetic, but it cannot mint `RealProviderIntegrationPass` for W10.

## 8. Validation Lab semantics

Add:

```rust
RealProviderCaseKind::W10MixedDpiGeometry {
    distinct_effective_dpi_observed: bool,
    coordinate_space_explicit: bool,
    first_rect_matches_oracle: bool,
    second_rect_matches_oracle: bool,
    double_scaling_observed: bool,
}
```

A measured W10 case is a semantic counterexample if any required fact is false or if `double_scaling_observed` is true.

Do not call `adapt_real_provider_case` for an environment that lacks distinct effective DPI. Such an environment emits capability evidence outside the measured campaign instead of fabricating a W10 observation.

## 9. Campaign promotion rule

The permanent prospective campaign may become the contiguous W01-W12 set only after a real W10 record exists for that execution environment.

Until then:

```text
required measured set = W01-W09 + W11 + W12
explicitly unmeasured = W10-mixed-dpi
RPOMR denominator = 11
```

When W10 is genuinely measured:

```text
required measured set = W01-W12
RPOMR denominator = 12
observation digests = 12
```

No workflow is allowed to relabel the 11-case artifact as a 12-case pass.

## 10. CI topology

Keep the current hosted Windows real-provider job unchanged as an 11-case required gate unless its monitor capability probe proves distinct DPI values.

Add a W10 capability/proof step that:

- records monitor count and effective DPI values;
- runs W10 exact real-provider test only when at least two distinct DPI values are available;
- otherwise writes an explicit `W10-CAPABILITY.json` with `measured=false` and reason `distinct_effective_dpi_unavailable`;
- never converts a skipped W10 into `RealProviderIntegrationPass`.

A future self-hosted/provisioned Windows runner with heterogeneous DPI may turn W10 into a required 12th case without changing production semantics.

## 11. Failure handling

Fail closed on:

- stale snapshot cut or element ref;
- provider/target incarnation mismatch;
- `GetDpiForWindow == 0`;
- UIA bounding rectangle read failure;
- inverted rectangle;
- oracle rectangle unavailable;
- ambiguous monitor placement;
- claimed mixed-DPI pass with equal DPI values.

No retry is permitted merely to obtain a more favorable rectangle. Moving the fixture between monitors is a declared test action, not product-side normalization.

## 12. Scope exclusions

This slice does not add pointer input, coordinate-based dispatch, arbitrary text input, clipboard behavior, monitor configuration mutation, global process DPI-awareness mutation, W13 redaction, W14 owner-drawn accessibility, W15 resource degradation, or a broad Windows provider `SUPPORTED` claim.

## 13. Completion boundary

W10 is implementation-complete when the typed geometry boundary, Lab semantics, seed/oracle, capability evidence, and permanent CI integration are merged and all ordinary gates remain green.

W10 is **real-provider-closure complete** only when an exact candidate executes on a genuine distinct-DPI topology and produces a 12-case prospective artifact with RPOMR `0/12` and 12 observation digests. If no such runner is available, the correct terminal state is implementation-complete with W10 explicitly unmeasured at the external topology boundary.