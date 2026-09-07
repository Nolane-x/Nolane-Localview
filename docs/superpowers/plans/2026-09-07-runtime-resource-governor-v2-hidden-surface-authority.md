# Runtime Resource Governor V2 — Hidden Surface Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bind native preview/workspace surface lifecycle to the existing RuntimeResourceGovernor so hidden-surface pressure is derived from exact desktop owner truth, not caller-provided counters.

**Architecture:** The Tauri desktop process owns a `DesktopSurfaceRegistry` containing exact surface kind/label/incarnation/visibility. The daemon remains the only resource governor and exposes authenticated lifecycle commands that reserve, activate, update, and release exact native-surface leases. Desktop Tauri operations happen in the order reserve → platform create/show/hide/close → owner registry transition → central lease transition, with fail-closed cleanup and stale-incarnation protection.

**Tech Stack:** Rust, Tokio, Axum, Tauri 2, serde, existing LocalView SessionManager and RuntimeResourceGovernor, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-07-runtime-resource-governor-v2-hidden-surface-authority-design.md`

## Global Constraints

- Preserve one existing RuntimeResourceGovernor; do not create a desktop governor.
- Do not add `hidden_surfaces` or any owner-owned live counter to caller-writable `RuntimeResourceSample`.
- Do not change Perception Budget dimensions or token-budget policy.
- Do not implement analysis-concurrency accounting in this slice.
- Main LocalView shell window and ordinary iframe rendering are not native target surfaces.
- Session cleanup may remove pending native-surface reservations but must not forge disappearance of a live surface.
- Late events from an older surface incarnation must never mutate/release a newer incarnation.
- Every production behavior begins with a failing test observed in CI before implementation.

---

### Task 1: Governor Native-Surface State Machine

**Files:**
- Modify: `crates/resource-governor/src/lib.rs`
- Create: `crates/resource-governor/tests/hidden_surface_authority.rs`

**Interfaces:**
- Produces:
  - `ResourceWorkKind::NativeSurface`
  - `LiveResourceKind::NativeSurface`
  - `SurfaceVisibility::{Visible, Hidden}`
  - `LiveSurfaceIdentity { surface_kind: String, label: String, incarnation: u64 }`
  - `ResourceReservation::activate_surface(identity, visibility) -> Result<LiveResourceLease, ResourceActivationError>`
  - `LiveResourceLease::set_surface_visibility(identity, visibility) -> Result<(), ResourceActivationError>`
  - `ResourceBudget.hidden_surfaces: usize`
  - internal `ResourceSample.hidden_surfaces: usize`
  - `DegradationAction::SuspendInactiveRenderSurfaces`

- [ ] **Step 1: Write the failing governor tests**

Create tests proving:

```rust
#[test]
fn hidden_live_surface_consumes_hidden_surface_budget_until_matching_lease_drops() {
    let mut budget = ResourceBudget::default();
    budget.hidden_surfaces = 1;
    let governor = RuntimeResourceGovernor::new(budget);
    let reservation = governor.reserve("s1", "open-1", ResourceWorkKind::NativeSurface).unwrap();
    let identity = LiveSurfaceIdentity::new("preview_window", "preview-a", 1);
    let lease = reservation.activate_surface(identity.clone(), SurfaceVisibility::Hidden).unwrap();
    assert!(governor.reserve("s2", "open-2", ResourceWorkKind::NativeSurface).is_err());
    drop(lease);
    assert!(governor.reserve("s2", "open-3", ResourceWorkKind::NativeSurface).is_ok());
}
```

```rust
#[test]
fn visible_surface_does_not_count_as_hidden_and_visibility_transition_preserves_identity() {
    let mut budget = ResourceBudget::default();
    budget.hidden_surfaces = 1;
    let governor = RuntimeResourceGovernor::new(budget);
    let reservation = governor.reserve("s1", "open", ResourceWorkKind::NativeSurface).unwrap();
    let identity = LiveSurfaceIdentity::new("workspace_child", "workspace-a", 7);
    let lease = reservation.activate_surface(identity.clone(), SurfaceVisibility::Visible).unwrap();
    assert!(!governor.decision().actions.contains(&DegradationAction::SuspendInactiveRenderSurfaces));
    lease.set_surface_visibility(identity.clone(), SurfaceVisibility::Hidden).unwrap();
    assert!(governor.decision().actions.contains(&DegradationAction::SuspendInactiveRenderSurfaces));
}
```

```rust
#[test]
fn stale_surface_identity_cannot_update_or_release_newer_incarnation() {
    // activate incarnation 2, then attempt visibility/release with incarnation 1;
    // newer live entry must remain counted and owned.
}
```

```rust
#[test]
fn session_cleanup_keeps_live_native_surface_authority() {
    // pending NativeSurface is removed by release_session;
    // Live NativeSurface is retained until the lease owner releases it.
}
```

- [ ] **Step 2: Commit the RED tests**

Commit message:

```text
test: define hidden surface governor authority
```

- [ ] **Step 3: Observe RED in exact-head CI**

Expected failure: missing native-surface work/live kinds, visibility/identity types, or activation methods.

- [ ] **Step 4: Implement the minimal governor state machine**

Use one reservation map. Extend live state so a native-surface live entry stores exact identity and visibility. Chromium behavior must remain unchanged.

`decision_for_state` computes:

```rust
hidden_surfaces += matches!(live_surface.visibility, SurfaceVisibility::Hidden) as usize;
```

`evaluate` emits `SuspendInactiveRenderSurfaces` when `sample.hidden_surfaces >= budget.hidden_surfaces.max(1)`.

Do not add `hidden_surfaces` to `RuntimeResourceSample`.

- [ ] **Step 5: Run/verify governor tests GREEN**

Required command in CI/local-capable environment:

```bash
cargo test -p localview-resource-governor --test hidden_surface_authority
```

- [ ] **Step 6: Commit production governor implementation**

Commit message:

```text
feat: add native surface governor authority
```

---

### Task 2: Authenticated Surface Lifecycle Control Contract

**Files:**
- Modify: `crates/control/src/resource_runtime.rs`
- Modify: `crates/control/src/lib.rs` only if exports are required
- Create: `crates/control/tests/hidden_surface_authority.rs`
- Modify: `crates/control/tests/runtime_resource_governor.rs`

**Interfaces:**
- Consumes Task 1 governor types.
- Produces authenticated endpoints under `/v1/runtime/resources/surfaces/...` and a daemon-owned in-memory mapping from exact surface identity to `LiveResourceLease`.

Define request types with `#[serde(deny_unknown_fields)]`:

```rust
struct SurfaceReserveRequest {
    session_id: SessionId,
    request_id: String,
}

struct SurfaceActivateRequest {
    session_id: SessionId,
    request_id: String,
    surface_kind: String,
    label: String,
    incarnation: u64,
    visibility: SurfaceVisibility,
}

struct SurfaceVisibilityRequest {
    session_id: SessionId,
    surface_kind: String,
    label: String,
    incarnation: u64,
    visibility: SurfaceVisibility,
}

struct SurfaceReleaseRequest {
    session_id: SessionId,
    surface_kind: String,
    label: String,
    incarnation: u64,
}
```

The control layer owns pending reservations by `(session_id, request_id)` and live leases by exact surface identity. No endpoint accepts an aggregate count.

- [ ] **Step 1: Write RED control tests**

Tests must prove:

1. unauthorized reserve/activate/update/release is rejected;
2. duplicate `(session_id, request_id)` reserve is rejected;
3. activate without matching pending reservation is rejected;
4. activation stores one exact live lease;
5. stale visibility update returns conflict/not-found and preserves current lease;
6. stale release cannot remove newer incarnation;
7. `release_session` behavior used by daemon cleanup cannot erase live native-surface owner truth;
8. `RuntimeResourceSample` rejects forged `hidden_surfaces` with 422 due deny-unknown-fields.

- [ ] **Step 2: Commit RED control contract**

```text
test: define hidden surface control authority
```

- [ ] **Step 3: Observe RED CI failure**

Expected failure is missing surface routes/registry types, not unrelated existing tests.

- [ ] **Step 4: Implement control-plane lifecycle registry**

Keep registry associated with the same `SessionManager` keying model used by `runtime_resource_governor_for_sessions` so tests and daemon share authority consistently.

Reserve calls `governor.reserve(..., ResourceWorkKind::NativeSurface)`.

Activate removes the pending reservation only through successful `activate_surface(...)` and stores the returned live lease keyed by exact identity.

Visibility update calls `LiveResourceLease::set_surface_visibility` only for exact active identity.

Release removes/drops only the exact matching lease.

- [ ] **Step 5: Verify targeted control tests GREEN**

```bash
cargo test -p localview-control --test hidden_surface_authority
cargo test -p localview-control --test runtime_resource_governor
```

- [ ] **Step 6: Commit production control authority**

```text
feat: expose exact native surface resource lifecycle
```

---

### Task 3: Desktop Surface Owner Registry

**Files:**
- Create: `apps/desktop/src-tauri/src/surface_registry.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` to declare/manage registry
- Create: `apps/desktop/src-tauri/tests/surface_registry_contract.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopSurfaceKind { PreviewWindow, WorkspaceChild }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSurfaceVisibility { Visible, Hidden }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSurfaceIdentity {
    pub session_id: SessionId,
    pub kind: DesktopSurfaceKind,
    pub label: String,
    pub incarnation: u64,
}

pub struct DesktopSurfaceRegistry { /* mutex-owned maps */ }
```

Required methods:

```rust
pub fn next_identity(&self, session_id: SessionId, kind: DesktopSurfaceKind, label: String) -> DesktopSurfaceIdentity;
pub fn record_created(&self, identity: DesktopSurfaceIdentity, visibility: DesktopSurfaceVisibility) -> Result<(), SurfaceRegistryError>;
pub fn set_visibility(&self, identity: &DesktopSurfaceIdentity, visibility: DesktopSurfaceVisibility) -> Result<(), SurfaceRegistryError>;
pub fn record_closed(&self, identity: &DesktopSurfaceIdentity) -> Result<(), SurfaceRegistryError>;
pub fn current(&self, session_id: SessionId, kind: DesktopSurfaceKind) -> Option<DesktopSurfaceRecord>;
```

`next_identity` reserves only an incarnation number; it must not claim the Tauri object exists. `record_created` is called only after successful platform creation.

- [ ] **Step 1: Write RED registry tests**

Prove:

- first incarnation is 1 and recreation increments monotonically;
- duplicate `record_created` for same incarnation is rejected;
- visibility changes require exact current identity;
- stale close for incarnation N cannot remove N+1;
- repeated create/close leaves zero live records;
- main shell has no registry API/path and therefore cannot accidentally count.

- [ ] **Step 2: Commit RED registry contract**

```text
test: define desktop surface owner registry
```

- [ ] **Step 3: Observe RED in Tauri/frontend CI job**

Expected failure: missing `surface_registry` module/types.

- [ ] **Step 4: Implement minimal registry**

Use a mutex-protected map keyed by `(SessionId, DesktopSurfaceKind)` plus monotonic next-incarnation map. Poisoned mutexes recover with `into_inner()` consistent with governor style.

- [ ] **Step 5: Verify registry contract GREEN**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test surface_registry_contract
```

- [ ] **Step 6: Commit production registry**

```text
feat: track exact desktop surface owner truth
```

---

### Task 4: Desktop Resource Client and Transaction Ordering

**Files:**
- Create: `apps/desktop/src-tauri/src/surface_resource.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/surface_resource_contract.rs`

**Interfaces:**
- Consumes Task 2 control endpoints and Task 3 identities.
- Produces helpers:

```rust
pub async fn reserve_surface(identity_seed: &DesktopSurfaceIdentitySeed) -> Result<SurfaceReservationToken, String>;
pub async fn activate_surface(token: &SurfaceReservationToken, identity: &DesktopSurfaceIdentity, visibility: DesktopSurfaceVisibility) -> Result<(), String>;
pub async fn update_surface_visibility(identity: &DesktopSurfaceIdentity, visibility: DesktopSurfaceVisibility) -> Result<(), String>;
pub async fn release_surface(identity: &DesktopSurfaceIdentity) -> Result<(), String>;
```

`SurfaceReservationToken` contains `session_id` and generated request id only. It cannot activate another session.

- [ ] **Step 1: Write RED source/behavior tests**

Use unit/source contracts to prove:

- reserve occurs before platform create hook;
- activation is a distinct call after successful creation;
- activation failure path exposes a cleanup hook;
- no helper posts aggregate hidden counts;
- authenticated token handling reuses existing `read_token()`/`control_client()` rather than adding a second credential mechanism.

- [ ] **Step 2: Commit RED desktop resource-client tests**

```text
test: define desktop surface resource transaction
```

- [ ] **Step 3: Observe RED Tauri job**

- [ ] **Step 4: Implement resource client**

Use the loopback control plane at `127.0.0.1:45454`, bearer auth, and exact request payloads. HTTP 429/resource denial must surface as an error before Tauri creation.

- [ ] **Step 5: Verify targeted desktop tests GREEN**

- [ ] **Step 6: Commit resource client**

```text
feat: connect desktop surface lifecycle to resource authority
```

---

### Task 5: Native Workspace Child Lifecycle Integration

**Files:**
- Modify: `apps/desktop/src-tauri/src/workspace_surface.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` if command signatures/state injection require it
- Create: `apps/desktop/src-tauri/tests/workspace_surface_authority_contract.rs`

**Interfaces:**
- Consumes registry/resource client from Tasks 3-4.

- [ ] **Step 1: Write RED workspace authority tests**

Contract must prove source/transaction order for the compiled native-workspace path:

```text
new surface: reserve -> add_child -> record_created -> activate
existing surface: platform show -> registry visible -> daemon visibility visible
close: platform close -> registry record_closed -> daemon release
activation failure: platform close -> no live registry record
```

Also prove a failed `add_child` cannot call `record_created` or `activate_surface`.

- [ ] **Step 2: Commit RED workspace tests**

```text
test: define native workspace surface authority
```

- [ ] **Step 3: Observe RED native-workspace CI**

- [ ] **Step 4: Integrate `open_native` and `close_native`**

Keep URL/bounds validation unchanged. Do not weaken current loopback navigation policy.

Existing child show path must use current registry identity; if Tauri reports a child but registry state is missing, fail closed or reconcile explicitly rather than invent incarnation 0.

- [ ] **Step 5: Verify stable + `native-workspace` backend checks GREEN**

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --features native-workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test workspace_surface_authority_contract
```

- [ ] **Step 6: Commit workspace integration**

```text
feat: bind workspace child lifecycle to surface authority
```

---

### Task 6: Preview WebviewWindow Lifecycle Integration

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/preview_surface_authority_contract.rs`

**Interfaces:**
- Consumes registry/resource client from Tasks 3-4.

- [ ] **Step 1: Write RED preview authority tests**

Prove:

- existing window successful `show()` marks current exact incarnation Visible;
- new preview reserves before `WebviewWindowBuilder::build()`;
- build success is followed by owner `record_created` then central activation;
- activation failure closes the just-built preview and removes registry owner truth;
- close/hide event reconciliation never releases before actual platform transition succeeds;
- main window close-to-tray behavior remains unchanged and main shell is not registered as target surface.

- [ ] **Step 2: Commit RED preview tests**

```text
test: define preview surface authority
```

- [ ] **Step 3: Observe RED Tauri CI**

- [ ] **Step 4: Integrate preview lifecycle**

Use `DesktopSurfaceKind::PreviewWindow` and `preview_surface_label(session)` as the label. Keep preview navigation and bridge capability isolation unchanged.

- [ ] **Step 5: Verify preview + capability regression tests GREEN**

Run targeted preview authority contract plus existing desktop capability/live bridge contracts.

- [ ] **Step 6: Commit preview integration**

```text
feat: bind preview window lifecycle to surface authority
```

---

### Task 7: Cleanup-to-Baseline and Session Removal Regression

**Files:**
- Modify: `apps/daemon/src/main.rs` only if a concrete desktop cleanup protocol exists in this branch; otherwise leave daemon cleanup semantics unchanged and document the remaining cross-process crash/reconciliation boundary.
- Modify: `crates/control/tests/hidden_surface_authority.rs`
- Create or modify: `apps/desktop/src-tauri/tests/surface_cleanup_contract.rs`

**Interfaces:**
- Consumes all previous tasks.

- [ ] **Step 1: Add RED cleanup regressions**

Prove:

- pending surface reservation is cleared by session cleanup;
- live surface lease survives generic `release_session` until exact owner release;
- repeated exact reserve→activate→hide/show→release cycles return governor reservation count/decision to baseline;
- stale release after recreation cannot erase new live surface;
- desktop registry repeated create/close returns to zero live records.

- [ ] **Step 2: Commit RED cleanup tests**

```text
test: close hidden surface cleanup oracle
```

- [ ] **Step 3: Implement only concrete missing cleanup behavior**

Do not add a speculative desktop heartbeat or daemon-to-desktop RPC if no real transport exists. If normal desktop lifecycle already supplies exact release, keep crash-heartbeat work explicitly out of scope.

- [ ] **Step 4: Verify cleanup regressions GREEN**

- [ ] **Step 5: Commit cleanup closure**

```text
fix: preserve exact native surface cleanup authority
```

---

### Task 8: Named CI Gate and Implementation Status

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`

**Interfaces:**
- Produces named gate: `Hidden surface authority control contract`.

- [ ] **Step 1: Add CI step**

Add after runtime resource governor/Chromium authority gates:

```yaml
- name: Hidden surface authority control contract
  run: cargo test -p localview-control --test hidden_surface_authority
```

Add desktop named contracts to Tauri/frontend section for registry/resource/workspace/preview authority as appropriate.

- [ ] **Step 2: Update implementation status truth**

State that Runtime Resource Governor V2 owner-truth closure now covers retained visual storage, Chromium process authority, and native preview/workspace surface authority. State explicitly that analysis concurrency remains open until a concrete concurrent heavy-analysis owner exists.

Do not claim desktop crash heartbeat/reconciliation closure beyond what tests prove.

- [ ] **Step 3: Commit CI/docs**

```text
ci: gate hidden surface owner authority
```

---

### Task 9: Exact-Head Full Verification and PR Closure

**Files:**
- No production files unless verification finds a real defect.

- [ ] **Step 1: Audit branch diff against base `ae2580b62bdbf31300697b01bb75e9c111cc15a1`**

Reject unrelated changes, Perception Budget edits, analysis-concurrency counters, or caller-writable aggregate surface counts.

- [ ] **Step 2: Run exact-head CI**

Require:

- Rust core Ubuntu/macOS/Windows green;
- Tauri + frontend green;
- stable Tauri backend green;
- native-workspace backend green;
- new hidden-surface named gates green;
- WebView2/WKWebView/WebKitGTK rendered-pixel smokes green;
- Windows UIA workflow green.

- [ ] **Step 3: Update PR body with RED→GREEN evidence and exact SHA**

Record each RED commit/run and the exact final-head green run.

- [ ] **Step 4: Mark ready only after exact-head checks are green**

- [ ] **Step 5: Merge with expected-head lock**

Use normal merge commit, matching Phase A/B repository practice.

- [ ] **Step 6: Verify `main` equals returned merge commit and start post-merge CI**

Do not claim post-merge closure until CI + Windows UIA on the merge SHA are observed green.
