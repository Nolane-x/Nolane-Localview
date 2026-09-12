# V4.3 Windows W11/W12 Race Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close Windows L7 seeds W11 modal-before-dispatch and W12 target-restart-after-authorization without expanding the public input authority surface.

**Architecture:** Reuse the existing journal-minted `WindowsUiaVerifiedInputRequest`, exact retained UIA lineage, and worker-owned final dispatch-context fence. Extend only the test seed/oracle, Validation Lab typed semantics, real-provider harness/campaign, and the narrow stale-target error mapping if W12 RED proves the current worker reports target death as a generic provider failure.

**Tech Stack:** Rust, Tokio, Windows UI Automation/Win32, WPF/.NET 8, serde/serde_json, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-12-v43-windows-w11-w12-race-authority-design.md`

## Global Constraints

- Semantic UIA actions remain preferred; this slice does not add a new public raw-input entry point.
- W11 must observe a real owned modal at the immediate worker fence and produce zero input effect.
- W12 authority minted for process A must never dispatch to restarted process B.
- No automatic modal dismissal, target rebinding, retry, pointer input, Unicode text input, clipboard path, password input, or privileged helper.
- The campaign case set is exactly W01-W09 + W11 + W12; W10 is explicitly unmeasured.
- Production crates must not depend on the seed/oracle harness.
- Every implementation task follows RED -> minimal GREEN -> focused verification -> commit.

---

### Task 1: Add pure Validation Lab W11/W12 semantics

**Files:**
- Modify: `crates/validation-lab/src/real_provider_adapter.rs`
- Create: `crates/validation-lab/tests/real_provider_w11_w12.rs`

**Interfaces:**
- Produces `RealProviderCaseKind::W11ModalBeforeDispatch`.
- Produces `RealProviderCaseKind::W12TargetRestartAfterAuthorization`.
- Reuses `adapt_real_provider_case` and RPOMR behavior unchanged.

- [ ] **Step 1: Write failing semantic tests**

Add tests that construct:

```rust
RealProviderCaseKind::W11ModalBeforeDispatch {
    modal_blocker_observed: true,
    input_inserted: false,
    target_effect_observed: false,
}
```

and require `RealProviderIntegrationPass`, plus negative cases where any of the three safety facts are wrong and must produce `CounterexampleFound`.

Add W12 pass case:

```rust
RealProviderCaseKind::W12TargetRestartAfterAuthorization {
    original_target_gone: true,
    replacement_target_present: true,
    stale_authority_rejected: true,
    replacement_effect_observed: false,
    fresh_reacquire_required: true,
}
```

and negative cases for stale authority accepted, replacement effect observed, or fresh reacquire not required.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-validation-lab --test real_provider_w11_w12 -- --nocapture
```

Expected: compile failure because W11/W12 variants do not yet exist.

- [ ] **Step 3: Add minimal variants and reducer arms**

Add:

```rust
W11ModalBeforeDispatch {
    modal_blocker_observed: bool,
    input_inserted: bool,
    target_effect_observed: bool,
},
W12TargetRestartAfterAuthorization {
    original_target_gone: bool,
    replacement_target_present: bool,
    stale_authority_rejected: bool,
    replacement_effect_observed: bool,
    fresh_reacquire_required: bool,
},
```

Reducer semantics:

```rust
W11 => !modal_blocker_observed || input_inserted || target_effect_observed
W12 => !original_target_gone
    || !replacement_target_present
    || !stale_authority_rejected
    || replacement_effect_observed
    || !fresh_reacquire_required
```

Do not add a new metric merely for these seeds.

- [ ] **Step 4: Run GREEN and full adapter tests**

```bash
cargo test -p localview-validation-lab --test real_provider_w11_w12 -- --nocapture
cargo test -p localview-validation-lab --test real_provider_adapter -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git commit -am "test(v43): define W11 W12 provider semantics"
```

### Task 2: Extend the WPF edge seed with a real owned-modal fixture

**Files:**
- Modify: `tools/provider-seeds/windows-uia-edge-seed/EdgeWindow.cs`
- Modify: `tools/provider-seeds/windows-uia-edge-seed/OracleProtocol.cs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_seed.rs`

**Interfaces:**
- Produces oracle commands `open_modal_blocker`, `close_modal_blocker`, `get_verified_input_state` modal fields.
- Produces harness methods `open_modal_blocker`, `close_modal_blocker`, `kill_and_wait`.

- [ ] **Step 1: Add source-level failing contract in harness support tests or compile target**

The support code must call `open_modal_blocker()` and read `modal_is_open`, `modal_window_handle`, and `modal_owner_window_handle`; current seed protocol cannot compile that path.

- [ ] **Step 2: Run RED on Windows seed/harness registration**

```powershell
dotnet build tools/provider-seeds/windows-uia-edge-seed/LocalView.WindowsUiaEdgeSeed.csproj -c Release --nologo
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w11 -- --list
```

Expected before implementation: missing W11 test/support methods.

- [ ] **Step 3: Implement deterministic owned modal**

`EdgeWindow` adds `Window? _modalBlocker` and methods:

```csharp
public void OpenModalBlocker()
public void CloseModalBlocker()
public long ModalBlockerWindowHandle()
public bool IsModalBlockerOpen()
```

The modal must set `Owner = this`, be visible, non-shipping, and be closed by `CleanupVerifiedInputFixture()`.

Extend `ReadVerifiedInputStateOnUiThread` with:

```text
modal_window_handle
modal_is_open
modal_owner_window_handle
```

Oracle commands return the same typed state after opening/closing.

- [ ] **Step 4: Extend process harness lifecycle**

Add `EdgeSeedProcess::open_modal_blocker`, `close_modal_blocker`, and `kill_and_wait`. `kill_and_wait` must mark the wrapper shutdown/terminated so `Drop` cannot double-kill and must not fake a graceful oracle shutdown.

- [ ] **Step 5: Build seed and list harness tests**

```powershell
dotnet build tools/provider-seeds/windows-uia-edge-seed/LocalView.WindowsUiaEdgeSeed.csproj -c Release --nologo
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --no-run
```

- [ ] **Step 6: Commit**

```bash
git commit -am "test(v43): add W11 modal seed fixture"
```

### Task 3: Prove W11 against the real worker boundary

**Files:**
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w11.rs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_cases.rs`

**Interfaces:**
- Uses `mint_verified_input_authority` with `require_no_modal_blocker = true`.
- Expects `WindowsUiaWorkerError::DispatchContextBlocked(WindowsUiaDispatchContextBlocker::ModalBlockerPresent { .. })`.
- Produces campaign helper `run_w11(...) -> RealProviderLabRecord`.

- [ ] **Step 1: Write the real W11 test**

Sequence:

```text
spawn seed -> prepare target -> attach/snapshot -> mint authority
-> open owned modal -> dispatch stale-clean-preflight authority
-> expect ModalBlockerPresent -> oracle effect_count == 0
-> close modal -> abandon one-shot authority -> clean shutdown
```

Assert the blocker HWND equals the independent oracle modal HWND and the modal owner is the target window.

- [ ] **Step 2: Run RED on Windows**

```powershell
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w11 windows_real_provider_w11::w11_modal_before_dispatch_is_blocked_before_any_real_input_effect -- --ignored --exact --nocapture --test-threads=1
```

Expected RED if the existing `GetLastActivePopup` fence does not observe the owned modal exactly as required.

- [ ] **Step 3: Minimal production fix only if RED proves a real gap**

Preferred GREEN is **no production change**. If the actual owned modal is not observed, modify only the existing modal observation inside `revalidate_dispatch_context`; do not add a second modal detector or dismiss the modal.

- [ ] **Step 4: Add `run_w11` Lab adapter helper and verify**

The record uses case id `W11-modal-before-dispatch`, canonical outcome `owned-modal-blocked-before-input`, oracle effect zero, and exact blocker HWND evidence.

- [ ] **Step 5: Commit**

```bash
git commit -am "test(v43): prove W11 modal dispatch fence"
```

### Task 4: Prove W12 stale authority rejection across a real process restart

**Files:**
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w12.rs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_cases.rs`
- Modify only if RED requires typed correction: `crates/windows-uia-provider/src/lib.rs`
- Test if production changes: `crates/windows-uia-provider/tests/windows_verified_input_worker_mta_contract.rs` or a focused Windows-only target-reincarnation contract.

**Interfaces:**
- Uses A attachment/snapshot/request after A termination.
- Produces typed stale-target rejection before `SendInput`.
- Fresh B attachment must have distinct target incarnation and a new request.
- Produces campaign helper `run_w12(...) -> RealProviderLabRecord`.

- [ ] **Step 1: Write the real W12 test**

Sequence:

```text
spawn A -> prepare A -> attach/snapshot A -> mint A authority
-> capture A pid/hwnd/target incarnation -> kill A and wait
-> spawn B from same executable -> prepare B
-> attempt A request with A attachment
-> require stale-target rejection before insertion
-> assert B effect_count == 0
-> attach/snapshot B and assert B target incarnation != A
-> mint fresh B authority, then abandon it without dispatch
```

The stale request must not be retargeted to B.

- [ ] **Step 2: Run RED on Windows**

```powershell
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w12 windows_real_provider_w12::w12_restart_rejects_pre_restart_authority_before_any_replacement_effect -- --ignored --exact --nocapture --test-threads=1
```

- [ ] **Step 3: If stale target death is reported as generic `ProviderFailure`, make it typed**

In `require_current_target`, map the already-attached target becoming non-live or moving to a different PID/lifetime to `WindowsUiaWorkerError::TargetReincarnated` before any action boundary. Keep initial `attach` validation behavior unchanged.

The minimal rule is:

```text
attached HWND no longer resolves -> TargetReincarnated
resolved PID != attachment.selection.expected_process_id -> TargetReincarnated
fresh derived target incarnation != attachment.target_incarnation_ref -> TargetReincarnated
```

Do not auto-attach to the replacement.

- [ ] **Step 4: Verify W12 GREEN and W07/W09 regressions**

```powershell
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w12 -- --ignored --nocapture --test-threads=1
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w07 -- --ignored --nocapture --test-threads=1
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_w09 -- --ignored --nocapture --test-threads=1
```

- [ ] **Step 5: Add `run_w12` Lab record**

Case id: `W12-target-restart-after-authorization`.

Evidence must bind A/B PID+HWND, A/B distinct target incarnation refs, typed stale rejection, B effect zero, and fresh B reacquire.

- [ ] **Step 6: Commit**

```bash
git commit -am "test(v43): prove W12 restart invalidates authority"
```

### Task 5: Expand the prospective campaign to the exact 11-case set

**Files:**
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_campaign.rs`

**Interfaces:**
- Required set exactly W01-W09 + W11 + W12.
- RPOMR expected `(0, 11)`.
- Observation digest count expected `11`.

- [ ] **Step 1: Update failing campaign assertions first**

Change `REQUIRED_CASES` to 11 explicit entries, add W11/W12 preregistration identities/distinctions, append `run_w11` and `run_w12`, and require exactly 11 seed IDs.

Rename test to:

```text
prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate
```

Do not use the phrase `W01 through W12` because W10 remains unmeasured.

- [ ] **Step 2: Run campaign RED/registration check**

```powershell
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_campaign -- --list
```

- [ ] **Step 3: Finalize campaign metadata**

Use new revisions such as:

```text
lab-v43-windows-l7-r4
windows-provider-seeds-w01-w09-w11-w12-r4
```

Final sequence is after W12, RPOMR is `(0, 11)`, and `observation_digests.len() == 11`.

- [ ] **Step 4: Run prospective campaign on Windows**

```powershell
cargo test --manifest-path tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml --test v43_real_provider_campaign windows_l7_real_provider_campaign::prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate -- --ignored --exact --nocapture --test-threads=1
```

- [ ] **Step 5: Commit**

```bash
git commit -am "test(v43): extend L7 campaign with W11 W12"
```

### Task 6: Make W11/W12 permanent Windows gates

**Files:**
- Modify: `.github/workflows/windows-real-provider-seeds.yml`
- Modify if needed for cross-platform contracts: `.github/workflows/v43-green-proof.yml`

**Interfaces:**
- Permanent Windows real-provider job names W01-W09/W11/W12 explicitly.
- Adds exact W11 and W12 real test registration/execution steps.
- Runs renamed exact 11-case prospective campaign.

- [ ] **Step 1: Update workflow title and seed build label**

Use `W01/W02/W03/W04/W05/W06/W07/W08/W09/W11/W12`; never imply W10.

- [ ] **Step 2: Add W11 exact gate**

List the test first, fail if registration is absent, then run ignored exact test with one thread.

- [ ] **Step 3: Add W12 exact gate**

Same registration guard and exact execution requirement.

- [ ] **Step 4: Update campaign gate to exact 11-case test name**

Artifact upload remains bounded to the exact candidate SHA.

- [ ] **Step 5: Commit**

```bash
git commit -am "ci(v43): gate Windows W11 W12 real-provider races"
```

### Task 7: Exact-head audit, PR, and integration

**Files:**
- No feature changes unless a verified failing gate exposes a root cause.

**Interfaces:**
- Final feature head is immutable while gates run.
- PR remains draft until exact-head required gates are green.

- [ ] **Step 1: Audit scope**

Verify:

```text
no public windows_insert_verified_key_events
no shipping dependency on tools/provider-seeds or validation harness
no W10 completion claim
no automatic retry/rebind/modal dismissal
```

- [ ] **Step 2: Run/fetch exact-head gates**

Required:

```text
CI
Windows UIA Observe
Windows Real Provider Seeds
V4.3 Green Proof
```

Diagnose root cause before any fix; no assertion weakening, timeout inflation, force-green, or evidence relabeling.

- [ ] **Step 3: Inspect the uploaded L7 artifact**

Require:

```text
11 observation digests
RPOMR 0/11
required case set exactly W01-W09 + W11 + W12
W11 modal HWND + zero effect evidence
W12 A/B lineage + stale rejection + zero B effect evidence
candidate SHA matches final feature head
```

- [ ] **Step 4: Mark PR ready and merge with expected head SHA**

Only after all exact-head gates succeed.

- [ ] **Step 5: Verify post-merge main**

Confirm the merge commit contains the exact verified feature head and post-merge CI/Windows real-provider/UIA evidence stays green before declaring W11/W12 integrated-complete.
