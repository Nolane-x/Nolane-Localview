# Wave 3 Performance-Lite Budget Packets — Engineering Design

## Status

Canonical implementation specification for the bounded Wave 3 performance-lite telemetry slice.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Base: `main@252f5df7cd53e3dca7e411bcbc6737d1a86db9bc`

Branch: `feat/wave3-performance-lite-packets`

## Goal

Turn LocalView's already-live long-task and layout-shift observations into a compact, deterministic, privacy-safe performance packet that can be consumed through the authenticated exact-session control plane without adding a profiler, a second browser collector, raw resource payload capture, or unbounded telemetry.

## Existing authority

Already landed:

- managed-page `PerformanceObserver` collection for `longtask` and layout-shift entries;
- bounded observer-ring retention and authenticated exact-session drain;
- desktop normalization into `ObserverEventKind::Performance`;
- `localview-live-analysis` conversion of performance events into deterministic findings;
- framework-specific HMR observation and the existing capture-settle HMR quiet gate.

This slice reuses those observations. It does not create another PerformanceObserver or permanent browser process.

## Packet schema

The canonical packet is aggregate-first:

- schema version;
- total observed long-task count;
- saturating total long-task duration;
- maximum observed long-task duration;
- a bounded deterministic sample of the longest durations;
- omitted-sample count;
- cumulative layout shift when finite and non-negative;
- the actually applied long-task sample budget.

No URL, route, DOM text, source path, stack, network body, cookie, token, resource name, module name, HMR payload, or arbitrary page payload is admitted to the packet.

## Budget authority

Default long-task sample budget: **8**.

Hard output cap: **16** samples, regardless of caller-provided budget.

A zero sample budget is valid and produces an aggregate-only packet.

The packet builder scans all supplied long-task durations so count/total/max remain truthful while allocating only the bounded output sample vector. The chosen samples are the longest durations in deterministic descending order. Oversized requested budgets are clamped to the hard cap.

The live control endpoint does not expose a caller-writable budget. It uses the canonical default budget so public callers cannot expand packet size.

## Live authority

Add authenticated:

`GET /v1/sessions/{id}/performance-lite`

Rules:

1. bearer authentication is mandatory;
2. the session must exist;
3. only the exact session's retained observer window is read;
4. the retained input window remains bounded by the existing 2,048-event analysis limit;
5. only `ObserverEventKind::Performance` events contribute to the packet;
6. malformed/unknown performance payloads degrade to harmless zero/ignored measurements rather than escaping arbitrary data;
7. the response contains only the bounded packet.

The existing `/analysis` response also carries the same canonical performance-lite packet so diagnosis and direct packet reads cannot diverge semantically.

## HMR documentation closure

Wave 3 HMR signal production landed immediately before this slice. Documentation in this branch may move framework-specific HMR signal production from “remaining” to “landed,” but performance-lite itself must remain unclaimed until exact-head verification is green.

## Verification gates

### Gate 1 — pure packet policy

`localview-performance` tests must prove:

- default eight-sample bound;
- hard sixteen-sample cap;
- zero-budget aggregate-only behavior;
- deterministic descending selection of the longest durations;
- truthful full-input count/total/max despite sampling;
- non-finite/negative CLS is not emitted.

### Gate 2 — live-analysis integration

`localview-live-analysis` tests must prove:

- live long-task/layout-shift events produce the canonical packet;
- packet sampling does not alter deterministic performance findings;
- unrelated observer event kinds cannot enter the packet;
- HMR events have their own count and are not misclassified as performance samples.

### Gate 3 — control-plane authority

An integration test must prove:

- unauthenticated access is rejected;
- unknown sessions are rejected;
- exact-session retained performance events produce the bounded packet;
- more than eight long tasks still return exactly eight samples by default;
- the response does not retain route or arbitrary payload strings.

### Gate 4 — repository regression

The repository's canonical CI normalizes current-toolchain Rust formatting before compile/test rather than requiring a clean formatting diff from the historical source tree. This focused gate follows that same policy and must not introduce a repository-wide formatting-only commit.

Minimum exact-head closure:

```text
cargo fmt --all
cargo test -p localview-performance
cargo test -p localview-live-analysis
cargo test -p localview-control --test performance_lite
cargo check -p localview-control --all-targets
cargo check --workspace --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- CPU profiling;
- JavaScript flamegraphs;
- heap snapshots or allocation profiling;
- resource-body/header capture;
- Core Web Vitals completeness;
- browser-wide or arbitrary internet performance monitoring;
- remote telemetry upload;
- user-configurable unbounded history;
- HMR compile-duration measurement;
- source-map ownership or component attribution;
- root-cause proof from performance telemetry alone.

## Completion definition

The slice is complete when one exact LocalView session can expose a deterministic bounded performance-lite packet derived only from its already-retained long-task/layout-shift telemetry, with hard output caps, exact-session authentication and regression proof, while raw application payloads and unrelated observer data remain outside the packet.
