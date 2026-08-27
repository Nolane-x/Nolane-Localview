# Deterministic Visual Verification Plan

## Goal

Close the native evidence loop from retained captures to a deterministic verification proof without creating a second capture path or moving raw pixels/file paths through the control plane.

## Authority boundaries

- The caller may identify a baseline evidence ID, candidate evidence ID, and a bounded expectation only.
- The daemon resolves both evidence objects and requires the same session, native-capture provenance, observed/untainted evidence, compatible route/viewport/region, and expected revision policy before enqueueing work.
- Only the daemon can create a native `VisualDiff` executor request.
- The desktop resolves artifact IDs from already-authorized evidence, reads bytes from its existing `ArtifactStore`, decodes bounded PNGs, and runs deterministic `localview_visual::pixel_diff`.
- Raw pixel bytes and artifact filesystem paths never cross daemon↔desktop transport.
- The daemon validates the exact native result origin and creates bounded `Proof` evidence whose parents are the baseline and candidate evidence IDs. The caller cannot submit a verdict or diff metrics.

## TDD sequence

1. Add RED pure verification contracts for unchanged/changed expectations and bounded ratio thresholds.
2. Implement deterministic expectation evaluation in `localview-verification`.
3. Add RED native bridge/desktop contracts for a typed `VisualDiff` request and artifact lookup without capture.
4. Extend `ArtifactStore` with safe ID-based bounded reads and implement desktop diff execution.
5. Add RED control integration for authenticated verification, provenance/context correlation, fail-closed mismatches, and Proof evidence parentage.
6. Implement the control coordinator and result validation.
7. Add explicit cross-platform CI gates, update implementation status/roadmap, review full patch, and merge only after exact-head CI is fully green.

## Non-goals

- No new screenshot implementation.
- No Chromium pixel verification in this slice.
- No caller-supplied visual verdict or raw diff payload.
- No filesystem paths in public responses.
