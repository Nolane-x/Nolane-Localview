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

Both are owned by the desktop process. Moving their mutable counters into the daemon through a new bearer-authenticated HTTP mutation endpoint would create a weaker authority boundary: any bearer holder could claim arbitrary retained usage. Phase A therefore enforces retained usage locally at the mutation owner and reuses `localview-resource-governor` as a shared policy/ledger crate rather than inventing a cross-process mutable global.

## Scope

### In scope

1. Add a typed retained-resource ledger to `localview-resource-governor` for:
   - `capture_storage` bytes;
   - `cache` bytes.
2. Keep retained-resource budgets in bytes internally so admission is exact and does not round encoded PNG/cache sizes to MiB.
3. Make the desktop `VisualCaptureState` own one retained-resource ledger with the production visual limits:
   - capture storage: 256 MiB;
   - baseline cache: 96 MiB.
4. Synchronize the ledger from resources the desktop actually owns:
   - restored/current `ArtifactStore::used_bytes()` after store open and after each successful put/eviction cycle;
   - `VisualBaselineCache::used_bytes()` after cache initialization, compatibility invalidation/removal, and insertion/eviction.
5. Require ledger admission before a retained mutation that would grow usage beyond the configured limit.
6. Preserve existing LRU eviction semantics, but make capacity reclamation happen before the new artifact write when a new object would otherwise exceed the artifact-store bound. An individual artifact larger than the limit must fail before filesystem write.
7. Ensure failed artifact writes or failed baseline insertions do not advance retained usage.
8. Keep deduplicated artifact writes at zero additional retained bytes.
9. Add direct unit tests and desktop contract tests that prove ordering and accounting invariants.
10. Add named CI gates for the new resource-governor and desktop retained-resource contracts.

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

`POST /v1/runtime/resources/sample` remains external telemetry and must not become the mechanism by which visual artifact/cache retained usage is committed. Phase A does not add any caller-writable retained-resource field.

### No success laundering

A ledger update is not proof that a mutation happened. Ordering is:

1. derive the exact resource delta from owner state;
2. admit the growth against the owner-local ledger;
3. perform the mutation;
4. derive actual post-mutation owner usage;
5. commit/synchronize that actual usage into the ledger.

If the mutation fails, step 5 does not occur.

## Core retained-resource API

Add the following conceptual API in `crates/resource-governor/src/lib.rs` (exact Rust names may be refined only if tests reveal a conflict, but semantics must remain unchanged):

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
pub struct RetainedResourceDenial {
    pub kind: RetainedResourceKind,
    pub current_bytes: u64,
    pub requested_additional_bytes: u64,
    pub limit_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct RetainedResourceLedger { /* private Arc<Mutex<...>> */ }
```

Required behavior:

- `RetainedResourceLedger::new(budget)` rejects zero limits rather than silently creating an unusable policy.
- `usage()` returns exact current bytes for both retained dimensions.
- `check_additional(kind, additional_bytes)` uses checked/saturating-safe arithmetic and denies `current + additional > limit`.
- `synchronize(kind, actual_bytes)` accepts only `actual_bytes <= limit`; it is the owner-only post-mutation reconciliation primitive.
- failed admission or failed synchronization leaves prior usage unchanged.
- zero-byte admission is always allowed when current usage is already valid.

The ledger does not delete files or cache entries itself. Resource owners remain responsible for mutations; the ledger owns only admission and canonical usage accounting.

## ArtifactStore integration

`ArtifactStore` already owns content IDs, deduplication, disk restoration, LRU ordering, and a local max-byte bound. Phase A will expose enough deterministic state to integrate it safely:

- `used_bytes() -> u64`;
- a pre-write capacity path that can distinguish deduplication from a new object;
- an individual object larger than `max_bytes` is rejected before `tokio::fs::write`;
- when a new object needs space, old LRU entries are evicted before the new file is written, so disk usage does not intentionally overshoot the configured store bound and then hide the overshoot with post-write GC.

The existing `put(kind, bytes)` public contract remains usable by other callers. Implementation may internally split capacity preparation from commit, but must not expose a caller-forgeable accounting delta.

Desktop persistence flow while holding the artifact-store mutex:

1. lazily open `ArtifactStore`;
2. synchronize ledger capture-storage usage from restored-and-GC'd `store.used_bytes()`;
3. ask the store for/perform pre-write capacity preparation;
4. synchronize any pre-write eviction result from `store.used_bytes()`;
5. ledger-check exact additional bytes for a non-deduplicated write;
6. write through `ArtifactStore`;
7. synchronize exact post-write `store.used_bytes()`;
8. only then continue to daemon Visual evidence registration.

If the filesystem write fails, ledger usage remains at the post-eviction value and must not include bytes that were never written.

## Baseline-cache integration

`VisualBaselineCache` already exposes `used_bytes()` and applies LRU eviction. Phase A keeps the cache implementation as the owner of compatibility and eviction policy.

Desktop baseline flow while holding the cache mutex:

- on lazy initialization, synchronize cache usage to zero;
- after `get_compatible`, synchronize usage because an incompatible context can remove an entry;
- before insertion, compute the incoming image byte count using its validated RGBA backing bytes;
- if the image itself exceeds the configured cache budget, preserve current cache semantics (`Ok(false)`) and do not grow ledger usage;
- after cache insertion/eviction, synchronize the ledger from `cache.used_bytes()`;
- a failed insertion does not advance ledger usage.

Because `VisualBaselineCache` may replace the same session baseline and evict LRU entries atomically inside its insertion policy, the ledger reconciles exact post-operation bytes instead of pretending every incoming frame is monotonically additive.

## Failure handling

All new accounting failures fail closed for the retained mutation being attempted.

- Invalid retained budget: desktop state construction cannot silently fall back to an unbounded ledger.
- Artifact too large: no filesystem write.
- Artifact ledger admission denied after pre-eviction: no new artifact write.
- Artifact write fails: no phantom new usage.
- Baseline insert rejects/errs: no phantom cache usage.
- Ledger synchronization detects owner usage above its configured limit: return a deterministic resource-governor error; do not emit success receipts that imply the retained state is healthy.

Existing visual safety ordering remains unchanged: settle → freeze → native capture → exact restore → private redaction → target validation/processing → budget admission → persistence/evidence → baseline commit.

## Concurrency and ownership

- `VisualCaptureState.artifacts` and `.baselines` remain Tokio mutex protected.
- Retained ledger internals may use the existing poison-tolerant standard mutex style from `localview-resource-governor`.
- Artifact/cache owner mutex must remain held while deriving owner usage, admitting, mutating, and reconciling that same owner to prevent an interleaving from invalidating the delta.
- No global mutable retained counter is added.

## Testing strategy

### Resource-governor unit contract

Prove:

- independent capture-storage/cache dimensions;
- exact-bound admission succeeds and bound+1 fails;
- overflow-safe denial;
- zero-byte admission;
- synchronization above limit is rejected without mutating prior usage;
- successful synchronization changes only the selected dimension;
- cloned ledger handles share one authority state.

### ArtifactStore contract

Prove:

- restored disk usage is exposed exactly;
- deduplicated put does not increase used bytes;
- object larger than limit fails without creating a file;
- LRU capacity is reclaimed before a new write and final disk/used bytes remain inside budget;
- failed write does not fabricate used bytes.

### Desktop retained-resource contract

A source-level/compile contract must lock the production wiring:

- `localview-resource-governor` is a desktop dependency;
- `VisualCaptureState` owns retained-resource authority;
- artifact usage is synchronized from `ArtifactStore::used_bytes()` rather than caller input;
- baseline usage is synchronized from `VisualBaselineCache::used_bytes()`;
- no new `/v1/...resource...` mutation endpoint is added for desktop retained usage;
- baseline commit remains after selected visual evidence succeeds.

Where practical, pure helper functions should be extracted so behavior can be exercised without creating a Tauri WebView.

### Regression

Existing artifact, changed-region, visual-packet, Perception Budget, native visual executor, deterministic verification, and cross-platform GUI smoke contracts must remain green.

## CI

Add named gates to `.github/workflows/ci.yml`:

- `Retained resource governor contract`;
- `Desktop retained visual resource authority contract`.

These are additive; existing CI and Windows UIA Observe gates are not weakened.

## Completion criteria for Phase A

Phase A is complete only when:

1. exact-head PR CI is green across all existing jobs;
2. retained-resource tests prove capture-storage/cache authority and failure ordering;
3. PR audit shows no public mutable retained-resource endpoint and no visual safety-order regression;
4. the PR is merged with expected-head SHA locking;
5. push CI on the exact merge commit is green.

Only then may the roadmap claim capture-storage/cache enforcement as landed. Browser-process, hidden-surface, and analysis-concurrency enforcement remain explicit follow-up phases.