# Wave 5 Affected-Region Trusted Verify — Engineering Design

## Status

Canonical design for connecting the trusted source-write/HMR/fresh-semantic verification path to bounded affected-region visual evidence.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@5f98f0286409e889ac73e1c8fa3be2b98501c9d6`

Branch: `feat/wave5-affected-region-verify`

## Goal

Close the remaining Wave 5 visual verification gap between:

```text
trusted Fix write
  -> HMR settle
  -> fresh semantic/source revalidation
  -> deterministic verification
```

and the intended:

```text
trusted Fix write
  -> HMR settle
  -> fresh semantic/source revalidation
  -> affected-region recapture
  -> deterministic semantic + visual verification
```

The slice must reduce retained/persisted pixels without weakening LocalView's existing native capture, restore, redaction, route, viewport or source authorities.

## Existing authorities reused

This slice does not invent a new capture engine or target policy.

It reuses:

- `localview_capture::resolve_progressive_targets`;
- the existing 120 CSS-pixel element expansion/clamp policy;
- managed-surface preflight;
- per-session capture gate;
- stable capture settle including the 300 ms HMR quiet authority;
- visual freeze;
- native viewport acquisition;
- exact restore acknowledgement;
- private-pixel redaction;
- `apply_capture_target` region crop after redaction;
- trusted Verify source postimage continuity;
- fresh semantic snapshot authority;
- deterministic semantic/console/network comparison;
- visual evidence registration.

## Baseline target

Before the source write, Apply already holds the exact fresh `PageSnapshot` and stable element reference.

LocalView resolves the progressive plan and selects only the exact `ProgressiveTargetKind::Element` target.

That target is already:

- exact-ref derived;
- geometry validated;
- expanded by the existing 120 CSS-pixel policy;
- clamped to the snapshot viewport.

If no valid element target exists, visual Verify falls back to the existing viewport path rather than inventing geometry.

The frontend does not author this region.

## Capture transaction

Native capture remains viewport acquisition.

For a region-scoped Verify capture the order is:

```text
settle
-> freeze
-> backend-derived trusted viewport
-> native viewport capture
-> live viewport validation
-> restore acknowledgement
-> private-pixel redaction
-> bounded region crop
-> optional persistence/evidence registration
```

Crop before redaction is forbidden.

A failed restore discards pixels.

## Stored visual baseline

`VerifyVisualBaseline` gains:

```text
capture_region: Option<Rect>
```

When present, `png` contains only that region's already-redacted pixels.

When absent, `png` retains the existing full-viewport semantics.

The record still stores the exact semantic `target_rect` separately. The expanded capture region must not be mislabeled as exact target geometry.

## Current capture

Verify reuses the backend-owned `capture_region` stored in the verification record.

It does not derive a new region from frontend input.

The current evidence record uses:

- `visual-region` when region scoped;
- `visual` when viewport scoped.

The visual-diff evidence mode is correspondingly:

- `region`;
- `viewport`;
- `unchanged` when the compared pixels are identical.

## Ratio semantics

Existing ratios keep their original meaning.

- `viewportChangedRatio` is populated only when full viewport pixels were compared.
- `targetChangedRatio` remains exact target-union comparison for legacy viewport baselines.
- New `affectedRegionChangedRatio` is populated only when the expanded affected region was compared.

An affected-region diff must never be reported as a viewport or exact-target ratio.

## Geometry drift

Before visual classification, the post-HMR fresh semantic target remains authoritative.

For region-scoped visual proof:

- viewport metadata must match;
- current capture must use the exact stored backend-owned capture region;
- if the fresh exact target rect no longer intersects or is no longer contained by the retained affected region, the visual region claim becomes unavailable/inconclusive rather than widening silently;
- semantic geometry change remains reportable independently.

No automatic fallback may silently turn a region proof into a different region.

## Deterministic classification

The deterministic classifier treats a positive `affectedRegionChangedRatio` as observed visual change.

For `semantic_visual` scope:

- a valid region ratio is sufficient visual availability;
- absence of both viewport and affected-region visual facts is inconclusive unless deterministic semantic/regression evidence already decides the result;
- viewport-only unrelated change remains inconclusive under the existing rule;
- region-scoped visual change is not treated as viewport-wide change.

Provider advice remains advisory and cannot override deterministic status.

## Privacy and retention

The affected-region baseline is retained only after private redaction.

The existing per-record and global verification visual byte budgets remain unchanged and therefore become stricter in practice for region-scoped baselines.

No new durable full-viewport baseline is created for an affected-region record.

No source text, CSS text, token, cookie or private pixels are added.

## API compatibility

Frontend Verify authority remains:

```text
verificationId
```

only.

The receipt is additive:

```text
affectedRegionChangedRatio?: number
```

No new frontend-authored session, reference, route, rect, baseline or source authority is introduced.

## Non-claims

This slice does not claim:

- browser-native region capture;
- that every source edit affects only the selected element region;
- exact target-pixel provenance when using the expanded region;
- automatic rollback;
- requirement correctness;
- cross-route verification;
- visual proof when the target escapes the retained affected region;
- full component/section affected-region inference;
- final root-cause proof.

## Verification

Focused contracts must prove:

1. Apply derives the region from `resolve_progressive_targets`, not frontend geometry.
2. The exact element target is selected.
3. Region crop happens only after restore acknowledgement and private redaction.
4. Baseline stores `capture_region`.
5. Verify current capture receives only the backend-stored region.
6. Registered current evidence uses `visual-region` for region scope.
7. Region diff evidence uses `region`, not `viewport`.
8. `affectedRegionChangedRatio` is distinct from viewport/target ratios.
9. Region visual availability participates in deterministic classification.
10. Legacy viewport baseline behavior remains covered.
11. Existing Trusted Fix/Verify, progressive capture, changed-region, HMR settle and full repository CI remain GREEN.

## Completion definition

Complete when a trusted Fix with valid semantic element geometry can retain only the already-redacted progressive element region as its visual before-state, wait for HMR settle after the write, revalidate fresh semantic/source authority, recapture the same backend-owned affected region, register region-scoped evidence, and deterministically report semantic/regression/affected-region visual facts without silently widening authority.
