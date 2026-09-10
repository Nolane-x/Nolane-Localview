# V4.3 Validation Lab Authority Core — Design

Date: 2026-09-10

Status: Design for implementation review

Branch: `feat/v43-validation-lab-authority-core`

Base: `main@9df2d2ae4817a1f85c3fef4e114db4ebbb9e0352`

## 1. Purpose

V4.3 requires falsification infrastructure, not merely additional runtime features. The Validation Lab must make correctness-critical semantics executable as bounded, reproducible research claims with machine-readable provenance. A clean run must mean exactly what was executed and nothing stronger; a counterexample must remain durable evidence rather than disappear into CI logs.

This design introduces a **thin Validation Lab authority core** above existing deterministic primitives. It does not reimplement mutation testing, state-space compilation, provider logic, or runtime verification. It creates the research-evidence boundary that binds preregistration, seed identity, observations, metric classification, and result strength.

The first implementation slice intentionally stops before L2–L9 campaign orchestration. Its job is to make later campaigns impossible to overclaim or misclassify.

## 2. Source-of-truth requirements

This design implements the authority/provenance foundation required by the V4.2/V4.3 specification, especially:

- Research Result Strength Taxonomy (§1003).
- Prediction-Before-Test Rule (§1004).
- Failed / Corrected Oracle Provenance (§1005).
- V4.2 Lab Artifact Set (§1006).
- Lab Revision Identity (§1007).
- Seed Object Contract (§1008).
- Seed Corpus Promotion (§1010).
- Counterexample Is Stronger Than Clean Bound (§1014).
- Reference Reducer constraints (§1015).
- Lab Layer 0–9 architecture (§1027 onward).
- V4.3 Core Lab Metrics (§1337).
- Silent Unsoundness Target (§1338).
- Definition of Done — V4.3 Lab (§1384).

The design preserves the specification rule that lab artifacts are research records and production LocalView must not depend on their presence on a user machine.

## 3. Architectural classification

This is an architectural change because no existing crate owns the complete lifecycle of lab preregistration, immutable run identity, result-strength classification, or metric authority. Existing crates provide useful pieces but do not own this boundary.

The selected architecture is:

```text
V4.3 spec / campaign definition
        |
        v
localview-validation-lab
  - preregistration authority
  - immutable revision identity
  - seed/result contracts
  - metric accounting
  - result-strength classification
  - canonical artifact serialization/digests
        |
        +---------------------+
        |                     |
        v                     v
localview-mutation      localview-state-space
(existing L2 primitive) (existing L4 primitive)
        |
        +---------- future adapters ----------+
                   L3/L5/L6/L7/L8/L9

Shipping daemon / desktop / runtime
        X
        |
        no dependency on validation-lab
```

The lab crate may depend on pure/shared contract crates when required, but shipping runtime crates must not depend on the lab crate.

## 4. Existing primitives to reuse

### 4.1 `localview-mutation`

The repository already has deterministic mutation semantics:

- mutation classes and operators;
- `MutationCase`;
- `MutationOutcome`;
- `MutationVerdict::{Killed, Survived, Invalid, SkippedUnsafe}`;
- `VerificationQualityMap` including overall/weighted kill rates and survivors.

The Validation Lab must reuse these semantics for future L2 integration. It must not create a second mutation verdict model.

### 4.2 `localview-state-space`

The repository already has a deterministic bounded state-space compiler with:

- dimensions and values;
- constraints;
- risk weighting and boundary values;
- bounded `max_states` selection;
- deterministic state keys and pair coverage.

The Validation Lab must reuse this compiler for future L4 bounded-state campaigns rather than inventing a competing state enumerator.

### 4.3 Reports and quality crates

`localview-reports` and `localview-quality` remain product/reporting surfaces. They are not the authority for research result strength. A future exporter may convert lab artifacts into human-readable reports, but the canonical result must be defined by the lab contracts first.

## 5. Core invariants

The first implementation must preserve all of the following.

### I1 — No prospective claim without preregistration

A run can be classified as prediction-valid only if the expected distinction, seed corpus revision, bound, random source profile, and comparison profile are committed to a canonical preregistration artifact before execution begins.

If this condition is not satisfied, the run may still execute but its maximum result class is `EXPLORATORY_OBSERVATION`.

### I2 — Result strength is typed and non-escalating

Every result has exactly one research-strength class from the specification taxonomy:

- `EXPLORATORY_OBSERVATION`
- `PREREGISTERED_SEED_PASS`
- `COUNTEREXAMPLE_FOUND`
- `NO_COUNTEREXAMPLE_WITHIN_BOUND_N`
- `MUTANT_KILLED`
- `MUTANT_SURVIVED`
- `DIFFERENTIAL_EQUIVALENT_WITHIN_VECTOR_SET`
- `DIFFERENTIAL_DIVERGENCE_FOUND`
- `PROPERTY_CAMPAIGN_PASS_N`
- `REAL_PROVIDER_INTEGRATION_PASS`
- `INDEPENDENT_REPLICATION_PASS`

No type or renderer may equate these with `PROVED`.

### I3 — Unknown and conservative blocks are not silent unsoundness

The metric layer must distinguish wrong success from explicit non-success. The following conservative outcomes are not counted as silent unsoundness merely because the desired action did not complete:

- `UNKNOWN`
- `INCONCLUSIVE`
- `RECONCILIATION_REQUIRED`
- `UNSUPPORTED`
- conservative block/rejection

A metric numerator increases only when its exact failure predicate is satisfied.

### I4 — Zero observations are not zero failures

A metric with denominator `0` is `NOT_MEASURED`, never `0.0` and never a passing rate.

This prevents an empty campaign from satisfying a release target accidentally.

### I5 — Counterexamples outrank clean bounded runs

A valid counterexample is retained as direct evidence against the relevant invariant for that fixture. A clean bounded run states only that no counterexample was found within the declared model/bound/assumptions.

### I6 — Oracle correction is append-only provenance

If an oracle is later shown to be wrong, the original oracle revision and result remain addressable. The correction creates a new oracle revision and requires a new preregistration. History must not be rewritten to make a dashboard green.

### I7 — Lab artifacts are canonical and content-bound

Run identity and result identity are not file names. Canonical serialized content is hashed. Any relevant semantic change changes the digest.

### I8 — Heavy lab is excluded from shipping authority

The daemon, desktop app, provider runtime, observation runtime, and normal CLI runtime must not require lab artifacts or the Validation Lab crate to function.

## 6. Proposed crate

Create:

```text
crates/validation-lab/
  Cargo.toml
  src/
    lib.rs
    canonical.rs
    identity.rs
    preregistration.rs
    seed.rs
    result.rs
    metrics.rs
    artifact.rs
```

The crate name should be `localview-validation-lab`.

It is a workspace development/research crate. It must remain pure Rust with `#![forbid(unsafe_code)]` and no OS/browser/model calls in the authority core.

The first slice should depend only on small deterministic shared dependencies such as `serde`, `serde_json`, and `sha2` unless an existing repository contract type is required. L2/L4 dependencies on `localview-mutation` and `localview-state-space` should be introduced only when their adapters are implemented, not preemptively.

## 7. Data contracts

### 7.1 `ResearchResultClass`

A closed enum matching the specification taxonomy exactly. Serialization uses stable snake_case wire names. Unknown future classes must fail deserialization rather than silently collapse into an existing class.

### 7.2 `LabRevisionIdentity`

The identity object binds:

```text
lab_revision
seed_corpus_revision
spec_revision_digest
reference_reducer_revision
mutation_catalog_revision
comparison_profile_revision
random_source_profile
platform_profile?        # required for provider-backed campaigns
start_sequence
```

`result_artifact_digest` is not an input to preregistration identity because that would be circular. It is added only to the completed run receipt after canonical result serialization.

Every field except optional `platform_profile` is non-empty and normalized structurally, not semantically. The lab does not trim or reinterpret arbitrary revision labels after construction.

### 7.3 `LabSeed`

Minimum fields:

```text
seed_id
family
spec_surface_refs[]
input_fixture
expected_semantic_outcome
forbidden_outcomes[]
comparison_mode
risk_if_missed
prediction_revision
oracle_revision
```

`input_fixture` is a canonical JSON value in the authority core. Typed campaign adapters may wrap richer domain objects, but the serialized lab seed must preserve an exact canonical representation.

Seeds are immutable by `(seed_id, prediction_revision, oracle_revision)` within a corpus revision. Reusing the same identity with different canonical content is an error.

### 7.4 `LabPreregistration`

Contains:

- `LabRevisionIdentity` excluding result digest;
- ordered seed identities or a seed-corpus digest;
- declared campaign layer/type;
- expected distinctions;
- model/state bound when applicable;
- random seed/source profile;
- comparison profile revision;
- assumptions;
- declared metric set;
- creation sequence.

The preregistration exposes a canonical digest. Execution APIs require the digest, not a mutable object reference.

### 7.5 `LabObservation`

An observation is the smallest exact claim used by metric reducers. It records:

```text
observation_id
seed_id?
principal_expected?
principal_dispatched?
expected_outcome
observed_outcome
failure_flags[]
evidence_refs[]
provider_backed
comparison_profile_revision
```

The core does not infer high-level failure semantics from free-form strings. Campaign adapters must set typed failure flags based on domain-specific evidence.

### 7.6 `LabResult`

A result binds:

- preregistration digest;
- full revision identity;
- research result class;
- executed seed identities;
- observation digests;
- metric snapshot;
- counterexample refs;
- minimized-seed refs;
- assumptions actually used;
- bound actually used;
- start/end logical sequence;
- environment artifact digest when applicable;
- canonical result digest.

A run that violates its preregistered comparison/bound inputs must not retain a prospective result class. It is downgraded to `EXPLORATORY_OBSERVATION` with an explicit mismatch reason.

## 8. Canonical serialization and digest rules

Research authority must not depend on map insertion order or pretty-printing. The crate therefore owns one canonical JSON encoding path.

Rules:

1. object keys are recursively sorted lexicographically;
2. arrays preserve declared semantic order unless the contract explicitly defines set semantics;
3. set-like fields are normalized into sorted unique arrays before canonicalization;
4. floating-point values are prohibited in identity, seed authority, preregistration, and metric counts unless a future contract explicitly defines canonical float semantics;
5. UTF-8 strings are preserved byte-for-byte after JSON decoding; no Unicode normalization is applied;
6. digests use SHA-256 over canonical UTF-8 bytes with a versioned domain prefix such as `localview-validation-lab/v1\0`.

The canonicalization version is part of the contract. A future incompatible canonicalization requires a new version, not an in-place semantic change.

## 9. Metric authority

### 9.1 Metric kinds

The core exposes all V4.3 metrics:

- `SUAR` — Silent Unsound Action Rate
- `WPDR` — Wrong-Principal Dispatch Rate
- `PILR` — Principal Information Leak Rate
- `EOFFR` — Event-Only False Freshness Rate
- `RMR` — Reconciliation Miss Rate
- `PIAER` — Provider-ID ABA Escape Rate
- `WFIR` — Wrong-Foreground Input Rate
- `PDMR` — Partial-Dispatch Misclassification Rate
- `SCAR` — Stale-Cache Authority Rate
- `UOBRR` — Unknown-Outcome Blind Retry Rate
- `MSR` — Mutation Survival Rate
- `CRDR` — Cross-Reducer Divergence Rate
- `RPOMR` — Real-Provider Oracle Mismatch Rate
- `CBFR` — Cleanup-to-Baseline Failure Rate

### 9.2 Metric value

Each metric snapshot stores integer authority, not only a float:

```text
kind
numerator: u64
denominator: u64
status: MEASURED | NOT_MEASURED
rate_ppb?: u64
```

`rate_ppb` is parts-per-billion computed with checked integer arithmetic when denominator > 0. This avoids float nondeterminism in canonical research artifacts while retaining exact numerator/denominator.

For denominator `0`:

```text
status = NOT_MEASURED
rate_ppb = absent
```

### 9.3 Silent-unsoundness release gate

A `SilentUnsoundnessGate` evaluates the required zero-target subset:

```text
SUAR
WPDR
PILR
PDMR
UOBRR
```

For each required metric:

- measured numerator `0` => PASS for that metric;
- measured numerator `>0` => FAIL;
- not measured => INCOMPLETE, not PASS.

Overall gate:

```text
FAIL       if any required metric fails
INCOMPLETE if none fail but one or more are not measured
PASS       only if all required metrics are measured and zero
```

This directly prevents an empty corpus from creating a false V4.3 closure signal.

### 9.4 Observation classification

The core metric reducer consumes typed flags, not human messages. Examples:

- `wrong_principal_dispatch` increments WPDR numerator and dispatch-attempt denominator;
- `principal_information_leak` increments PILR numerator and principal-sensitive observation denominator;
- `blind_retry_after_unknown` increments UOBRR numerator and unknown-outcome retry-opportunity denominator;
- `partial_dispatch_misclassified_success` increments PDMR numerator and partial-dispatch denominator;
- a conservative `INCONCLUSIVE` with no dispatch-as-success flag does not increment SUAR.

Detailed predicates for each metric belong to their campaign adapters and permanent seed suites. The core guarantees deterministic accounting once the typed classification is supplied.

## 10. Artifact model

The authority core defines canonical schemas and helper writers/readers for the specification artifact set:

```text
LAB-PREREGISTRATION.json
LAB-SEED-CATALOG.json
LAB-RESULTS.json
LAB-MUTATION-REPORT.json
LAB-DIFFERENTIAL-REPORT.json
LAB-COVERAGE-REPORT.json
LAB-ENVIRONMENT.json
LAB-COUNTEREXAMPLES/
LAB-MINIMIZED-SEEDS/
```

The first implementation slice needs to fully support canonical preregistration, seed catalog, and results. It may define typed placeholders for future report artifact kinds only as enum variants/manifest entries; it must not emit fabricated empty reports for layers that did not run.

Artifact manifests distinguish:

- `PRESENT`
- `NOT_APPLICABLE`
- `NOT_RUN`

They do not treat an absent L2/L6 report as a successful empty campaign.

## 11. Lifecycle

### 11.1 Preregister

1. Build immutable seed catalog.
2. Validate duplicate seed identities.
3. Build `LabPreregistration` with exact revisions/bounds/comparison profile.
4. Canonicalize and digest it.
5. Persist preregistration artifact before campaign execution.
6. Return `PreregistrationReceipt { digest, logical_sequence }`.

### 11.2 Execute

The authority core itself does not execute OS/provider actions. A campaign adapter receives only an immutable preregistration receipt plus its typed fixture inputs.

### 11.3 Record observations

Campaign adapters submit typed observations. Observations are append-only within a run builder and receive canonical digests.

### 11.4 Finalize

Finalization:

1. verifies preregistration digest and declared campaign inputs still match;
2. deterministically reduces metrics;
3. evaluates any applicable release gate;
4. derives the strongest permitted result class from execution facts without escalation;
5. canonicalizes `LabResult`;
6. computes `result_artifact_digest`;
7. emits a completed run receipt.

After finalization, the run builder cannot accept more observations.

## 12. Failure semantics

The authority core must fail closed on:

- duplicate seed identity with different content;
- empty required revision identities;
- canonicalization failure;
- preregistration digest mismatch;
- execution input drift from preregistration;
- counter overflow in metrics;
- invalid metric numerator greater than denominator where the metric contract defines the numerator as a subset;
- prospective result requested without valid preregistration;
- provider-backed prospective result without platform profile;
- result finalization attempted twice;
- observation appended after finalization.

Research execution may continue as exploratory when preregistration validity is absent, but the system must encode that downgrade explicitly. It must never silently preserve a stronger class.

## 13. TDD plan for the first implementation slice

Implementation must follow permanent RED -> GREEN lineage.

### RED-A — Metric completeness and zero-denominator safety

Permanent tests require:

- all 14 `LabMetricKind` values exist;
- a fresh metric snapshot has `denominator = 0`, `status = NOT_MEASURED`, and no rate;
- the silent-unsoundness gate over an empty snapshot is `INCOMPLETE`, never `PASS`.

Expected initial RED: crate/types do not exist.

### RED-B — Exact wrong-principal accounting

A fixture with one dispatch opportunity and one wrong-principal dispatch must produce:

```text
WPDR numerator = 1
WPDR denominator = 1
WPDR rate_ppb = 1_000_000_000
silent-unsoundness gate = FAIL
```

### RED-C — Conservative inconclusive is not SUAR

A fixture whose observed outcome is `INCONCLUSIVE` and has no unsound-success classification must not increment SUAR numerator. It still contributes only to the denominator defined by its campaign metric profile when applicable.

### RED-D — Preregistration digest is semantic

Changing any of the following must change the preregistration digest:

- expected distinction;
- seed corpus revision;
- model/state bound;
- comparison profile revision;
- random source profile.

Reordering canonical set-like fields must not change the digest.

### RED-E — Prospective claim downgrade

A result requested as `PREREGISTERED_SEED_PASS` without a valid preregistration receipt must finalize as `EXPLORATORY_OBSERVATION` or fail with a typed authority error; implementation will choose one invariant and apply it consistently. The selected contract for this design is **typed downgrade with an explicit downgrade reason**, because the specification explicitly permits exploratory execution after preregistration failure.

### RED-F — Oracle revision immutability

Two seeds with the same `(seed_id, prediction_revision, oracle_revision)` and different canonical expected outcomes must be rejected as an identity collision. A corrected oracle uses a new `oracle_revision` and both revisions remain addressable.

### RED-G — Shipping dependency exclusion

A repository-level contract test verifies that shipping members (`apps/daemon`, `apps/desktop/src-tauri`, runtime/provider crates) do not list `localview-validation-lab` as a dependency. Future lab CLI/tools may depend on it; production authority may not.

## 14. Implementation boundaries for the first PR

The first PR will implement only:

1. `localview-validation-lab` crate and workspace registration;
2. canonical JSON + SHA-256 domain-separated digests;
3. research result taxonomy;
4. revision identity;
5. machine-readable seed catalog and oracle revision collision checks;
6. preregistration and receipt;
7. observation/result lifecycle;
8. all 14 metric kinds and deterministic integer accounting;
9. silent-unsoundness zero-target gate;
10. canonical preregistration/seed/result artifact serialization;
11. permanent tests including shipping-dependency exclusion.

The first PR will **not** implement campaign engines for L2 mutation, L3 property generation, L4 bounded temporal search, L5 concurrency exploration, L6 differential reducers, L7 fake provider, L8 real provider, or L9 resource stress. Those become separate TDD slices after this authority core is exact-head GREEN.

## 15. Future integration order

After the authority core is verified, the recommended sequence is:

1. **L1 deterministic semantic seed runner** — cheapest end-to-end consumer of preregistration + seed + result authority.
2. **L2 mutation adapter** — consume `localview-mutation` outcomes and produce MSR plus survivor/counterexample artifacts.
3. **L4 bounded-state adapter** — consume `localview-state-space` plans/results with exact bound provenance.
4. **L3 property/metamorphic campaigns** — deterministic random-source profiles and counterexample minimization.
5. **L6 differential reducer harness** — CRDR and first-divergence artifacts.
6. **L7 fake-provider campaigns** — provider lifecycle/freshness/principal faults without OS uncertainty.
7. **L5 concurrency exploration** — deterministic scheduler over narrow authority/resource models.
8. **L8 real-provider seeds** — platform-profile-bound runs and RPOMR.
9. **L9 resource/failure campaigns** — CBFR, leak/cleanup/recovery and long-lived stress.

This order deliberately obtains deterministic semantic evidence before adding OS/provider nondeterminism.

## 16. CI strategy

The first PR should add one named, fast cross-platform gate such as:

```text
V4.3 validation lab authority contract
```

It runs the authority-core tests on Ubuntu, macOS, and Windows as part of existing Rust core CI.

Heavy future campaigns must use dedicated workflows or explicit manual/scheduled lanes. They must not make every normal code push run million-case searches or real-provider ceremonies.

CI output must never translate a bounded clean result into `PROVED`.

## 17. Security and privacy

The authority core must not require plaintext user secrets, desktop-wide capture, provider credentials, or arbitrary environment dumps.

Provider-backed future `LAB-ENVIRONMENT.json` data must use explicit allowlisted fields. Secret-bearing environment variables, tokens, clipboard data, password fields, and raw private UI content are outside this core contract.

Seed fixtures that intentionally contain secret-like synthetic values must be clearly synthetic and must not be sourced from real user state.

## 18. Performance and resident-runtime policy

The authority core is not resident work. No observer thread, background timer, provider crawl, screen recording, or model process is introduced by this design.

Normal LocalView startup must perform zero Validation Lab work unless a future explicit lab command/workflow invokes it.

## 19. Alternatives rejected

### Monolithic L0–L9 implementation

Rejected because it would combine research authority, generators, mutation, state exploration, concurrency, fake/real providers, and stress into one unreviewable change. RED -> GREEN lineage and root-cause attribution would become weak.

### Embed lab authority in `localview-quality`

Rejected because product quality findings and research result strength have different authority semantics. A UI quality warning is not a preregistered falsification result.

### Embed lab authority in `localview-mutation`

Rejected because mutation is only one lab layer. Doing so would make mutation semantics the accidental owner of differential, provider, freshness, reconciliation, and cleanup research evidence.

## 20. Acceptance criteria for authority-core completion

The authority-core slice is complete only when all of the following are true on one exact PR head:

- permanent RED lineage exists for the new contracts;
- all first-slice tests are GREEN on Ubuntu, macOS, and Windows;
- canonical digest tests are deterministic across platforms;
- all 14 V4.3 metrics are represented;
- zero-denominator metrics are `NOT_MEASURED`;
- empty silent-unsoundness evidence cannot pass the gate;
- wrong-principal and blind-retry fixtures fail the zero-target gate;
- conservative `INCONCLUSIVE` does not become silent unsoundness by default;
- preregistration drift prevents prospective classification;
- oracle correction preserves revision history;
- result classes never render or serialize as `PROVED`;
- shipping runtime crates have no dependency on `localview-validation-lab`;
- no temporary probe workflow/script remains in the branch;
- PR exact head is verified before merge;
- post-merge `main` is re-verified before the slice is called closed.

## 21. Non-goals

This slice does not claim that V4.3 is fully validated. It creates the authority needed to make future validation claims meaningful.

It does not:

- prove LocalView correct;
- implement every L0–L9 campaign;
- replace existing runtime tests;
- turn model checking into a claim of formal proof;
- add resident background validation;
- add a second mutation/state-space framework;
- weaken conservative unknown/inconclusive semantics to improve pass rates.

## 22. Design decision summary

The Validation Lab becomes a **research authority plane**, separate from the resident runtime. It records what was predicted, what exact corpus/model/profile was run, what was observed, how metrics were counted, and the strongest result class justified by that evidence.

The first implementation slice is intentionally narrow: authority before scale. Once this core is exact-head GREEN, existing `localview-mutation` and `localview-state-space` primitives can be connected into stronger L2/L4 campaigns without inventing provenance rules ad hoc in each layer.
