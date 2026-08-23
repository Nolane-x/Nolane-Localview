# Native Workspace Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the unsafe path toward an iframe-only workspace with an opt-in, capability-isolated native child WebView surface that is lifecycle-controlled by the bundled LocalView dashboard and compile-gated until the platform policy is proven safe.

**Architecture:** Keep the existing standalone native preview for compatibility, but add a second `workspace-*` child-WebView path hosted inside the `main` Tauri window. Capabilities target WebView labels rather than window labels so the bundled dashboard keeps `maincommands` while loopback preview/workspace surfaces receive only `previewbridge`. React renders through a `WorkspaceSurface` abstraction: native when the backend reports the `native-workspace` feature is compiled in, iframe fallback otherwise.

**Tech Stack:** Rust 1.85+, Tauri 2.11.5, Tauri ACL v2, React 19, TypeScript 5.9, GitHub Actions.

**Spec:** `docs/ROADMAP.md`, `docs/SPEC_COVERAGE.md`, and the approved LocalView expanded product specification (native preview, lifecycle, security/permission boundaries, and AI observation/control requirements).

## Global Constraints

- Preserve `#![forbid(unsafe_code)]`.
- Remote preview/workspace IPC remains loopback-only.
- Preview/workspace surfaces must never inherit `core:default` or `maincommands`.
- Bundled dashboard must never inherit `previewbridge`.
- Tauri multi-WebView APIs are compiled only behind Cargo feature `native-workspace` which enables `tauri/unstable`.
- Default build must continue to compile without Tauri unstable APIs.
- Native workspace lifecycle commands must validate session labels and top-level loopback navigation.
- Existing standalone preview and bridge behavior must remain compatible.

---

### Task 1: Capability isolation regression gate

**Files:**
- Create: `apps/desktop/src-tauri/tests/capability_isolation.rs`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/capabilities/preview-bridge.json`

**Interfaces:**
- Consumes: Tauri capability JSON files.
- Produces: invariant that `main` and `preview-*`/`workspace-*` WebViews cannot merge privileged command sets through window-level ACL matching.

- [ ] Write regression tests that require `webviews` selectors, reject `windows`, assert disjoint selectors, assert permission separation, and keep remote bridge URLs loopback-only.
- [ ] Run `cargo test -p localview-desktop --test capability_isolation` and confirm failure against the current `windows` ACL.
- [ ] Refactor both capabilities to `webviews` and add `workspace-*` to the bounded bridge capability.
- [ ] Re-run the test and confirm pass.

### Task 2: Feature-gated native workspace backend

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produces `WorkspaceBounds { x, y, width, height }` and `WorkspaceSurfaceSupport { compiled, default_mode, reason }` serialized to React.
- Produces Tauri commands `workspace_surface_open`, `workspace_surface_set_bounds`, `workspace_surface_navigate`, and `workspace_surface_close`.
- `workspace_surface_open` creates child WebView label `workspace-<session-key>` inside native window `main` when the feature is enabled.

- [ ] Add unit tests for workspace label derivation, allowed bridge callers, loopback navigation, and bounds validation.
- [ ] Verify tests fail before production helpers exist.
- [ ] Add Cargo feature `native-workspace = ["tauri/unstable"]` with an empty default feature set.
- [ ] Implement support reporting and validated geometry helpers outside unstable-only code.
- [ ] Implement child WebView create/show, bounds, navigate, and close behind `#[cfg(feature = "native-workspace")]`; return a deterministic unsupported error otherwise.
- [ ] Reuse instrumentation and preview bridge scripts for workspace child WebViews.
- [ ] Register the commands and update `maincommands` permission.
- [ ] Run normal and feature-enabled desktop checks.

### Task 3: React `WorkspaceSurface` abstraction

**Files:**
- Create: `apps/desktop/src/app/WorkspaceSurface.tsx`
- Modify: `apps/desktop/src/app/LocalViewShell.tsx`
- Modify: `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/styles.css`

**Interfaces:**
- `WorkspaceSurface` consumes `{ current?: Session, url?: string, support: WorkspaceSurfaceSupport }`.
- Native mode measures a DOM slot in logical CSS pixels and drives Tauri lifecycle/bounds/navigation commands.
- Fallback mode renders the existing sandboxed iframe path.

- [ ] Define support/bounds types and API methods.
- [ ] Extract existing empty/disconnected/iframe rendering into `WorkspaceSurface`.
- [ ] Add native slot measurement with `ResizeObserver`, lifecycle cleanup, URL navigation updates, and deterministic fallback when native support is absent.
- [ ] Wire `LocalViewShell` to backend support data without changing user-facing tool behavior.
- [ ] Run `npm run build`.

### Task 4: Native-workspace CI compile gate

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces an explicit GitHub Actions compiler gate for the unstable multi-WebView build.

- [ ] Add `cargo check -p localview-desktop --features native-workspace` after the normal Tauri backend check on Linux.
- [ ] Keep the default build as a separate check so accidental unconditional unstable API use is caught.
- [ ] Run/inspect PR CI and fix compile/test failures before integration.

### Task 5: Documentation and integration checkpoint

**Files:**
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/ROADMAP.md`

**Interfaces:**
- Documents exactly what is live, what remains gated, and the safety condition for making native workspace the unconditional default.

- [ ] Record capability-isolated multi-WebView support and fallback semantics.
- [ ] Mark the native workspace compile gate complete but retain the next policy work (overlay/chrome composition, platform-specific behavior, crash/reconnect lifecycle) as explicit follow-up rather than claiming the entire product spec is complete.
- [ ] Verify all changed files and CI status.
