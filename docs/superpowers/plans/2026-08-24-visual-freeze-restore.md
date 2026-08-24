# Visual Freeze / Restore Capture Transaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate native viewport capture inside a token-owned, self-healing visual motion freeze and require successful restore before artifact persistence.

**Architecture:** Extend the existing managed-WebView bridge with internal freeze/restore actions, expose them only through narrow authenticated capture endpoints, and wrap the current desktop settle/native-capture flow in a per-session serialized freeze/capture/restore transaction. Page-side instrumentation owns the lease and auto-restores after 8 seconds if explicit restore is lost.

**Tech Stack:** Rust 1.98, Tokio, Axum, Serde/UUID, Tauri 2.11.5, existing LocalView instrumentation JavaScript, GitHub Actions matrix.

**Spec:** `docs/superpowers/specs/2026-08-24-visual-freeze-restore-design.md`

## Global Constraints

- Desktop remains `#![forbid(unsafe_code)]`.
- Native platform adapters are unchanged.
- No arbitrary browser evaluation endpoint.
- Generic `/actions` must reject visual freeze/restore actions.
- Freeze lease is 8,000 ms and self-restores.
- No timer/Date/performance monkey patching.
- No response body, storage, cookie, form value, DOM text or pixel bytes in freeze action results.
- Different sessions may capture concurrently; only same-session captures are serialized.
- Artifact/evidence persistence happens only after explicit restore acknowledgement succeeds.

---

### Task 1: Internal bridge action contract

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Create: `crates/live-bridge/tests/visual_freeze_contract.rs`

**Interfaces:**
- Produces: `BridgeActionKind::FreezeVisuals`
- Produces: `BridgeActionKind::RestoreVisuals { token: Uuid }`
- Produces: `BridgeActionKind::is_internal_capture_action(&self) -> bool`

- [ ] **Step 1: Write failing serialization/internal-action tests**

Require snake-case JSON:

```json
{"type":"freeze_visuals"}
{"type":"restore_visuals","token":"00000000-0000-0000-0000-000000000001"}
```

Require `is_internal_capture_action()` true only for the two new variants.

- [ ] **Step 2: Run RED**

Run `cargo test -p localview-live-bridge --test visual_freeze_contract` and verify unresolved variants/method.

- [ ] **Step 3: Implement minimal enum variants + helper**

Add the two variants and the pure helper; extend result sanitization match arms without retaining arbitrary payload for internal control actions.

- [ ] **Step 4: Run GREEN**

Run `cargo test -p localview-live-bridge` and `cargo clippy -p localview-live-bridge --all-targets -- -D warnings`.

- [ ] **Step 5: Commit**

Commit `feat: add internal visual freeze bridge actions`.

---

### Task 2: Page-side freeze lease and exact restore

**Files:**
- Modify: `crates/instrumentation/src/lib.rs`
- Modify: `crates/instrumentation/tests/live_semantic_contract.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `crates/instrumentation/tests/visual_freeze_contract.rs`

**Interfaces:**
- Produces JS API: `window.__LOCALVIEW__.freezeVisuals(token)` returning bounded metadata/promise.
- Produces JS API: `window.__LOCALVIEW__.restoreVisuals(token)` returning bounded metadata.
- Consumes bridge actions from Task 1 in `PREVIEW_BRIDGE_SCRIPT`.

- [ ] **Step 1: Write failing source-contract tests**

Require bootstrap source to contain:

```text
freezeVisuals
restoreVisuals
document.getAnimations
8000
animation-play-state
transition-duration
caret-color
```

Require source not to contain monkey patches for `window.setTimeout =`, `window.setInterval =`, `Date.now =`, `performance.now =`, canvas screenshot APIs or page-content extraction.

Require preview executor cases `freeze_visuals` and `restore_visuals`.

- [ ] **Step 2: Run RED**

Run instrumentation tests and desktop live semantic bridge contract; verify missing API/action cases.

- [ ] **Step 3: Implement lease state**

Inside the bootstrap closure add one active lease object containing token, owned style node, auto-restore timer and a bounded list of animation references with their pre-freeze running/pending state.

`freezeVisuals(token)`:
- reject empty token;
- reject a different active token;
- pause running/pending animations from `document.getAnimations()` when supported;
- inject one LocalView-owned style that pauses CSS animations, sets transition duration/delay to zero, hides caret and forces auto scroll behavior;
- mark root with the token;
- arm 8,000 ms token-owned auto-restore;
- await one `requestAnimationFrame` before returning counts/capabilities.

`restoreVisuals(token)`:
- require exact active token;
- clear timer;
- remove only owned style/root marker;
- resume only animations recorded as running/pending before freeze;
- clear retained references/state;
- tolerate detached animation errors.

- [ ] **Step 4: Wire preview action executor**

`freeze_visuals` calls `await window.__LOCALVIEW__.freezeVisuals(queued.id)`.

`restore_visuals` parses `action.token` and calls `window.__LOCALVIEW__.restoreVisuals(action.token)`.

- [ ] **Step 5: Run GREEN**

Run `cargo test -p localview-instrumentation` plus desktop semantic bridge contract and Clippy for instrumentation.

- [ ] **Step 6: Commit**

Commit `feat: add self-healing visual freeze lease`.

---

### Task 3: Narrow authenticated capture freeze/restore endpoints

**Files:**
- Modify: `crates/control/src/capture_settle.rs`
- Modify: `crates/control/src/runtime.rs`
- Modify: `crates/control/src/lib.rs` only if router composition requires it
- Create: `crates/control/tests/capture_visual_state.rs`

**Interfaces:**
- Produces: `POST /v1/sessions/{id}/capture-freeze`
- Produces: `POST /v1/sessions/{id}/capture-restore`
- Freeze response: `{ token: Uuid, paused_animations: u32, web_animations_supported: bool, lease_ms: u64 }`
- Restore request: `{ token: Uuid }`

- [ ] **Step 1: Write RED router tests**

Prove unauthorized and missing-session behavior, exact internal action queueing, bounded freeze response parsing, matching restore token and exact result id.

Also POST both internal action kinds through generic `/actions` and require rejection.

- [ ] **Step 2: Run RED**

Run `cargo test -p localview-control --test capture_visual_state`.

- [ ] **Step 3: Generalize exact action-result wait helper**

Reuse the existing bounded action-result polling in `capture_settle.rs` for Snapshot, FreezeVisuals and RestoreVisuals without trusting page timestamps.

- [ ] **Step 4: Implement freeze endpoint**

Enqueue `FreezeVisuals`; use action id as token; wait bounded acknowledgement; validate only count/bool metadata; return fixed `lease_ms: 8000` from daemon-owned constant.

- [ ] **Step 5: Implement restore endpoint**

Accept UUID token only; enqueue `RestoreVisuals { token }`; require exact successful acknowledgement; return `204`.

- [ ] **Step 6: Reject internal actions from generic queue**

Return `BAD_REQUEST` with stable error code `internal_capture_action_not_public` before enqueue.

- [ ] **Step 7: Sanitize evidence/result storage**

Internal visual actions store only action id/type/ok/error/completed_at; never retain page-returned payload beyond the narrow freeze endpoint response extraction.

- [ ] **Step 8: Run GREEN**

Run full `localview-control` tests and Clippy with `-D warnings`.

- [ ] **Step 9: Commit**

Commit `feat: add internal capture freeze control endpoints`.

---

### Task 4: Desktop per-session freeze/capture/restore transaction

**Files:**
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/visual_freeze_contract.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Adds bounded per-session capture gate in `VisualCaptureState`.
- Adds `freeze_visual_state(session_id) -> Result<FreezeReceipt, String>`.
- Adds `restore_visual_state(session_id, token) -> Result<(), String>`.

- [ ] **Step 1: Write RED desktop contract**

Require textual/control-flow ordering:

```text
session gate
wait_for_capture_settle
freeze_visual_state
capture_managed_surface
restore_visual_state
persist_and_register
```

Require no `eval(`/`evaluate_script` visual-freeze path and no single app-global capture mutex.

- [ ] **Step 2: Add CI gate and run RED**

Add `cargo test -p localview-desktop --test visual_freeze_contract` to the Tauri job and verify the new contract alone fails while existing gates stay green.

- [ ] **Step 3: Implement bounded per-session gate registry**

Use a mutex-protected map of `SessionId -> Weak<tokio::sync::Mutex<()>>`; upgrade/create an `Arc` for the current transaction and retain only live weak entries, pruning dead entries on acquisition. This avoids serializing unrelated sessions and avoids unbounded historical locks.

- [ ] **Step 4: Implement control client helpers**

Deserialize only the bounded freeze receipt fields. Restore sends only the UUID token.

- [ ] **Step 5: Implement finally-style acquisition helper**

Create a helper that freezes, runs `capture_managed_surface`, then always attempts restore before returning.

Outcome rules:
- capture ok + restore ok -> return frame;
- capture err + restore ok -> return capture error;
- capture ok + restore err -> drop frame and return restore error;
- both err -> return bounded combined error.

- [ ] **Step 6: Keep persistence after restore**

`persist_and_register` must only run after the helper returns a frame, proving explicit restore acknowledgement succeeded first.

- [ ] **Step 7: Run GREEN**

Run stable + native-workspace backend check, existing desktop contracts and new visual-freeze contract.

- [ ] **Step 8: Commit**

Commit `feat: freeze and restore managed visuals around native capture`.

---

### Task 5: Coverage, review, fresh full verification and merge

**Files:**
- Modify: `docs/ROADMAP.md`
- Modify: `docs/SPEC_COVERAGE.md`
- Modify: design/plan only if review finds a mismatch

**Interfaces:** none.

- [ ] **Step 1: Update claims conservatively**

Mark animation/transition freeze+restore as live for native viewport capture, while preserving `Partial` for stable capture overall because masking, true in-flight network accounting, GUI smoke, progressive regions and visual diff remain.

Explicitly state JS-driven canvas/video/WebGL loops are not frozen by this slice.

- [ ] **Step 2: Review full PR diff**

Check token ownership, auto-restore behavior, generic-action rejection, payload retention, per-session lock lifecycle, route revalidation, restore-on-error ordering and absence of unsafe/arbitrary eval.

- [ ] **Step 3: Run fresh full CI on final head**

Require all four jobs green:
- Rust core Ubuntu;
- Rust core macOS;
- Rust core Windows;
- Tauri + frontend including new desktop visual-freeze contract.

- [ ] **Step 4: Mark PR ready and merge with expected head SHA**

Do not merge if the head moves or any final job is not green.

- [ ] **Step 5: Verify merged `main` commit**

Fetch merged PR/main SHA before making completion claims.
