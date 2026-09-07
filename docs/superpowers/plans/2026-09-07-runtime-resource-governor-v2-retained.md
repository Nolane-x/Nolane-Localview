# Runtime Resource Governor V2 — Retained Visual Resources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce and reconcile authoritative retained usage for desktop visual artifact storage and changed-region baseline cache without adding caller-writable resource authority.

**Architecture:** Add a clone-shared retained-resource ledger to `localview-resource-governor`, then let each owner compute projected/actual usage through its own deterministic policy. Desktop visual capture holds the owner mutex across reconcile → project → admit → mutate → reconcile, preserving existing capture/evidence/baseline ordering and avoiding cross-process mutable counters.

**Tech Stack:** Rust, Tokio, Tauri 2, localview-resource-governor, localview-artifacts, localview-visual, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-07-runtime-resource-governor-v2-retained-design.md`

## Global Constraints

- Capture-storage retained limit remains exactly `256 * 1024 * 1024` bytes.
- Baseline cache retained limit remains exactly `96 * 1024 * 1024` bytes and 32 entries.
- No new public or bearer-authenticated endpoint may mutate desktop artifact/cache retained usage.
- Observed owner usage is truth: `synchronize` records an over-limit observation before returning a violation.
- Projected admission is read-only and must not mutate usage.
- Perception Budget dimensions remain unchanged.
- Existing visual transaction order remains settle → freeze → native capture → exact restore → private redaction → validation/processing → budget admission → artifact/evidence emission → baseline commit.
- No existing CI, Windows UIA, or GUI smoke gate may be weakened.

---

### Task 1: Retained-resource ledger core

**Files:**
- Create: `crates/resource-governor/tests/retained_resources.rs`
- Modify: `crates/resource-governor/src/lib.rs`

**Interfaces:**
- Produces:
  - `RetainedResourceKind::{CaptureStorage, Cache}`
  - `RetainedResourceBudget { capture_storage_bytes, cache_bytes }`
  - `RetainedResourceUsage { capture_storage_bytes, cache_bytes }`
  - `RetainedResourceViolation { kind, current_bytes, projected_or_observed_bytes, limit_bytes }`
  - `RetainedResourceLedger::new(...) -> Result<Self, RetainedResourceViolation>`
  - `RetainedResourceLedger::usage() -> RetainedResourceUsage`
  - `RetainedResourceLedger::admit_projected(kind, projected_bytes) -> Result<(), RetainedResourceViolation>`
  - `RetainedResourceLedger::synchronize(kind, actual_bytes) -> Result<(), RetainedResourceViolation>`

- [ ] **Step 1: Write the failing integration contract**

Create `crates/resource-governor/tests/retained_resources.rs` with tests equivalent to:

```rust
use localview_resource_governor::{
    RetainedResourceBudget, RetainedResourceKind, RetainedResourceLedger,
};

fn ledger() -> RetainedResourceLedger {
    RetainedResourceLedger::new(RetainedResourceBudget {
        capture_storage_bytes: 100,
        cache_bytes: 40,
    })
    .expect("valid retained budget")
}

#[test]
fn projected_admission_is_dimension_specific_and_exact() {
    let ledger = ledger();
    ledger.synchronize(RetainedResourceKind::CaptureStorage, 90).unwrap();
    ledger.synchronize(RetainedResourceKind::Cache, 25).unwrap();
    ledger.admit_projected(RetainedResourceKind::CaptureStorage, 100).unwrap();
    let denial = ledger
        .admit_projected(RetainedResourceKind::CaptureStorage, 101)
        .unwrap_err();
    assert_eq!(denial.limit_bytes, 100);
    assert_eq!(ledger.usage().cache_bytes, 25);
}

#[test]
fn synchronize_records_over_limit_reality_before_returning_violation() {
    let ledger = ledger();
    let violation = ledger
        .synchronize(RetainedResourceKind::Cache, 41)
        .unwrap_err();
    assert_eq!(violation.projected_or_observed_bytes, 41);
    assert_eq!(ledger.usage().cache_bytes, 41);
    assert!(ledger
        .admit_projected(RetainedResourceKind::Cache, 40)
        .is_err());
}

#[test]
fn clones_share_authority_state() {
    let ledger = ledger();
    let clone = ledger.clone();
    ledger.synchronize(RetainedResourceKind::CaptureStorage, 33).unwrap();
    assert_eq!(clone.usage().capture_storage_bytes, 33);
}

#[test]
fn zero_limit_budget_is_rejected() {
    assert!(RetainedResourceLedger::new(RetainedResourceBudget {
        capture_storage_bytes: 0,
        cache_bytes: 1,
    })
    .is_err());
}
```

- [ ] **Step 2: Run the exact RED test**

Run:

```bash
cargo test -p localview-resource-governor --test retained_resources
```

Expected: compile/test failure because retained-resource types do not exist.

- [ ] **Step 3: Implement the minimal clone-shared ledger**

In `crates/resource-governor/src/lib.rs`, add the types above and a private state:

```rust
#[derive(Debug)]
struct RetainedResourceState {
    budget: RetainedResourceBudget,
    usage: RetainedResourceUsage,
}

#[derive(Clone, Debug)]
pub struct RetainedResourceLedger {
    inner: Arc<Mutex<RetainedResourceState>>,
}
```

`admit_projected` must fail if current usage for the selected kind is already over limit or if `projected_bytes > limit`. `synchronize` must assign the selected usage first, then return a violation if actual exceeds limit. Reuse the crate's poison-tolerant `lock` helper.

- [ ] **Step 4: Run focused and crate tests**

```bash
cargo test -p localview-resource-governor --test retained_resources
cargo test -p localview-resource-governor
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/resource-governor/src/lib.rs crates/resource-governor/tests/retained_resources.rs
git commit -m "feat(governor): add retained resource authority"
```

---

### Task 2: ArtifactStore projection and truthful retained usage

**Files:**
- Modify: `crates/artifacts/src/lib.rs`
- Add tests inside `crates/artifacts/src/lib.rs` or create `crates/artifacts/tests/retained_usage.rs` if a separate integration test is cleaner.

**Interfaces:**
- Produces:
  - `ArtifactStore::used_bytes() -> u64`
  - `ArtifactStore::projected_used_bytes_after_put(bytes: &[u8]) -> Result<u64>`
- Keeps existing `ArtifactStore::put(&mut self, kind, bytes) -> Result<ArtifactMeta>` unchanged for callers.

- [ ] **Step 1: Add RED tests for projection and oversized pre-write rejection**

Cover all of:

```rust
assert_eq!(store.used_bytes(), 0);
let projected = store.projected_used_bytes_after_put(b"1234").unwrap();
assert_eq!(projected, 4);
store.put("visual/png", b"1234").await.unwrap();
assert_eq!(store.projected_used_bytes_after_put(b"1234").unwrap(), 4); // dedupe
assert_eq!(store.used_bytes(), 4);
```

For a 5-byte store, write `1234`, then project/write `5678`; projection and actual must both be 4 after LRU GC. Also attempt a 6-byte single object and assert `put` returns an error and no content-ID file was created.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-artifacts
```

Expected: FAIL because `used_bytes`/projection do not exist and oversized writes currently write then GC.

- [ ] **Step 3: Implement deterministic projection**

Add:

```rust
pub fn used_bytes(&self) -> u64 { self.used }

pub fn projected_used_bytes_after_put(&self, bytes: &[u8]) -> Result<u64> {
    let id = content_id(bytes);
    if self.index.contains_key(&id) {
        return Ok(self.used);
    }
    let incoming = bytes.len() as u64;
    anyhow::ensure!(incoming <= self.max_bytes, "artifact exceeds store byte budget");
    let mut projected = self.used.saturating_add(incoming);
    for id in &self.lru {
        if projected <= self.max_bytes { break; }
        if let Some(meta) = self.index.get(id) {
            projected = projected.saturating_sub(meta.bytes);
        }
    }
    Ok(projected)
}
```

Call projection at the beginning of new-object `put` so an individually oversized object fails before `tokio::fs::write`.

- [ ] **Step 4: Make GC accounting truthful**

Change `gc()` so an index entry is removed and `used` is decremented only after `remove_file` succeeds or returns `ErrorKind::NotFound`. Any other removal error must leave the entry/usage represented and return the error rather than pretending bytes disappeared.

- [ ] **Step 5: Run crate regression**

```bash
cargo test -p localview-artifacts
```

Expected: PASS, including existing reopen/dedupe tests.

- [ ] **Step 6: Commit**

```bash
git add crates/artifacts/src/lib.rs
git commit -m "feat(artifacts): expose truthful retained usage"
```

---

### Task 3: VisualBaselineCache side-effect-free retained projection

**Files:**
- Modify: `crates/visual/src/baseline.rs`
- Modify/add tests in `crates/visual/src/lib.rs` or `crates/visual/src/baseline.rs` test module following existing crate style.

**Interfaces:**
- Produces:
  - `VisualBaselineCache::projected_used_bytes_after_insert(session_id, image_bytes) -> Result<Option<usize>, VisualError>`
- Must not mutate `entries`, `used_bytes`, touch clock, or LRU/touched order.

- [ ] **Step 1: Add RED tests**

Test:

1. empty cache projection equals incoming bytes;
2. projection over byte budget returns `None`;
3. same-session replacement subtracts the old session bytes before adding new bytes;
4. max-entry and byte-budget eviction projection equals actual `insert` result;
5. calling projection does not change `used_bytes`, `len`, or which entry is evicted by the next real insertion.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-visual
```

Expected: FAIL because projection method does not exist.

- [ ] **Step 3: Implement pure projection**

Compute against metadata only:

```rust
pub fn projected_used_bytes_after_insert(
    &self,
    session_id: SessionId,
    image_bytes: usize,
) -> Result<Option<usize>, VisualError>
```

Reject zero/overflow-invalid arithmetic as `VisualError::InvalidBuffer` or the closest existing bounded error. Return `Ok(None)` if `image_bytes > byte_budget`. Start from `used_bytes - existing_session_bytes + image_bytes`, build a small ordered list of remaining `(session_id, bytes, touched_at)`, and subtract oldest entries until both byte and entry-count limits are satisfied. Do not call `next_tick`, `remove`, or otherwise mutate the cache.

- [ ] **Step 4: Run visual regression**

```bash
cargo test -p localview-visual
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/visual/src/baseline.rs crates/visual/src/lib.rs
git commit -m "feat(visual): project baseline retained usage"
```

---

### Task 4: Desktop owner-local retained authority and CI wiring

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/retained_visual_resource_authority_contract.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/ROADMAP.md`

**Interfaces:**
- Consumes:
  - Task 1 retained ledger types/methods.
  - Task 2 artifact projection/used bytes.
  - Task 3 baseline projection/used bytes.
- Produces:
  - owner-local `RetainedResourceLedger` inside `VisualCaptureState`;
  - exact desktop reconcile/project/admit/mutate/reconcile flow.

- [ ] **Step 1: Write desktop authority RED contract**

Create a source contract that reads production files and asserts at minimum:

```rust
let cargo = include_str!("../Cargo.toml");
assert!(cargo.contains("localview-resource-governor"));

let source = include_str!("../src/visual_capture.rs");
assert!(source.contains("RetainedResourceLedger"));
assert!(source.contains("RetainedResourceKind::CaptureStorage"));
assert!(source.contains("RetainedResourceKind::Cache"));
assert!(source.contains("projected_used_bytes_after_put"));
assert!(source.contains("projected_used_bytes_after_insert"));
assert!(source.contains("used_bytes()"));
assert!(!source.contains("/v1/runtime/resources/retained"));
```

Also lock ordering by comparing source indexes for owner helper calls rather than asserting only string presence. Keep the contract resilient to formatting but strict about reconcile → project → admit → mutation → reconcile.

- [ ] **Step 2: Run RED desktop contract**

```bash
cargo test -p localview-desktop --test retained_visual_resource_authority_contract
```

Expected: FAIL because desktop has no retained-governor dependency/wiring.

- [ ] **Step 3: Add desktop dependency and state authority**

In `Cargo.toml` add:

```toml
localview-resource-governor = { path = "../../../crates/resource-governor" }
```

In `visual_capture.rs`, change `VisualCaptureState` from derived default to explicit construction with:

```rust
retained_resources: RetainedResourceLedger,
```

using exact production budgets converted to `u64` bytes. `Default` must call `RetainedResourceLedger::new(...)` and `expect` only on compile-time-valid nonzero production constants.

- [ ] **Step 4: Wire artifact reconcile/project/admit/mutate/reconcile**

Inside the existing artifact mutex block in `persist_and_register`:

1. initialize store if needed;
2. `synchronize(CaptureStorage, store.used_bytes())`;
3. compute `projected_used_bytes_after_put(&png)`;
4. `admit_projected(CaptureStorage, projected)`;
5. call `put`;
6. always reconcile `store.used_bytes()` before leaving the owner block;
7. if put or reconciliation fails, return an error before Visual evidence registration.

Use a small private formatter/helper for deterministic retained-resource errors rather than exposing raw debug output.

- [ ] **Step 5: Wire baseline reconcile/project/admit/mutate/reconcile**

For `compatible_changed_baseline`, reconcile after lazy initialization and after `get_compatible` because incompatibility may remove a baseline.

For `commit_changed_baseline`:

1. validate/init/reconcile current cache usage;
2. call pure projection with `image.data.len()`;
3. if projection is `None`, return `Ok(false)` without mutation;
4. admit projected cache usage;
5. call `insert`;
6. reconcile exact `used_bytes()` even when insert returns an error;
7. only return `Ok(true)` if insert retained the baseline and reconciliation is healthy.

If direct field access to `RgbaImage.data` is private outside `localview-visual`, add a small public `byte_len()` method there with a focused test rather than weakening field visibility.

- [ ] **Step 6: Run focused owner regressions**

```bash
cargo test -p localview-resource-governor --test retained_resources
cargo test -p localview-artifacts
cargo test -p localview-visual
cargo test -p localview-desktop --test retained_visual_resource_authority_contract
cargo test -p localview-desktop --test changed_region_schedule_contract
cargo test -p localview-desktop --test perception_budget_capture_contract
cargo test -p localview-desktop --test perception_budget_authorization_contract
cargo test -p localview-desktop --test native_visual_executor_worker_contract
cargo test -p localview-desktop --test native_visual_diff_executor_contract
```

Expected: PASS.

- [ ] **Step 7: Add named CI gates**

In the `Tauri + frontend`/appropriate Rust contract section of `.github/workflows/ci.yml`, add exact named commands:

```yaml
- name: Retained resource governor contract
  run: cargo test -p localview-resource-governor --test retained_resources
- name: Artifact retained usage contract
  run: cargo test -p localview-artifacts
- name: Visual baseline retained usage contract
  run: cargo test -p localview-visual
- name: Desktop retained visual resource authority contract
  run: cargo test -p localview-desktop --test retained_visual_resource_authority_contract
```

Do not remove or rename existing gates.

- [ ] **Step 8: Update status docs without overclaiming**

Update `docs/IMPLEMENTATION_STATUS.md` and `docs/ROADMAP.md` to say capture-storage/cache owner-local enforcement is landed only after exact-head verification. Keep browser-process, hidden-surface, and analysis-concurrency enforcement explicitly remaining.

- [ ] **Step 9: Run full local/static verification available in the execution environment**

At minimum:

```bash
cargo test -p localview-resource-governor
cargo test -p localview-artifacts
cargo test -p localview-visual
cargo check -p localview-desktop
```

Then rely on GitHub Actions for the full cross-platform matrix and native GUI evidence.

- [ ] **Step 10: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml \
  apps/desktop/src-tauri/src/visual_capture.rs \
  apps/desktop/src-tauri/tests/retained_visual_resource_authority_contract.rs \
  .github/workflows/ci.yml docs/IMPLEMENTATION_STATUS.md docs/ROADMAP.md
git commit -m "feat(desktop): enforce retained visual resource authority"
```

---

### Task 5: PR audit, exact-head verification, merge, and post-merge proof

**Files:** No new production files unless exact CI exposes a defect.

- [ ] **Step 1: Open/update draft PR**

PR title:

```text
feat: enforce retained visual resource authority
```

Body must state Phase A scope and explicitly list browser-process/hidden-surface/analysis-concurrency as not yet complete.

- [ ] **Step 2: Require exact-head GitHub Actions**

Require full CI success on the final head. Windows UIA Observe must also remain green if triggered for the branch. Never merge using an older green SHA.

- [ ] **Step 3: Audit exact final diff and review debt**

Check changed filenames, full patch, comments, reviews, and review threads. Confirm no caller-writable retained-resource endpoint exists and no unrelated refactor entered scope.

- [ ] **Step 4: Mark ready and merge with expected head SHA**

Use `expected_head_sha=<final exact branch SHA>`.

- [ ] **Step 5: Verify exact merge commit on `main`**

Require `refs/heads/main` to resolve to the merge result before treating merge as landed.

- [ ] **Step 6: Require post-merge push CI**

Require full CI on the exact merge commit. If Windows UIA Observe is push-triggered, require it green too. Only then call Phase A complete and move automatically to Runtime Resource Governor Phase B.