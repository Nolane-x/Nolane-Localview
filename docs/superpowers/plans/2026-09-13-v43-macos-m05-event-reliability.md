# V4.3 macOS M05 Event Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close V4.3 macOS L7 M05 so an unsupported AX notification can never be interpreted as complete event coverage.

**Architecture:** Add a small provider-owned observer reliability model that records requested semantic dimensions and exact AX registration outcomes. Registration completeness is derived by LocalView, never supplied by callers. Then prove the model against real `AXObserverAddNotification` results from a deterministic AppKit seed while keeping M06 application reincarnation and M07 run-loop liveness out of scope.

**Tech Stack:** Rust, macOS ApplicationServices/CoreFoundation FFI, AppKit Swift seed, GitHub Actions macOS runner.

**Spec:** `LocalView_AI_Native_Localhost_Runtime_Product_Spec_v4_3_Principal_Provider_Reconciliation_Closure(9).md` §§1255–1258, 1324, 1381.

## Global Constraints

- Unsupported notification registration lowers event assurance; it never becomes a completeness claim.
- Direct reads/reconciliation remain possible even when event coverage is incomplete.
- Registration result is OS/provider evidence, not caller-submitted PASS/FAIL.
- M06 observer/application reincarnation and M07 run-loop liveness remain separate slices.
- Real-provider evidence must bind the exact candidate SHA and use independent test-only seed ground truth.

---

### Task 1: Registration reliability authority

**Files:**
- Create: `crates/macos-ax-provider/src/observer_reliability.rs`
- Modify: `crates/macos-ax-provider/src/provider.rs`
- Test: `crates/macos-ax-provider/tests/m05_event_reliability_contract.rs`

**Interfaces:**
- Consumes: requested notification + semantic dimension + raw AX registration result.
- Produces: typed registration record and derived `Complete`/`Incomplete` event assurance with unsupported dimensions explicit.

- [ ] Write the contract first and verify RED because the authority API is absent.
- [ ] Implement the minimal provider-owned reducer.
- [ ] Verify supported-only registrations may be complete while any unsupported/inconclusive registration remains incomplete.
- [ ] Verify unsupported notification does not imply direct-read unavailability.

### Task 2: Real macOS M05 oracle

**Files:**
- Create: `tools/validation-lab/macos-l7-real-provider-harness/fixtures/m05-notification-seed.swift`
- Create: `tools/validation-lab/macos-l7-real-provider-harness/tests/v43_real_provider_m05.rs`
- Modify: `tools/validation-lab/macos-l7-real-provider-harness/Cargo.toml`
- Create: `.github/workflows/v43-m05-real-provider.yml`

**Interfaces:**
- Consumes: real `AXObserverCreate` / `AXObserverAddNotification` results plus seed PID/target identity.
- Produces: exact-head JSON evidence proving the unsupported registration is preserved and event assurance is incomplete while direct AX observation remains usable.

- [ ] Add an exact registration gate before the real-provider test exists and verify coverage RED.
- [ ] Add the deterministic AppKit seed and real AX oracle.
- [ ] Discover/lock a real notification-target pair returning the OS unsupported result; do not fabricate it in production code.
- [ ] Publish and validate exact-head evidence.

### Task 3: Closure and regression

**Files:**
- Create: `.github/workflows/v43-m05-contract.yml`
- Modify only M05 files if diagnostics expose a defect.

- [ ] Run M01–M05 macOS gates on one immutable head.
- [ ] Run CI, Windows UIA, Windows real-provider and retained W13–W15 regressions.
- [ ] Audit diff/reviews/threads and remove temporary diagnostics.
- [ ] Merge only with expected-head SHA after every required gate is GREEN.
