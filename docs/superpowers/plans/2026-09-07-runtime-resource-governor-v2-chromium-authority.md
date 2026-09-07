# Chromium Process Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Runtime Resource Governor Chromium instance accounting transition atomically from pre-spawn admission to a live process lease held by the real Chromium executor lifecycle.

**Architecture:** Preserve the existing pending reservation as a race-free admission token. Add a governor-owned live-resource lease and atomically convert a Chromium reservation into that lease only after `Command::spawn()` succeeds. Keep `localview-chromium` independent of the governor by adding lifecycle-aware executor variants that accept a generic spawn callback and retain the callback guard for the child lifetime.

**Tech Stack:** Rust, Tokio process/runtime, Axum control plane, Cargo tests, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-07-runtime-resource-governor-v2-chromium-authority-design.md`

## Global Constraints

- Preserve `#![forbid(unsafe_code)]` in touched Rust crates.
- Do not add caller-writable `chromium_instances`, hidden-surface, or analysis-concurrency fields to `RuntimeResourceSample`.
- Do not change Perception Budget dimensions.
- Chromium remains spawn-on-demand, ephemeral-profile, `kill_on_drop(true)`, and loopback-only.
- Pending admission plus live lease must never double-count or leave a zero-count race window.
- Session cleanup may clear pending work but must not forge the death of an actually live Chromium child.
- Existing native visual capture reservation semantics must remain unchanged.

---

### Task 1: Governor pending-to-live lifecycle primitive

**Files:**
- Modify: `crates/resource-governor/src/lib.rs`

**Interfaces:**
- Consumes: existing `RuntimeResourceGovernor`, `ResourceReservation`, `ResourceWorkKind::Chromium`.
- Produces: `LiveResourceKind::ChromiumProcess`, `ResourceActivationError`, `ResourceReservation::activate_live`, `LiveResourceLease`.

- [ ] **Step 1: Write failing unit tests**

Add tests that express the external behavior, not internal map shape:

```rust
#[test]
fn chromium_activation_keeps_one_slot_consumed_until_live_lease_drops() {
    let governor = RuntimeResourceGovernor::default();
    let pending = governor
        .reserve("session-a", "request-a", ResourceWorkKind::Chromium)
        .expect("first Chromium admission");

    let live = pending
        .activate_live(LiveResourceKind::ChromiumProcess)
        .expect("spawn transition");

    assert!(governor
        .reserve("session-b", "request-b", ResourceWorkKind::Chromium)
        .is_err());
    drop(live);
    assert!(governor
        .reserve("session-b", "request-c", ResourceWorkKind::Chromium)
        .is_ok());
}

#[test]
fn releasing_session_does_not_forge_live_chromium_exit() {
    let governor = RuntimeResourceGovernor::default();
    let pending = governor
        .reserve("session-a", "request-a", ResourceWorkKind::Chromium)
        .expect("first Chromium admission");
    let live = pending
        .activate_live(LiveResourceKind::ChromiumProcess)
        .expect("spawn transition");

    assert_eq!(governor.release_session("session-a"), 0);
    assert!(governor
        .reserve("session-b", "request-b", ResourceWorkKind::Chromium)
        .is_err());
    drop(live);
    assert!(governor
        .reserve("session-b", "request-c", ResourceWorkKind::Chromium)
        .is_ok());
}
```

Also add a mismatch test proving a non-Chromium reservation cannot activate `ChromiumProcess`.

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test -p localview-resource-governor chromium_activation -- --nocapture
cargo test -p localview-resource-governor releasing_session_does_not_forge_live_chromium_exit -- --nocapture
```

Expected: compile/test failure because live-resource activation API does not exist yet.

- [ ] **Step 3: Implement the minimal governor primitive**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveResourceKind {
    ChromiumProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceActivationError {
    ReservationMissing,
    KindMismatch,
}
```

Extend `RuntimeGovernorState` with a live-resource map keyed by the same reservation identity. Implement `ResourceReservation::activate_live(mut self, kind)` so the lock-protected transition removes exactly one pending Chromium reservation and inserts exactly one live Chromium process entry before releasing the lock. On failure, do not fabricate a live entry.

Add `LiveResourceLease` whose `Drop` removes only its own live key. Do not make `release_session` remove live entries.

Update `decision_for_state` to count pending Chromium reservations plus live Chromium process leases.

- [ ] **Step 4: Run resource-governor tests and verify GREEN**

```bash
cargo test -p localview-resource-governor
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/resource-governor/src/lib.rs
git commit -m "feat: add live chromium resource lease"
```

---

### Task 2: Bind lifecycle guard to the real Chromium child

**Files:**
- Modify: `crates/chromium/src/lib.rs`
- Test: `crates/chromium/tests/executor_contract.rs`

**Interfaces:**
- Consumes: existing `execute_ephemeral`, `execute_rendered_screenshot`, `ChromiumExecutorError`.
- Produces: `execute_ephemeral_with_lifecycle`, `execute_rendered_screenshot_with_lifecycle`; legacy functions delegate with no-op lifecycle.

- [ ] **Step 1: Write failing executor tests**

Use the existing fake-executable contract harness to assert:

```rust
// spawn failure must not call the lifecycle callback
let calls = Arc::new(AtomicUsize::new(0));
let observed = calls.clone();
let result = execute_ephemeral_with_lifecycle(
    missing_executable,
    &target,
    &policy,
    move || {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    },
).await;
assert!(matches!(result, Err(ChromiumExecutorError::Spawn)));
assert_eq!(calls.load(Ordering::SeqCst), 0);
```

Add a successful/terminal-path test with a small drop-counting guard proving the callback is called exactly once and the guard is dropped before the function returns.

- [ ] **Step 2: Run the focused tests and verify RED**

```bash
cargo test -p localview-chromium --test executor_contract lifecycle -- --nocapture
```

Expected: compile failure because lifecycle-aware functions do not exist.

- [ ] **Step 3: Implement lifecycle-aware execution**

Add `ChromiumExecutorError::Lifecycle`.

Add generic lifecycle-aware variants with an `FnOnce() -> Result<G, ChromiumExecutorError>` spawn callback. Invoke the callback immediately after successful `Command::spawn()`. If activation fails, kill/wait the just-spawned child, clean the ephemeral profile, and return `Lifecycle`/the callback error rather than allowing an ungoverned child to continue.

Retain the returned guard while the child is alive. Drop it after the child has definitely exited or been killed/waited. On future cancellation, lexical drop must destroy the guard together with the `kill_on_drop` child.

Make existing executor functions delegate using `|| Ok(())` so their API remains source-compatible.

- [ ] **Step 4: Run Chromium tests and verify GREEN**

```bash
cargo test -p localview-chromium
```

Expected: PASS including executable discovery, executor contract, and rendered-pixel contract.

- [ ] **Step 5: Commit**

```bash
git add crates/chromium/src/lib.rs crates/chromium/tests/executor_contract.rs
git commit -m "feat: bind chromium lifecycle guards to child processes"
```

---

### Task 3: Convert control admission at the successful spawn boundary

**Files:**
- Modify: `crates/control/src/chromium_runtime.rs`
- Modify/Test: `crates/control/tests/runtime_resource_governor.rs`

**Interfaces:**
- Consumes: Task 1 `ResourceReservation::activate_live`, Task 2 lifecycle-aware executor.
- Produces: control compatibility probe that holds pending admission before spawn and a live process lease only after spawn.

- [ ] **Step 1: Write a failing control/resource contract test**

Preserve the existing caller-forgery test and add a focused governor integration assertion proving live Chromium authority is not cleared by session pending-work cleanup. If the control test harness can inject a fake Chromium executable with a blocking lifetime, run two probes and prove the second is denied until the first child exits; otherwise keep the process-lifecycle proof in Tasks 1–2 and add a compile-time integration path here.

- [ ] **Step 2: Run the focused control tests and verify RED**

```bash
cargo test -p localview-control --test runtime_resource_governor -- --nocapture
```

Expected: failure until control uses the lifecycle-aware execution path/new activation API.

- [ ] **Step 3: Implement control transition**

Replace the current `_reservation` binding in `execute_compatibility_probe` with an owned pending Chromium reservation. Pass it into the lifecycle callback:

```rust
let pending = resource_governor(state)
    .reserve(..., ResourceWorkKind::Chromium)
    .map_err(ChromiumRuntimeError::ResourceGovernor)?;

let execution = execute_ephemeral_with_lifecycle(
    &config.executable,
    &target,
    &policy,
    move || {
        pending
            .activate_live(LiveResourceKind::ChromiumProcess)
            .map_err(|_| ChromiumExecutorError::Lifecycle)
    },
)
.await
.map_err(ChromiumRuntimeError::Executor)?;
```

Do not expose any HTTP mutation for the live resource state.

- [ ] **Step 4: Run the control and cross-crate tests**

```bash
cargo test -p localview-control --test runtime_resource_governor
cargo test -p localview-control --test live_visual_capture_resource_governor
cargo test -p localview-resource-governor
cargo test -p localview-chromium
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/control/src/chromium_runtime.rs crates/control/tests/runtime_resource_governor.rs
git commit -m "feat: activate chromium authority at spawn"
```

---

### Task 4: Contracts, status, and exact-head verification

**Files:**
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify if needed: `.github/workflows/ci.yml`
- Modify if needed: `docs/ROADMAP.md`

**Interfaces:**
- Consumes: completed Tasks 1–3.
- Produces: explicit closure evidence for Chromium process authority and an honest statement that hidden-surface/analysis-concurrency authority remain open.

- [ ] **Step 1: Add/confirm named CI gate**

If existing Rust core tests already execute all affected packages, add only a narrow named gate when it materially improves regression visibility. Do not duplicate the full matrix.

- [ ] **Step 2: Update implementation status**

Record:

```text
Runtime Resource Governor V2
- retained artifact/cache authority: closed
- Chromium process lifecycle authority: closed by pending→live RAII transition
- hidden native surface authority: open
- analysis concurrency authority: open until a concrete concurrent analysis owner exists
```

Do not claim the entire governor is complete.

- [ ] **Step 3: Run formatting/lint/test verification**

```bash
cargo fmt --all -- --check
cargo test -p localview-resource-governor
cargo test -p localview-chromium
cargo test -p localview-control --test runtime_resource_governor
cargo test -p localview-control --test live_visual_capture_resource_governor
```

Then run the repository CI workflow on the exact branch head and require all platform jobs/named rendered-pixel gates to pass.

- [ ] **Step 4: Audit PR scope**

Verify no Perception Budget fields changed, no caller-writable resource counters were added, and no hidden-surface/analysis-concurrency fake authority was introduced.

- [ ] **Step 5: Merge only with exact-head lock**

Mark the PR ready only after exact-head checks are green. Merge with the verified expected head SHA. Confirm `main` points to the resulting merge commit and require post-merge CI on that exact merge SHA before declaring this slice closed.
