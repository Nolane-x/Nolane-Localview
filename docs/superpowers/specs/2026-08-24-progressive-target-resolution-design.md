# Progressive Target Resolution Design

## Goal

Turn LocalView's existing `element → component → section → viewport` capture planning primitives into a live, evidence-backed target-resolution path that feeds the already-audited native viewport → restore → private redaction → bounded crop pipeline.

## Non-goals

This slice does not implement React/Vue/Svelte runtime adapters, sourcemap consumers, token-aware visual packet selection, full-page stitching, or the complete capture → diff → deterministic verification loop. It must not fabricate framework ownership when current runtime evidence cannot prove it.

## Architecture

The feature is split into two boundaries:

1. `localview-capture` owns deterministic target resolution from a fresh `PageSnapshot` and stable `ElementRef`.
2. The desktop coordinator obtains that fresh snapshot through the authenticated local control plane, asks the resolver for a bounded ordered plan, then executes selected rectangles through the existing native visual transaction. Platform adapters remain viewport-only.

The deterministic resolver is pure Rust and carries no Tauri, HTTP, DOM, WebView, or framework dependency.

## Resolver input and output

Input:

- one fresh `PageSnapshot`;
- one stable `ElementRef`;
- snapshot route and viewport are authoritative for the resolution pass.

Output is a `ProgressiveTargetPlan` bound to:

- `reference`;
- `snapshot_version`;
- `route`;
- `viewport`;
- ordered targets.

Each resolved target carries:

- kind: `element`, `component`, `section`, or `viewport`;
- bounded CSS `Rect`;
- deterministic provenance;
- confidence in integer milli-units.

## Element semantics

The stable ref must exist in the fresh semantic tree and have finite, positive geometry intersecting the snapshot viewport. Missing, NaN/infinite, zero-sized, or fully offscreen geometry fails closed.

The element packet is the target geometry expanded by 120 CSS pixels and clamped to the viewport, preserving the existing `progressive_regions` behavior.

## Component semantics

Component ownership is evidence-first. A component target may be emitted only when the target node has `source.component = Some(name)` and a semantic ancestor with the same component name has valid geometry containing the target element. The nearest qualifying ancestor wins.

Tag names, class names, DOM depth, size, naming style, or arbitrary attributes must not by themselves create component ownership. If source evidence is absent or no ancestor corroborates it, the component level is omitted.

## Section semantics

The section target is the nearest valid semantic ancestor whose tag or role is an explicit section/landmark boundary. Accepted tags are `section`, `main`, `article`, `nav`, `aside`, and `form`. Accepted roles are `region`, `main`, `navigation`, `complementary`, and `form`.

The section rect must contain the target element, be finite/positive, and intersect the viewport. It is clamped to the viewport. If no qualifying ancestor exists, section is omitted.

## Viewport semantics

Viewport is always the final explicit fallback and is exactly `(0, 0, viewport.width, viewport.height)`. Duplicate rectangles among element/component/section levels are removed while preserving the strongest earlier target kind, but the `viewport` level is never deduplicated away even when a section occupies the exact viewport bounds. This keeps an explicit caller-selectable fallback level.

## Freshness and drift

Target resolution must be based on a fresh semantic snapshot action, not an arbitrary observer-history packet. The control plane exposes a narrow authenticated fresh-snapshot endpoint that executes the existing internal snapshot action and returns the bounded semantic snapshot payload.

The desktop command binds resolution to the snapshot route and viewport. Before capture it requires the requested viewport to equal the snapshot viewport. The existing native transaction then revalidates the live viewport after restore. Route or viewport drift fails closed rather than silently cropping stale coordinates.

## Live execution

The desktop command `capture_progressive_target` takes a session, stable element ref, viewport, optional revision, and desired target level.

Execution order:

1. validate caller viewport;
2. preflight exact session-owned LocalView surface;
3. acquire the existing per-session capture gate;
4. request one authenticated fresh semantic snapshot;
5. resolve deterministic progressive targets;
6. reject snapshot/request viewport mismatch;
7. choose the requested level if present, otherwise fail closed; callers may explicitly request the fallback `viewport` level;
8. run the existing shared redacted native transaction exactly once;
9. revalidate live viewport after restore;
10. crop only after private redaction;
11. persist/register the target as viewport or region evidence.

No new WebView2/WKWebView/WebKitGTK screenshot implementation is permitted.

## Security and privacy

- Snapshot retrieval remains authenticated and session-scoped.
- No arbitrary route/window may be supplied by the caller.
- Existing private selectors/freeze receipts remain private capture control state.
- Raw native pixels are never entered into target resolution.
- Target resolution happens on semantic metadata only.
- Pixel processing remains restore → target/live validation → private redaction → crop → artifact/evidence.
- Invalid ownership/geometry/freshness fails closed.

## Error model

The deterministic resolver exposes explicit errors for missing ref, missing/invalid geometry, invalid viewport, and ownership-free requested levels. The desktop surface converts these into bounded user-facing strings and does not fall back silently from `component`/`section` to a broader target unless the caller explicitly requests `viewport`.

## Verification

Required deterministic tests:

- exact stable-ref lookup;
- expanded/clamped element target;
- source-backed nearest component ancestor;
- no fabricated component without source evidence;
- explicit section/landmark ancestor;
- duplicate removal/order stability while retaining explicit viewport fallback;
- invalid/offscreen geometry fail-closed;
- missing ref fail-closed.

Required control/desktop contracts:

- authenticated fresh semantic snapshot endpoint is session-scoped;
- desktop command is registered;
- fresh snapshot precedes target resolution;
- snapshot viewport must match caller viewport;
- shared native transaction performs one viewport acquisition;
- restore precedes live validation/redaction;
- redaction precedes target crop/persistence;
- platform adapters remain `CaptureTarget::Viewport` only;
- region evidence carries the selected resolved rectangle.

## Completion rule

This slice is complete only when deterministic resolver tests, authenticated fresh-snapshot control tests, desktop transaction contracts, cross-platform Rust core CI, stable/native-workspace Tauri compiler gates, and all three real rendered-pixel GUI smoke gates pass on the exact final PR head. The larger `Progressive capture regions` capability remains Partial until token-aware policy and the later verification loop are connected.