# Tier-3 Chromium Rendered Evidence Design

## Goal

Connect the bounded Chromium rendered-pixel executor to LocalView's live Active Perception path without creating a second browser authority, sending PNG bytes through the control API, or pretending Chromium pixels are natively equivalent to WebView2/WKWebView/WebKitGTK pixels.

## Scope

This slice adds one planner-owned Tier-3 rendered capture mode. It does not replace native WebView capture and it does not convert the existing Chromium compatibility Contract probe into a hidden screenshot side effect.

The two Tier-3 modes remain distinct:

- `ChromiumEscalation`: one bounded `--dump-dom` compatibility probe producing Contract evidence.
- `ChromiumRenderedCapture`: one bounded `--screenshot` process producing local PNG artifact storage plus Visual evidence metadata.

Exactly one of those actions may be selected for one planner step.

## Authority

1. Tier-3 Chromium remains planner-owned. Public callers cannot submit a pre-authorized Chromium action, an escalation reason, an evidence id, or a rendered target URL.
2. Both Chromium action kinds require `browser_specific_suspicion`.
3. The planner forces `chromium_spawns = 1` for either action even if a candidate is malformed with zero estimated spawns.
4. Rendered capture additionally forces `image_regions = 1`.
5. `ChromiumRenderedCapture` is derived only when a browser-specific suspicion exists, a viewport is present, and at least one image region remains in the original Perception Budget Contract. Otherwise LocalView derives the existing compatibility probe.
6. Engine Tier-3 admission accepts exactly one planner-authorized Chromium action of either supported Chromium kind. No raw caller boolean may admit Chromium.
7. The existing Runtime Resource Governor `ResourceWorkKind::Chromium` reservation covers process execution and rendered artifact persistence.

## Rendered Viewport Semantics

Headless Chromium rendered evidence uses the requested CSS width and CSS height, but this first live slice normalizes Chromium device scale to `1.0`.

The evidence must therefore record:

- requested CSS width/height;
- actual rendered scale factor `1.0`;
- actual PNG pixel width/height;
- the caller's requested device-scale factor only as non-authoritative request metadata when it differs.

The runtime must never claim that a Chromium screenshot at scale 1.0 is pixel-equivalent to a native WebView capture at another device scale factor. Deterministic visual comparison remains gated by compatible provenance/viewport metadata.

## Target Resolution and Privacy

The target URL is resolved server-side from the exact session endpoint plus the most recent observed route, using the same loopback/same-origin authority as the compatibility probe.

- Only HTTP(S) loopback targets are executable.
- URL username/password are rejected by the Chromium executor.
- Query and fragment are removed from the public route identity retained in evidence and receipts.
- Raw PNG bytes never enter `EvidenceObject.payload`, HTTP JSON, MCP output, or planner state.
- Artifact filesystem paths never enter evidence or public receipts.

## Local Artifact Storage

Rendered PNG bytes are persisted through `localview-artifacts::ArtifactStore` as `visual/png`.

The Chromium runtime configuration owns a lazy, process-local artifact store handle:

- root: `<chromium temp root>/rendered-artifacts`;
- maximum retained bytes: 128 MiB;
- opened lazily on first rendered capture;
- serialized through one async mutex per configured SessionManager runtime;
- content-addressed deduplication and existing ArtifactStore GC semantics are reused.

The evidence/receipt exposes the artifact id and byte count, never the path.

## Evidence Contract

Successful rendered capture inserts exactly one `EvidenceKind::Visual` object:

- `session_id`: exact session;
- `region`: planner-selected target/ref when present;
- provenance source: `chromium-rendered`;
- provenance engine: `chromium`;
- revision: current request revision;
- uncertainty: `Observed`;
- confidence: `1.0`;
- `secret_taint = false`;
- payload fields:
  - `artifact_id`;
  - `bytes`;
  - `target` (private-safe canonical route identity);
  - `backend = "chromium-headless"`;
  - `viewport.css_width`;
  - `viewport.css_height`;
  - `viewport.device_scale_factor = 1.0`;
  - `pixel_width`;
  - `pixel_height`.

No stdout/stderr body bytes are persisted in Visual evidence. Only bounded byte-count/truncation metadata may appear in the execution receipt if needed for diagnostics.

## Active Perception Selection

When `browser_specific_suspicion` is true:

- if a viewport exists and at least one image region remains, derive `ChromiumRenderedCapture` and do not derive `ChromiumEscalation` for that same planning pass;
- otherwise derive `ChromiumEscalation` as today.

This guarantees at most one Chromium spawn for that observation step and prevents a compatibility probe followed by an automatic second browser spawn merely to obtain pixels.

A trusted, current `chromium-rendered` Visual evidence object satisfies the browser-specific observation in the same way a current `chromium-compatibility` Contract does, provided revision and canonical route still match. Route/revision changes invalidate satisfaction and allow a fresh planner action.

## Perception Cycle Receipt and Budget

`PerceptionCycleExecutionReceipt` gains `ChromiumRendered` with metadata only:

- public target;
- artifact id;
- evidence id;
- CSS viewport;
- pixel width/height;
- usage.

Actual usage is:

- `chromium_spawns = 1`;
- `image_regions = 1`;
- `text_tokens = 0`;
- latency replaced by whole-cycle measured wall-clock latency at the existing coordinator boundary.

The coordinator sets `visual_satisfied = true` after successful rendered capture and re-plans from retained evidence.

## Failure Semantics

Fail closed before Visual evidence insertion when:

- Chromium executor is unavailable;
- session/route is invalid;
- Resource Governor denies Chromium;
- viewport dimensions are zero or exceed rendered executor bounds;
- process spawn/I/O/timeout fails;
- process exit is non-zero;
- screenshot is missing, too large, malformed, or dimensions do not match the requested CSS viewport at scale 1.0;
- artifact persistence fails.

No partial Visual evidence may survive a failed capture.

## Tests

TDD must prove:

1. Planner chooses rendered Chromium only with browser-specific suspicion + viewport + remaining image budget.
2. Zero image budget preserves compatibility-only Chromium behavior.
3. Both Chromium action kinds force one spawn; rendered capture additionally forces one image region.
4. Engine Tier-3 admission accepts either exact planner-authorized Chromium kind and rejects unplanned/malformed authority.
5. Rendered runtime resolves only server-owned loopback/same-origin routes, strips query/fragment from public identity, reserves `ResourceWorkKind::Chromium`, persists PNG locally, and retains Visual evidence without PNG bytes or paths.
6. Nonzero exit, invalid screenshot, timeout, invalid viewport and artifact failure create no Visual evidence.
7. Perception cycle returns only metadata, charges one Chromium spawn + one image region, and does not run the compatibility probe in the same step.
8. Current rendered Visual evidence suppresses repeated browser escalation; stale route/revision does not.
9. Existing compatibility, native visual, cancellation, budget and rendered-pixel executor contracts stay green.

## Non-goals

- No cross-engine pixel equality claim.
- No full-page stitching.
- No Chromium pool or permanent browser process.
- No DevTools Protocol screenshot transport.
- No remote URL browsing.
- No PNG bytes in daemon evidence or public JSON.
- No caller-controlled Tier-3 authorization.
