# V4.3 Validation Lab Core Authority Design

## Status

Approved continuation slice for the V4.3 adversarial-validation roadmap. This design intentionally covers only the model-free research authority core. It does not implement OS/provider execution, mutation campaigns, bounded temporal search, concurrency exploration, or differential runners.

## Problem

V4.3 requires executable falsification evidence rather than prose-only correctness claims. The repository already contains reusable primitives such as `localview-mutation` and `localview-state-space`, but it lacks a dedicated authority layer that can bind a validation run to immutable revisions, distinguish prospective from exploratory evidence, represent machine-readable seeds, classify result strength, and report core unsoundness metrics without converting missing measurements into false success.

Without that layer, later mutation/provider/model-check campaigns can produce results but cannot reliably answer which preregistration, corpus revision, reducer revision, comparison profile, or random-source profile the result belongs to. A zero observed failure count can also be confused with a zero failure rate when no eligible cases were measured.

## Scope

Create a new workspace crate `localview-validation-lab` that remains research/dev-only by dependency direction: it may depend on generic serialization/hash libraries, but no shipping application or runtime crate will depend on it in this slice.

The crate owns five narrow contracts:

1. research-result strength taxonomy;
2. immutable run/revision identity and prospective preregistration identity;
3. machine-readable seed contract;
4. canonical per-seed observed result contract;
5. V4.3 core metric aggregation with explicit measurement state.

The crate does not execute LocalView actions and does not decide provider truth. Callers supply already-observed outcomes and semantic flags. Later slices will adapt real reducers/providers into these contracts.

## Result strength taxonomy

`ResearchResultClass` has exactly these public variants:

- `ExploratoryObservation`
- `PreregisteredSeedPass`
- `CounterexampleFound`
- `NoCounterexampleWithinBoundN`
- `MutantKilled`
- `MutantSurvived`
- `DifferentialEquivalentWithinVectorSet`
- `DifferentialDivergenceFound`
- `PropertyCampaignPassN`
- `RealProviderIntegrationPass`
- `IndependentReplicationPass`

None is named or serialized as `proved`.

## Run identity

`LabRevisionIdentity` binds:

- `lab_revision`
- `seed_corpus_revision`
- `spec_revision_digest`
- `reference_reducer_revision`
- `mutation_catalog_revision`
- `comparison_profile_revision`
- `random_source_profile`
- optional `platform_profile`
- `start_sequence`

A result record additionally carries the preregistration digest and its own deterministic result digest. Filename/path is never treated as identity.

## Preregistration

`LabPreregistration` records the expected distinction before execution:

- run identity;
- model bound;
- random seed;
- comparison rule revision;
- expected distinction text;
- ordered seed IDs.

The crate exposes `preregistration_digest(&LabPreregistration) -> Result<String, LabEncodingError>` using deterministic canonical bytes derived from a struct with ordered collections. Any content change must change the digest.

`classify_seed_result` may emit `PreregisteredSeedPass` only when the caller supplies the exact preregistration digest expected by the execution record. Missing or mismatched prospective authority downgrades an otherwise passing observation to `ExploratoryObservation`; it must never be upgraded retroactively.

## Seed contract

`LabSeed` contains:

- `seed_id`
- `family`
- ordered `spec_surface_refs`
- JSON `input_fixture`
- `expected_semantic_outcome`
- ordered `forbidden_outcomes`
- `comparison_mode`
- `risk_if_missed`
- `prediction_revision`

`ComparisonMode` starts with `Exact`. New comparison semantics require a concrete counterexample and a later design change.

## Observed result contract

`LabSeedResult` binds one executed seed to:

- seed ID;
- preregistration digest used by the execution;
- expected preregistration digest;
- observed semantic outcome;
- result class;
- zero or more metric events;
- ordered evidence/provenance IDs.

The result class is derived through public constructors/functions; callers should not need to infer prospective validity from strings.

## V4.3 metrics

`LabMetricKind` has exactly the fourteen V4.3 core metrics:

- SUAR — Silent Unsound Action Rate
- WPDR — Wrong-Principal Dispatch Rate
- PILR — Principal Information Leak Rate
- EOFFR — Event-Only False Freshness Rate
- RMR — Reconciliation Miss Rate
- PIAER — Provider-ID ABA Escape Rate
- WFIR — Wrong-Foreground Input Rate
- PDMR — Partial-Dispatch Misclassification Rate
- SCAR — Stale-Cache Authority Rate
- UOBRR — Unknown-Outcome Blind Retry Rate
- MSR — Mutation Survival Rate
- CRDR — Cross-Reducer Divergence Rate
- RPOMR — Real-Provider Oracle Mismatch Rate
- CBFR — Cleanup-to-Baseline Failure Rate

Each result may emit `MetricEvent { kind, eligible, violation }`. Aggregation is deterministic and returns numerator, denominator, and `MetricMeasurementStatus`:

- `NotMeasured` when denominator is zero;
- `Measured` when denominator is positive.

A `NotMeasured` metric has no numeric rate. It is never rendered as zero.

## Silent-unsoundness gate

The first slice implements the V4.3 zero-target gate for SUAR, WPDR, PILR, PDMR, and UOBRR.

`SilentUnsoundnessVerdict` is:

- `Pass` only when every target metric is measured and has numerator zero;
- `Fail` when any measured target metric has numerator greater than zero;
- `Inconclusive` when none fail but at least one target metric is not measured.

Explicit conservative semantic outcomes (`UNKNOWN`, `INCONCLUSIVE`, `RECONCILIATION_REQUIRED`, `UNSUPPORTED`, or a conservative block) are represented by callers as non-violation observations; the metric core must not invent a silent-unsoundness violation from those labels.

## Determinism

All public collections that affect encoding use `BTreeMap`/`BTreeSet` or ordered vectors with explicit caller order. Hash/digest helpers must return lowercase SHA-256 hex. No wall clock, UUID generation, filesystem state, OS APIs, network APIs, AI/model calls, or hidden retries are allowed in this crate.

## Dependency boundary

`localview-validation-lab` is a workspace member so its tests are permanently gated by `cargo test --workspace`. No existing crate gains a dependency on it in this slice. Therefore it is not linked into `localview-daemon`, CLI, desktop, MCP, or runtime libraries.

Future lab runners may depend on production reducers; production reducers must not depend on the lab.

## TDD acceptance tests

The first implementation must be preceded by tests that fail on the test-only commit and then pass without weakening assertions:

1. taxonomy serializes without a `proved` class;
2. the metric catalog contains exactly fourteen kinds;
3. denominator zero yields `NotMeasured` and no numeric rate;
4. one wrong-principal violation yields WPDR numerator/denominator `1/1` and a failing silent-unsoundness verdict;
5. unmeasured zero-target metrics keep the zero-target verdict `Inconclusive`, not `Pass`;
6. measured target metrics with zero violations produce `Pass`;
7. changing preregistration expected distinction changes its SHA-256 digest;
8. missing or mismatched preregistration authority downgrades a passing seed observation to `ExploratoryObservation`;
9. matching preregistration authority permits `PreregisteredSeedPass`;
10. result digest changes when seed/provenance/result content changes.

## Out of scope for this PR

- static spec linter L0;
- complete seed corpus L1;
- mutation runner L2;
- property/metamorphic generation L3;
- bounded temporal exploration L4;
- concurrency scheduler/model checker L5;
- cross-language differential runner;
- fake/real provider harnesses;
- performance/resource stress campaigns;
- dashboard/UI;
- release-completion claims for the overall V4.3 Validation Lab.

Those require separate RED→GREEN slices after this authority core is exact-head GREEN.