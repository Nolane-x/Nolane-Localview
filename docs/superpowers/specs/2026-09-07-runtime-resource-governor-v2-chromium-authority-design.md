# Runtime Resource Governor V2 — Chromium Process Authority Design

## Status

Bounded Phase B slice following the merged retained-resource authority slice.

## Goal

Make the Chromium instance budget reflect the lifecycle of real LocalView-owned Chromium child processes rather than treating a caller-side reservation as the final source of truth, while preserving pre-spawn admission and the existing ephemeral `kill_on_drop` execution model.

## Product/spec anchors

LocalView must stay lightweight, spawn Chromium only when a higher engine tier is actually required, kill it after use, and avoid paying for observation channels that no current decision needs. Resource pressure may degrade background work, but it must not silently invalidate active verification.

This slice does **not** change Perception Budget dimensions and does **not** add any caller-writable runtime-resource counter.

## Existing seam

Today `RuntimeResourceGovernor::reserve(..., ResourceWorkKind::Chromium)` inserts a reservation before `localview_chromium::execute_ephemeral` spawns the child. `decision_for_state` counts that reservation as a Chromium instance. The reservation is useful for race-free admission, but it is not process-owner truth: it begins before spawn and ends when the caller future unwinds, rather than being explicitly activated by the executor at successful process creation.

The Chromium crate already owns the actual child lifecycle. It creates an ephemeral profile, spawns a `tokio::process::Child` with `kill_on_drop(true)`, waits with a timeout, kills on I/O/timeout failure, waits for termination, and cleans the profile.

## Design

### 1. Keep a pending admission phase

`RuntimeResourceGovernor::reserve` remains the pre-spawn admission boundary. A pending Chromium reservation contributes one projected Chromium instance so two concurrent callers cannot both pass a one-instance budget before either child has spawned.

### 2. Add an atomic pending → live transition

A Chromium reservation can be activated exactly once after successful child spawn.

The transition is atomic under the governor mutex:

```text
pending Chromium reservation
        ↓ successful OS child spawn
remove pending reservation
add live Chromium lease
        ↓ child exits / is killed / task is cancelled
remove live Chromium lease
```

At no point is one process counted twice, and there is no gap in which a second spawn can bypass the budget.

### 3. Make the executor hold the live lease

The Chromium crate must stay independent of the resource-governor crate. Therefore the executor receives a generic one-shot lifecycle callback/guard factory rather than importing governor types.

Immediately after `Command::spawn()` succeeds, the executor invokes that callback and retains the returned guard in the same lexical scope as the child. The guard therefore drops on every terminal path: normal exit, non-zero exit, I/O failure, timeout, cleanup failure, or async task cancellation. Because the child is already `kill_on_drop(true)`, cancellation drops both the process owner and its live resource lease together.

The existing public `execute_ephemeral` and `execute_rendered_screenshot` APIs remain available by delegating to lifecycle-aware internal/public variants with a no-op guard, so downstream callers are not broken.

### 4. Governor state distinguishes projected and observed-live work

The governor stores:

- pending work reservations, as today;
- a separate map of live resource leases, initially used only for `ChromiumProcess`.

`decision_for_state` computes Chromium instances as:

```text
pending Chromium reservations + live ChromiumProcess leases
```

The pending → live transition removes one side before adding the other under the same lock.

### 5. No caller-writable live count

`RuntimeResourceSample` remains unchanged and `#[serde(deny_unknown_fields)]` continues to reject forged fields such as `chromium_instances`. No new HTTP endpoint can set live Chromium count.

### 6. Failure semantics

- Spawn fails: activation callback is never called; pending reservation drops and no live process is recorded.
- Activation fails because the reservation was already consumed/released: the just-spawned child must not continue ungoverned. The executor immediately kills/waits the child and returns a lifecycle error.
- Timeout/I/O error/cancellation: live lease drops with the child lifecycle.
- Session cleanup may release pending reservations, but must not erase a genuinely live Chromium lease; live truth ends only with the process-owning guard.

## API shape

Resource governor:

```rust
pub enum LiveResourceKind {
    ChromiumProcess,
}

impl ResourceReservation {
    pub fn activate_live(self, kind: LiveResourceKind)
        -> Result<LiveResourceLease, ResourceActivationError>;
}

pub struct LiveResourceLease { /* RAII */ }
```

Chromium executor:

```rust
pub async fn execute_ephemeral_with_lifecycle<G, F>(
    executable: &Path,
    target: &Url,
    policy: &ChromiumExecutionPolicy,
    on_spawned: F,
) -> Result<ChromiumExecutionReceipt, ChromiumExecutorError>
where
    F: FnOnce() -> Result<G, ChromiumExecutorError>,
    G: Send;
```

The concrete signature may use a small generic lifecycle error adapter if needed to avoid coupling; the architectural invariant is that the callback is invoked **after spawn succeeds and before the process is allowed to proceed ungoverned**.

## Tests / proof obligations

1. A pending Chromium reservation consumes the one-instance budget.
2. Activating it to a live lease does not change the effective count from one to two.
3. A second Chromium reservation remains denied while the live lease exists.
4. Dropping the live lease reopens admission.
5. Releasing a session cannot falsely clear a live process lease.
6. Executor lifecycle hook is not invoked when spawn fails.
7. Executor hook guard lives until normal process exit and drops on terminal paths.
8. Control compatibility probe converts its pending reservation at the real spawn boundary.
9. `RuntimeResourceSample` still rejects caller-supplied `chromium_instances`.
10. Existing native visual capture reservation behavior is unchanged.

## Explicitly out of scope

- hidden native/WebView surface authority;
- analysis concurrency authority;
- Perception Budget changes;
- browser-process tree accounting beyond the LocalView-owned top-level child lifecycle;
- persistent Chromium pools;
- responsive sweep/contact-sheet work;
- native-workspace defaulting.

Those require their own concrete owners and should be implemented as separate Runtime Resource Governor slices rather than fake counters.