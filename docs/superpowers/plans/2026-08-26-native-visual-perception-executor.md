# Native Visual Perception Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect planner-selected native visual observations to the existing audited Tauri visual-packet transaction through a dedicated daemon↔desktop executor bridge, without creating a second screenshot authority or caller-controlled escalation path.

**Architecture:** The daemon and desktop are separate processes. A dedicated native-executor queue will live beside, but remain distinct from, page `BridgeAction` queues. The control-plane Active Perception cycle will enqueue an internally-authorized visual request and wait for its exact result; the desktop will poll that private executor endpoint and delegate the request to the existing `capture_visual_packet_authorized` helper. Actual desktop-reported usage becomes the cycle's cumulative delta, and planner authority remains the only source of escalation reasons.

**Tech Stack:** Rust 2024, Axum, Tokio, Tauri 2, Reqwest, serde, LocalView `protocol`, `live-bridge`, `token-budget`, `control`, `native-capture`, desktop visual capture.

**Spec:** `docs/IMPLEMENTATION_STATUS.md`, `docs/ROADMAP.md`, expanded V3 Active Perception requirements already encoded by the four-dimensional `PerceptionBudgetContract`.

## Global Constraints

- Perception Budget dimensions remain exactly `latency_ms`, `text_tokens`, `image_regions`, `chromium_spawns`.
- CPU/RAM/storage/browser-process/hidden-surface/concurrency/cache limits remain Runtime Resource Governor concerns.
- No public caller can submit native-executor requests, cumulative `spent`, serialized planner authority, or `budget_escalation_reason`.
- Native visual execution must reuse `capture_visual_packet_authorized`; do not call platform screenshot APIs from `localview-control` or add a second capture transaction.
- Native-executor requests/results are session-bound, request-ID correlated, bounded, authenticated and isolated from page `BridgeAction` queues.
- No raw PNG bytes or filesystem paths cross the daemon↔desktop executor result boundary.
- Chromium remains unimplemented in this slice and must continue to fail closed.
- Existing public `/perception/plan` and `/perception/step` behavior remains backward compatible.

---

### Task 1: Shared viewport and dedicated native-executor queue

**Files:**
- Modify: `crates/protocol/src/lib.rs`
- Modify: `crates/native-capture/src/lib.rs`
- Modify: `crates/live-bridge/Cargo.toml`
- Modify: `crates/live-bridge/src/lib.rs`
- Test: `crates/live-bridge/tests/native_executor_queue.rs`

**Interfaces:**
- Produces shared `ViewportMeta` in `localview-protocol`, re-exported by `localview-native-capture`.
- Produces `NativeExecutorAction::VisualPacket`, `NativeExecutorRequest`, `NativeExecutorResult`, and dedicated enqueue/take/claim/complete/result methods on `LiveBridge`.

- [ ] **Step 1: Write RED queue contracts** proving native requests are not returned by `take_actions`, exact session/request correlation is required, results cannot complete without a claimed native origin, and queue retention is bounded.
- [ ] **Step 2: Run CI and observe the RED failure due to missing native-executor types/methods.**
- [ ] **Step 3: Move/re-export `ViewportMeta` through protocol and implement the minimal dedicated queue with its own pending/inflight/claimed/results storage.**
- [ ] **Step 4: Run focused live-bridge tests plus workspace check/Clippy.**

### Task 2: Authenticated daemon native-executor endpoints

**Files:**
- Modify: `crates/control/src/runtime.rs`
- Test: `crates/control/tests/native_executor_transport.rs`

**Interfaces:**
- Produces authenticated desktop-only polling/result endpoints for requests that have already been created internally by control code.
- No public POST endpoint exists for creating native-executor requests.

- [ ] **Step 1: Write RED HTTP contracts** proving unauthorized polling/result submission is rejected, unknown sessions fail, exact inflight origin is mandatory, and the API exposes no request-creation route.
- [ ] **Step 2: Observe RED.**
- [ ] **Step 3: Implement GET take + POST result endpoints over the dedicated LiveBridge queue.**
- [ ] **Step 4: Run focused control transport tests.**

### Task 3: Whole-cycle RegionCapture execution through the native bridge

**Files:**
- Modify: `crates/control/src/perception.rs`
- Modify: `crates/control/src/perception_cycle.rs`
- Test: `crates/control/tests/perception_cycle_visual_executor.rs`

**Interfaces:**
- `LivePerceptionPlanRequest` gains optional shared `viewport` and `revision` fields; non-visual callers remain compatible.
- Internal cycle code derives a remaining operation budget from cumulative spent, enqueues one `NativeExecutorAction::VisualPacket`, waits only for the matching result, and replaces planner reservation with actual returned non-latency usage plus measured cycle latency.

- [ ] **Step 1: Write RED integration contracts** with a fake native worker consuming the real queue: semantic-known/visual-unknown cycle must enqueue a native visual request, reject missing viewport, preserve planner-owned escalation reason, accept exact actual usage, re-plan after returned visual evidence, and never enqueue a generic page action.
- [ ] **Step 2: Observe RED.**
- [ ] **Step 3: Implement the minimal RegionCapture executor path and exact-result wait with a bounded timeout. Chromium/other unsupported kinds remain fail-closed.**
- [ ] **Step 4: Run planner/control suites and confirm cumulative receipts use actual visual usage rather than planner estimates.**

### Task 4: Desktop native worker delegates to the audited capture authority

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/visual_packet_impl.rs`
- Create: `apps/desktop/src-tauri/src/native_executor.rs`
- Test: `apps/desktop/src-tauri/tests/native_visual_executor_contract.rs`

**Interfaces:**
- Desktop setup spawns a bounded poller using the existing bearer token.
- `NativeExecutorAction::VisualPacket` converts shared viewport metadata and calls `capture_visual_packet_authorized` with the planner-owned operation budget/reason.
- Result posts only request ID, success/error, actual `PerceptionBudgetUsage` and metadata receipt JSON.

- [ ] **Step 1: Write RED desktop authority contract** locking delegation to `capture_visual_packet_authorized` and forbidding calls to raw capture helpers/platform adapters in the worker.
- [ ] **Step 2: Observe RED.**
- [ ] **Step 3: Make the helper `pub(crate)`, implement the poll/execute/post worker, and start it from Tauri `.setup()` after managed state exists.**
- [ ] **Step 4: Run stable/native-workspace desktop checks and all visual authority contracts.**

### Task 5: Verification, CI gate, docs, merge

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1: Add explicit cross-platform native-executor transport/cycle gates and desktop native visual delegation gate.**
- [ ] **Step 2: Update docs truthfully: typed bridge + desktop delegation are landed; real hosted Active Perception visual-cycle GUI proof is required before claiming complete end-to-end native visual autonomous execution if CI does not exercise a real managed surface through the new worker.**
- [ ] **Step 3: Manual PR diff review for authority duplication, secret leakage, unbounded queue/poll loops, and budget conflation.**
- [ ] **Step 4: Require exact-final-head CI success on Ubuntu/macOS/Windows core, Tauri/frontend and WebKitGTK/WKWebView/WebView2 rendered-pixel smokes.**
- [ ] **Step 5: Squash-merge with expected-head lock only after exact-head verification.**
