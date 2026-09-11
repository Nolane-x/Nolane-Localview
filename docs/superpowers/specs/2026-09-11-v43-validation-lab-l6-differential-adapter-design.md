# V4.3 Validation Lab L6 Differential Adapter Design

## Scope

This bounded slice adds a research-only L6 differential adapter inside `crates/validation-lab`. It does not add a new differential execution engine, provider/browser/model calls, shipping-runtime dependencies, or a second metric reducer.

## Authority model

The adapter receives an explicit vector-set identity, a comparison mode, a comparison-profile revision, and concrete reference/candidate outputs for each vector. The caller does not submit a pass/fail verdict.

Only exact comparison is admitted in this slice. Empty comparison authority and unsupported comparison modes fail closed before observations are minted. Vector identities must be unique inside the set.

For every vector, the adapter creates one Lab observation eligible for CRDR. Exact equality adds no failure flag. Exact inequality adds `CrossReducerDivergence`. Evidence references remain deterministic set data.

The vector-set result is derived from typed observations:

- no vectors: no research result class; CRDR remains NOT_MEASURED through the existing metric reducer;
- all vectors equivalent: `DifferentialEquivalentWithinVectorSet`;
- one or more divergences: `DifferentialDivergenceFound`, retaining the first divergent vector identity while CRDR counts every divergent vector.

## Reuse boundaries

CRDR accounting is delegated to the existing `reduce_metric_observations` path. The adapter does not duplicate metric arithmetic or infer correctness from human-readable strings beyond the admitted exact output comparison itself.

## Verification

Permanent RED tests cover equivalent sets, multiple divergences and first-divergence identity, empty-set non-measurement, unsupported comparison authority, empty comparison-profile revision, and duplicate vector identity rejection. Exact-head cross-platform CI and the named `V4.3 validation lab authority contract` remain required before merge.
