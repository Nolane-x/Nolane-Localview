# Stable Capture Settle Transaction Design

## Status

Approved by standing user instruction to continue the LocalView roadmap without additional approval gates. This slice starts from `main` merge commit `5c0ab035e2e4e8d558a697959d0a7546ca47b4bb` after the native viewport capture vertical slice landed.

## Goal

Prevent LocalView from taking a native screenshot while the managed page is still materially changing. A viewport capture must either enter a deterministic stable state within a bounded deadline or fail with explicit instability reasons; it must never silently sleep for an arbitrary duration and then claim the resulting image is stable.

The slice is:

`live observer/readiness metadata -> pure Rust settle evaluator -> authenticated daemon settle endpoint -> bounded desktop poll -> native viewport capture`

## Architectural choice

The settle state is derived in Rust from the existing bounded observer history rather than implemented as a page-side long-running promise or a desktop-only debounce.

Reasons:

- observer events already carry session identity, sequence ordering, timestamps, route and event kinds;
- the control plane already owns the live session state and can expose a narrow authenticated read endpoint;
- a pure evaluator is deterministic and portable across Windows, macOS and Linux;
- the desktop remains a coordinator rather than a second runtime-state authority;
- native capture adapters remain concerned only with platform pixels;
- timeout behavior can return structured reasons instead of degrading to a blind screenshot.

## Existing primitives reused

`localview-capture` already defines `StableCapturePolicy` and stages such as `DomReady`, `FontsReady`, `ImagesReady`, `HmrSettled`, `LayoutStable` and `NetworkQuiet`. The existing `LiveBridge` stores bounded `ObserverEvent` history with `captured_at` timestamps and kinds including `DomMutation`, `Layout`, `Network`, `Hmr` and `SemanticSnapshot`. The semantic snapshot already contains `readyState`.

This slice turns those planner concepts into a live settle decision without moving platform capture or unsafe code into the control plane.

## Page readiness metadata

The semantic snapshot packet gains a small `readiness` object:

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

- `readyState` remains the browser `document.readyState` value already present.
- `fonts` is `document.fonts.status` when the Font Loading API exists; otherwise `"unsupported"`.
- `pendingImages` counts only `<img>` elements where `complete == false`.
- `totalImages` is the bounded DOM count returned by `document.images.length`.
- no image URL, response body, cookie, storage value, canvas pixels or font resource URL is included.
- no additional network interception is introduced.

## Pure settle evaluator

`localview-capture` gains portable types:

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

To avoid adding new policy fields in this slice, existing settings are interpreted as follows:

- DOM ready: require semantic snapshot `readyState == "complete"` when `wait_dom_ready` is true.
- fonts: require `fonts_status == "loaded"` or `"unsupported"` when `wait_fonts` is true.
- images: require `pending_images == 0` when `wait_images` is true.
- HMR settle: require no HMR event in the last 300 ms when `wait_hmr_settle` is true.
- DOM mutation settle: when `wait_layout_stable` is true, require no DOM mutation in the last 200 ms.
- layout settle: when `wait_layout_stable` is true, require no layout event in the last 200 ms.
- network quiet: when `network_quiet_ms = Some(ms)`, require no network event in the last `ms`.
- `retry_after_ms` is bounded to `25..=100` ms and represents the shortest useful re-check delay.

A missing semantic snapshot is an explicit unstable reason whenever DOM/font/image readiness is required.

This slice does not infer true browser network-idle from request lifecycle because current instrumentation records metadata events rather than a durable in-flight request counter. `NetworkRecent` therefore means a quiet-period heuristic, not proof that zero requests are in flight. Documentation and evidence must preserve that distinction.

## Control-plane endpoint

Add:

`GET /v1/sessions/{id}/capture-settle`

Authentication and session checks match the existing control routes.

The daemon constructs `SettleObservation` from the bounded `LiveBridge::recent` history:

- latest semantic snapshot provides `readyState`, `readiness.fonts` and `readiness.pendingImages`;
- latest event timestamp by kind supplies HMR/DOM/layout/network timestamps;
- `now_unix_ms` is daemon UTC wall-clock time.

The request accepts no caller-controlled timestamps or observer state. The endpoint uses `StableCapturePolicy::default()` for this first live path so the desktop cannot weaken settle requirements through arbitrary IPC arguments.

Response:

```json
{
  "stable": false,
  "reasons": ["layout_recent", "network_recent"],
  "retry_after_ms": 50
}
```

No observer payload bodies are returned by this endpoint.

## Desktop transaction

`capture_viewport` performs:

1. validate viewport;
2. call the authenticated settle endpoint for the session;
3. if stable, immediately resolve the managed surface and invoke native capture;
4. if unstable, sleep only `retry_after_ms` and poll again;
5. stop after the stable-capture policy timeout (`5_000 ms` default);
6. on timeout, return a bounded error listing the final instability reason names;
7. only after settle success may native capture execute and persist/register the resulting artifact.

The native adapter retains its own 3-second completion timeout. The 5-second settle deadline and 3-second platform capture timeout are separate budgets.

The desktop may not silently capture after settle timeout.

## Explicit non-goals

This slice does not implement:

- animation/transition freezing;
- private-selector masking;
- element/region/full-page capture;
- network request lifecycle accounting;
- browser-side screenshot reconstruction;
- hosted GUI screenshot smoke;
- visual diffing or stitching.

Those remain later Wave 2 slices.

## Error model

The desktop returns stable bounded error strings at the outer Tauri boundary, including:

- daemon unavailable/auth failure;
- settle endpoint malformed response;
- `capture settle timed out: <comma-separated reasons>`;
- existing managed-surface and native-capture errors.

Settle reasons contain no URLs, selector text, network bodies or user values.

## Testing strategy

### Pure evaluator

RED -> GREEN tests must prove:

- fully ready/quiet observation is stable;
- absent semantic snapshot blocks required readiness;
- loading DOM blocks capture;
- pending fonts and images block capture;
- recent HMR/DOM/layout/network events block capture independently;
- events exactly outside their quiet windows do not block capture;
- disabling a policy gate removes only that reason;
- retry delay is bounded.

### Instrumentation contract

Tests require snapshot script to expose `readiness`, `document.fonts.status`, `pendingImages` and `document.images`, while privacy tests continue rejecting response bodies, storage, cookies and canvas screenshot paths.

### Control endpoint

Router tests with a real `LiveBridge` verify stable and unstable responses, authorization, missing session behavior, and parsing of semantic readiness without returning arbitrary snapshot payloads.

### Desktop contract

A source contract verifies `capture_viewport` calls the settle endpoint before `capture_managed_surface`, uses the default 5-second deadline, respects `retry_after_ms`, and has no fallback that proceeds after timeout.

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
- new stable-settle desktop contract.

## Completion criteria

This slice is complete when a live `capture_viewport` request cannot reach native platform capture until the daemon reports stable under the default policy, timeout fails closed with explicit bounded reasons, readiness metadata remains privacy-safe, and the final branch head passes all cross-platform CI gates.
