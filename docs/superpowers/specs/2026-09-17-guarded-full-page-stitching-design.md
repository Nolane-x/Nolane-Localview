# Guarded Full-Page Stitching Design

## Status

Approved in-chat architectural direction for the next unblocked Wave 2 visual-runtime slice after Linux L05 closure. This document defines the implementation contract before code changes.

## Goal

Add a native, evidence-backed full-page capture path that stitches multiple captures from the existing LocalView-managed WebView surface without introducing a second browser authority, without persisting intermediate unredacted pixels, and without weakening the existing settle/freeze/redaction/resource-governor contracts.

The first closure is intentionally vertical-only: viewport-width by document-height. Horizontal document overflow is rejected rather than silently cropped or independently stitched.

## Non-goals

- No Chromium/Playwright full-page screenshot fallback.
- No DOM/canvas/html2canvas reconstruction.
- No arbitrary external-page capture.
- No horizontal page stitching.
- No attempt to make fixed/sticky overlays correct by heuristic duplication removal in this slice.
- No caller-provided evidence, scroll authority, private selectors, tile offsets, or document dimensions.
- No relaxation of the existing native viewport capture backend rules on WebView2, WKWebView, or WebKitGTK.
- No change to the four-dimensional Perception Budget contract.

## Existing authority that must be reused

The implementation must reuse, not fork, the following existing authorities:

1. LocalView-managed surface ownership and loopback navigation checks.
2. Per-session capture gate.
3. Stable-settle polling through the authenticated daemon endpoint.
4. Internal visual freeze/restore action authority.
5. Native platform viewport acquisition.
6. Private selector transport and geometry-only masking.
7. Pre-persistence pixel redaction.
8. Visual artifact retained-resource accounting and ArtifactStore admission.
9. Daemon-side visual evidence registration.
10. Runtime Resource Governor admission where the current capture path already requires it.

## Safety limits

The initial full-page authority is bounded by constants owned by production code, not caller input:

- `MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT = 100_000.0`
- `MAX_FULL_PAGE_TILES = 128`
- `MAX_FULL_PAGE_PIXELS = 64_000_000`
- `MAX_FULL_PAGE_PNG_BYTES = 128 * 1024 * 1024`
- device-scale factor must continue to satisfy the existing visual-capture bound `0 < dsf <= 8`
- document width must be finite, positive, and no more than `0.5 CSS px` wider than the captured viewport width
- document height must be finite, positive, at least the viewport height, and no greater than `MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT`

A page outside those limits is unsupported for guarded stitching and must fail before persistence.

## High-level architecture

The feature has three explicit units.

### 1. Pure full-page planner/stitcher in `localview-visual`

This unit owns no browser or Tauri state. It accepts validated document geometry, viewport geometry, device-scale information, and decoded redacted tile images.

It produces a deterministic vertical stitch plan and a single final PNG. The planner is independently testable and must not invent browser state.

The plan uses real scroll-clamping semantics. For a document height `H` and viewport height `V`, offsets advance by `V` until the last offset and must always include `max_scroll = H - V` exactly. The final tile can overlap the previous tile. Each planned tile therefore contains:

```text
scroll_y_css
source_start_y_css
contribution_height_css
destination_start_y_css
```

The contribution intervals must be contiguous, non-overlapping in destination space, and cover `[0, H)` exactly.

Pixel boundaries are derived from one stable vertical scale taken from the actual captured native frame, not by independently rounding every tile height. Destination boundaries are computed from cumulative CSS coordinates and rounded once per boundary. This prevents one-pixel gaps and duplicated rows under fractional device scale factors.

The compositor must reject:

- inconsistent tile width or scale;
- impossible source ranges;
- missing or duplicate tile positions;
- non-contiguous destination coverage;
- final dimensions beyond `MAX_FULL_PAGE_PIXELS`;
- encoded output beyond `MAX_FULL_PAGE_PNG_BYTES`.

Intermediate tiles are never persisted by this unit.

### 2. Internal stitch-scroll authority in instrumentation/live bridge

The existing public `Scroll { x, y }` action is not sufficient authority for a capture transaction. Full-page stitching introduces internal capture-only actions bound to the exact active visual-freeze token.

The logical contract is:

```rust
StitchProbe { token }
StitchScroll { token, y }
```

These actions are internal capture actions, are not exposed through the public page-action API, and are not cancellable through the public action-cancellation authority.

`StitchProbe` returns geometry-only state required for capture correctness:

```text
scroll_x
scroll_y
viewport_css_width
viewport_css_height
document_css_width
document_css_height
fixed_or_sticky_count
mask_rects
lease_ms
```

`StitchScroll` must:

1. verify the exact freeze token is still active;
2. reject non-finite/negative requested positions;
3. perform an exact vertical document scroll with `x = original_scroll_x`;
4. read back the actual clamped scroll offset;
5. refresh document/viewport geometry;
6. refresh private-mask geometry for the new viewport using the already-private selector authority;
7. refresh the visual-freeze self-healing lease for the same token;
8. return the same geometry-only receipt shape.

No selector string, page payload, text content, storage value, cookie, URL query, or response body may cross this boundary.

### 3. Desktop orchestration in visual capture

A new Tauri command `capture_full_page` orchestrates the transaction. The command accepts only the same caller-owned identifiers already appropriate to native capture, such as `session_id`, viewport metadata and optional revision. It does not accept tile offsets, document dimensions, scroll state, evidence IDs, private selectors, mask rectangles or verdicts.

The transaction is serialized by the existing per-session capture gate.

## Transaction sequence

The exact successful sequence is:

1. Validate requested viewport metadata.
2. Preflight LocalView-managed surface ownership and loopback route.
3. Acquire the existing per-session capture gate.
4. Run the existing stable-settle gate.
5. Freeze visual state through the existing internal freeze authority.
6. Record original scroll position from the freeze-bound stitch probe.
7. Validate initial document geometry and safety limits.
8. Reject if `fixed_or_sticky_count > 0` in this initial slice.
9. Build the deterministic stitch plan.
10. For each planned tile:
    - issue exact-token `StitchScroll` to the planned `scroll_y_css`;
    - require returned actual `scroll_y` to match the plan within `0.5 CSS px`;
    - require route-independent viewport/document geometry to equal the initial geometry within `0.5 CSS px`;
    - run stable-settle again after the scroll so lazy-loading/network activity cannot be silently stitched mid-change;
    - issue `StitchProbe` again and require the same scroll/document/viewport geometry;
    - capture one viewport through the existing native WebView backend;
    - require route, viewport and device-scale factor to remain compatible;
    - redact that tile immediately using the mask rectangles returned by the same exact-token post-settle probe;
    - decode/validate the redacted tile and feed only the redacted image into the stitcher;
    - discard the original native PNG and decoded tile as soon as its contribution is committed.
11. Before final persistence, scroll back to the exact original `scroll_y` using the same freeze token and verify the read-back offset.
12. Restore visual state with the exact existing freeze token and require restore acknowledgement.
13. Only after both scroll restoration and visual restoration succeed may the final stitched PNG be admitted to retained-resource storage.
14. Persist one final artifact.
15. Register one full-page visual evidence record derived from server/desktop-owned facts.
16. Return one receipt that references the final artifact/evidence only.

Any failure from step 5 through step 12 discards all pixels and prevents artifact/evidence persistence.

## Coherence and drift rules

Full-page capture is an evidence transaction, not a best-effort screenshot. It fails closed on any of the following:

- top-level route changes;
- viewport CSS size changes;
- native pixel width changes;
- device-scale factor changes;
- document width or height changes after initial probe;
- actual scroll offset differs from the planned offset by more than `0.5 CSS px`;
- private-mask refresh fails;
- settle times out for any tile;
- freeze lease expires or token changes;
- native capture fails;
- redaction fails;
- tile decode/encode/stitch validation fails;
- original scroll cannot be restored exactly enough;
- visual restore acknowledgement fails;
- retained-resource admission fails;
- evidence registration fails before the artifact transaction can be considered complete.

The initial closure deliberately treats dynamic document-height growth caused by infinite/lazy feeds as unsupported instead of chasing a moving end-of-page target.

## Fixed and sticky positioning

A naive stitch duplicates fixed headers, cookie banners, floating buttons and sticky navigation. Heuristic image de-duplication would be visually fragile and would move correctness authority away from browser state.

Therefore the initial guarded implementation fails before tile capture when the freeze-bound probe reports any rendered element whose computed `position` is `fixed` or `sticky`.

This is an explicit capability limit, not a hidden fallback. A later separately reviewed slice may introduce an exact-token temporary suppression protocol with proof of restoration, but this design does not claim it.

## Privacy and redaction invariant

The strongest invariant of this feature is:

> No unredacted tile may enter the stitcher, baseline cache, ArtifactStore, daemon evidence path, logs or returned receipt.

The current viewport freeze receipt cannot be reused after scrolling because selector-matched elements move relative to the viewport. Every tile therefore requires private-mask geometry refreshed under the exact active freeze token after that tile has settled.

Mask refresh is geometry-only. The private selectors remain in the private capture envelope/page instrumentation and never become daemon evidence or public command fields.

## Artifact and retained-resource authority

Intermediate tiles are ephemeral process memory only.

The final stitched PNG is admitted through the same owner-local retained-resource authority as other visual artifacts. Before `ArtifactStore::put`, the implementation must synchronize current storage usage, project the final encoded byte count, reject over-budget admission, perform the store mutation, and reconcile actual retained usage.

A failed store mutation or failed accounting reconciliation cannot be counted as successful persistence.

The final PNG byte length must also satisfy `MAX_FULL_PAGE_PNG_BYTES` even when the ArtifactStore has more free capacity.

## Evidence contract

The daemon gets one new fail-closed full-page visual evidence schema or an explicitly extended visual schema with a distinct target value. The evidence must be produced from trusted desktop facts, never caller-provided values.

Required provenance fields:

```text
target = "full_page"
artifact_id
backend
route (canonicalized without query/fragment under existing rules)
revision
captured_at_unix_ms
viewport
pixel_width
pixel_height
document_css_width
document_css_height
tile_count
device_scale_factor
scroll_offsets_css[]
original_scroll_y_css
fixed_or_sticky_count = 0
redaction_applied = true
```

The daemon validates finite/range-bounded geometry, tile count `1..=128`, monotonic offsets, and consistency between target type and required full-page fields.

## Error handling

Public errors remain coarse and privacy-safe. They identify the failed invariant class without exposing private selectors or page content. Examples:

- `full-page document geometry is outside the bounded capture policy`
- `full-page capture refuses fixed or sticky content in guarded mode`
- `full-page document geometry changed during capture; pixels discarded`
- `full-page scroll acknowledgement drifted; pixels discarded`
- `full-page private-mask refresh failed; pixels discarded`
- `full-page restore acknowledgement failed; pixels discarded`
- `full-page stitched output exceeds the bounded pixel budget`

## Testing strategy

Implementation follows TDD. At minimum, RED then GREEN coverage must prove the following.

### Pure planner/stitcher tests

- one-viewport document produces one tile;
- exact two-viewport document produces two non-overlapping contributions;
- partial final page forces an overlapping clamped final scroll but contributes each document row exactly once;
- fractional scale produces gap-free integer pixel boundaries;
- `MAX_FULL_PAGE_TILES + 1` is rejected;
- document height above `100_000 CSS px` is rejected;
- final pixel count above `64_000_000` is rejected;
- inconsistent tile width/scale is rejected;
- missing tile or duplicate offset is rejected;
- final PNG byte cap is enforced.

### Instrumentation/live-bridge contract tests

- stitch actions are internal capture actions;
- public action API cannot enqueue them;
- wrong/expired freeze token is rejected;
- `StitchScroll` reports actual clamped offset;
- each stitch scroll refreshes the freeze lease;
- each stitch scroll/probe refreshes private-mask geometry;
- selector strings never appear in the serialized result;
- fixed/sticky count is geometry-only metadata.

### Desktop transaction tests

- session capture gate covers the entire multi-tile transaction;
- no artifact is persisted before original-scroll and visual restore succeed;
- every native tile is redacted before it reaches the stitcher;
- route drift rejects and discards all pixels;
- viewport/DSF/document-height drift rejects and discards all pixels;
- settle timeout on tile N rejects the whole capture;
- fixed/sticky content is rejected before the first native tile;
- original scroll is restored on success;
- failed original-scroll restore prevents persistence;
- failed visual restore prevents persistence;
- retained-resource projection occurs before final store mutation;
- one successful transaction creates exactly one final artifact/evidence receipt and no tile artifacts.

### Cross-platform regression

The feature must preserve all existing native rendered-pixel proofs for Linux/WebKitGTK, macOS/WKWebView and Windows/WebView2. No platform adapter receives a new full-page API; each still captures only the visible viewport.

## Documentation truth after implementation

Only after implementation and exact-head CI are green may `docs/ROADMAP.md`, `docs/IMPLEMENTATION_STATUS.md`, and `docs/SPEC_COVERAGE.md` move guarded full-page stitching from remaining/partial to landed.

The docs must continue to state the limits of the initial slice: vertical viewport-width capture only, fixed/sticky guarded rejection, bounded document/tile/pixel size, and no Chromium default fallback.

## Acceptance criteria

This slice is complete only when all of the following are true on one immutable exact head:

1. the pure planner/stitcher contracts are green;
2. internal freeze-token stitch authority contracts are green;
3. desktop orchestration contracts are green;
4. existing native capture/privacy/retained-resource tests remain green;
5. Linux, macOS and Windows compile/test gates remain green;
6. no intermediate tile artifact exists in the successful path;
7. failure-path tests prove no final artifact is emitted before successful scroll + visual restoration;
8. documentation truth is updated only after the implementation gates above pass.
