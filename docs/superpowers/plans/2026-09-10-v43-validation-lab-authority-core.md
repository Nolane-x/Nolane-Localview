# V4.3 Validation Lab Authority Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic, research-only `localview-validation-lab` authority core that makes preregistration, seed/oracle provenance, result strength, canonical artifacts, V4.3 metrics, and silent-unsoundness gates executable without becoming a shipping-runtime dependency.

**Architecture:** Add one pure Rust workspace crate that owns canonical research contracts, not campaign execution. It accepts typed evidence from future campaign adapters, reuses existing mutation/state-space primitives only in later PRs, and separates prepared preregistration from externally acknowledged durable persistence so prospective claims cannot be minted from an in-memory object alone.

**Tech Stack:** Rust 2024 / rust-version 1.85, Serde/serde_json, SHA-256, thiserror, existing Cargo workspace and GitHub Actions matrix on Ubuntu/macOS/Windows.

**Spec:** `docs/superpowers/specs/2026-09-10-v43-validation-lab-authority-core-design.md`

**Normative clarification:** `docs/superpowers/specs/2026-09-10-v43-validation-lab-authority-core-clarification.md`

## Global Constraints

- Validation Lab is research/dev authority and must not become a dependency of `apps/daemon`, `apps/cli`, `apps/desktop/src-tauri`, provider runtimes, observation runtimes, or normal product startup.
- The first PR does not implement L2 mutation execution, L3 property generation, L4 bounded temporal search, L5 concurrency exploration, L6 differential reducers, L7 fake-provider campaigns, L8 real-provider campaigns, or L9 stress orchestration.
- Research result classes are closed and never include or imply `PROVED`.
- Missing preregistration/persistence before execution can only create an explicit exploratory run; drift after a valid prospective receipt is a hard authority error.
- A prepared preregistration is not prospective execution authority. Prospective execution requires exact externally acknowledged persistence evidence for the same canonical digest.
- Zero denominator means `NOT_MEASURED`, never `0%`, never PASS.
- V4.3 zero-target gate covers `SUAR`, `WPDR`, `PILR`, `PDMR`, and `UOBRR`; not-measured required metrics make the gate `INCOMPLETE`.
- Conservative `UNKNOWN`, `INCONCLUSIVE`, `RECONCILIATION_REQUIRED`, `UNSUPPORTED`, and conservative blocks do not count as silent unsoundness unless a typed unsoundness flag is also present.
- Canonical research identity uses domain-separated SHA-256 over deterministic UTF-8 JSON. Set-like fields are sorted/unique; semantically ordered arrays retain order.
- Canonicalization must be cross-platform deterministic and use no wall-clock ordering, OS/provider/browser/model calls, hidden retry, or nondeterministic map iteration.
- Every production slice starts from a permanent RED test and preserves exact RED -> GREEN lineage.

---

## File Structure

Create:

```text
crates/validation-lab/
  Cargo.toml
  src/
    lib.rs             # public exports and LabError
    canonical.rs       # deterministic JSON + domain-separated SHA-256
    identity.rs        # revision context + completed run identity
    preregistration.rs # prepared artifact + persisted receipt + execution admission
    seed.rs            # seed identity/catalog/oracle collision rules
    metrics.rs         # 14 metrics + typed accounting + zero-target gate
    result.rs          # observations, execution mode, run builder/finalization
    artifact.rs        # canonical lab artifact kinds/states/serialized envelopes
  tests/
    metrics_authority.rs
    canonical_preregistration.rs
    seed_oracle_provenance.rs
    run_authority_lifecycle.rs
    artifact_contract.rs
    shipping_dependency_exclusion.rs
```

Modify:

```text
Cargo.toml
.github/workflows/ci.yml
```

No existing runtime/provider crate is modified for implementation logic.

---

### Task 1: Workspace Bootstrap + Permanent RED-A Metric Authority

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/validation-lab/Cargo.toml`
- Create: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/metrics_authority.rs`

**Interfaces:**
- Produces crate `localview-validation-lab`.
- Test will consume future exports `LabMetricKind`, `LabMetricValue`, `MetricStatus`, `MetricSnapshot`, `SilentUnsoundnessGateStatus`, `evaluate_silent_unsoundness_gate`.

- [ ] **Step 1: Register an empty research crate and write the permanent failing test before metric implementation**

Add workspace member:

```toml
"crates/validation-lab",
```

Create `crates/validation-lab/Cargo.toml`:

```toml
[package]
name = "localview-validation-lab"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true
```

Create `src/lib.rs` containing only:

```rust
#![forbid(unsafe_code)]
```

Create `tests/metrics_authority.rs` that imports the future metric API and requires all 14 enum values plus zero-denominator behavior:

```rust
use localview_validation_lab::{
    LabMetricKind, LabMetricValue, MetricStatus, MetricSnapshot,
    SilentUnsoundnessGateStatus, evaluate_silent_unsoundness_gate,
};

#[test]
fn v43_metric_catalog_is_complete_and_empty_evidence_cannot_pass() {
    assert_eq!(LabMetricKind::ALL.len(), 14);
    let empty = MetricSnapshot::empty();
    for kind in LabMetricKind::ALL {
        let value = empty.get(kind).expect("every metric is represented");
        assert_eq!(value.denominator, 0);
        assert_eq!(value.status, MetricStatus::NotMeasured);
        assert_eq!(value.rate_ppb, None);
    }
    assert_eq!(
        evaluate_silent_unsoundness_gate(&empty),
        SilentUnsoundnessGateStatus::Incomplete,
    );
}
```

- [ ] **Step 2: Commit the test-only/scaffold RED**

```bash
git add Cargo.toml crates/validation-lab
git commit -m "test(v43): require validation lab metric authority"
```

- [ ] **Step 3: Run exact RED**

```bash
cargo test -p localview-validation-lab --test metrics_authority -- --nocapture
```

Expected: compile failure because metric types/functions do not exist. CI `cargo check --workspace --all-targets` may also fail on this test-only head; that is acceptable RED evidence if the error is the missing metric API and no unrelated failure precedes it.

---

### Task 2: GREEN-A Deterministic Metric Catalog and Silent-Unsoundness Gate

**Files:**
- Create: `crates/validation-lab/src/metrics.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Extend: `crates/validation-lab/tests/metrics_authority.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabMetricKind {
    Suar, Wpdr, Pilr, Eoffr, Rmr, Piaer, Wfir,
    Pdmr, Scar, Uobrr, Msr, Crdr, Rpomr, Cbfr,
}

impl LabMetricKind {
    pub const ALL: [Self; 14] = [/* exact order above */];
    pub const SILENT_UNSOUNDNESS_ZERO_TARGET: [Self; 5] = [
        Self::Suar, Self::Wpdr, Self::Pilr, Self::Pdmr, Self::Uobrr,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStatus { Measured, NotMeasured }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabMetricValue {
    pub kind: LabMetricKind,
    pub numerator: u64,
    pub denominator: u64,
    pub status: MetricStatus,
    pub rate_ppb: Option<u64>,
}

pub struct MetricSnapshot { values: BTreeMap<LabMetricKind, LabMetricValue> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilentUnsoundnessGateStatus { Pass, Fail, Incomplete }
```

`LabMetricValue::new(kind, numerator, denominator)` rules:

```text
denominator == 0 && numerator == 0 -> NOT_MEASURED, no rate
numerator > denominator -> LabError::InvalidMetricSubset
otherwise -> MEASURED and rate_ppb = floor(numerator * 1_000_000_000 / denominator)
```

Use `u128` for intermediate multiplication and convert back to `u64` after bounded division.

- [ ] **Step 1: Extend failing test with exact WPDR and gate semantics**

```rust
#[test]
fn one_wrong_principal_dispatch_is_one_of_one_and_fails_gate() {
    let mut snapshot = MetricSnapshot::empty();
    snapshot.set(LabMetricValue::new(LabMetricKind::Wpdr, 1, 1).unwrap());
    for kind in [LabMetricKind::Suar, LabMetricKind::Pilr, LabMetricKind::Pdmr, LabMetricKind::Uobrr] {
        snapshot.set(LabMetricValue::new(kind, 0, 1).unwrap());
    }
    let wpdr = snapshot.get(LabMetricKind::Wpdr).unwrap();
    assert_eq!(wpdr.rate_ppb, Some(1_000_000_000));
    assert_eq!(evaluate_silent_unsoundness_gate(&snapshot), SilentUnsoundnessGateStatus::Fail);
}
```

Also test five measured-zero required metrics => PASS and one unmeasured required metric => INCOMPLETE.

- [ ] **Step 2: Implement `metrics.rs` minimally**

Do not add observation reduction yet; Task 6 owns typed evidence reduction.

- [ ] **Step 3: Run GREEN**

```bash
cargo test -p localview-validation-lab --test metrics_authority -- --nocapture
cargo check -p localview-validation-lab --all-targets
cargo clippy -p localview-validation-lab --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/validation-lab/src crates/validation-lab/tests/metrics_authority.rs
git commit -m "feat(v43): add validation lab metric authority"
```

---

### Task 3: Canonical JSON, Revision Context, and Two-Phase Preregistration

**Files:**
- Create: `crates/validation-lab/src/canonical.rs`
- Create: `crates/validation-lab/src/identity.rs`
- Create: `crates/validation-lab/src/preregistration.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/canonical_preregistration.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalDigest(pub String);

pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, LabError>;
pub fn canonical_digest<T: Serialize>(value: &T) -> Result<CanonicalDigest, LabError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabRevisionContext {
    pub lab_revision: String,
    pub seed_corpus_revision: String,
    pub spec_revision_digest: String,
    pub reference_reducer_revision: String,
    pub mutation_catalog_revision: String,
    pub comparison_profile_revision: String,
    pub random_source_profile: String,
    pub platform_profile: Option<String>,
    pub start_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedPreregistration {
    pub canonical_bytes: Vec<u8>,
    pub digest: CanonicalDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPreregistrationReceipt {
    pub digest: CanonicalDigest,
    pub logical_sequence: u64,
    pub persistence_ref: String,
}
```

`LabPreregistration` fields:

```rust
pub revision_context: LabRevisionContext,
pub seed_catalog_digest: CanonicalDigest,
pub seed_identities: Vec<LabSeedIdentity>,
pub campaign_layer: CampaignLayer,
pub expected_distinctions: BTreeSet<String>,
pub model_bound: Option<u64>,
pub assumptions: BTreeSet<String>,
pub declared_metrics: BTreeSet<LabMetricKind>,
pub creation_sequence: u64,
```

`CampaignLayer` is a closed enum `L0..L9` serialized as `l0`..`l9`.

- [ ] **Step 1: Write RED for digest semantics and persistence authority**

Tests must prove:

```text
same maps/set-like fields in different insertion order -> same digest
change expected distinction -> different digest
change seed corpus revision -> different digest
change model bound -> different digest
change comparison profile -> different digest
change random source profile -> different digest
prepared-only object cannot create prospective authority
matching persisted receipt validates
mismatched persisted digest -> PreregistrationPersistenceMismatch
logical_sequence == 0 -> invalid receipt
empty persistence_ref -> invalid receipt
```

Include a canonical numeric golden vector using `serde_json::json!({"a":1,"b":1.5,"c":1e6})`; pin the exact canonical bytes produced by the chosen serializer in the test once the RED demonstrates the API is absent. The test must run identically on all three OSes.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-validation-lab --test canonical_preregistration -- --nocapture
```

Expected: missing canonical/preregistration types.

- [ ] **Step 3: Implement deterministic canonicalization**

Implementation algorithm:

```rust
fn normalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map.into_iter()
                .map(|(k, v)| (k, normalize(v)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(normalize).collect()),
        other => other,
    }
}
```

Set-like contract fields must already be `BTreeSet` or explicitly normalized before serialization. Hash bytes:

```text
b"localview-validation-lab/v1\0" + canonical_json_bytes
```

Digest wire value is lowercase 64-hex SHA-256.

- [ ] **Step 4: Implement two-phase preregistration**

`LabPreregistration::prepare()` returns `PreparedPreregistration` only. `validate_persisted_receipt(prepared, receipt)` returns a validated prospective token/receipt only after exact digest, nonzero sequence and non-empty persistence ref checks. There is no public API that turns `PreparedPreregistration` directly into prospective execution mode.

- [ ] **Step 5: Run GREEN + determinism repetition**

```bash
cargo test -p localview-validation-lab --test canonical_preregistration -- --nocapture
for i in 1 2 3 4 5; do cargo test -p localview-validation-lab --test canonical_preregistration canonical -- --nocapture; done
```

- [ ] **Step 6: Commit**

```bash
git add crates/validation-lab/src crates/validation-lab/tests/canonical_preregistration.rs
git commit -m "feat(v43): add canonical lab preregistration authority"
```

---

### Task 4: Seed Catalog and Append-Only Oracle Revision Provenance

**Files:**
- Create: `crates/validation-lab/src/seed.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/seed_oracle_provenance.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LabSeedIdentity {
    pub seed_id: String,
    pub prediction_revision: String,
    pub oracle_revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabSeed {
    pub identity: LabSeedIdentity,
    pub family: String,
    pub spec_surface_refs: BTreeSet<u32>,
    pub input_fixture: serde_json::Value,
    pub expected_semantic_outcome: String,
    pub forbidden_outcomes: BTreeSet<String>,
    pub comparison_mode: String,
    pub risk_if_missed: String,
}

pub struct LabSeedCatalog {
    revision: String,
    seeds: BTreeMap<LabSeedIdentity, LabSeed>,
}
```

- [ ] **Step 1: Write RED identity-collision/oracle-correction tests**

Require:

```text
same exact identity twice -> DuplicateSeedIdentity
same seed_id+prediction_revision+oracle_revision with different expected outcome -> DuplicateSeedIdentity
same seed_id+prediction_revision with oracle_revision r1 and r2 -> both retained
catalog digest changes when oracle revision/content changes
set insertion order in spec refs/forbidden outcomes does not change digest
empty seed_id/prediction_revision/oracle_revision/revision -> typed validation error
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-validation-lab --test seed_oracle_provenance -- --nocapture
```

- [ ] **Step 3: Implement immutable catalog**

`LabSeedCatalog::new(revision, seeds)` validates identity strings, rejects duplicate identities, stores a deterministic `BTreeMap`, and exposes `canonical_digest()` using Task 3 canonicalization.

- [ ] **Step 4: Run GREEN**

```bash
cargo test -p localview-validation-lab --test seed_oracle_provenance -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add crates/validation-lab/src/seed.rs crates/validation-lab/src/lib.rs crates/validation-lab/tests/seed_oracle_provenance.rs
git commit -m "feat(v43): preserve validation seed and oracle provenance"
```

---

### Task 5: Result Taxonomy, Prospective/Exploratory Admission, and Completed Identity

**Files:**
- Create: `crates/validation-lab/src/result.rs`
- Modify: `crates/validation-lab/src/identity.rs`
- Modify: `crates/validation-lab/src/preregistration.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/run_authority_lifecycle.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchResultClass {
    ExploratoryObservation,
    PreregisteredSeedPass,
    CounterexampleFound,
    NoCounterexampleWithinBoundN,
    MutantKilled,
    MutantSurvived,
    DifferentialEquivalentWithinVectorSet,
    DifferentialDivergenceFound,
    PropertyCampaignPassN,
    RealProviderIntegrationPass,
    IndependentReplicationPass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum ExecutionMode {
    Prospective { receipt: ValidatedPreregistrationReceipt },
    Exploratory { downgrade_reason: DowngradeReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedLabRunIdentity {
    pub revision_context: LabRevisionContext,
    pub result_artifact_digest: CanonicalDigest,
}
```

Use a typed `ResultEvidence` enum whose variants map one-to-one to `ResearchResultClass`; no API accepts a free `requested_class` string. `class()` derives the result class from the evidence variant. `Exploratory` execution always finalizes as `ExploratoryObservation` regardless of stronger supplied evidence and retains the downgrade reason.

Prospective `ActualExecutionAuthority` binds:

```rust
pub seed_catalog_digest: CanonicalDigest,
pub comparison_profile_revision: String,
pub random_source_profile: String,
pub model_bound: Option<u64>,
```

- [ ] **Step 1: Write RED admission/finalization tests**

Prove:

```text
prepared-only preregistration -> cannot construct Prospective
missing persistence -> Exploratory(MissingPersistedPreregistration)
exploratory + SeedPass evidence -> final class still ExploratoryObservation
validated persisted receipt + exact actual authority + SeedPass -> PreregisteredSeedPass
prospective seed-catalog drift -> hard ProspectiveAuthorityDrift
prospective comparison-profile drift -> hard ProspectiveAuthorityDrift
prospective random-source drift -> hard ProspectiveAuthorityDrift
prospective bound drift -> hard ProspectiveAuthorityDrift
serialized classes never contain "proved"
result payload is hashed first; CompletedLabRunIdentity then binds that digest without self-reference
finalize twice -> AlreadyFinalized
append after finalization -> AlreadyFinalized
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-validation-lab --test run_authority_lifecycle -- --nocapture
```

- [ ] **Step 3: Implement the minimum state machine**

`LabRunBuilder` stores mode, revision context, observations and `finalized: bool`. `finalize(&mut self, request)` returns an owned `CompletedLabRun` and flips `finalized` only after all validation and digest computation succeed.

For prospective mode, the persisted receipt digest must equal the preregistration digest and actual execution authority must match the preregistered values exactly. Any mismatch returns a hard error; do not downgrade.

- [ ] **Step 4: Run GREEN**

```bash
cargo test -p localview-validation-lab --test run_authority_lifecycle -- --nocapture
cargo check -p localview-validation-lab --all-targets
cargo clippy -p localview-validation-lab --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/validation-lab/src crates/validation-lab/tests/run_authority_lifecycle.rs
git commit -m "feat(v43): add validation lab run authority lifecycle"
```

---

### Task 6: Typed Observation Reduction and All 14 Metric Numerators

**Files:**
- Modify: `crates/validation-lab/src/metrics.rs`
- Modify: `crates/validation-lab/src/result.rs`
- Extend: `crates/validation-lab/tests/metrics_authority.rs`
- Extend: `crates/validation-lab/tests/run_authority_lifecycle.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabFailureFlag {
    SilentUnsoundAction,
    WrongPrincipalDispatch,
    PrincipalInformationLeak,
    EventOnlyFalseFreshness,
    ReconciliationMiss,
    ProviderIdAbaEscape,
    WrongForegroundInput,
    PartialDispatchMisclassifiedSuccess,
    StaleCacheAuthority,
    BlindRetryAfterUnknown,
    MutationSurvived,
    CrossReducerDivergence,
    RealProviderOracleMismatch,
    CleanupToBaselineFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabObservation {
    pub observation_id: String,
    pub seed_id: Option<String>,
    pub expected_outcome: String,
    pub observed_outcome: String,
    pub principal_expected: Option<String>,
    pub principal_dispatched: Option<String>,
    pub eligible_metrics: BTreeSet<LabMetricKind>,
    pub failure_flags: BTreeSet<LabFailureFlag>,
    pub evidence_refs: BTreeSet<String>,
    pub provider_backed: bool,
    pub comparison_profile_revision: String,
    pub logical_sequence: u64,
}
```

Each failure flag maps to exactly one metric kind. A failure flag whose metric is not in `eligible_metrics` is a typed error; this prevents a numerator with no denominator opportunity.

- [ ] **Step 1: Write RED typed-accounting tests**

Include one observation per failure flag and assert exact numerator/denominator. Specifically require:

```text
one WPDR-eligible + WrongPrincipalDispatch -> WPDR 1/1
one UOBRR-eligible + BlindRetryAfterUnknown -> UOBRR 1/1
one SUAR-eligible observed_outcome="INCONCLUSIVE" with no SilentUnsoundAction -> SUAR 0/1
failure flag without matching eligibility -> typed error
five required metrics measured 0 -> gate PASS
one required metric 1/N -> gate FAIL
required metric absent -> gate INCOMPLETE
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-validation-lab --test metrics_authority -- --nocapture
```

- [ ] **Step 3: Implement reducer with checked counters**

Start every kind at `0/0`. For each observation, increment denominators for `eligible_metrics`. Increment numerator only for mapped failure flags. Use `checked_add`; overflow returns `MetricCounterOverflow`. Reject `failure_flags` that lack matching eligibility before mutating counts.

- [ ] **Step 4: Run GREEN**

```bash
cargo test -p localview-validation-lab --test metrics_authority -- --nocapture
cargo test -p localview-validation-lab --test run_authority_lifecycle -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add crates/validation-lab/src/metrics.rs crates/validation-lab/src/result.rs crates/validation-lab/tests
git commit -m "feat(v43): reduce typed validation observations into metrics"
```

---

### Task 7: Canonical Artifact Envelopes and Shipping Dependency Exclusion

**Files:**
- Create: `crates/validation-lab/src/artifact.rs`
- Modify: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/artifact_contract.rs`
- Create: `crates/validation-lab/tests/shipping_dependency_exclusion.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabArtifactKind {
    Preregistration,
    SeedCatalog,
    Results,
    MutationReport,
    DifferentialReport,
    CoverageReport,
    Environment,
    Counterexamples,
    MinimizedSeeds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabArtifactState { Present, NotApplicable, NotRun }

pub struct CanonicalArtifact {
    pub kind: LabArtifactKind,
    pub canonical_bytes: Vec<u8>,
    pub digest: CanonicalDigest,
}
```

Only preregistration, seed-catalog and results get first-slice concrete serialization helpers. The other kinds can appear in a manifest with `NotApplicable` or `NotRun`; do not emit fabricated empty report payloads.

- [ ] **Step 1: Write RED artifact tests**

Require:

```text
Preregistration/SeedCatalog/Results -> canonical bytes + digest stable under repeated serialization
NotRun mutation report has no payload/digest and never looks like a zero-result success artifact
artifact digest changes if semantic payload changes
artifact envelope serialization never includes ResearchResultClass::Proved because no such variant exists
```

- [ ] **Step 2: Write RED repository dependency contract**

`shipping_dependency_exclusion.rs` resolves repo root from `env!("CARGO_MANIFEST_DIR")` and scans exact shipping manifests:

```text
apps/daemon/Cargo.toml
apps/cli/Cargo.toml
apps/desktop/src-tauri/Cargo.toml
crates/native-provider/Cargo.toml
crates/windows-uia-provider/Cargo.toml
crates/windows-observe-runtime/Cargo.toml
crates/observation/Cargo.toml
crates/control/Cargo.toml
```

For each file:

```rust
assert!(!text.contains("localview-validation-lab"));
```

The test also inspects `Cargo.toml` workspace membership and requires the lab crate to be a member; being a workspace member is not considered a shipping dependency.

- [ ] **Step 3: Run RED**

```bash
cargo test -p localview-validation-lab --test artifact_contract -- --nocapture
cargo test -p localview-validation-lab --test shipping_dependency_exclusion -- --nocapture
```

- [ ] **Step 4: Implement artifact module and make both contracts GREEN**

```bash
cargo test -p localview-validation-lab --test artifact_contract -- --nocapture
cargo test -p localview-validation-lab --test shipping_dependency_exclusion -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add crates/validation-lab/src/artifact.rs crates/validation-lab/src/lib.rs crates/validation-lab/tests
git commit -m "feat(v43): add canonical validation lab artifacts"
```

---

### Task 8: Named Cross-Platform CI Contract and Exact-Head Closure

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: PR #104 body/evidence only after runs exist.

**Interfaces:**
- Adds one fast named step to the existing `rust-core` matrix; does not create a heavy lab workflow.

- [ ] **Step 1: Add the permanent named CI step after generic workspace Tests**

```yaml
      - name: V4.3 validation lab authority contract
        run: cargo test -p localview-validation-lab -- --nocapture
```

The step runs on Ubuntu, Windows and macOS because it lives inside the existing `rust-core` matrix.

- [ ] **Step 2: Run focused full crate verification before commit**

```bash
cargo fmt --all -- --check
cargo check -p localview-validation-lab --all-targets
cargo clippy -p localview-validation-lab --all-targets -- -D warnings
cargo test -p localview-validation-lab -- --nocapture
```

Expected: all pass.

- [ ] **Step 3: Commit CI wiring**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(v43): gate validation lab authority cross-platform"
```

- [ ] **Step 4: Exact-head PR verification**

Lock PR head SHA and require on that exact SHA:

```text
CI / Rust core (ubuntu-latest) = SUCCESS
CI / Rust core (windows-latest) = SUCCESS
CI / Rust core (macos-latest) = SUCCESS
V4.3 validation lab authority contract = SUCCESS in all three jobs
Tauri/frontend and native GUI smoke jobs = SUCCESS or demonstrably unaffected existing gates
no temporary workflow/script in PR diff
```

Do not call the slice complete while a required exact-head run is queued/in-progress/cancelled/action_required.

- [ ] **Step 5: Scope/review gate**

Audit PR changed files. The authority-core implementation should be limited to:

```text
Cargo.toml
crates/validation-lab/**
.github/workflows/ci.yml
docs/superpowers/specs/2026-09-10-v43-validation-lab-authority-core*.md
docs/superpowers/plans/2026-09-10-v43-validation-lab-authority-core.md
```

Any runtime/provider implementation change requires a new explicit reason and RED evidence; otherwise remove it from this PR.

Check submitted reviews and unresolved review threads. Do not claim independent review if none exists.

- [ ] **Step 6: Exact-head merge and post-merge verification**

Merge only with expected-head lock after exact-head gates are GREEN. Re-lock `main` to the returned merge SHA and require fresh push-triggered CI on that exact merge SHA before declaring the authority-core slice closed.

- [ ] **Step 7: Record closure without overclaim**

PR/body/evidence must say this PR establishes the **Validation Lab authority core only**. It must not claim L0–L9 completion, formal proof, real-provider validation, mutation closure, bounded-state closure, or V4.3 overall completion.

---

## Self-Review Checklist Applied to This Plan

### Spec coverage

- Result-strength taxonomy: Task 5.
- Prediction-before-test and two-phase persisted preregistration: Tasks 3 and 5.
- Oracle correction provenance: Task 4.
- Lab revision/completed-run identity without circular digest: Tasks 3 and 5.
- Machine-readable seeds: Task 4.
- Canonical artifacts: Task 7.
- All 14 V4.3 metrics: Tasks 1, 2, and 6.
- Silent-unsoundness zero-target gate: Tasks 2 and 6.
- Conservative unknown/inconclusive handling: Task 6.
- Heavy-lab shipping exclusion: Task 7.
- Cross-platform determinism/CI: Tasks 3 and 8.
- L2/L4 reuse: explicitly deferred to later adapter PRs; no duplicate engine is introduced here.

### Type consistency

- `CanonicalDigest` is introduced in Task 3 and reused by Tasks 4–7.
- `LabSeedIdentity` is introduced in Task 4 and referenced by preregistration/result APIs; implementation order must add the module/export before compiling the Task 3 final form if the concrete Rust compiler requires it. If needed, Task 3 may use an internal string seed identity placeholder only on the RED head, but GREEN must use the exact Task 4 type after Task 4 lands; do not ship two identity types.
- `LabMetricKind` and metric gate types are introduced in Task 2 and reused thereafter.
- `PersistedPreregistrationReceipt` is the only prospective persistence authority; `PreparedPreregistration` is never sufficient.
- `ResearchResultClass` has no `Proved` variant.

### Placeholder scan

This plan intentionally contains no TODO/TBD implementation placeholders. Future L2–L9 campaigns are explicit non-goals of this PR, not omitted work inside the authority-core contract.

## Execution Mode

User has already requested continued implementation in this session, so execute inline with `superpowers:executing-plans`. Preserve permanent RED commits and do not skip exact-head CI checkpoints.