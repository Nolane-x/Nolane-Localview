# Runtime Resource Governor V2 — Hidden Surface Authority Design

## Status

Approved design for Phase C after Chromium process authority closure.

Base: `main` at `ae2580b62bdbf31300697b01bb75e9c111cc15a1`.

## Goal

Bind hidden native/WebView surface accounting to the real desktop surface lifecycle instead of caller-supplied counters, while preserving one existing RuntimeResourceGovernor as the only resource-admission authority.

## Product contract

The LocalView V4.3 spec requires the runtime governor to account for hidden surfaces, suspend inactive render surfaces under pressure, keep hidden/suspended visual work at zero or near-zero cadence, avoid a second desktop governor, and converge to cleanup-to-baseline without orphan hidden high-rate surfaces or reservations.

This slice therefore closes owner truth for native preview/workspace surfaces. It does not implement analysis-concurrency accounting, new Perception Budget dimensions, high-rate mirror scheduling, or platform capture-stream lifecycle.

## Existing seams

### Desktop owner

`apps/desktop/src-tauri/src/lib.rs` owns isolated preview `WebviewWindow` creation/show behavior.

`apps/desktop/src-tauri/src/workspace_surface.rs` owns native child WebView create/show/navigate/close behavior.

These Tauri objects are the concrete resource owners. A session flag such as `preview_visible` is policy/UI state, not proof that a Tauri surface exists or is visible.

### Central governor

`crates/resource-governor` owns admission state and live resource leases. `crates/control/src/resource_runtime.rs` exposes the governor associated with the daemon SessionManager. Desktop and daemon are separate processes, so desktop cannot borrow the daemon governor object directly.

## Architecture

Phase C uses two cooperating authorities without creating a second governor.

### 1. DesktopSurfaceRegistry — owner truth only

Add a desktop-local lifecycle registry whose only job is to describe surfaces the desktop process actually owns.

Each entry is keyed by exact surface identity:

- `session_id`
- `surface_kind`: `preview_window` or `workspace_child`
- `label`
- monotonic `incarnation`

Each entry tracks lifecycle state:

- `Visible`
- `Hidden`

Closed surfaces are removed. Registry state changes happen only after the corresponding Tauri operation succeeds.

The registry does not contain budgets, degradation policy, or permission logic. It is not a resource governor.

### 2. Central surface reservation/lease authority

Extend the existing RuntimeResourceGovernor with a native-surface work/live resource class.

Admission model:

1. desktop requests a surface reservation from the daemon before creating a new governed Tauri surface;
2. daemon creates a pending reservation bound to session + request id;
3. after successful Tauri create, desktop activates the reservation using exact surface identity/incarnation;
4. activation becomes a live native-surface lease in the existing governor;
5. live surface state updates report Visible/Hidden for the same incarnation;
6. successful close releases that exact live lease;
7. stale close/update from an old incarnation cannot remove or mutate a newer surface incarnation.

Pending admission remains distinct from live ownership so create races cannot bypass the surface cap.

## Surface counting semantics

The governor distinguishes existence from visibility.

- Visible live surface: resource exists and is user-visible.
- Hidden live surface: resource still exists but is not user-visible; this is the count relevant to hidden-surface pressure.
- Closed: no live lease.

A hidden surface must not be treated as released merely because it is hidden.

The main LocalView application window is shell infrastructure and is not counted as a target render surface. Ordinary DOM iframe rendering inside the main shell is also not represented as a native hidden-surface lease in this slice.

## Budget shape

Add a `hidden_surfaces` budget dimension to `ResourceBudget` and internal `ResourceSample`, but never to caller-writable `RuntimeResourceSample`.

Default policy is conservative and bounded. The exact default must be encoded explicitly in production code and tests; callers cannot raise it through the runtime sample endpoint.

Under hidden-surface pressure, the governor may deny creation of additional native render surfaces and emit an explicit degradation action for suspending/closing inactive render surfaces. This action is availability guidance; it does not silently alter verification proof requirements.

## Desktop/daemon protocol

Add authenticated control-plane routes for desktop owner transitions. The payload must be exact and deny unknown fields.

Conceptual operations:

- reserve surface creation
- activate exact surface incarnation
- update exact live surface visibility
- release exact live surface incarnation

The API does not accept an aggregate hidden-surface count. It accepts lifecycle transitions for exact owner identities.

The daemon remains authoritative for admission; the desktop remains authoritative for Tauri lifecycle truth.

## Incarnation safety

A surface label may be reused over time. Therefore label equality is insufficient for release/update authority.

The desktop registry issues a monotonically increasing incarnation per logical surface key. Activation binds the incarnation into the governor live lease identity. Every visibility update and release must match the active incarnation.

Late cleanup for incarnation N must be a no-op if incarnation N+1 is active.

## Preview window lifecycle

`open_preview` behavior becomes:

1. if an existing exact preview surface is present, show it;
2. after successful show, report Visible for the current incarnation;
3. if no surface exists, reserve admission before build;
4. build the WebviewWindow;
5. only after successful build, register owner truth and activate the reservation;
6. on activation failure, close the just-created window and fail closed.

Preview close/hide events must reconcile registry + central lease. User close semantics may hide or close depending product behavior, but the registry must mirror the actual Tauri outcome.

## Workspace child lifecycle

`workspace_surface_open` behavior becomes:

1. existing child: reposition/navigate/show; then report Visible;
2. missing child: reserve before `add_child`;
3. successful `add_child`: register and activate exact incarnation;
4. activation failure: close the just-created child and fail closed;
5. `workspace_surface_close`: close Tauri child first, then release exact registry/lease state.

React cleanup remains best-effort caller orchestration; Rust owner truth is decisive.

## Failure semantics

### Reservation denial

No Tauri surface is created. Caller receives resource-governor denial.

### Tauri create failure

Pending reservation is released by RAII/drop or explicit cancellation. No live surface entry is created.

### Activation failure after create

The newly created Tauri surface is closed immediately. Registry does not retain it as live. The request fails closed.

### Visibility update failure

If the underlying Tauri show/hide operation fails, owner state is not advanced. If reporting to daemon fails after the platform transition succeeded, the registry retains actual local owner state and marks central reconciliation debt rather than fabricating rollback.

### Close failure

Do not release central owner truth merely because a close was requested. Release only after Tauri close succeeds or a later reconciliation proves the object no longer exists.

### Desktop crash

The daemon cannot assume a live desktop-owned surface survived. A future desktop-owner heartbeat/reconciliation slice may harden crash detection further. This Phase C must at minimum make normal lifecycle and stale-incarnation transitions correct and make cleanup-to-baseline observable in tests.

## Session cleanup semantics

Daemon `release_session` may clear pending surface work for that session, but must not forge the disappearance of a desktop-owned live surface. This matches the Chromium process rule established in Phase B.

Normal session removal must explicitly ask the desktop owner to close/reconcile surfaces where the architecture exposes such a path. Until concrete owner exit is observed, live surface truth remains live.

## Tests / failure oracles

Required RED-before-GREEN contracts:

1. duplicate reserve/open cannot double-admit one logical surface key;
2. failed create never activates a live surface;
3. activation requires the correct pending native-surface reservation;
4. hidden live surface increments hidden-surface pressure;
5. visible live surface does not count as hidden;
6. visibility transition preserves incarnation and lease identity;
7. stale visibility update for an old incarnation is rejected/no-op;
8. close releases exactly one matching live incarnation;
9. stale close cannot erase a newer incarnation;
10. `release_session` clears pending work but not live native-surface owner truth;
11. callers cannot forge `hidden_surfaces` through `/v1/runtime/resources/sample`;
12. repeated open/close cycles converge to baseline reservation count;
13. desktop source contracts prove reserve-before-create and activate-after-success ordering;
14. a named CI gate runs the surface-authority contract.

## Scope boundaries

In scope:

- desktop owner-truth registry;
- native preview window lifecycle;
- native workspace child lifecycle;
- central native-surface reservation/live lease state;
- hidden-surface budget/degradation decision;
- exact control-plane lifecycle transitions;
- tests, CI gate, implementation-status updates.

Out of scope:

- analysis concurrency until a concrete heavy concurrent owner exists;
- Perception Budget changes;
- Chromium process-tree accounting;
- generic cross-process heartbeat framework;
- persistent surface pools;
- automatic high-rate mirror FPS controller;
- OCR/vision worker authority;
- iframe counting as native surface authority.

## Acceptance criteria

Phase C is complete only when:

- no aggregate hidden-surface count is caller-writable;
- real native surface lifecycle transitions drive governor truth;
- pending admission occurs before creation;
- successful creation transitions to exact live owner truth;
- visibility changes update exact live incarnation;
- close/release cannot be forged by stale session cleanup or stale incarnation events;
- resource decisions account for hidden live surfaces;
- full workspace/desktop CI is green on exact PR head;
- post-merge verification is checked before claiming the slice closed.
