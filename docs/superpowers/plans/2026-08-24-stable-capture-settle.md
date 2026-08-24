# Stable Capture Settle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate native viewport capture on a deterministic, bounded settle decision derived from existing live observer history and privacy-safe page readiness metadata.

**Architecture:** `localview-capture` owns a pure settle evaluator. Instrumentation enriches semantic snapshots with font/image readiness. The daemon derives settle observations from `LiveBridge` and exposes one authenticated read endpoint. Desktop polls that endpoint under the default 5-second policy before invoking the already-landed native capture path.

**Tech Stack:** Rust, Axum, Tokio, Tauri 2.11.5, existing LocalView instrumentation JavaScript, serde/chrono.

**Spec:** `docs/superpowers/specs/2026-08-24-stable-capture-settle-design.md`

## Global Constraints

- Keep desktop, capture, control, evidence, protocol and artifacts crates `#![forbid(unsafe_code)]`.
- Do not add browser-side screenshot reconstruction, Chromium fallback, cloud/account requirements or response-body capture.
- Use `StableCapturePolicy::default()` for the first live settle path.
- Settle timeout is 5,000 ms; native platform capture timeout remains 3 seconds.
- Timeout fails closed; never silently capture an unstable page.
- Network settle is explicitly a quiet-period heuristic over metadata events, not proof of zero in-flight requests.

---

### Task 1: Pure settle evaluator

**Files:**
- Modify: `crates/capture/src/lib.rs`
- Test: `crates/capture/src/lib.rs` unit tests

**Interfaces:**
- Produces `SettleReason`, `SettleObservation`, `SettleDecision`.
- Produces `pub fn evaluate_settle(policy: &StableCapturePolicy, observation: &SettleObservation) -> SettleDecision`.

- [ ] Add RED tests for ready/quiet, missing snapshot, DOM, fonts, images, HMR, DOM mutation, layout, network, disabled policy gates and retry bounds.
- [ ] Run `cargo test -p localview-capture` and verify RED due missing settle API.
- [ ] Implement the minimal pure evaluator using 300 ms HMR, 200 ms DOM/layout and `network_quiet_ms` windows; treat an event as recent when `now - event_at < window` and tolerate future timestamps by treating them as recent.
- [ ] Run `cargo test -p localview-capture` and verify GREEN.
- [ ] Commit `feat(capture): add deterministic settle evaluator`.

### Task 2: Privacy-safe readiness metadata

**Files:**
- Modify: `crates/instrumentation/src/lib.rs`
- Test: `crates/instrumentation/tests/live_semantic_contract.rs`
- Test: existing privacy/source contracts

**Interfaces:**
- Semantic snapshot adds `readiness: { fonts, pendingImages, totalImages }`.

- [ ] Add RED source-contract assertions for `readiness`, `document.fonts.status`, `pendingImages`, `document.images`.
- [ ] Run the instrumentation contract and verify RED.
- [ ] Add a small `readinessPacket()` helper. `fonts` is `document.fonts?.status || 'unsupported'`; pending images count only `!image.complete`; total images is `document.images.length`.
- [ ] Re-run live semantic, privacy and visibility/source tests; verify GREEN and no new secret-bearing API strings.
- [ ] Commit `feat(instrumentation): expose capture readiness metadata`.

### Task 3: Authenticated capture-settle endpoint

**Files:**
- Modify: `crates/control/Cargo.toml` only if `localview-capture` is not already a dependency.
- Modify: `crates/control/src/lib.rs`
- Create: `crates/control/tests/capture_settle.rs`

**Interfaces:**
- `GET /v1/sessions/{id}/capture-settle` returns `SettleDecision`.
- Consumes `LiveBridge::recent(id, 2048)` and `StableCapturePolicy::default()`.

- [ ] Write RED router tests: unauthorized -> 401, unknown session -> 404, no snapshot -> unstable/no_semantic_snapshot, stable semantic snapshot -> stable, recent layout/network/HMR -> corresponding reasons.
- [ ] Run `cargo test -p localview-control --test capture_settle` and verify RED because route does not exist.
- [ ] Add helper that finds latest timestamps by event kind and parses only `readyState`, `readiness.fonts`, `readiness.pendingImages` from the latest semantic snapshot payload.
- [ ] Add authenticated route and response using daemon `Utc::now().timestamp_millis()`.
- [ ] Run control settle + visual evidence tests and verify GREEN.
- [ ] Commit `feat(control): expose capture settle state`.

### Task 4: Desktop settle-before-capture transaction

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/stable_capture_settle_contract.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Add private `async fn wait_for_capture_settle(session_id: SessionId) -> Result<(), String>`.
- `capture_viewport` must call it before `capture_managed_surface`.

- [ ] Add RED source contract requiring `/capture-settle`, `StableCapturePolicy::default`, `timeout_ms`, `retry_after_ms`, and ordering of settle call before `capture_managed_surface`; require timeout text and prohibit fallback capture after timeout.
- [ ] Add CI invocation for the new desktop contract, then run/observe RED.
- [ ] Implement authenticated polling with one overall `tokio::time::timeout(Duration::from_millis(policy.timeout_ms), ...)`. Each unstable response sleeps `Duration::from_millis(decision.retry_after_ms.clamp(25, 100))`. Stable returns immediately. Timeout reports bounded final reason names.
- [ ] Verify desktop stable + native-workspace compile and all desktop contracts GREEN.
- [ ] Commit `feat(desktop): settle managed page before native capture`.

### Task 5: Coverage, review and final verification

**Files:**
- Modify: `docs/ROADMAP.md`
- Modify: `docs/SPEC_COVERAGE.md`

**Interfaces:**
- Stable settle remains honestly scoped: readiness + observer quiet-window heuristic is live; animation freeze/masking/network in-flight accounting remain incomplete.

- [ ] Update Wave 2 documentation to list live settle transaction as landed and preserve remaining gaps.
- [ ] Review PR diff for payload leakage, caller-controlled timestamps, timeout bypass, stale-route capture and unbounded histories.
- [ ] Run fresh-head CI and require `completed/success` on Ubuntu/macOS/Windows/Tauri plus explicit native-capture and new stable-settle desktop gates.
- [ ] If review finds a bug, add a RED regression first, fix GREEN, then rerun fresh-head CI.
- [ ] Open/finish PR against `main`; merge only after verified head is mergeable and CI is green, using `expected_head_sha`.
