# Wave 4 Live Layout Intelligence Design

Date: 2026-09-20  
Lane: Worker AI-1 / Wave 4 Layout Intelligence  
Branch: `feat/wave4-layout-live-intelligence`

## Scope

This lane closes the missing live layout-intelligence slice of Wave 4:

```text
fresh/recent live semantic snapshot evidence
  -> bounded layout-only projection
  -> grid/flex/overflow/collision/spacing/alignment analyzer
  -> typed evidence-backed issues/facts
  -> live-analysis + diagnostics
```

Non-goals: adaptive/binary responsive execution, desktop responsive capture, point-select/source-open, CSS ownership/source tracing, source maps, instrumentation rewrites, screenshot engines, protocol expansion, and arbitrary computed-CSS retention.

## Existing authority reused

Managed-page instrumentation already emits a bounded semantic tree, viewport geometry, visibility evidence and a fixed-property computed-style packet. This lane consumes it; it does not redefine collection.

The projection can retain only element ref, rect, actual semantic-tree parent, interactive state, visibility authority, display, position, overflow X/Y, flex direction/wrap/justify/align, row/column gap, bounded grid templates, padding/font size, and numeric z-index when actually observed.

Names, descriptions, text/form values, arbitrary attributes, arbitrary CSS, URLs, cookies, storage and private values are not retained.

## Bounds and fail-closed rules

Hard bounds:
- 512 layout nodes;
- 16 tree levels;
- 128-byte refs;
- 180-byte retained style strings;
- 256 issues;
- 256 facts;
- 2,048 spacing samples.

A node without usable ref/geometry is not invented. Children can continue with unknown parent authority, never a fabricated replacement parent. Missing style remains unknown. Invalid viewport geometry fails the audit closed.

NaN/infinity, non-finite edges and negative width/height are deterministic `invalid_geometry`. Zero width/height is deterministic `zero_area`. Invalid elements are excluded from relational checks.

## Grid/flex evidence

`DisplayMode` distinguishes block, inline, flex, inline-flex, grid and inline-grid, with bounded `Other` for unsupported values. Flex evidence preserves observed direction/wrap/justify/align and gaps. Grid evidence preserves only the already-bounded computed row/column template strings and gaps. These are observations, not reconstructed stylesheets.

## Overflow authority

Overflow is never inferred from size alone.

For a child crossing its observed parent:
- `hidden|clip` -> `intentional_clip` fact;
- `auto|scroll` -> `scroll_container_overflow` fact;
- explicit `visible` on the crossed axis -> deterministic `container_overflow`;
- missing/unsupported overflow evidence -> no deterministic parent-overflow claim.

Viewport overflow is emitted only for an axis crossing the viewport without an observed constraining ancestor. Fully offscreen nodes (`inViewport=false`) are not called accidental viewport overflow merely because they are elsewhere in the document. A large child fully inside its container is valid.

## Occlusion and collision authority

`control_occluded` requires an interactive target, an actual center-point sample, `occluded=true`, and a retained valid `occludedBy` ref.

`fixed_sticky_collision` additionally requires geometric overlap and pairwise sampled occlusion authority. Fixed/sticky position, overlap, or a large z-index alone is insufficient.

Geometry-only checks remain heuristic:
- `sibling_collision` for substantial actual-sibling overlap;
- `substantial_overlap` for large unrelated overlap involving an interactive region.

Ancestor/descendant pairs are excluded from generic collision checks. Numeric z-index can appear in evidence when observed, but is never used to synthesize stacking order.

## Spacing rhythm

Bounded samples come from recurring sibling gaps, computed padding and child-to-parent edge distances. Values within 0.75 CSS px cluster together to absorb subpixel rounding. Recurrence is required before returning an inferred family.

`spacing_outlier` is local to parent + source. It needs at least two supporting family samples, with an outlier threshold of max(2 CSS px, 20% of inferred family). Inferred families are never called official design tokens.

## Alignment families

Alignment is parent-local, never one global page median. Sibling groups evaluate left edge, right edge, horizontal center, top edge and bottom edge. A family needs at least two siblings within 1 CSS px; an outlier must deviate more than 2.5 CSS px.

Evidence records outlier ref, expected family, expected measurement, measured value, deviation, threshold, support and confidence. Instrumentation exposes no text baseline, so box edges are not promoted to typographic baseline authority.

## Explicit issue classification

`LayoutIssue` carries `LayoutIssueClass::{Deterministic, Heuristic}`. Diagnostics maps this field directly instead of turning confidence into truth.

Deterministic examples: invalid/zero geometry, explicit visible-axis overflow, uncontained viewport overflow, sampled control occlusion, sampled fixed/sticky collision.

Heuristic examples: sibling/substantial overlap from geometry alone, spacing outlier, alignment outlier.

## Live integration

`localview-live-analysis` selects the newest `SemanticSnapshot` by observer sequence and supports both live observer shape (`payload.snapshot`) and trusted retained native-snapshot shape (snapshot fields directly in `payload`).

The existing `LiveAnalysis` response gains a `LiveLayoutAnalysis` packet containing source snapshot sequence/version and bounded `LayoutAnalysis`. `diagnose_live` projects layout issues into the existing diagnosis stream while preserving explicit issue class. No alternate runtime/control plane is introduced.

## Verification matrix

Tests cover horizontal/vertical flex, grid, nested containers, intentional scroll overflow, accidental overflow, clipping, large valid children, overlap/non-overlap, supported fixed/sticky collision, insufficient authority, spacing families/outliers, alignment families/outliers, subpixel jitter, invalid/zero/negative geometry, bounded node count, live semantic snapshot -> analyzer integration, latest-snapshot selection, retained-snapshot shape, private/arbitrary-style non-retention, and diagnostic class preservation.

## Truth boundaries / remaining non-closures

This lane does not claim:
- computed style equals author CSS intent;
- inferred spacing families are design tokens;
- box edges are text baselines;
- overlap proves a bug without the stated authority;
- z-index alone proves stacking order;
- occlusion is known without sampling;
- missing style means absence;
- responsive/adaptive/content/locale stress is complete;
- source ownership or source-map resolution is complete.
