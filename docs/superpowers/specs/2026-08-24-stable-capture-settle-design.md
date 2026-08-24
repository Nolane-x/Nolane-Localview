# Stable Capture Settle Transaction Design

## Status

Approved by standing user instruction to continue the LocalView roadmap without additional approval gates. This slice starts from `main` merge commit `5c0ab035e2e4e8d558a697959d0a7546ca47b4bb` after the native viewport capture vertical slice landed.

Implementation review hardened the original design in three important ways:

1. every settle sample requests an exact fresh `Snapshot` action instead of trusting the latest semantic snapshot already present in observer history;
2. fresh-snapshot presence is anchored to daemon evaluation time, not the WebView-provided action completion clock;
3. desktop capture preflights a managed LocalView surface before settling, then the native acquisition path reads and loopback-validates the live route again after settle.

## Goal

Prevent LocalView from taking a native screenshot while the managed page is still materially changing. A viewport capture must either enter a deterministic stable state within a bounded deadline or fail with explicit instability reasons; it must never silently sleep for an arbitrary duration and then claim the resulting image is stable.

The implemented slice is:

`managed-surface preflight -> exact fresh snapshot + live activity history -> pure Rust settle evaluator -> authenticated daemon settle endpoint -> bounded desktop poll -> route revalidation -> native viewport capture`

## Architectural choice

The settle decision is derived in Rust from two sources with different responsibilities:

- an exact fresh completed `Snapshot` action supplies current DOM/font/image readiness;
- bounded `LiveBridge` observer history supplies recent DOM mutation, layout, fetch/XHR completion and optional HMR event timestamps.

Reasons:

- a fresh snapshot prevents a stale `DOMContentLoaded` or earlier semantic packet from satisfying current readiness;
- observer events already carry session identity, sequence ordering, timestamps, route and event kinds;
- the control plane owns the live session state and exposes only a narrow authenticated decision endpoint;
- a pure evaluator is deterministic and portable across Windows, macOS and Linux;
- the desktop remains a coordinator rather than a second runtime-state authority;
- native capture adapters remain concerned only with platform pixels;
- timeout behavior returns structured reasons instead of degrading to a blind screenshot.

## Existing primitives reused

`localview-capture` already defines `StableCapturePolicy` and stages such as `DomReady`, `FontsReady`, `ImagesReady`, `HmrSettled`, `LayoutStable` and `NetworkQuiet`. `LiveBridge` provides a bounded action queue/result history and bounded observer history. The managed WebView can execute a `Snapshot` action through the existing authenticated caller/session-owned bridge.

The implementation does not move platform capture or unsafe code into the control plane.

## Page readiness metadata

The semantic snapshot packet contains a small `readiness` object:

```json
{
  "readyState": "complete",
  "readiness": {
    "fonts": "loaded",
    "pendingImages": 0,
    "totalImages": 12
  }
}
```

Rules:

- `readyState` is browser `document.readyState`.
- `fonts` is `document.fonts.status` when the Font Loading API exists; otherwise `"unsupported"`.
- `pendingImages` counts `<img>` elements where `complete == false`.
- `totalImages` is `document.images.length`.
- no image URL, response body, cookie, storage value, canvas pixels or font resource URL is included.
- no screenshot reconstruction is added.

Readiness freshness is transaction-driven: the control plane asks the exact managed WebView for a new snapshot on every settle sample. It does not require a permanent font/image readiness listener and does not trust an arbitrary old semantic snapshot.

## Pure settle evaluator

`localview-capture` exposes portable types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettleReason {
    NoSemanticSnapshot,
    DomNotReady,
    FontsPending,
    ImagesPending,
    HmrRecent,
    DomMutationRecent,
    LayoutRecent,
    NetworkRecent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettleObservation {
    pub now_unix_ms: i64,
    pub latest_semantic_at_unix_ms: Option<i64>,
    pub ready_state: Option<String>,
    pub fonts_status: Option<String>,
    pub pending_images: Option<u32>,
    pub latest_hmr_at_unix_ms: Option<i64>,
    pub latest_dom_mutation_at_unix_ms: Option<i64>,
    pub latest_layout_at_unix_ms: Option<i64>,
    pub latest_network_at_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettleDecision {
    pub stable: bool,
    pub reasons: Vec<SettleReason>,
    pub retry_after_ms: u64,
}
```

The evaluator signature is:

```rust
pub fn evaluate_settle(
    policy: &StableCapturePolicy,
    observation: &SettleObservation,
) -> SettleDecision
```

### Quiet windows

- DOM ready: require fresh snapshot `readyState == "complete"` when `wait_dom_ready` is true.
- fonts: require `fonts_status == "loaded"` or `"unsupported"` when `wait_fonts` is true.
- images: require `pending_images == 0` when `wait_images` is true.
- HMR settle: if an HMR observer event exists, require no HMR event in the last 300 ms when `wait_hmr_settle` is true.
- DOM mutation settle: when `wait_layout_stable` is true, require no DOM mutation in the last 200 ms.
- layout settle: when `wait_layout_stable` is true, require no layout event in the last 200 ms.
- network quiet: when `network_quiet_ms = Some(ms)`, require no captured fetch/XHR completion event in the last `ms`.
- `retry_after_ms` is bounded to `25..=100` ms.

A missing or failed fresh snapshot is an explicit unstable reason whenever DOM/font/image readiness is required.

The HMR gate is evaluator support, not a claim that framework-specific HMR signal production is already complete. That live signal source remains Wave 3 work.

The network gate is also deliberately described as a completion-event quiet-period heuristic. Current instrumentation records fetch/XHR metadata at completion; it does not yet prove zero requests are currently in flight. True in-flight accounting remains a later Wave 2 hardening slice.

## Control-plane endpoint

Endpoint:

`GET /v1/sessions/{id}/capture-settle`

Authentication and session checks match existing control routes. The endpoint accepts no caller-controlled policy, timestamps or observer state.

For every request the daemon:

1. verifies authentication and session existence;
2. enqueues an exact `BridgeActionKind::Snapshot` for that session;
3. waits up to 650 ms for the result with 20 ms bounded polling;
4. treats a missing/failed exact result as no fresh semantic snapshot;
5. reads only `readyState`, `readiness.fonts` and `readiness.pendingImages` from the fresh payload;
6. anchors fresh-snapshot presence to daemon `now_unix_ms`, ignoring the page-provided `completed_at` for settle freshness;
7. reads only relevant activity timestamps from bounded observer history;
8. evaluates `StableCapturePolicy::default()` and returns only `SettleDecision`.

Response example:

```json
{
  "stable": false,
  "reasons": ["layout_recent", "network_recent"],
  "retry_after_ms": 50
}
```

No semantic payload body, URL body, selector text, form value or secret is returned by this endpoint.

## Desktop transaction

`capture_viewport` performs:

1. validate viewport metadata;
2. preflight that an exact session-owned LocalView preview or feature-gated workspace WebView exists and currently has an allowed loopback route;
3. poll the authenticated settle endpoint;
4. if unstable, sleep only the bounded `retry_after_ms` value and poll again;
5. fail closed after `StableCapturePolicy::timeout_ms` (`5_000 ms` default), retaining only bounded reason enums for diagnostics;
6. only after settle success enter the native capture coordinator;
7. read the managed WebView route again and loopback-validate it again, closing the preflight/navigation race;
8. invoke the native platform adapter;
9. persist/register the artifact only after successful native capture.

The native adapter retains its separate 3-second completion timeout. The 5-second settle deadline and 3-second platform capture timeout are separate budgets.

There is no fallback that captures after settle timeout.

## Security and trust boundaries

- Caller cannot select arbitrary top-level windows or arbitrary routes.
- Capture resolves only exact session-owned LocalView managed surfaces.
- The route is validated before settle and immediately before acquisition.
- The page supplies bounded readiness values but does not supply settle policy or daemon time.
- Fresh snapshot action ID must exactly match the result used for that settle sample.
- Stale semantic observer payloads cannot satisfy current readiness.
- Page action `completed_at` is not trusted as the daemon freshness timestamp.
- Snapshot response content is not echoed from the settle endpoint.
- Existing secret/body/storage privacy constraints remain in force.

## Explicit non-goals for this slice

This PR does not implement:

- animation/transition freezing and restoration;
- private-selector screenshot masking;
- element/region/full-page capture;
- true network in-flight request accounting;
- framework-specific live HMR signal production;
- browser-side screenshot reconstruction;
- hosted GUI screenshot smoke;
- visual diffing or stitching.

Those remain later Wave 2/Wave 3 slices and must not be presented as completed by this transaction.

## Error model

The desktop can fail for:

- missing/invalid managed surface;
- daemon unavailable/auth failure;
- malformed settle response;
- failed or missing fresh snapshot, represented as an unstable settle reason;
- stable settle timeout with bounded final reason names;
- existing native capture/artifact/evidence errors.

Settle reasons contain no URLs, selector text, network bodies or user values.

## Testing strategy

### Pure evaluator

RED -> GREEN tests prove:

- fully ready/quiet observation is stable;
- absent semantic snapshot blocks required readiness;
- loading DOM blocks capture;
- pending fonts and images block capture;
- recent HMR/DOM/layout/network events block independently when present;
- events exactly outside quiet windows do not block capture;
- future activity timestamps fail safe as recent;
- disabling a policy gate removes only its reason;
- retry delay is bounded.

### Instrumentation contract

Tests require readiness metadata while privacy tests continue rejecting response bodies, storage, cookies and image URL capture.

### Control endpoint

Tests use a real `LiveBridge` action lifecycle and prove:

- auth and known-session checks;
- failed fresh snapshot fails closed;
- ready fresh snapshot can settle;
- stale observer snapshot cannot override a contradictory fresh snapshot;
- private snapshot payload is not returned;
- independent recent activity reasons are preserved;
- page-provided action completion clock cannot become the daemon fresh-snapshot timestamp.

### Desktop contract

Source contracts prove:

- managed surface is preflighted before polling;
- settle succeeds before native pixels can be acquired;
- default five-second policy deadline is enforced;
- retry is clamped to 25–100 ms;
- there is no timeout fallback;
- native coordinator reads and validates the route after settle.

### Full verification

Final head must pass:

- workspace format/check;
- Clippy with `-D warnings`;
- full Rust tests on Ubuntu, macOS and Windows;
- native capture platform contract on all three OSes;
- frontend build;
- stable Tauri backend compile;
- `native-workspace` backend compile;
- capability/workspace/semantic/native-capture desktop regression contracts;
- stable-settle desktop contract.

## Completion criteria

This slice is complete when a live `capture_viewport` request cannot reach native platform capture until the daemon reports stable under the default policy, every readiness sample is backed by an exact fresh snapshot, timeout fails closed with explicit bounded reasons, page clocks cannot set fresh-snapshot provenance, route/session ownership is revalidated before acquisition, readiness metadata remains privacy-safe, and the final branch head passes all cross-platform CI gates.
