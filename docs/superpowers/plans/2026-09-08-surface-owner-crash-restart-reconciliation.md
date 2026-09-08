# Surface Owner Crash/Restart Reconciliation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fence native preview/workspace surface authority by desktop owner instance and daemon boot epoch, then support exact crash/restart reattachment through fresh governor leases and explicit recovery debt.

**Architecture:** D2 adds a current-boot owner registry to `localview-control`, threads owner proof through all surface lifecycle routes, and records only surface recovery debt durably. The desktop owns a random process-lifetime `DesktopOwnerInstanceId`; the daemon owns a random process-lifetime `DaemonBootEpoch` and issues a fresh `OwnerLeaseId` during registration. Reattachment never restores old leases: it validates exact recovery debt and creates a new governor reservation/lease.

**Tech Stack:** Rust 2021, Axum, Tokio, Serde/serde_json, UUID, existing `RuntimeResourceGovernor`, Tauri desktop Rust, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-08-surface-owner-crash-restart-reconciliation-design.md`

## Global Constraints

- `SessionId` continuity is identity only; it must never authorize surface ownership.
- No provider, action, evidence, or Perception Budget authority is restored by D2.
- No production behavior is added without a failing test first.
- Unknown owner-protocol fields fail closed through `serde(deny_unknown_fields)`.
- A stale daemon boot epoch or stale owner lease cannot mutate current owner state.
- Owner timeout may revoke authority; it may never prove replacement identity.
- Reattach must create a fresh governor lease.
- Recovery debt persistence contains no owner lease IDs, daemon epochs, visibility authority, provider authority, action authority, or full Session state.

---

### Task 1: Current-Boot Owner Fence

**Files:**
- Create: `crates/control/tests/surface_owner_fence.rs`
- Create: `crates/control/src/surface_owner.rs`
- Modify: `crates/control/src/lib.rs`
- Modify: `crates/control/src/resource_runtime.rs`

**Interfaces:**
- Produces: `SurfaceOwnerRegistry`, `SurfaceOwnerProof`, `SurfaceOwnerRegistration`, `SurfaceOwnerError`.
- Produces route: `POST /v1/runtime/resources/surfaces/owners/register`.
- Later tasks consume exact proof fields: `owner_instance_id`, `boot_epoch`, `owner_lease_id`.

- [ ] **Step 1: Write the failing owner-registration/fence integration test**

Create `crates/control/tests/surface_owner_fence.rs` using the existing `hidden_surface_authority.rs` Axum `router(...).oneshot(...)` pattern. The test must assert:

```rust
let owner_a = Uuid::new_v4();
let registration = register_owner(state.clone(), owner_a).await;
assert_eq!(registration.owner_instance_id, owner_a);
assert!(!registration.boot_epoch.is_nil());
assert!(!registration.owner_lease_id.is_nil());

let stale_epoch = Uuid::new_v4();
assert_eq!(
    reserve_with_owner(state.clone(), session_id, "r1", owner_a, stale_epoch, registration.owner_lease_id).await.0,
    StatusCode::CONFLICT,
);

assert_eq!(
    reserve_with_owner(state.clone(), session_id, "r1", owner_a, registration.boot_epoch, registration.owner_lease_id).await.0,
    StatusCode::NO_CONTENT,
);
```

Also assert a random lease under the correct epoch is rejected with `surface_owner_lease_mismatch`.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-control --test surface_owner_fence -- --nocapture
```

Expected: FAIL because owner registration route does not exist and current reserve payload rejects owner-proof fields.

- [ ] **Step 3: Implement minimal current-boot owner registry**

Create `surface_owner.rs` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceOwnerProof {
    pub owner_instance_id: Uuid,
    pub boot_epoch: Uuid,
    pub owner_lease_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SurfaceOwnerRegistration {
    pub owner_instance_id: Uuid,
    pub boot_epoch: Uuid,
    pub owner_lease_id: Uuid,
    pub recovery_required: bool,
}
```

`SurfaceOwnerRegistry::new()` generates one `boot_epoch`. `register(owner_instance_id)` replaces any previous lease for that owner instance and returns a new lease. `validate(proof)` checks owner -> exact current epoch -> exact current lease.

Keep registry state associated with the exact `SessionManager` owner using the existing weak-owner registry pattern used by `resource_runtime.rs`; do not create global authority shared across unrelated control states.

- [ ] **Step 4: Thread proof through reserve/cancel only**

Extend `SurfaceReserveRequest` with the three proof UUIDs and validate before any session/governor mutation. Add owner registration route.

- [ ] **Step 5: Run GREEN**

Run the focused test, then:

```bash
cargo test -p localview-control --test hidden_surface_authority -- --nocapture
```

Update the legacy hidden-surface test helpers to register an owner and include the current proof in reserve payloads; preserve all pre-D2 lifecycle assertions.

- [ ] **Step 6: Commit**

```bash
git add crates/control/src/surface_owner.rs crates/control/src/lib.rs crates/control/src/resource_runtime.rs crates/control/tests/surface_owner_fence.rs crates/control/tests/hidden_surface_authority.rs
git commit -m "feat(control): fence surface reservations by owner boot"
```

---

### Task 2: Fence Activate / Visibility / Release

**Files:**
- Modify: `crates/control/tests/surface_owner_fence.rs`
- Modify: `crates/control/tests/hidden_surface_authority.rs`
- Modify: `crates/control/src/resource_runtime.rs`

**Interfaces:**
- Consumes `SurfaceOwnerProof` validation from Task 1.
- Changes pending/live keys so stale owner state cannot consume or mutate another owner.

- [ ] **Step 1: Write RED tests**

After owner A activates surface incarnation `1`, register owner B and assert:

```rust
assert_eq!(visibility(owner_b, same_surface).await.0, StatusCode::CONFLICT);
assert_eq!(release(owner_b, same_surface).await.0, StatusCode::CONFLICT);
assert_eq!(visibility(owner_a, same_surface).await.0, StatusCode::NO_CONTENT);
```

Re-register owner A to rotate its lease; assert requests using the previous lease fail and requests using the new lease succeed only after explicit current-owner state rules are satisfied.

- [ ] **Step 2: Run RED**

Expected failure: existing activate/visibility/release ignore owner proof.

- [ ] **Step 3: Implement minimal owner-keyed pending/live state**

Pending key:

```text
(owner_instance_id, session_id, request_id)
```

Live key:

```text
(owner_instance_id, session_id, LiveSurfaceIdentity)
```

Before all mutations, validate the proof. Logical collision checks still reject two current owners for the same `(session, surface_kind, label)`.

- [ ] **Step 4: Run GREEN + regression tests**

Run both control tests plus `cargo test -p localview-control`.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(control): fence live surfaces by owner instance"
```

---

### Task 3: Durable Surface Recovery Debt

**Files:**
- Create: `crates/control/src/surface_recovery.rs`
- Create: `crates/control/tests/surface_recovery_journal.rs`
- Modify: `crates/control/src/lib.rs`
- Modify: `crates/control/src/resource_runtime.rs`
- Modify: `apps/daemon/src/main.rs`

**Interfaces:**
- Produces `SurfaceRecoveryJournal::open(path)`.
- Produces `record_activated`, `record_released`, `record_reattached`, `outstanding_exact`.
- `ControlState` obtains or is associated with the journal through a focused registry keyed by its `SessionManager` so test states can use an in-memory/temp journal without expanding unrelated public state.

- [ ] **Step 1: Write RED journal replay tests**

Use a temp directory and assert:

```rust
journal.record_activated(key).await?;
drop(journal);
let reopened = SurfaceRecoveryJournal::open(path).await?;
assert!(reopened.outstanding_exact(&key));
reopened.record_released(key).await?;
drop(reopened);
assert!(!SurfaceRecoveryJournal::open(path).await?.outstanding_exact(&key));
```

Also corrupt one line and assert open fails closed rather than silently returning an empty inventory.

- [ ] **Step 2: Run RED**

Expected: module/API absent.

- [ ] **Step 3: Implement bounded JSONL journal**

Schema:

```rust
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum SurfaceRecoveryEvent {
    Activated { key: SurfaceRecoveryKey },
    Released { key: SurfaceRecoveryKey },
    Reattached { key: SurfaceRecoveryKey },
}
```

The exact key includes session, surface kind, label, incarnation, and owner instance ID. Append each event with newline and `sync_data()` before returning success. Replay maintains a `BTreeSet<SurfaceRecoveryKey>`; `Activated` inserts; `Released`/`Reattached` remove exact debt. Bound bytes/events and reject overflow.

- [ ] **Step 4: Integrate activation/release durability**

Activation ordering:

```text
governor activate -> append Activated -> publish live entry -> success
```

If append fails, drop the new lease and return `surface_recovery_journal_unavailable`.

Release ordering:

```text
remove live entry/lease -> append Released -> success
```

If append fails, return explicit recovery-journal failure while retaining fail-closed debt for boot replay.

- [ ] **Step 5: Run GREEN + full control tests**

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(control): persist native surface recovery debt"
```

---

### Task 4: Exact Reattach with Fresh Governor Lease

**Files:**
- Create: `crates/control/tests/surface_owner_restart.rs`
- Modify: `crates/control/src/resource_runtime.rs`
- Modify: `crates/control/src/surface_recovery.rs`

**Interfaces:**
- Produces route `POST /v1/runtime/resources/surfaces/reattach`.

- [ ] **Step 1: Write RED daemon-restart contract test**

Construct control state A, register owner A, reserve/activate a hidden surface, and persist debt. Drop state A. Construct state B with a new owner registry/boot epoch but the same D1-capable session identity and same recovery journal. Register the surviving desktop owner instance A under boot B.

Assert old boot-A proof is rejected. Assert exact reattach under boot B succeeds. Assert owner B cannot reattach the same debt.

- [ ] **Step 2: Run RED**

Expected: no reattach route/current-boot fresh lease path.

- [ ] **Step 3: Implement exact reattach**

Validate current owner proof, current session existence, and exact debt. Obtain a fresh governor `NativeSurface` reservation with a new internal request ID, activate it, append `Reattached`, then publish current-boot live owner.

Never reconstruct the old `LiveResourceLease` and never infer old visibility except from the explicit current reattach request.

- [ ] **Step 4: Run GREEN and assert resource baseline**

After release, governor hidden/native-surface accounting must equal the baseline observed before reattach.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(control): reattach exact recovered surfaces"
```

---

### Task 5: Desktop Owner Instance and Automatic Re-registration

**Files:**
- Modify: `apps/desktop/src-tauri/src/surface_registry.rs`
- Modify: `apps/desktop/src-tauri/src/surface_resource.rs`
- Modify: `apps/desktop/src-tauri/src/workspace_surface.rs`
- Modify: preview window lifecycle source that uses `DesktopSurfaceRegistry`
- Add focused desktop unit tests beside `surface_registry.rs` / existing desktop tests.

**Interfaces:**
- Produces `DesktopSurfaceOwner` with process-lifetime `owner_instance_id` and mutable current daemon registration.
- `DesktopSurfaceIdentity` gains `owner_instance_id`.

- [ ] **Step 1: Write RED registry ABA test**

Owner A creates incarnation `1`. A fresh registry/owner B also creates incarnation `1` for the same session/kind/label. Assert owner-B identity cannot close/mutate owner-A snapshot when both are presented to the same registry contract.

- [ ] **Step 2: Run RED**

- [ ] **Step 3: Add process-lifetime owner state and proof threading**

Create one `Uuid::new_v4()` owner instance at desktop startup. Register before first surface operation and cache `boot_epoch + owner_lease_id`. Include proof in all surface requests.

- [ ] **Step 4: Add re-registration/reattach behavior**

On explicit `surface_owner_boot_epoch_mismatch`, `surface_owner_not_registered`, or `surface_owner_lease_mismatch`, perform one registration refresh. If the platform surface already exists and local registry has exact owner truth, perform exact reattach before retrying visibility. Never loop indefinitely.

- [ ] **Step 5: Run desktop/Tauri tests**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(desktop): reconcile native surface owner after daemon restart"
```

---

### Task 6: Owner Heartbeat and Dead-Owner Cleanup

**Files:**
- Modify: `crates/control/src/surface_owner.rs`
- Modify: `crates/control/src/resource_runtime.rs`
- Modify: `crates/control/tests/surface_owner_fence.rs`
- Modify: desktop owner state/heartbeat source.

**Interfaces:**
- Produces heartbeat route bound to exact owner proof.
- Produces deterministic `revoke_expired(now)` test seam.

- [ ] **Step 1: Write RED cleanup test**

Activate one hidden/live surface under owner A, advance a test clock beyond the configured lease TTL, call explicit reaper seam, and assert:

- owner A proof is invalid;
- owner A pending/live entries are removed;
- governor accounting returns to baseline;
- owner B still cannot adopt A state.

- [ ] **Step 2: Run RED**

- [ ] **Step 3: Implement heartbeat/revocation**

Track `last_seen` server-side. Heartbeat renews only exact current proof. Reaper removes expired owner authority and current-boot leases. It never assigns those resources to another owner.

- [ ] **Step 4: Desktop heartbeat**

Send heartbeat every 5 seconds while the desktop process is alive. Registration refresh replaces heartbeat target atomically.

- [ ] **Step 5: Run GREEN**

- [ ] **Step 6: Commit**

```bash
git commit -am "feat: revoke dead desktop surface owners"
```

---

### Task 7: Permanent Restart Contract and Scope Gate

**Files:**
- Add/modify permanent D2 contract test in `crates/control/tests/`.
- Modify CI workflow only if the permanent test is not already covered by existing workspace test jobs.
- Update D2 design doc with final exact implementation details if names changed during TDD.

- [ ] **Step 1: Add exact restart matrix**

Cover:

```text
daemon restart + desktop survives -> exact owner reattach + fresh lease
desktop restart + daemon survives -> predecessor fenced + cleanup baseline
both restart -> no adoption of predecessor owner authority
same SessionId + same label + incarnation=1 + different owner -> fenced
stale boot epoch -> fenced
stale owner lease -> fenced
corrupt recovery journal -> no reattach authority
```

- [ ] **Step 2: Run targeted suites**

```bash
cargo test -p localview-control
cargo test -p localview-resource-governor
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

- [ ] **Step 3: Run whole workspace CI-equivalent commands**

Use the commands encoded by `.github/workflows/ci.yml` for Ubuntu/macOS/Windows-relevant Rust and frontend/Tauri checks.

- [ ] **Step 4: Scope audit**

Compare branch against base `a12401397b7326bd453e99d4e255007c8bcaec32`. Confirm changes are limited to D2 owner/recovery plumbing, tests, docs, and unavoidable daemon startup wiring. Explicitly verify no behavior changes to Perception Budget, provider/action authority, or unrelated evidence flows.

- [ ] **Step 5: Open PR and exact-head lock**

Push/open a PR against `main`, record exact head SHA, and do not merge until CI and Windows UIA are green on that exact head.
