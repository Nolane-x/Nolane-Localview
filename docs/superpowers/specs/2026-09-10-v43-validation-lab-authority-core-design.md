# V4.3 Validation Lab Authority Core — Design

Date: 2026-09-10

Status: Design for implementation review

Branch: `feat/v43-validation-lab-authority-core`

Base: `main@9df2d2ae4817a1f85c3fef4e114db4ebbb9e0352`

## 1. Purpose

V4.3 requires falsification infrastructure, not merely more runtime features. Correctness-critical semantics must be executable as bounded, reproducible research claims with machine-readable provenance. A clean run must mean exactly what was executed and nothing stronger; a counterexample must remain durable evidence rather than disappear into CI logs.

This design introduces a thin **Validation Lab authority core** above existing deterministic primitives. It does not reimplement mutation testing, state-space compilation, provider logic, or runtime verification. It creates the research-evidence boundary that binds preregistration, seed identity, oracle revision, observations, metric accounting, result strength, and canonical artifacts.

The first implementation slice intentionally stops before L2–L9 campaign orchestration. Its job is to make later campaigns impossible to overclaim or silently misclassify.

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

Lab artifacts are research records. Production LocalView must not depend on their existence on a user machine.

## 3. Selected architecture

This is an architectural change because no existing crate owns the complete lifecycle of lab preregistration, immutable run identity, result-strength classification, or metric authority.

```text
V4.3 spec / campaign definition
        |
        v
localview-validation-lab
  - preregistration authority
  - immutable revision context
  - completed-run identity
  - seed/oracle contracts
  - observation/result contracts
  - deterministic metric accounting
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
                   L1/L3/L5/L6/L7/L8/L9

Shipping daemon / desktop / runtime
        X
        |
        no dependency on validation-lab
```

The Validation Lab may depend on pure/shared contracts. Shipping runtime crates must not depend on the lab crate.

## 4. Existing primitives to reuse

### 4.1 `localview-mutation`

The repository already has deterministic mutation semantics: mutation classes/operators, `MutationCase`, `MutationOutcome`, `MutationVerdict::{Killed, Survived, Invalid, SkippedUnsafe}`, and `VerificationQualityMap`.

Future L2 integration must reuse these semantics. The lab must not create a second mutation verdict system.

### 4.2 `localview-state-space`

The repository already has a deterministic bounded state-space compiler with dimensions, values, constraints, risk weighting, boundary values, bounded `max_states`, stable state keys, and pair coverage.

Future L4 integration must reuse this compiler instead of creating a competing state enumerator.

### 4.3 `localview-reports` and `localview-quality`

These remain product/reporting surfaces. They are not the authority for research result strength. A future exporter may render lab artifacts, but canonical research meaning is defined by the lab contracts first.

## 5. Core invariants

### I1 — No prospective claim without preregistration

A run can receive a prospective result class only when expected distinctions, seed corpus revision, declared bound, random source profile, and comparison profile were committed to a canonical preregistration artifact before execution began.

If preregistration is missing or invalid before execution, execution may continue only as exploratory work. Finalization must force `EXPLORATORY_OBSERVATION` and record a typed downgrade reason.

### I2 — Valid receipt drift is a hard error

Once execution starts under a valid preregistration receipt, a digest mismatch or drift in bound/comparison/corpus authority is not equivalent to “no preregistration.” It is a broken authority chain and finalization must fail with a typed error. The system must not silently downgrade a tampered or mismatched prospective run into exploratory output.

### I3 — Result strength is typed and non-escalating

Every completed result has exactly one research-strength class:

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

No type, serializer, renderer, or CI summary may equate these with `PROVED`.

### I4 — Conservative non-success is not silent unsoundness

`UNKNOWN`, `INCONCLUSIVE`, `RECONCILIATION_REQUIRED`, `UNSUPPORTED`, and conservative block/rejection do not become silent unsoundness merely because the desired action did not complete. A metric numerator increases only when the exact typed failure predicate is satisfied.

### I5 — Zero observations are not zero failures

A metric with denominator `0` is `NOT_MEASURED`. It is never rendered as `0%`, never treated as passing, and never satisfies a release target.

### I6 — Counterexamples outrank clean bounded runs

A valid counterexample is direct evidence against an invariant for the witnessed fixture. A clean bounded run states only that no counterexample was found inside the declared model, bound, and assumptions.

### I7 — Oracle correction is append-only provenance

An incorrect oracle remains addressable together with the result that exposed the problem. Correction creates a new oracle revision and requires a new preregistration. History is not rewritten to make dashboards green.

### I8 — Canonical content, not file names, carries identity

Relevant semantic changes must alter canonical digests. Cosmetic pretty-printing, map insertion order, or set insertion order must not.

### I9 — Heavy lab is excluded from shipping authority

The daemon, desktop app, provider runtime, observation runtime, and normal product startup must not require the Validation Lab crate or lab artifacts.

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

Crate name: `localview-validation-lab`.

The authority core is pure Rust, uses `#![forbid(unsafe_code)]`, and performs no OS, browser, provider, network, wall-clock ordering, or model calls.

The first slice should depend only on deterministic shared dependencies such as `serde`, `serde_json`, and `sha2`. Dependencies on `localview-mutation` and `localview-state-space` are added only when L2/L4 adapters are implemented.

## 7. Identity model

The specification requires each completed lab run to bind the revisions and the result artifact digest. To avoid a circular digest, the design splits pre-execution context from completed identity.

### 7.1 `LabRevisionContext`

Pre-execution immutable context:

```text
lab_revision
seed_corpus_revision
spec_revision_digest
reference_reducer_revision
mutation_catalog_revision
comparison_profile_revision
random_source_profile
platform_profile?
start_sequence
```

All required string fields are non-empty. `platform_profile` is mandatory for provider-backed prospective campaigns and absent for model-free campaigns.

### 7.2 `CompletedLabRunIdentity`

Created only after result serialization:

```text
revision_context: LabRevisionContext
result_artifact_digest
```

The canonical `LabResultPayload` is serialized and hashed first. That digest is then attached to `CompletedLabRunIdentity`, which is stored in the completed run receipt. The digest is therefore bound to the run without hashing itself.

File names are not identities.

## 8. Seed and oracle contracts

### 8.1 `LabSeed`

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

Seeds are machine-readable and immutable by `(seed_id, prediction_revision, oracle_revision)` within one seed corpus revision. Reusing that identity with different canonical content is an error.

### 8.2 Fixture representation

`input_fixture` is ordinary JSON data. Identity/revision fields and metric counters use integer/string authority only. Fixture JSON may contain finite JSON numbers because geometry, ratios, and timeout fixtures can require non-integer values.

Canonicalization owns numeric encoding and pins it with golden vectors. JSON does not admit NaN or infinity; such values are rejected before canonicalization.

### 8.3 Oracle correction

A corrected oracle increments or otherwise changes `oracle_revision`. Both old and corrected seeds remain addressable. A correction does not mutate prior result artifacts.

## 9. Preregistration

`LabPreregistration` contains:

- `LabRevisionContext`;
- seed-corpus digest and ordered executed seed identities when declared in advance;
- campaign layer/type;
- expected distinctions;
- model/state bound when applicable;
- random seed/source profile;
- comparison profile revision;
- assumptions;
- declared metric set;
- creation logical sequence.

Preregistration is canonicalized and hashed before prospective execution. Execution receives an immutable `PreregistrationReceipt { digest, logical_sequence }`.

There are two explicit modes:

```text
Prospective { receipt }
Exploratory { downgrade_reason }
```

There is no implicit fallback mode.

## 10. Observation and result lifecycle

### 10.1 `LabObservation`

An observation records the smallest exact fact used by metric reducers:

```text
observation_id
seed_id?
expected_outcome
observed_outcome
principal_expected?
principal_dispatched?
failure_flags[]
evidence_refs[]
provider_backed
comparison_profile_revision
```

The authority core never derives correctness-critical failure semantics from human-readable strings. Campaign adapters provide typed failure flags backed by domain evidence.

### 10.2 `LabRunBuilder`

The run builder is append-only until finalization. Each observation receives a canonical digest. After finalization, appending another observation is a typed error.

### 10.3 `LabResultPayload`

The payload binds:

- preregistration digest when prospective;
- exact `LabRevisionContext`;
- execution mode;
- research result class;
- downgrade reason when exploratory due to preregistration failure;
- executed seed identities;
- observation digests;
- metric snapshot;
- counterexample refs;
- minimized-seed refs;
- assumptions actually used;
- bound actually used;
- start/end logical sequence;
- environment artifact digest when applicable.

Finalization canonicalizes this payload, computes its digest, and returns:

```text
CompletedLabRun {
  identity: CompletedLabRunIdentity,
  payload: LabResultPayload
}
```

## 11. Canonical serialization and digest rules

Research authority must not depend on pretty-printing or map construction order.

Rules:

1. recursively sort object keys lexicographically;
2. preserve arrays whose order is semantically meaningful;
3. normalize set-like fields into sorted unique arrays before canonicalization;
4. reject non-finite numeric values before they reach canonical JSON;
5. serialize finite JSON numbers through one pinned canonical serializer and cover integers/fractions/exponents with cross-platform golden vectors;
6. preserve UTF-8 string content byte-for-byte after JSON decoding; do not perform Unicode normalization;
7. hash canonical UTF-8 bytes using SHA-256 with versioned domain separation, beginning with `localview-validation-lab/v1\0`;
8. changing canonicalization semantics requires a new version, never an in-place reinterpretation.

## 12. Metric authority

### 12.1 Required metric kinds

The core represents all V4.3 metrics:

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

### 12.2 Metric value

Canonical metric authority stores counts, not a floating-point percentage:

```text
kind
numerator: u64
denominator: u64
status: MEASURED | NOT_MEASURED
rate_ppb?: u64
```

When denominator is positive, `rate_ppb` is computed with checked integer arithmetic. When denominator is `0`, status is `NOT_MEASURED` and rate is absent.

### 12.3 Silent-unsoundness gate

The V4.3 zero-target subset is:

```text
SUAR
WPDR
PILR
PDMR
UOBRR
```

Per metric:

```text
PASS       measured, numerator == 0
FAIL       measured, numerator > 0
INCOMPLETE denominator == 0 / NOT_MEASURED
```

Overall:

```text
FAIL       if any required metric fails
INCOMPLETE if none fail but one or more are not measured
PASS       only if all required metrics are measured and zero
```

An empty campaign therefore cannot produce a release-pass signal.

### 12.4 Typed accounting

The core reducer consumes typed flags. Examples:

- `wrong_principal_dispatch` increments WPDR numerator and its dispatch-opportunity denominator;
- `principal_information_leak` increments PILR numerator and its principal-sensitive observation denominator;
- `blind_retry_after_unknown` increments UOBRR numerator and its unknown-outcome retry-opportunity denominator;
- `partial_dispatch_misclassified_success` increments PDMR numerator and its partial-dispatch denominator;
- conservative `INCONCLUSIVE` without an unsound-success flag does not increment SUAR.

Exact denominator eligibility for each metric is defined by permanent campaign adapters and seed suites. The authority core guarantees deterministic counting once eligibility and typed failure flags are supplied.

## 13. Artifact model

The specification artifact set is represented explicitly:

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

The first slice fully implements canonical preregistration, seed-catalog, and result artifacts. Future artifact kinds may exist in a manifest enum so absence is typed, but the first slice must not fabricate empty mutation/differential/coverage/environment reports.

Artifact state is one of:

```text
PRESENT
NOT_APPLICABLE
NOT_RUN
```

`NOT_RUN` is never interpreted as a successful zero-result campaign.

## 14. Lifecycle and authority transitions

### 14.1 Preregister

1. Build immutable seed catalog.
2. Validate duplicate seed identities and oracle collisions.
3. Build `LabPreregistration` with exact revisions, bounds, expected distinctions, and comparison profile.
4. Canonicalize and digest it.
5. Persist the preregistration artifact before prospective execution.
6. Return the immutable receipt.

### 14.2 Start execution

A caller chooses explicitly:

- `Prospective(receipt)`: receipt must validate against the canonical preregistration artifact before first observation;
- `Exploratory(reason)`: no prospective result class can later be minted for that run.

### 14.3 Record observations

Campaign adapters append typed observations. Observation order uses logical sequence, not wall-clock time, for semantic authority.

### 14.4 Finalize

Prospective finalization:

1. revalidates preregistration digest;
2. verifies actual corpus/bound/comparison authority matches preregistration;
3. deterministically reduces metrics;
4. evaluates applicable release gates;
5. derives the strongest permitted research result class;
6. canonicalizes `LabResultPayload`;
7. computes `result_artifact_digest`;
8. returns `CompletedLabRunIdentity + LabResultPayload`.

Exploratory finalization follows steps 3, 6, 7, and 8 but research result class is forced to `EXPLORATORY_OBSERVATION` with the original downgrade reason.

## 15. Failure semantics

Hard errors:

- duplicate seed identity with different canonical content;
- empty required revision fields;
- non-finite fixture numbers;
- canonicalization failure;
- valid preregistration receipt whose digest no longer matches persisted preregistration;
- prospective execution whose actual seed corpus, bound, or comparison authority drifts after admission;
- provider-backed prospective run without platform profile;
- counter overflow;
- numerator greater than denominator for subset-defined metrics;
- finalization attempted twice;
- observation appended after finalization.

Typed downgrade to exploratory:

- execution begins without a preregistration receipt;
- preregistration creation/persistence failed before prospective authority was admitted;
- caller explicitly selects exploratory mode.

A hard authority mismatch after prospective admission is never laundered into exploratory success.

## 16. TDD plan for the first implementation slice

Implementation must preserve permanent RED -> GREEN lineage.

### RED-A — Metric completeness and zero-denominator safety

Tests require:

- all 14 metric kinds;
- fresh metrics are `NOT_MEASURED` with denominator `0` and no rate;
- an empty silent-unsoundness gate is `INCOMPLETE`, never `PASS`.

### RED-B — Wrong-principal accounting

One eligible dispatch opportunity containing one wrong-principal dispatch yields:

```text
WPDR numerator = 1
WPDR denominator = 1
WPDR rate_ppb = 1_000_000_000
silent-unsoundness gate = FAIL
```

### RED-C — Conservative inconclusive is not SUAR

`INCONCLUSIVE` without an unsound-success flag does not increment SUAR numerator.

### RED-D — Canonical preregistration digest

Changing expected distinction, seed corpus revision, bound, comparison profile, or random source profile changes the digest. Reordering set-like fields does not.

Golden vectors cover integer, fractional, exponent, Unicode, nested-object, and set-order cases on Ubuntu/macOS/Windows.

### RED-E — Missing preregistration cannot mint a prospective class

A run started without valid preregistration finalizes as `EXPLORATORY_OBSERVATION` with a typed downgrade reason even if the caller asks for `PREREGISTERED_SEED_PASS`.

### RED-F — Admitted prospective drift is not downgraded

A valid prospective run whose preregistration digest or declared bound changes before finalization must return a typed authority error and produce no completed prospective result artifact.

### RED-G — Oracle revision immutability

Same `(seed_id, prediction_revision, oracle_revision)` plus different canonical expected outcome is rejected. A corrected oracle uses a new revision and both remain addressable.

### RED-H — Completed identity has no digest cycle

Golden test proves `LabResultPayload` digest is stable, `CompletedLabRunIdentity` embeds that digest, and changing the result payload changes the completed identity without requiring self-hashing.

### RED-I — Shipping dependency exclusion

Repository-level test verifies shipping members, including daemon/desktop/runtime/provider crates, do not depend on `localview-validation-lab`.

## 17. First-PR implementation boundary

The first PR implements only:

1. `localview-validation-lab` crate and workspace registration;
2. canonical JSON + versioned SHA-256 digest authority;
3. result-strength taxonomy;
4. `LabRevisionContext` and `CompletedLabRunIdentity`;
5. seed catalog and oracle-revision collision checks;
6. preregistration and immutable receipts;
7. prospective vs exploratory execution mode;
8. observation/result lifecycle;
9. all 14 metric kinds with deterministic integer accounting;
10. silent-unsoundness zero-target gate;
11. canonical preregistration/seed/result artifacts;
12. permanent cross-platform golden tests;
13. shipping-dependency exclusion contract.

It does **not** implement campaign engines for L1 semantic execution, L2 mutation, L3 property generation, L4 bounded temporal search, L5 concurrency, L6 differential reducers, L7 fake provider, L8 real provider, or L9 resource stress.

## 18. Future integration order

After exact-head GREEN authority core:

1. L1 deterministic semantic seed runner.
2. L2 adapter over `localview-mutation` and MSR.
3. L4 adapter over `localview-state-space` with exact bound provenance.
4. L3 property/metamorphic campaigns plus minimization.
5. L6 differential reducer harness and CRDR.
6. L7 fake-provider lifecycle/freshness/principal campaigns.
7. L5 deterministic concurrency exploration over narrow authority/resource models.
8. L8 platform-profile-bound real-provider seeds and RPOMR.
9. L9 cleanup/resource/failure campaigns and CBFR.

Deterministic semantic evidence intentionally comes before OS/provider nondeterminism.

## 19. CI strategy

The first PR adds one fast named gate:

```text
V4.3 validation lab authority contract
```

It runs authority-core tests and canonical golden vectors on Ubuntu, macOS, and Windows within the Rust-core CI structure.

Heavy future campaigns use dedicated manual/scheduled workflows. Normal pushes must not run million-case searches or real-provider ceremonies by default.

CI summaries may report exact result classes but never `PROVED`.

## 20. Security, privacy, and resident-runtime policy

The authority core requires no real secrets, provider credentials, clipboard data, password values, desktop-wide recording, or arbitrary environment dumps.

Future provider-backed `LAB-ENVIRONMENT.json` uses an explicit allowlist. Secret-bearing environment variables, tokens, raw private UI text, and password fields are excluded.

Synthetic secret-like fixture strings are allowed only when explicitly synthetic and not sourced from user state.

No observer thread, timer, provider crawl, screen recorder, browser, or model process is introduced by the authority core. Normal LocalView startup performs zero Validation Lab work unless an explicit future lab command/workflow invokes it.

## 21. Alternatives rejected

### Monolithic L0–L9 implementation

Rejected because research authority, generators, mutation, temporal state search, concurrency, differential execution, providers, and stress would become one unreviewable change with weak failure attribution.

### Embed lab authority in `localview-quality`

Rejected because product quality findings and preregistered research claims have different authority semantics.

### Embed lab authority in `localview-mutation`

Rejected because mutation is one lab layer, not the owner of differential, provider, freshness, reconciliation, or cleanup research evidence.

## 22. Acceptance criteria

The authority-core slice is complete only when one exact PR head satisfies all of the following:

- permanent RED lineage exists for the new contracts;
- all first-slice tests are GREEN on Ubuntu, macOS, and Windows;
- canonical digest golden vectors are identical across platforms;
- all 14 V4.3 metrics are represented;
- zero-denominator metrics are `NOT_MEASURED`;
- empty silent-unsoundness evidence cannot pass;
- wrong-principal and blind-retry fixtures fail the zero-target gate;
- conservative `INCONCLUSIVE` does not become silent unsoundness by default;
- missing preregistration forces exploratory classification;
- admitted prospective drift fails with a typed authority error;
- oracle correction preserves revision history;
- completed-run identity binds result digest without a circular hash;
- no result class renders or serializes as `PROVED`;
- shipping runtime crates have no dependency on `localview-validation-lab`;
- no temporary probe workflow/script remains;
- PR exact head is verified before merge;
- post-merge `main` is re-verified before closure.

## 23. Non-goals

This slice does not claim V4.3 is fully validated. It creates the authority needed to make future validation claims meaningful.

It does not:

- prove LocalView correct;
- implement every L0–L9 campaign;
- replace existing runtime tests;
- treat bounded model search as formal proof;
- add resident background validation;
- duplicate mutation/state-space frameworks;
- weaken unknown/inconclusive semantics to improve pass rates.

## 24. Design decision summary

Validation Lab is a **research authority plane** separate from the resident runtime. It records what was predicted, what exact corpus/model/profile was admitted, what was observed, how metrics were counted, and the strongest research result class justified by that evidence.

The first implementation slice is intentionally narrow: authority before scale. Once it is exact-head GREEN, `localview-mutation` and `localview-state-space` can be connected into stronger L2/L4 campaigns without inventing provenance rules ad hoc in each layer.
