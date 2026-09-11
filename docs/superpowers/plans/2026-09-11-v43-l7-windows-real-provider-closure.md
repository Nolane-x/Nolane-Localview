# V4.3 L7 Windows Real-Provider Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a non-overclaiming V4.3 L7 real-provider authority path and execute bounded Windows UIA seed coverage for W01, W02, and W06 with independent oracle evidence.

**Architecture:** Keep `localview-validation-lab` provider-neutral. A test-only Windows seed process exposes a real Win32/UIA surface plus a process-pipe ground-truth protocol; existing Windows UIA runtime code observes the real surface, and a narrow Lab adapter compares provider assertions with independent oracle truth. `LabRunBuilder` must refuse `REAL_PROVIDER_INTEGRATION_PASS` unless the run was admitted as an L7 real-provider campaign and RPOMR is actually measured cleanly.

**Tech Stack:** Rust 2024, Cargo workspace, `localview-validation-lab`, `localview-protocol`, `localview-windows-uia-provider`, `localview-windows-observe-runtime`, Win32/UIA through `windows = 0.61`, Tokio, GitHub Actions Windows hosted runner.

**Spec:** `docs/superpowers/specs/2026-09-11-v43-l7-windows-real-provider-closure-design.md`

## Global Constraints

- Fake/provider-contract simulation is L6; real-provider seed applications are L7.
- Historical commit subjects are provenance and are not rewritten; PR #110 metadata is reconciled to L6.
- No shipping crate may depend on provider seed code or require Validation Lab artifacts at runtime.
- Production LocalView cannot read the seed process ground-truth channel.
- `UNKNOWN`, `INCONCLUSIVE`, `UNSUPPORTED`, or conservative block cannot count as RPOMR failure, but cannot mint `REAL_PROVIDER_INTEGRATION_PASS`.
- Provider/oracle mismatch is retained as a counterexample; no retry-until-green behavior.
- Exact Windows runtime/provider APIs are reused; no second Lab-owned UIA implementation.
- First slice claims only Windows W01/W02/W06 seed coverage, not universal Windows or V4.3 completion.

---

### Task 1: Lock provider campaign layer semantics and real-provider pass authority

**Files:**
- Modify: `crates/validation-lab/src/preregistration.rs`
- Modify: `crates/validation-lab/src/result.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/provider_campaign_authority.rs`

**Interfaces:**
- Produces: `ProviderCampaignKind::{FakeProviderSimulator, RealProviderSeedApplications}`.
- Produces: `validate_provider_campaign_layer(kind: ProviderCampaignKind, layer: CampaignLayer) -> Result<(), LabError>`.
- Produces: `LabRunBuilder::start_provider_campaign(kind, admission, authority) -> Result<LabRunBuilder, LabError>`.
- `LabRunBuilder::finalize(ResultEvidence::RealProviderIntegrationPass, ...)` is legal only for a builder admitted through `start_provider_campaign(RealProviderSeedApplications, ...)`, with L7 preregistration, a platform profile, at least one measured RPOMR comparison, RPOMR numerator zero, and no incomplete provider observation/failure.

- [ ] **Step 1: Write permanent RED tests**

Create tests that require these exact behaviors:

```rust
#[test]
fn provider_campaign_layers_are_semantically_fixed() {
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::FakeProviderSimulator,
        CampaignLayer::L6,
    ).is_ok());
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::RealProviderSeedApplications,
        CampaignLayer::L7,
    ).is_ok());
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::FakeProviderSimulator,
        CampaignLayer::L7,
    ).is_err());
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::RealProviderSeedApplications,
        CampaignLayer::L6,
    ).is_err());
}
```

Add a prospective run fixture with a valid persisted preregistration and assert:

```rust
let err = LabRunBuilder::start_provider_campaign(
    ProviderCampaignKind::RealProviderSeedApplications,
    l6_real_provider_admission,
    authority,
).unwrap_err();
assert_eq!(err, LabError::ProviderCampaignLayerMismatch {
    campaign: ProviderCampaignKind::RealProviderSeedApplications,
    expected: CampaignLayer::L7,
    actual: CampaignLayer::L6,
});
```

Add pass-gate tests that require `RealProviderIntegrationPass` to reject:

```rust
builder.finalize(ResultEvidence::RealProviderIntegrationPass, 30)
```

when the builder was started generically, when no RPOMR observation exists, when an RPOMR observation has a mismatch, or when a provider-backed observation lacks RPOMR eligibility.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-validation-lab --test provider_campaign_authority -- --nocapture
```

Expected: compile failure for missing `ProviderCampaignKind`, `validate_provider_campaign_layer`, `start_provider_campaign`, and the new `LabError` variants.

- [ ] **Step 3: Implement minimal layer authority**

In `preregistration.rs` add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCampaignKind {
    FakeProviderSimulator,
    RealProviderSeedApplications,
}

impl ProviderCampaignKind {
    pub const fn required_layer(self) -> CampaignLayer {
        match self {
            Self::FakeProviderSimulator => CampaignLayer::L6,
            Self::RealProviderSeedApplications => CampaignLayer::L7,
        }
    }
}

pub fn validate_provider_campaign_layer(
    campaign: ProviderCampaignKind,
    actual: CampaignLayer,
) -> Result<(), LabError> {
    let expected = campaign.required_layer();
    if actual != expected {
        return Err(LabError::ProviderCampaignLayerMismatch {
            campaign,
            expected,
            actual,
        });
    }
    Ok(())
}
```

Export the type/function from `lib.rs` and add:

```rust
ProviderCampaignLayerMismatch {
    campaign: ProviderCampaignKind,
    expected: CampaignLayer,
    actual: CampaignLayer,
}
```

- [ ] **Step 4: Add provider-campaign admission to `LabRunBuilder`**

Add private field:

```rust
provider_campaign_kind: Option<ProviderCampaignKind>,
```

Generic `start()` sets it to `None`.

Add:

```rust
pub fn start_provider_campaign(
    campaign: ProviderCampaignKind,
    admission: LabRunAdmission,
    actual_execution_authority: ActualExecutionAuthority,
) -> Result<Self, LabError> {
    let layer = match &admission {
        LabRunAdmission::Prospective { preregistration, .. } => preregistration.campaign_layer,
        LabRunAdmission::Exploratory { .. } => {
            return Err(LabError::ProviderCampaignRequiresProspectiveAdmission);
        }
    };
    validate_provider_campaign_layer(campaign, layer)?;
    let mut builder = Self::start(admission, actual_execution_authority)?;
    builder.provider_campaign_kind = Some(campaign);
    Ok(builder)
}
```

- [ ] **Step 5: Gate `REAL_PROVIDER_INTEGRATION_PASS`**

Before building the final payload, when `evidence == ResultEvidence::RealProviderIntegrationPass`, require:

```rust
self.provider_campaign_kind == Some(ProviderCampaignKind::RealProviderSeedApplications)
```

and `platform_profile.is_some()`.

Reduce metrics, fetch `LabMetricKind::Rpomr`, and require `MetricStatus::Measured`, `denominator > 0`, and `numerator == 0`.

Also require every observation in this narrow campaign to be provider-backed, RPOMR-eligible, and failure-free. Return typed `LabError::InvalidRealProviderPass { reason: ... }` for each failed guard.

- [ ] **Step 6: Run GREEN + regressions**

```bash
cargo test -p localview-validation-lab --test provider_campaign_authority -- --nocapture
cargo test -p localview-validation-lab -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/validation-lab/src crates/validation-lab/tests/provider_campaign_authority.rs
git commit -m "feat(v43): gate L7 real-provider campaign authority"
```

---

### Task 2: Add provider-neutral real-provider oracle adapter and RPOMR semantics

**Files:**
- Create: `crates/validation-lab/src/real_provider_adapter.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/real_provider_adapter.rs`

**Interfaces:**
- Produces: `RealProviderCaseKind` for W01/W02/W06.
- Produces: `RealProviderObservedOutcome::{Asserted(String), Unknown, Inconclusive, Unsupported, ConservativeBlock}`.
- Produces: `RealProviderGroundTruth { canonical_outcome: String, digest: CanonicalDigest }`.
- Produces: `RealProviderCaseInput<'a>` with `case_id`, `seed_app_digest`, `platform_profile_revision`, `environment_artifact_digest`, `provider_evidence_refs`, `ground_truth`, `observed_outcome`, `comparison_profile_revision`, `logical_sequence`, and typed case facts.
- Produces: `adapt_real_provider_case(...) -> Result<RealProviderLabRecord, LabError>`.
- Produces: `derive_real_provider_campaign_evidence(records: &[RealProviderLabRecord]) -> Option<ResultEvidence>`.

- [ ] **Step 1: Write RED tests for exact oracle accounting**

Require an asserted exact match to create one provider-backed observation with `LabMetricKind::Rpomr` eligibility, no mismatch flag, and `Some(ResultEvidence::RealProviderIntegrationPass)`.

Require an asserted mismatch to add exactly `LabFailureFlag::RealProviderOracleMismatch`, yielding RPOMR `(1,1)` and `Some(CounterexampleFound)`.

Require `Unknown`, `Inconclusive`, `Unsupported`, and `ConservativeBlock` to create no RPOMR denominator and return `result_evidence == None`.

Require whitespace/empty `case_id`, `seed_app_digest`, `platform_profile_revision`, `environment_artifact_digest`, `ground_truth.digest.0`, or `comparison_profile_revision` to fail before observation minting.

- [ ] **Step 2: Write RED tests for W01/W02/W06 typed failures**

W01 input uses real protocol states:

```rust
RealProviderCaseKind::W01MissingPropertyEvent {
    continuity: EventContinuityState::GapDetected,
    reconciliation: Some(ReconciliationCompleteness::Established),
    accepted_as_fresh: true,
    accepted_as_reconciled: true,
}
```

Established reconciliation prevents EOFFR/RMR failure. The same case with no established reconciliation and `accepted_as_fresh=true` sets `EventOnlyFalseFreshness`.

W02 only measures PIAER when `provider_identity_reuse_observed=true`; if stale identity is accepted, set `ProviderIdAbaEscape`. If exact reuse is not observed, PIAER remains unmeasured for that case.

W06 requires distinct old/new provider incarnation identities. `stale_authority_survived_reacquire=true` maps to `StaleCacheAuthority`; `cleanup_to_baseline=false` maps to `CleanupToBaselineFailure`.

- [ ] **Step 3: Run RED**

```bash
cargo test -p localview-validation-lab --test real_provider_adapter -- --nocapture
```

Expected: compile failure for missing real-provider adapter symbols.

- [ ] **Step 4: Implement minimal adapter**

Every asserted case inserts `Rpomr` eligibility and compares the asserted canonical outcome to `ground_truth.canonical_outcome` exactly. Do not infer correctness from prose or error strings.

For conservative non-asserting outcomes, leave RPOMR ineligible and return no pass evidence.

Build evidence refs from the submitted provider refs plus canonical references:

```rust
format!("ground-truth:{}", input.ground_truth.digest.0)
format!("environment:{}", input.environment_artifact_digest)
format!("seed-app:{}", input.seed_app_digest)
format!("platform:{}", input.platform_profile_revision)
```

- [ ] **Step 5: Implement campaign evidence derivation**

Rules:

```text
empty records -> None
any counterexample -> Some(CounterexampleFound)
otherwise any incomplete record -> None
otherwise all records pass -> Some(RealProviderIntegrationPass)
```

Counterexamples outrank incomplete records.

- [ ] **Step 6: Run GREEN + full Lab regression**

```bash
cargo test -p localview-validation-lab --test real_provider_adapter -- --nocapture
cargo test -p localview-validation-lab -- --nocapture
```

- [ ] **Step 7: Commit**

```bash
git add crates/validation-lab/src crates/validation-lab/tests/real_provider_adapter.rs
git commit -m "feat(v43): add L7 real-provider oracle authority"
```

---

### Task 3: Build the isolated Windows seed process and ground-truth pipe

**Files:**
- Create: `tools/provider-seeds/windows-uia-seed/Cargo.toml`
- Create: `tools/provider-seeds/windows-uia-seed/src/main.rs`
- Create: `tools/provider-seeds/windows-uia-seed/src/state.rs`
- Create: `tools/provider-seeds/windows-uia-seed/src/control.rs`
- Create: `tools/provider-seeds/windows-uia-seed/tests/protocol.rs`

**Interfaces:**
- Seed control transport is stdin/stdout JSON Lines. This is an OS pipe owned by the test process tree; production LocalView receives neither pipe handle nor protocol token.
- Commands: `get_ground_truth`, `burst_name_changes`, `recreate_control`, `shutdown`.
- Ground truth response includes `seed_run_id`, `process_incarnation`, `window_handle`, `control_handle`, `control_incarnation`, `logical_name`, `recreation_generation`, and `logical_sequence`.

- [ ] **Step 1: Write protocol RED tests**

Tests must require deterministic state transitions without UIA:

```text
initial generation = 1
burst_name_changes([A,B,C]) -> logical_name C and monotonic sequence
a recreate increments control_incarnation and recreation_generation
shutdown is terminal
```

- [ ] **Step 2: Run RED**

```bash
cargo test --manifest-path tools/provider-seeds/windows-uia-seed/Cargo.toml
```

Expected: missing package/source symbols.

- [ ] **Step 3: Implement package boundary**

`Cargo.toml` owns an inner `[workspace]`, uses edition 2024 / rust-version 1.85, `serde = { version = "1", features=["derive"] }`, `serde_json = "1"`, `uuid = { version = "1", features=["v4", "serde"] }`, and Windows-only `windows = 0.61` with Foundation + WindowsAndMessaging features.

The package is never added to root workspace members.

- [ ] **Step 4: Implement Win32 surface**

On Windows, create a visible standard `BUTTON` window/control and run its message loop on the UI thread. The control thread reads JSON commands and uses Win32 messaging to mutate/recreate the control.

`burst_name_changes` performs multiple real `SetWindowTextW` mutations without allowing the harness to drain between each change; this is used with runtime event capacity 1 to produce a real event-buffer gap.

`recreate_control` destroys and recreates the target control and reports both old/new HWND and control incarnation so the harness can detect whether provider-local identity reuse was actually observed rather than fabricate it.

- [ ] **Step 5: Run protocol tests and compile Windows seed**

```bash
cargo test --manifest-path tools/provider-seeds/windows-uia-seed/Cargo.toml
cargo check --manifest-path tools/provider-seeds/windows-uia-seed/Cargo.toml --all-targets
```

On non-Windows, protocol/state tests compile without attempting Win32 UI.

- [ ] **Step 6: Commit**

```bash
git add tools/provider-seeds/windows-uia-seed
git commit -m "test(v43): add isolated Windows UIA seed process"
```

---

### Task 4: Execute W01 real event-gap + reconciliation against Windows UIA runtime

**Files:**
- Create: `crates/windows-observe-runtime/tests/v43_real_provider_seeds.rs`
- Modify test dependencies only if required: `crates/windows-observe-runtime/Cargo.toml`

**Interfaces:**
- Launch seed process with piped stdin/stdout.
- Attach the exact seed HWND through `spawn_windows_uia_runtime_manager`.
- Configure `WindowsObserveRuntimeConfig { event_capacity: 1, drain_limit: 32 }`.
- Use seed `burst_name_changes` to generate more than one real UIA property event before `drain_once`.

- [ ] **Step 1: Write ignored Windows RED test `w01_event_gap_requires_reconciliation`**

The test must assert that after the burst and first drain the observation continuity is not treated as certified continuous when the buffer reports dropped events; then perform runtime reconciliation and require the current semantic snapshot to agree with the independent seed ground truth final name.

Convert the exact evidence into `RealProviderCaseInput::W01...` and assert RPOMR denominator is 1 and numerator 0 for a clean implementation.

- [ ] **Step 2: Run RED on Windows workflow**

```bash
LOCALVIEW_UIA_SMOKE=1 cargo test -p localview-windows-observe-runtime --test v43_real_provider_seeds w01_event_gap_requires_reconciliation -- --ignored --nocapture --test-threads=1
```

Expected: failure before harness implementation/wiring is complete.

- [ ] **Step 3: Implement minimal W01 harness**

Reuse existing runtime manager and `LiveBridge`; no new production event/reconciliation path. Preserve the dropped-event evidence and ground-truth digest as Lab evidence refs.

- [ ] **Step 4: Run GREEN**

Run the exact command from Step 2. Expected: PASS, with a measured RPOMR denominator > 0.

- [ ] **Step 5: Commit**

```bash
git add crates/windows-observe-runtime/tests/v43_real_provider_seeds.rs crates/windows-observe-runtime/Cargo.toml
git commit -m "test(v43): prove W01 real provider reconciliation"
```

---

### Task 5: Execute W02 recreated-element identity and W06 provider reacquisition

**Files:**
- Modify: `crates/windows-observe-runtime/tests/v43_real_provider_seeds.rs`

- [ ] **Step 1: Write RED W02 test**

`w02_recreated_element_never_resurrects_stale_identity` records the original provider/target/element evidence, asks the seed to recreate its target control, then forces reacquisition through the real runtime/provider path. Ground truth proves the control incarnation changed.

If provider-local identity reuse is actually observed, mark PIAER eligible; otherwise record the reuse condition as not measured and still require the stale prior binding not to authorize the new control.

- [ ] **Step 2: Write RED W06 test**

`w06_provider_reacquire_rebinds_incarnation_and_cleans_up` attaches with runtime manager A, records its provider incarnation, releases/drops that manager, creates runtime manager B, reattaches to the same seed surface, and requires current evidence to bind to the new provider incarnation. Release B and prove runtime/bridge cleanup baseline.

Map stale-authority survival to SCAR and cleanup failure to CBFR through the real-provider adapter.

- [ ] **Step 3: Run RED on Windows**

```bash
LOCALVIEW_UIA_SMOKE=1 cargo test -p localview-windows-observe-runtime --test v43_real_provider_seeds -- --ignored --nocapture --test-threads=1
```

- [ ] **Step 4: Implement minimal harness support and run GREEN**

No production behavior change unless a real failing seed demonstrates a concrete bug. If a production bug is exposed, stop this task at the counterexample, add a separate test-first corrective commit, then resume the seed test.

- [ ] **Step 5: Commit**

```bash
git add crates/windows-observe-runtime/tests/v43_real_provider_seeds.rs
git commit -m "test(v43): prove W02 and W06 provider lifecycles"
```

---

### Task 6: Bind environment/prospective Lab run and CI gate

**Files:**
- Modify: `crates/windows-observe-runtime/tests/v43_real_provider_seeds.rs`
- Modify: `.github/workflows/windows-uia-observe.yml`
- Modify if needed: `.github/workflows/ci.yml`

- [ ] **Step 1: Build canonical environment manifest inside test harness**

Bind at least: Windows build string available from runner, architecture, exact LocalView candidate SHA/build identity supplied by CI, provider profile revision, display topology descriptor, DPI scale, locale/input method descriptor, permission state, and seed executable digest.

Unknown values are explicit strings such as `unknown:hosted-runner-not-exposed`; fields are never silently omitted.

- [ ] **Step 2: Preregister prospective L7 run**

Create `LabPreregistration` with `campaign_layer: CampaignLayer::L7`, Windows `platform_profile`, seed identities W01/W02/W06, and declared metrics including RPOMR plus the scenario metrics used by the three cases. Persist/validate the preregistration receipt before run start, then call:

```rust
LabRunBuilder::start_provider_campaign(
    ProviderCampaignKind::RealProviderSeedApplications,
    admission,
    execution_authority,
)
```

Append all three real-provider observations and derive campaign evidence with `derive_real_provider_campaign_evidence`.

`REAL_PROVIDER_INTEGRATION_PASS` is legal only if all required records are complete and the authority core gate accepts it.

- [ ] **Step 3: Add named Windows CI gate**

Append to `windows-uia-observe.yml`:

```yaml
- name: V4.3 L7 Windows real-provider seeds W01/W02/W06
  env:
    LOCALVIEW_UIA_SMOKE: "1"
    LOCALVIEW_CANDIDATE_SHA: ${{ github.sha }}
  run: cargo test -p localview-windows-observe-runtime --test v43_real_provider_seeds -- --ignored --nocapture --test-threads=1
```

- [ ] **Step 4: Add pure Lab tests to ordinary CI if not already covered by workspace tests**

Require `provider_campaign_authority` and `real_provider_adapter` under the named `V4.3 validation lab authority contract` or workspace test gate.

- [ ] **Step 5: Verify exact candidate**

Run/require:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Plus the Windows real-provider command above.

- [ ] **Step 6: Review shipping dependency exclusion**

Confirm root workspace members do not include `tools/provider-seeds/windows-uia-seed`, shipping crates have no path dependency to it, and no normal runtime startup references seed/oracle code.

- [ ] **Step 7: Commit**

```bash
git add .github/workflows crates/windows-observe-runtime docs/superpowers
git commit -m "ci(v43): gate Windows L7 real-provider seed evidence"
```

---

### Task 7: Exact-head review and integration

**Files:** no planned production changes; this is an evidence gate.

- [ ] **Step 1:** Compare branch against `main` and verify only design/plan, Validation Lab authority, test-only seed code, Windows test harness, and workflow changes are present unless a separately evidenced production fix was required.
- [ ] **Step 2:** Confirm every required W01/W02/W06 case produced individually addressable evidence and no campaign-level Boolean replaced case receipts.
- [ ] **Step 3:** Confirm RPOMR is measured (`denominator > 0`) and clean candidate numerator is zero on exact Windows candidate SHA.
- [ ] **Step 4:** Confirm no unknown/inconclusive required case was promoted to pass.
- [ ] **Step 5:** Confirm CI + Windows UIA workflow are successful on the exact PR head.
- [ ] **Step 6:** Merge with expected-head lock only after the exact-head gates succeed, then verify fresh push-triggered workflows on the returned merge SHA before claiming the slice closed.
