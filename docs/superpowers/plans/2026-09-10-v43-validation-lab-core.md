# V4.3 Validation Lab Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the minimal deterministic research-authority core required to make later V4.3 validation campaigns preregistered, provenance-bound, and metrically honest.

**Architecture:** Add a standalone `localview-validation-lab` workspace crate that exposes pure data contracts and deterministic aggregation/digest functions. Production/runtime crates do not depend on it. TDD starts with an integration-test-only RED commit, then the minimal library implementation is added in GREEN.

**Tech Stack:** Rust 2024, serde, serde_json, sha2, thiserror; existing Cargo workspace.

**Spec:** `docs/superpowers/specs/2026-09-10-v43-validation-lab-core-design.md`

## Global Constraints

- No OS, browser, provider, network, AI/model, filesystem, wall-clock, UUID generation, or hidden retry logic in `localview-validation-lab`.
- No existing production/runtime crate may gain a dependency on `localview-validation-lab`.
- All digest-affecting collections must have deterministic ordering.
- Zero metric denominator means `NotMeasured`, never a zero rate.
- Silent-unsoundness zero-target metrics are SUAR, WPDR, PILR, PDMR, and UOBRR only in this slice.
- No production code is added before the failing integration tests are committed and observed RED.

---

### Task 1: Create the test-only RED contract

**Files:**
- Create: `crates/validation-lab/Cargo.toml`
- Create: `crates/validation-lab/src/lib.rs`
- Create: `crates/validation-lab/tests/core_contract.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: workspace `serde`, `serde_json`, `sha2`, `thiserror` dependencies.
- Produces: compile-time expectations for `ResearchResultClass`, `LabRevisionIdentity`, `LabPreregistration`, `LabSeed`, `ComparisonMode`, `RiskLevel`, `LabMetricKind`, `MetricEvent`, `MetricMeasurementStatus`, `MetricSummary`, `SilentUnsoundnessVerdict`, `LabSeedResult`, `aggregate_metrics`, `silent_unsoundness_verdict`, `preregistration_digest`, `classify_seed_result`, and `result_digest`.

- [ ] **Step 1: Add the crate manifest and empty library**

`crates/validation-lab/Cargo.toml`:

```toml
[package]
name = "localview-validation-lab"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
thiserror = { workspace = true }
```

`crates/validation-lab/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

Add `"crates/validation-lab",` to the root workspace members.

- [ ] **Step 2: Write failing integration tests**

Create `crates/validation-lab/tests/core_contract.rs` with tests that import the public API listed above and assert:

```rust
assert_eq!(LabMetricKind::ALL.len(), 14);
```

```rust
let summaries = aggregate_metrics(&[]);
let suar = summaries.get(&LabMetricKind::Suar).unwrap();
assert_eq!(suar.status, MetricMeasurementStatus::NotMeasured);
assert_eq!(suar.rate(), None);
```

```rust
let summaries = aggregate_metrics(&[MetricEvent {
    kind: LabMetricKind::Wpdr,
    eligible: true,
    violation: true,
}]);
assert_eq!(summaries[&LabMetricKind::Wpdr].numerator, 1);
assert_eq!(summaries[&LabMetricKind::Wpdr].denominator, 1);
assert_eq!(silent_unsoundness_verdict(&summaries), SilentUnsoundnessVerdict::Fail);
```

Create helper events measuring SUAR/WPDR/PILR/PDMR/UOBRR with zero violations and assert `Pass`; omit one target metric and assert `Inconclusive`.

Create two otherwise identical preregistrations that differ only in `expected_distinction`; assert both digests succeed and differ.

Call `classify_seed_result(true, Some("actual"), "expected")` and assert mismatch gives `ExploratoryObservation`; exact match gives `PreregisteredSeedPass`; missing digest gives `ExploratoryObservation`.

Create two `LabSeedResult` values that differ only in provenance and assert `result_digest` differs.

Serialize every `ResearchResultClass` variant and assert no serialized value contains `proved`.

- [ ] **Step 3: Commit test-only RED**

Commit only workspace membership, crate manifest, empty library, and integration tests:

```text
test(v43): define validation lab core RED contract
```

- [ ] **Step 4: Verify RED on exact test-only SHA**

Run/observe CI for the exact commit. Expected failure: unresolved imports/types/functions from `localview_validation_lab`; failures must originate from `core_contract.rs`, not manifest syntax or unrelated existing tests.

---

### Task 2: Implement deterministic lab contracts and aggregation

**Files:**
- Modify: `crates/validation-lab/src/lib.rs`

**Interfaces:**
- Produces the public API compiled by Task 1.

- [ ] **Step 1: Add public enums and data contracts**

Implement `ResearchResultClass` with serde `snake_case`; `LabMetricKind` with fourteen variants and `pub const ALL: [Self; 14]`; `MetricMeasurementStatus`; `SilentUnsoundnessVerdict`; `ComparisonMode::Exact`; `RiskLevel`; `LabRevisionIdentity`; `LabPreregistration`; `LabSeed`; `MetricEvent`; `MetricSummary`; and `LabSeedResult`.

Use `BTreeMap`/`BTreeSet` where key ordering affects canonical serialization.

- [ ] **Step 2: Implement metric aggregation**

`aggregate_metrics(events: &[MetricEvent]) -> BTreeMap<LabMetricKind, MetricSummary>` must initialize all fourteen metrics, increment denominator only when `eligible`, increment numerator only when both `eligible && violation`, and set status from denominator.

`MetricSummary::rate() -> Option<f64>` returns `None` for denominator zero and `Some(numerator as f64 / denominator as f64)` otherwise.

- [ ] **Step 3: Implement zero-target verdict**

`silent_unsoundness_verdict(&BTreeMap<LabMetricKind, MetricSummary>) -> SilentUnsoundnessVerdict` examines SUAR/WPDR/PILR/PDMR/UOBRR. Any measured numerator > 0 returns `Fail`; otherwise any absent/unmeasured target returns `Inconclusive`; otherwise return `Pass`.

- [ ] **Step 4: Implement deterministic digest helpers**

Define `LabEncodingError` wrapping `serde_json::Error`. Serialize deterministic structs to bytes with `serde_json::to_vec`, hash with SHA-256, and lowercase hex encode without adding a new hex dependency.

Expose:

```rust
pub fn preregistration_digest(value: &LabPreregistration) -> Result<String, LabEncodingError>
pub fn result_digest(value: &LabSeedResult) -> Result<String, LabEncodingError>
```

- [ ] **Step 5: Implement prospective classification**

```rust
pub fn classify_seed_result(
    passed: bool,
    used_preregistration_digest: Option<&str>,
    expected_preregistration_digest: &str,
) -> ResearchResultClass
```

Return `PreregisteredSeedPass` only for `passed == true` and exact non-empty digest equality. A passing observation without exact authority returns `ExploratoryObservation`; a failing observation returns `CounterexampleFound` regardless of preregistration validity so counterexamples are not hidden by ceremony state.

- [ ] **Step 6: Run focused and workspace verification**

Run:

```text
cargo test -p localview-validation-lab
cargo clippy -p localview-validation-lab --all-targets -- -D warnings
cargo test --workspace
```

Expected: GREEN.

- [ ] **Step 7: Commit GREEN**

```text
feat(v43): add validation lab core authority
```

---

### Task 3: Add permanent CI boundary evidence

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`

**Interfaces:**
- Consumes: GREEN `localview-validation-lab` crate.
- Produces: named CI evidence that the crate passes and remains outside shipping dependency manifests.

- [ ] **Step 1: Add named validation-lab core CI gate**

Add a Rust-core step after general tests:

```text
V4.3 validation lab core contract
```

running:

```text
cargo test -p localview-validation-lab
```

- [ ] **Step 2: Add dependency-direction guard**

In the same step or a dedicated script-free shell block, fail if any `Cargo.toml` outside `crates/validation-lab` declares `localview-validation-lab` as a dependency. Exclude the workspace member path itself from the search. This guard is evidence for the heavy-lab-not-shipping boundary, not a claim that the whole Lab is complete.

- [ ] **Step 3: Document landed scope precisely**

Update `docs/IMPLEMENTATION_STATUS.md` to state that the model-free Validation Lab authority core is landed/under verification, while runners/campaigns L0-L9 remain incomplete.

- [ ] **Step 4: Verify exact-head CI**

Observe all required CI jobs on the new exact SHA; do not declare the slice complete until CI is GREEN on that SHA.

- [ ] **Step 5: Open draft PR with RED→GREEN lineage**

PR body must record base SHA, test-only RED SHA and failure evidence, GREEN SHA, exact-head verification state, dependency-boundary scope, and explicit out-of-scope layers.
