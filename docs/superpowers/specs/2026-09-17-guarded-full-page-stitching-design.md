# Guarded Full-Page Stitching Design

Date: 2026-09-17
Status: proposed for implementation after review
Base: `main@70ebd3ff54a5e8b038445ccd6651944d4be53c1c`
Branch: `feat/w2-guarded-full-page-stitching`

## 1. Purpose

Close the Wave 2 guarded full-page stitching gap without introducing a second screenshot authority, Chromium fallback, DOM/canvas reconstruction, or unbounded capture behavior.

The implementation must reuse the existing LocalView-managed native viewport path on WebView2, WKWebView and WebKitGTK. A full-page artifact is valid only when every tile belongs to one coherent, fail-closed capture transaction and the final image can be traced to the exact LocalView session, route, revision, viewport, document geometry and capture transaction.

## 2. Existing authority that must remain authoritative

The existing desktop capture path already owns:

- exact LocalView-managed surface preflight;
- per-session capture serialization;
- capture settle;
- visual freeze/restore with a bounded lease;
- native viewport acquisition;
- private-region redaction before persistence;
- artifact-store retained-resource accounting;
- evidence registration;
- route/viewport drift rejection.

Full-page capture must extend this path. It must not add a platform-specific full-page screenshot API or bypass the existing native viewport capture helpers.

## 3. Non-goals

This slice does not:

- make Chromium/Playwright the default or fallback full-page engine;
- capture arbitrary external pages;
- reconstruct the page with DOM/canvas/html2canvas;
- persist intermediate tiles;
- silently tolerate route, viewport, document-size or scale drift;
- fabricate support for viewport-anchored content that cannot yet be stitched faithfully;
- add responsive/contact-sheet execution;
- change Perception Budget dimensions;
- create synthetic analysis-concurrency counters.

## 4. Chosen architecture

Use one native coherent stitching transaction.

The transaction is owned by the desktop and holds the existing per-session capture gate for its entire lifetime. It settles the page, freezes visual motion/private masking, captures an authoritative full-page metrics record, scrolls only through a private capture-only action bound to the active freeze token, captures one native viewport at each acknowledged position, redacts each tile before it can reach the compositor, restores the exact original scroll position, restores visual state, and only then allows the final stitched artifact to be persisted and registered.

Any failure before exact scroll + visual restoration causes all captured pixels to be discarded.

## 5. Internal capture actions

Existing public `Scroll` remains a user/page action and is not reused as stitching authority.

Add private internal-capture operations bound to the active freeze token.

### 5.1 Full-page metrics

`FullPageMetrics { token }`

Returns a bounded receipt containing:

- exact token;
- `scroll_x`, `scroll_y`;
- CSS viewport width/height;
- document scroll width/height;
- maximum scroll x/y;
- bounded viewport-anchored scan count;
- count of visible `position: fixed` / `position: sticky` elements;
- a deterministic **document invariant fingerprint** that excludes expected scroll coordinates but includes document/viewport dimensions and bounded structural geometry inputs used by the stitch guard;
- current private mask rectangles/count for this exact viewport position, never selector strings.

The instrumentation scan is bounded. If the scan budget is exceeded, metrics acquisition fails closed.

A successful metrics acknowledgement renews the active visual-freeze lease for another normal lease interval. It does not create a new token.

### 5.2 Capture-only scroll

`CaptureScroll { token, x, y }`

Requirements:

- token must match the active visual-freeze lease;
- x/y must be finite and inside the metrics-defined scroll range;
- the page scrolls using the managed page runtime;
- acknowledgement is emitted only after the browser reports the resulting position after at least two animation-frame turns;
- the receipt returns actual `scroll_x`, `scroll_y`, current document dimensions, viewport dimensions, current visible fixed/sticky count, the document invariant fingerprint, and private mask rectangles/count for that exact scroll position;
- successful acknowledgement renews the same freeze token's lease for another normal lease interval;
- clamping is allowed only when it exactly matches the precomputed terminal tile target; arbitrary mismatch fails.

Every tile, including a tile at the original scroll position, begins with one `CaptureScroll` acknowledgement. This gives every native acquisition a fresh token-bound lease renewal and a tile-specific mask receipt.

These actions remain outside public action cancellation, like existing freeze/restore actions. They are capture-internal and never become a public page-control primitive.

## 6. Guard against fixed/sticky ambiguity

Version 1 is deliberately conservative.

Before the first tile, the frozen page is scanned for visible viewport-anchored elements. If any visible fixed/sticky element is present, full-page stitching returns an explicit unsupported/guarded error and captures nothing.

Reason: repeating or suppressing fixed/sticky UI would invent a visual interpretation. A later dedicated anchored-element compositor may relax this gate only with its own evidence-backed contract.

The guard is checked again in every capture-scroll acknowledgement. If viewport-anchored state appears later, the transaction aborts and discards pixels.

## 7. Geometry and tile planning

Tile planning is pure, deterministic Rust logic.

Inputs:

- document CSS width/height;
- viewport CSS width/height;
- native pixel width/height from the first tile;
- acknowledged maximum scroll x/y;
- configured safety limits.

### 7.1 Two-dimensional support

The planner supports horizontal and vertical overflow. Targets are generated in deterministic row-major order.

The last target on each axis is exactly the maximum scroll position, so browser clamping is represented by the plan rather than tolerated after the fact.

### 7.2 Fractional scale and seam prevention

Do not accumulate rounded tile sizes.

Derive `scale_x = native_pixel_width / viewport_css_width` and `scale_y = native_pixel_height / viewport_css_height` from the real native frame. Require both to be finite, positive, mutually sane and compatible with the reported device-scale factor within an implementation constant locked by tests.

For every global CSS boundary, calculate its final native pixel boundary from the global coordinate and scale, then round once. The compositor crops overlapping tile pixels using these global boundaries. This prevents cumulative fractional-DPI seam drift.

Every later tile must have the same native dimensions, viewport CSS dimensions and scale values. Any change aborts the transaction.

## 8. Safety bounds

The first implementation uses explicit hard bounds. Exact constants may be tuned during implementation only if tests preserve the same fail-closed properties.

Initial design targets:

- maximum CSS document dimension per axis: 100,000 px;
- maximum tile count: 64;
- maximum decoded final RGBA allocation: 96 MiB;
- every native tile remains subject to the existing 24 MiB native frame limit;
- final encoded PNG must pass existing artifact-store retained-resource projection/admission before mutation;
- capture uses the existing per-session serialization gate;
- full-page execution receives a bounded overall timeout independent of the per-native-capture timeout;
- the overall timeout must still allow repeated lease renewal, but no individual native capture may outlive the renewed visual-freeze lease;
- integer conversions use checked/saturating-safe arithmetic; overflow is an error, never truncation.

The 96 MiB compositor bound is a local transient-allocation bound, not a fabricated retained-resource counter. Retained artifact bytes continue to be governed by the existing owner-local artifact-store ledger.

If the page cannot fit the bounds, LocalView returns a bounded unsupported/resource error and captures nothing.

## 9. Capture transaction

For one `capture_full_page` request:

1. Validate caller viewport metadata.
2. Preflight exact LocalView-managed surface and loopback route.
3. Acquire the existing per-session capture gate.
4. Wait for stable capture settle.
5. Freeze visual state and private masking; record freeze token.
6. Acquire full-page metrics and original scroll position; this renews the freeze lease.
7. Reject bounded-scan overflow or any visible fixed/sticky element.
8. Plan the complete tile grid before native pixel acquisition.
9. Validate projected final RGBA allocation against the hard transient compositor bound.
10. For each tile target:
   - perform token-bound capture scroll, even when the target equals the current position;
   - renew the freeze lease through that acknowledgement;
   - verify exact acknowledged position, zero fixed/sticky count, document invariant fingerprint, document dimensions and viewport dimensions;
   - capture through the existing native managed-surface viewport path;
   - verify backend, route, revision, viewport, native dimensions and scale invariants;
   - redact private pixels in memory using the mask geometry from that tile's capture-scroll acknowledgement;
   - decode and copy only the globally owned crop into the final compositor;
   - drop tile PNG/RGBA buffers as soon as copied.
11. Token-bound scroll back to the exact original x/y and verify acknowledgement; this also refreshes the lease.
12. Restore visual state with the exact token and require acknowledgement.
13. Only after both restores succeed, encode the final compositor image.
14. Reconcile/project/admit final encoded bytes against the existing artifact-store retained-resource authority and persist one final artifact.
15. Register one full-page visual evidence record.
16. Drop compositor memory and release the session gate.

Cleanup must attempt both scroll restoration and visual restoration even when an earlier tile fails. Failure of either restoration makes the transaction unsuccessful and prevents final persistence.

If artifact persistence succeeds but evidence registration fails, no successful command receipt or authoritative full-page evidence is returned. The content-addressed artifact may remain as unreferenced retained data under the existing ArtifactStore/LRU semantics; retained-resource accounting must remain exact. This slice does not invent an unsafe ad-hoc delete path solely to simulate cross-process atomicity.

## 10. Private redaction

Private pixels must never enter the stitched canvas unredacted.

Each native tile is redacted before decode/copy into the final compositor using the private-selector authority already owned by the managed page. The capture-scroll acknowledgement returns only bounded mask geometry for that exact tile position. Selector strings never leave the private capture envelope.

The desktop forms a tile-local redaction receipt from the active freeze token plus the just-acknowledged mask geometry, then reuses the existing in-memory redaction implementation before compositor access.

If mask resolution changes unexpectedly, exceeds its limits or cannot be proven for a tile, the transaction aborts and all pixels are discarded.

## 11. Evidence and receipt

Add a dedicated full-page receipt/evidence shape rather than pretending the result is a normal viewport artifact.

Required provenance:

- artifact/evidence ids;
- session id association;
- backend;
- canonical loopback route;
- revision;
- capture timestamp;
- source viewport CSS dimensions and device-scale factor;
- final document CSS width/height;
- final pixel width/height;
- tile count;
- deterministic ordered tile targets/acknowledged offsets or a bounded digest over them;
- document invariant fingerprint;
- redaction-applied flag and bounded redaction-count metadata without selectors;
- exact transaction outcome.

No filesystem path or tile pixels are exposed in command receipts.

## 12. Failure semantics

The operation fails closed on at least:

- no managed surface;
- non-loopback route;
- settle timeout;
- freeze failure;
- freeze lease expiration/renewal failure;
- metrics scan overflow;
- invalid/non-finite document geometry;
- document too large;
- too many tiles;
- projected compositor memory overflow;
- fixed/sticky guard hit;
- token mismatch;
- scroll acknowledgement mismatch;
- unexpected browser clamping;
- document invariant fingerprint drift;
- document-size drift;
- viewport/DSF/native-size drift;
- route/revision drift where revision is authoritative;
- native capture failure/timeout;
- redaction failure;
- decode/copy/encode failure;
- original-scroll restoration failure;
- visual restore acknowledgement failure;
- retained-resource admission/persistence failure;
- evidence-registration failure.

A failed transaction must not emit a successful full-page evidence record or successful full-page command receipt.

## 13. Implementation boundaries

Expected files/components:

- `crates/live-bridge`: private full-page metrics/capture-scroll action variants and internal-action classification;
- `crates/instrumentation`: bounded metrics, anchored-element guard, token-bound scroll/lease renewal and per-tile mask geometry receipt;
- `crates/visual`: pure tile planner/compositor geometry helpers and adversarial tests;
- `apps/desktop/src-tauri/src/visual_capture.rs`: full transaction coordinator and final artifact/evidence path;
- `apps/desktop/src-tauri/src/lib.rs`: command registration/managed-page executor cases;
- daemon/control evidence schema only as required for dedicated full-page visual provenance;
- `docs/ROADMAP.md`, `docs/IMPLEMENTATION_STATUS.md`, `docs/SPEC_COVERAGE.md`: update only after implementation and tests prove the new path.

Do not duplicate native screenshot code in platform adapters.

## 14. Test strategy

Implementation is test-driven.

### 14.1 Pure planner/compositor tests

Must cover:

- one-tile document;
- exact two-tile boundary;
- partial terminal tile;
- horizontal + vertical overflow;
- fractional device scales;
- deterministic row-major plan;
- overlap crop ownership with no gaps/duplicates;
- zero/NaN/infinite dimensions;
- checked arithmetic overflow;
- tile-count bound;
- final RGBA bound;
- unexpected terminal-clamp rejection.

### 14.2 Instrumentation/live-bridge contracts

Must prove:

- new actions are internal-capture-only;
- stable wire shapes;
- token mismatch denial;
- bounded element scan;
- fixed/sticky guard;
- exact scroll acknowledgement;
- lease renewal without token replacement;
- original scroll restoration;
- per-tile private mask re-resolution without selector leakage;
- invariant fingerprint excludes expected scroll coordinates but changes when guarded document geometry changes.

### 14.3 Desktop transaction tests

Must lock ordering:

`settle -> freeze -> metrics/renew -> plan/bound -> [scroll+renew -> native capture -> redact -> copy]* -> restore scroll+renew -> restore visuals -> encode -> retained project/admit/persist -> evidence`

Adversarial tests must show no persistence/evidence before restoration and no successful receipt/evidence when route, geometry, scale, mask, lease, capture, restoration, resource admission or evidence registration fails.

### 14.4 Cross-platform compile/CI

All existing Linux/Windows/macOS checks must remain green. Hosted GUI proof should add a deterministic tall fixture where practical, but platform-specific claims remain limited to what each runner actually renders and verifies.

## 15. Rollout and compatibility

Add `capture_full_page` as a new explicit command/path. Existing `capture_viewport`, `capture_region`, changed-region, progressive-target and visual-packet behavior remain source- and behavior-compatible.

No existing caller is silently switched to full-page capture.

The guarded fixed/sticky rejection is an explicit capability limitation, not silently downgraded to viewport capture.

## 16. Completion criteria

This wave is complete only when:

1. full-page capture uses only the existing managed native viewport authority;
2. one coherent token-bound transaction owns every tile;
3. the 8-second visual-freeze lease is safely renewed without replacing the transaction token;
4. private pixels are redacted with tile-current mask geometry before entering the compositor;
5. exact original scroll and visual state are restored before persistence;
6. fractional-DPI/overlap math is deterministic and adversarially tested;
7. fixed/sticky ambiguity fails closed;
8. transient memory, tile, timeout and retained-storage bounds are enforced by their correct owners;
9. no intermediate tile is persisted;
10. final full-page artifact has dedicated provenance/evidence;
11. existing capture behavior remains unchanged;
12. exact-head CI is green across supported platforms;
13. roadmap/status claims are updated only to the level actually proven.
