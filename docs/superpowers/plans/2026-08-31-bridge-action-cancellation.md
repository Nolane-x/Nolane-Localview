# BridgeAction Cancellation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add exact-session cooperative cancellation for public BridgeAction work without weakening internal visual-capture restore authority.

**Architecture:** Extend `localview-live-bridge`'s cancellable wrapper with a page-action lifecycle registry separate from native-executor cancellation. Public action cancellation is serialized against enqueue/take/result-claim; internal capture actions remain delegated directly to the base bridge. The control plane exposes authenticated cancellation routes and the desktop preview executor performs exact pre/post execution cancellation checks at cooperative safe boundaries.

**Tech Stack:** Rust, Tokio, Axum, Tauri 2, injected JavaScript preview bridge, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-08-31-bridge-action-cancellation-design.md`

## Global Constraints

- `(session_id, action_id)` is the only public-action cancellation ownership key.
- `FreezeVisuals` and `RestoreVisuals` are not cancellable through the public action API.
- Cancellation is cooperative; do not force-kill WebView/JavaScript/platform calls.
- Cancelled actions must not create Interaction, Semantic or Layout evidence.
- Cancellation state is bounded and session cleanup removes all authority state.
- Public cancellation must not introduce raw page payload, typed input, cookies, storage or authorization data into cancellation telemetry.

---

### Task 1: LiveBridge public-action cancellation authority

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Modify: `crates/live-bridge/src/cancellable_lib.rs`
- Create: `crates/live-bridge/tests/action_cancellation.rs`

**Interfaces:**
- Produces: `ActionCancellationState`, `ActionCancellationSignal`, `ActionCancellationOutcome`.
- Produces: `LiveBridge::request_action_cancellation`, `action_cancellation`, `action_cancellations`, `acknowledge_action_cancellation`.
- Produces: base-only `discard_public_action(session_id, action_id) -> bool` lifecycle cleanup primitive.

- [ ] **Step 1: Write RED lifecycle tests**

Cover queued cancellation filtering, inflight fencing, duplicate cancellation, cross-session isolation, queue eviction, bounded tombstones, exact lookup after >32 signals, ACK capacity release and internal capture isolation.

- [ ] **Step 2: Run the focused test and confirm RED**

Run: `cargo test -p localview-live-bridge --test action_cancellation`

Expected: compile/test failure because page-action cancellation types and methods do not exist.

- [ ] **Step 3: Add the minimal authority implementation**

Use a separate `ActionCancellationAuthority` under the existing wrapper mutex boundary. Intercept only public `enqueue_action`, `take_actions` / `take_public_actions`, and public result `claim_action`. Treat successful public result claim as the cancellation linearization point: if claim wins, remove the cancellation entry so a later cancel is too late; if cancel wins, claim returns `None`.

Add `base::LiveBridge::discard_public_action` to remove a public action from queued/inflight/claimed state without creating a result.

- [ ] **Step 4: Run focused and crate tests GREEN**

Run:

```bash
cargo test -p localview-live-bridge --test action_cancellation
cargo test -p localview-live-bridge
```

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: add bridge action cancellation authority`

---

### Task 2: Authenticated control cancellation API and evidence fence

**Files:**
- Create: `crates/control/src/action_cancellation.rs`
- Modify: `crates/control/src/lib.rs`
- Create: `crates/control/tests/action_cancellation.rs`

**Interfaces:**
- Consumes: Task 1 action cancellation types/methods.
- Produces routes:
  - `POST /v1/sessions/{id}/actions/cancel`
  - `GET /v1/sessions/{id}/actions/cancellations`
  - `GET /v1/sessions/{id}/actions/cancellations/{action_id}`
  - `POST /v1/sessions/{id}/actions/cancellations/{action_id}/ack`

- [ ] **Step 1: Write RED control tests**

Drive the real router. Prove bearer auth/session checks, pending cancel response, inflight `202`, exact lookup, ACK, cross-session `404`, internal capture action `404`, caller cannot cancel an arbitrary unknown UUID, and a cancelled action result returns `409 action_result_without_inflight_origin` before any evidence is inserted.

- [ ] **Step 2: Run control contract and confirm RED**

Run: `cargo test -p localview-control --test action_cancellation`

Expected: route/type failure before production implementation.

- [ ] **Step 3: Implement the narrow router**

Mirror native-cancellation authentication/session behavior while using page-action error names (`action_not_found`, `action_cancellation_not_pending`). Merge the router from `crates/control/src/lib.rs`.

- [ ] **Step 4: Run focused and control tests GREEN**

Run:

```bash
cargo test -p localview-control --test action_cancellation
cargo test -p localview-control
```

Expected: PASS and cancelled results create no new Interaction/Semantic/Layout evidence.

- [ ] **Step 5: Commit**

Commit message: `feat: expose bridge action cancellation control`

---

### Task 3: Desktop cooperative cancellation checkpoints

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/tests/action_cancellation_worker_contract.rs`

**Interfaces:**
- Consumes exact cancellation lookup/ACK routes from Task 2.
- Produces Tauri commands `preview_action_cancellation` and `preview_ack_action_cancellation`.
- Preview script checks cancellation immediately before public execution and immediately before public result publication.

- [ ] **Step 1: Write RED desktop source contract**

Assert that the injected preview worker uses exact per-action cancellation lookup, never applies that lookup to `freeze_visuals`/`restore_visuals`, acknowledges cancellation, and checks both before `execute(action)` and before `complete(...)`.

- [ ] **Step 2: Run the desktop contract and confirm RED**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test action_cancellation_worker_contract`

Expected: FAIL because the Tauri commands/checkpoints are absent.

- [ ] **Step 3: Implement cooperative checkpoints**

Add exact GET/ACK Tauri commands. In JavaScript, only public actions call the cancellation check. If cancellation is observed before execute, ACK and skip execution. If cancellation is observed after execute or error but before result publication, ACK and suppress the late result. Internal freeze/restore execution remains unchanged.

- [ ] **Step 4: Run desktop contracts GREEN**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test action_cancellation_worker_contract
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --tests
```

Expected: PASS, including existing visual freeze/restore transaction contracts.

- [ ] **Step 5: Commit**

Commit message: `feat: cooperatively cancel preview actions`

---

### Task 4: CI gates, roadmap and exact-head proof

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `IMPLEMENTATION_STATUS.md`
- Modify: roadmap document containing the BridgeAction cancellation gap if present.

**Interfaces:**
- Produces named CI gates `Bridge action cancellation authority contract`, `Bridge action cancellation control contract`, and desktop `Bridge action cancellation worker contract`.

- [ ] **Step 1: Add explicit CI commands**

Run live-bridge/control cancellation contracts on Ubuntu/macOS/Windows core jobs and the desktop worker contract in Tauri/frontend.

- [ ] **Step 2: Update implementation status/roadmap**

Record the exact authority boundary, cooperative safe points, result/evidence fence, internal capture exclusion and remaining cancellation limitations.

- [ ] **Step 3: Run full CI on exact head**

Require Ubuntu/macOS/Windows Rust core, Tauri/frontend, WebKitGTK/WKWebView/WebView2 rendered-pixel smoke, Clippy and full workspace tests to pass on the same SHA.

- [ ] **Step 4: Keep PR Draft until exact-head proof is green**

Do not merge independently while stacked prerequisite PRs remain Draft.

## Self-review

- Spec coverage: all authority, API, executor, evidence and capture-isolation requirements are mapped to Tasks 1-4.
- Placeholder scan: no TBD/TODO implementation gaps.
- Type consistency: action cancellation uses `action_id`; native cancellation keeps `request_id`; registries are intentionally separate.
- Race linearization: result claim versus cancellation is explicitly serialized in Task 1 so control evidence creation never races after a cancellation that won authority.
