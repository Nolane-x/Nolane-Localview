# Runtime Resource Governor V2 — Retained Visual Resources Design

## Status

Approved architectural slice for the next LocalView production-completeness increment after Windows UIA Expand/Collapse.

## Goal

Make visual artifact storage and changed-region baseline cache usage authoritative, bounded, observable, and fail-closed at the process that actually owns those resources, without allowing HTTP callers to mint retained-resource authority.

This is Phase A of the broader Runtime Resource Governor completion program. Later phases will cover browser-process, hidden-surface, and analysis-concurrency enforcement using the same owner-local authority principle.

## Why this slice

The live runtime already has daemon-side CPU/RAM sampling, native visual reservations, Chromium admission, and native semantic resource gates. The remaining roadmap explicitly calls for capture-storage, browser-process, hidden-surface, analysis-concurrency, and cache enforcement.

Two of those dimensions are already live and concrete today:

- the desktop visual `ArtifactStore`, bounded locally to 256 MiB;
- the desktop changed-region `VisualBaselineCache`, bounded locally to 96 MiB / 32 entries.

Both are owned by the desktop process. Moving their mutable counters into the daemon through a new bearer-authenticated HTTP mutation endpoint would weaken authority because any bearer holder could claim arbitrary retained usage. Phase A therefore enforces retained usage locally at the mutation owner and reuses `localview-resource-governor` as a shared policy/ledger crate rather than inventing a cross-process mutable global.

## Scope

### In scope

1. Add a typed retained-resource ledger to `localview-resource-governor` for:
   - `capture_storage` bytes;
   - `cache` bytes.
2. Keep retained-resource budgets in bytes internally so admission is exact and does not round encoded PNG/cache sizes to MiB.
3. Make desktop `VisualCaptureState` own one retained-resource ledger with the production visual limits:
   - capture storage: 256 MiB;
   - baseline cache: 96 MiB.
4. Synchronize the ledger only from resources the desktop actually owns:
   - restored/current `ArtifactStore::used_bytes()` after store open and every put/GC result;
   - `VisualBaselineCache::used_bytes()` after initialization, compatibility invalidation/removal, and insertion/eviction.
5. Require projected retained usage to be admitted before a new retained mutation.
6. Keep deduplicated artifact writes at zero projected growth.
7. Reject an individual artifact larger than the artifact-store limit before filesystem write.
8. Make artifact GC accounting truthful: a failed file deletion must not be subtracted from `ArtifactStore::used_bytes()` merely because eviction was attempted.
9. Ensure failed artifact writes or failed baseline insertions never fabricate a successful retained-resource state.
10. Add unit, owner-integration, desktop authority, and CI contracts.

### Out of scope

- Chromium/browser-process lifecycle enforcement beyond the existing daemon reservation gate.
- Hidden WebView/surface counting.
- Analysis-concurrency counting.
- Changing Perception Budget dimensions.
- New public resource mutation APIs.
- Replacing the existing daemon Runtime Resource Governor.
- Full-page stitching, responsive sweeps/contact sheets, framework ownership, or native-workspace defaulting.

## Authority model

### Owner-local authority

A resource is authoritatively accounted by the process that owns and mutates it.

- Daemon/control continues to own transient daemon-side reservations and CPU/RAM/process telemetry.
- Desktop owns visual artifact files and the in-memory baseline cache, so desktop owns their retained-resource ledger.

There is deliberately no cross-process `set_capture_storage_bytes` or `set_cache_bytes` HTTP endpoint in this slice.

### Caller data is not authority

`POST /v1/runtime/resources/sample` remains external telemetry and must not become the mechanism by which visual artifact/cache retained usage is committed. Phase A adds no caller-writable retained-resource endpoint or payload.

### Observation and admission are different

The ledger has two distinct operations:

1. **reconcile observed owner state** — record the exact bytes the owner actually retains, even when that observed state is already over limit;
2. **admit a projected mutation** — reject a proposed post-mutation retained state above limit before that mutation is attempted.

This distinction prevents a dangerous failure mode where the ledger refuses to record an already-over-budget filesystem/cache state and therefore under-reports reality.

### No success laundering

For each mutation:

1. reconcile exact current owner usage;
2. compute owner-specific projected post-mutation usage, including deterministic deduplication/replacement/LRU effects;
3. admit that projected usage;
4. perform the mutation;
5. reconcile exact actual post-mutation owner usage;
6. only then expose success to the next product layer.

If mutation/GC fails, the owner state is reconciled as it actually exists and no success receipt may imply a healthy retained state.

## Core retained-resource API

Add the following API shape in `crates/resource-governor/src/lib.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RetainedResourceKind {
    CaptureStorage,
    Cache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedResourceBudget {
    pub capture_storage_bytes: u64,
    pub cache_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetainedResourceUsage {
    pub capture_storage_bytes: u64,
    pub cache_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedResourceViolation {
    pub kind: RetainedResourceKind,
    pub current_bytes: u64,
    pub projected_or_observed_bytes: u64,
    pub limit_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct RetainedResourceLedger { /* private Arc<Mutex<...>> */ }
```

Required semantics:

- `RetainedResourceLedger::new(budget)` rejects zero limits.
- `usage()` returns exact current bytes for both dimensions.
- `admit_projected(kind, projected_bytes)` is read-only and fails when `projected_bytes > limit` or when current owner state is already over the same limit.
- `synchronize(kind, actual_bytes)` **always records `actual_bytes` first**; if the resulting observed state is over limit it returns `RetainedResourceViolation` after recording it.
- failed projected admission never changes usage.
- synchronizing one dimension never changes the other.
- cloned ledger handles share one authority state.

The ledger never deletes files or cache entries. Resource owners remain responsible for mutation and deterministic reclamation policy.

## ArtifactStore integration

`ArtifactStore` already owns content IDs, deduplication, disk restoration, LRU ordering, and a local max-byte bound. Phase A adds owner-introspection/projection while preserving its public `put(kind, bytes)` contract:

- `used_bytes() -> u64`;
- `max_bytes() -> u64` if useful for invariant tests;
- `projected_used_bytes_after_put(bytes: &[u8]) -> Result<u64>` (or equivalent) that:
  - returns current usage for an existing content ID;
  - rejects a single new object larger than `max_bytes`;
  - simulates existing LRU GC deterministically and returns final retained bytes without mutating disk/index/LRU.

Artifact `put` remains responsible for the real write and GC. GC must stop laundering failed deletion:

- only remove an index entry/subtract bytes after the backing file was removed successfully or was already absent;
- if deletion returns another I/O error, retain the indexed usage and return the error;
- `used_bytes()` therefore tracks the owner's known retained disk state instead of assuming eviction succeeded.

Desktop persistence flow while holding the artifact-store mutex:

1. lazily open `ArtifactStore`;
2. reconcile ledger capture-storage usage from restored-and-GC'd `store.used_bytes()`;
3. compute projected post-put usage from the store itself;
4. admit the projected capture-storage usage;
5. execute `store.put`;
6. reconcile exact `store.used_bytes()` whether the put succeeds or fails;
7. only on successful put plus successful reconciliation continue to daemon Visual evidence registration.

A normal new write may temporarily exist before post-write GC because `ArtifactStore` preserves write-before-evict semantics; the retained-resource projection concerns canonical retained state after the store transaction, not temporary I/O staging. An individually oversized artifact is still rejected before write.

## Baseline-cache integration

`VisualBaselineCache` already owns compatibility, one-entry-per-session replacement, LRU eviction, byte budget, entry count, and `used_bytes()`. Phase A adds a pure projection method rather than duplicating cache policy in the desktop:

- `projected_used_bytes_after_insert(session_id, image_bytes) -> Result<Option<usize>, VisualError>` (or equivalent):
  - returns `None` when the incoming image alone exceeds the cache byte budget, matching current `insert(...)->Ok(false)` behavior;
  - otherwise simulates same-session replacement plus LRU byte/entry eviction and returns deterministic final retained bytes;
  - does not mutate cache/touch timestamps.

Desktop baseline flow while holding the cache mutex:

1. lazily initialize the cache and reconcile usage;
2. after `get_compatible`, reconcile usage because incompatibility may remove the existing session baseline;
3. validate the decoded image/context as required by the existing insert path;
4. compute projected post-insert cache usage through the cache's own projection method;
5. if projection is `None`, preserve `baseline_cached = false` without admitting or growing cache usage;
6. admit projected cache usage;
7. execute cache insertion/eviction;
8. reconcile exact `cache.used_bytes()` whether insertion succeeds or fails;
9. only report `baseline_cached = true` when the cache actually retained the new session baseline and reconciliation is healthy.

The ledger never assumes every incoming frame is additive.

## Failure handling

All accounting paths fail closed without falsifying observed state.

- Invalid retained budget: desktop state construction must not silently become unbounded.
- Artifact too large: no filesystem write.
- Projected retained usage above limit: no attempted retained mutation.
- Artifact write/GC failure: no Visual evidence success; reconcile the store's actual known usage first.
- Baseline insertion error: no cached-baseline success; reconcile actual cache usage first.
- Observed owner usage above limit: record the over-limit reality, return a deterministic retained-resource violation, and deny later projected growth until owner usage is again within policy.

Existing visual safety ordering remains unchanged: settle → freeze → native capture → exact restore → private redaction → target validation/processing → Perception Budget admission where applicable → artifact/evidence emission → baseline commit.

## Concurrency and ownership

- `VisualCaptureState.artifacts` and `.baselines` remain Tokio-mutex protected.
- Retained ledger internals use the existing poison-tolerant standard-mutex style in `localview-resource-governor`.
- The corresponding owner mutex remains held while reconciling, projecting, admitting, mutating, and reconciling that resource again, so no interleaving can invalidate the projected state.
- No global mutable retained counter is added.

## Testing strategy

### Resource-governor unit contract

Prove:

- independent capture-storage/cache dimensions;
- exact-bound projected admission succeeds and bound+1 fails;
- current-over-limit state denies further projected admission;
- `synchronize` records an over-limit observation before returning violation;
- failed projected admission leaves usage unchanged;
- successful synchronization changes only one dimension;
- cloned handles share state.

### ArtifactStore contract

Prove:

- restored disk usage is exposed exactly;
- deduplicated projection/put does not increase retained bytes;
- a single object larger than limit fails before file creation;
- projected usage matches actual post-GC usage for deterministic LRU cases;
- failed GC deletion is not subtracted from known usage;
- final successful disk/index usage remains inside budget.

### VisualBaselineCache contract

Prove:

- projection is side-effect-free (including touch/LRU order);
- same-session replacement is projected correctly;
- byte-budget and max-entry LRU projection matches actual insertion;
- an individually oversized image projects to `None` and is not retained.

### Desktop retained-resource authority contract

Lock the production wiring:

- `localview-resource-governor` is a desktop dependency;
- `VisualCaptureState` owns retained-resource authority;
- artifact usage comes only from `ArtifactStore::used_bytes()` and artifact projection comes from the store;
- baseline usage comes only from `VisualBaselineCache::used_bytes()` and projection comes from the cache;
- reconcile → project → admit → mutate → reconcile ordering is present at both owners;
- there is no new `/v1/...resource...` mutation endpoint for desktop retained usage;
- baseline commit remains after selected Visual evidence succeeds.

Pure owner/ledger helpers should be tested without requiring a native WebView whenever possible.

### Regression

Existing artifact, changed-region, visual-packet, Perception Budget, native visual executor, deterministic verification, Windows UIA, and cross-platform GUI smoke contracts remain green.

## CI

Add named gates to `.github/workflows/ci.yml`:

- `Retained resource governor contract`;
- `Artifact retained usage contract`;
- `Visual baseline retained usage contract`;
- `Desktop retained visual resource authority contract`.

They are additive; no existing gate is weakened.

## Completion criteria for Phase A

Phase A is complete only when:

1. exact-head PR CI is green across all existing jobs;
2. retained-resource tests prove capture-storage/cache authority, projection, reconciliation, and failure ordering;
3. PR audit shows no public mutable retained-resource endpoint and no visual safety-order regression;
4. PR merges with expected-head SHA locking;
5. push CI on the exact merge commit is green.

Only then may the roadmap claim capture-storage/cache enforcement as landed. Browser-process, hidden-surface, and analysis-concurrency enforcement remain explicit follow-up phases.