# V4.3 macOS M01 Accessibility Permission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the first macOS L7 real-provider seed, M01, by proving that absent Accessibility trust is explicit runtime state and cannot mint AX semantic-control authority.

**Architecture:** Add a small `localview-macos-ax-provider` boundary that owns macOS Accessibility permission observation and typed authority admission. Keep the first slice deliberately permission-only: no AX tree crawling, observer, actions, capture, or fallback input. A dedicated macOS real-provider harness and exact-head workflow prove the actual hosted runner is untrusted and that the provider refuses semantic-control authority without weakening to screenshot or raw-input success.

**Tech Stack:** Rust 2024, direct macOS ApplicationServices FFI (`AXIsProcessTrusted`) behind `cfg(target_os = "macos")`, existing LocalView protocol/provider conventions, GitHub Actions `macos-latest`.

**Spec:** `LocalView_AI_Native_Localhost_Runtime_Product_Spec_v4_3_Principal_Provider_Reconciliation_Closure(20260913-072021).md` §§1250–1269, 1324, 1374, 1381.

## Global Constraints

- M01 is only `accessibility permission absent`; it must not claim M02–M12 or macOS provider completion.
- `prompt requested != permission granted`; a prompt request is never converted into trusted state by assumption.
- Revoked, absent, or unknown AX permission cannot mint semantic-control authority.
- Accessibility and capture permissions remain separate authority domains.
- No screenshot/raw-input fallback may turn an AX permission denial into semantic-control success.
- Real OS/provider evidence is required before M01 is called closed.
- The workflow must bind artifacts to the exact candidate SHA.
- Non-macOS workspace builds must continue to compile; no macOS framework symbol may leak into other targets.

---

### Task 1: Freeze the M01 permission contract in RED

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/macos-ax-provider/Cargo.toml`
- Create: `crates/macos-ax-provider/src/lib.rs`
- Create: `crates/macos-ax-provider/tests/m01_permission_contract.rs`

**Interfaces:**
- Produces: `AxPermissionState`, `AxPermissionRevision`, `AxPermissionError`, `AxSemanticControlPermit`, `AxPermissionProvider::current_permission_revision()`, `AxPermissionProvider::authorize_semantic_control(&AxPermissionRevision)`.
- Consumes: no Windows-specific provider type; M01 starts from the cross-platform product invariant only.

- [ ] **Step 1: Add a failing contract test**

The test constructs an untrusted permission revision and requires `authorize_semantic_control` to return `AxPermissionError::PermissionRequired` with no permit. It also constructs `prompt_requested=true, state=Untrusted` and proves the prompt bit does not change authority.

- [ ] **Step 2: Run the focused test and record intended RED**

Run: `cargo test -p localview-macos-ax-provider --test m01_permission_contract -- --nocapture`
Expected: FAIL because the permission/authority API is not implemented yet.

- [ ] **Step 3: Commit only the RED contract/scaffold**

Commit message: `test(v43): freeze macOS M01 AX permission authority`.

---

### Task 2: Implement the minimal typed permission boundary

**Files:**
- Modify: `crates/macos-ax-provider/src/lib.rs`
- Test: `crates/macos-ax-provider/tests/m01_permission_contract.rs`

**Interfaces:**
- `AxPermissionState::{Trusted, Untrusted, Unknown}`.
- `AxPermissionRevision { state, check_sequence, prompt_requested }`.
- `AxPermissionProvider::current_permission_revision(prompt_requested: bool) -> AxPermissionRevision` increments a process-local check sequence and observes current trust on macOS.
- `authorize_semantic_control` returns a permit only for `Trusted`; `Untrusted` returns `PermissionRequired`; `Unknown` returns `PermissionUnknown`.

- [ ] **Step 1: Add macOS FFI observation**

On macOS, call `AXIsProcessTrusted()` from ApplicationServices. On other targets, report `Unknown`; do not pretend that a non-macOS build has AX permission.

- [ ] **Step 2: Keep prompt semantics observational**

`prompt_requested` records caller intent only. This M01 slice does not display a system prompt and never changes `state` because a prompt was requested.

- [ ] **Step 3: Run focused and cross-target-safe tests**

Run: `cargo test -p localview-macos-ax-provider --test m01_permission_contract -- --nocapture`
Run: `cargo check -p localview-macos-ax-provider --all-targets`
Expected: PASS.

- [ ] **Step 4: Commit minimal GREEN**

Commit message: `feat(v43): add macOS AX permission authority boundary`.

---

### Task 3: Prove M01 against the real macOS permission regime

**Files:**
- Create: `tools/validation-lab/macos-l7-real-provider-harness/Cargo.toml`
- Create: `tools/validation-lab/macos-l7-real-provider-harness/tests/v43_real_provider_m01.rs`
- Create: `.github/workflows/v43-m01-real-provider.yml`

**Interfaces:**
- Consumes: `AxPermissionProvider` from Task 2.
- Produces: `M01-REAL-PROVIDER-RECORD.json` with `schema`, `case_id`, `candidate_sha`, `permission_state`, `prompt_requested`, `semantic_control_permit_minted`, and `typed_permission_denial`.

- [ ] **Step 1: Add a real-runner M01 oracle**

The ignored macOS test calls the production permission boundary with `prompt_requested=false`. It requires `Untrusted`, requires a typed `PermissionRequired`, requires no semantic-control permit, writes the exact-head record to `LOCALVIEW_L7_ARTIFACT_DIR`, and fails rather than fabricating M01 evidence if the runner is unexpectedly trusted.

- [ ] **Step 2: Add an exact-head workflow**

Use `macos-latest`, set `LOCALVIEW_MACOS_AX_SMOKE=1`, set `LOCALVIEW_CANDIDATE_SHA` from the PR head/push SHA, checkout that exact SHA, register the exact test name, run the ignored M01 oracle, verify every record field, then upload the artifact with `if-no-files-found: error`.

- [ ] **Step 3: Run workflow and preserve real RED/GREEN evidence**

The first workflow execution is authoritative only if it exercises the real ApplicationServices permission call on the exact branch head. A trusted runner is `UNMEASURED_FOR_M01`, not success.

- [ ] **Step 4: Commit the real-provider gate**

Commit message: `test(v43): add macOS M01 real-provider permission oracle`.

---

### Task 4: Exact-head regression and closure

**Files:**
- Modify only if evidence reveals a real defect: `crates/macos-ax-provider/*`, M01 harness/workflow.
- Update PR body with immutable evidence; do not rewrite unrelated roadmap claims.

**Interfaces:**
- Consumes: exact candidate SHA and workflow artifact.
- Produces: a merge-ready M01 PR with no macOS provider-complete claim.

- [ ] **Step 1: Require normal CI**

`cargo check`, clippy, workspace tests, native GUI smoke, and desktop gates must remain green on the exact candidate.

- [ ] **Step 2: Require M01 exact-head proof**

The artifact must say `case_id=M01`, `permission_state=untrusted`, `prompt_requested=false`, `semantic_control_permit_minted=false`, `typed_permission_denial=true`, and bind the exact candidate SHA.

- [ ] **Step 3: Re-run prior relevant macOS visual regression**

The existing WKWebView rendered-pixel smoke must remain green, demonstrating that adding AX permission logic does not conflate Accessibility authority with visual capture authority.

- [ ] **Step 4: Merge with expected-head SHA only**

Use a merge commit after all exact-head gates are green. Any code change after real-provider evidence invalidates that evidence and requires rerunning the proof.
