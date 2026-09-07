# Runtime Resource Governor V2 — Retained Visual Resources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce and reconcile authoritative retained usage for desktop visual artifact storage and changed-region baseline cache without adding caller-writable resource authority.

**Architecture:** Add a clone-shared retained-resource ledger to `localview-resource-governor`, then let each owner compute projected and actual usage through its own deterministic policy. Desktop visual capture holds the owner mutex across reconcile → project → admit → mutate → reconcile, preserving existing capture/evidence/baseline ordering and avoiding cross-process mutable counters.

**Tech Stack:** Rust, Tokio, Tauri 2, localview-resource-governor, localview-artifacts, localview-visual, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-07-runtime-resource-governor-v2-retained-design.md`

## Global Constraints

- Capture-storage retained limit remains exactly `256 * 1024 * 1024` bytes.
- Baseline cache retained limit remains exactly `96 * 1024 * 1024` bytes and 32 entries.
- No new public or bearer-authenticated endpoint may mutate desktop artifact/cache retained usage.
- `synchronize` records observed owner reality before reporting an over-limit violation.
- `admit_projected` is read-only and never mutates usage.
- Perception Budget dimensions remain unchanged.
- Existing visual order remains settle → freeze → native capture → exact restore → private redaction → validation/processing → budget admission → artifact/evidence emission → baseline commit.
- No existing CI, Windows UIA, or GUI smoke gate may be weakened.

---

### Task 1: Retained-resource ledger core

**Files:**
- Create: `crates/resource-governor/tests/retained_resources.rs`
- Modify: `crates/resource-governor/src/lib.rs`

**Interfaces:**
- Produces `RetainedResourceKind::{CaptureStorage, Cache}`.
- Produces `RetainedResourceBudget { capture_storage_bytes: u64, cache_bytes: u64 }`.
- Produces `RetainedResourceUsage { capture_storage_bytes: u64, cache_bytes: u64 }`.
- Produces `RetainedResourceViolation { kind, current_bytes, projected_or_observed_bytes, limit_bytes }`.
- Produces `RetainedResourceLedger::{new, usage, admit_projected, synchronize}`.

- [ ] **Step 1: Write the failing integration contract**

Create `crates/resource-governor/tests/retained_resources.rs` with these exact behavioral tests:

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
    assert_eq!(denial.kind, RetainedResourceKind::CaptureStorage);
    assert_eq!(denial.current_bytes, 90);
    assert_eq!(denial.projected_or_observed_bytes, 101);
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
fn failed_projected_admission_does_not_mutate_usage() {
    let ledger = ledger();
    ledger.synchronize(RetainedResourceKind::CaptureStorage, 7).unwrap();
    assert!(ledger
        .admit_projected(RetainedResourceKind::CaptureStorage, 101)
        .is_err());
    assert_eq!(ledger.usage().capture_storage_bytes, 7);
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
    assert!(RetainedResourceLedger::new(RetainedResourceBudget {
        capture_storage_bytes: 1,
        cache_bytes: 0,
    })
    .is_err());
}
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-resource-governor --test retained_resources
```

Expected: compile failure because retained-resource types do not exist.

- [ ] **Step 3: Implement the minimal ledger**

Add the four public types and private shared state in `crates/resource-governor/src/lib.rs`. Use the crate's existing `Arc<Mutex<_>>` and poison-tolerant `lock` helper.

`RetainedResourceLedger::new` returns `Err(RetainedResourceViolation)` for the first zero limit, with `current_bytes = 0`, `projected_or_observed_bytes = 0`, and `limit_bytes = 0`.

`admit_projected(kind, projected)` reads the selected current/limit; it returns violation if `current > limit` or `projected > limit`, otherwise `Ok(())`.

`synchronize(kind, actual)` writes the selected usage first. If `actual > limit`, return a violation containing the newly recorded actual value; otherwise return `Ok(())`.

- [ ] **Step 4: Run GREEN and regression**

```bash
cargo test -p localview-resource-governor --test retained_resources
cargo test -p localview-resource-governor
```

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat(governor): add retained resource authority`.

---

### Task 2: ArtifactStore projection and truthful retained usage

**Files:**
- Modify: `crates/artifacts/src/lib.rs`

**Interfaces:**
- Produce `ArtifactStore::used_bytes() -> u64`.
- Produce `ArtifactStore::projected_used_bytes_after_put(bytes: &[u8]) -> Result<u64>`.
- Keep `ArtifactStore::put(&mut self, kind, bytes) -> Result<ArtifactMeta>` unchanged.

- [ ] **Step 1: Add RED unit tests in the existing `#[cfg(test)] mod tests`**

Add tests that prove:

```rust
#[tokio::test]
async fn retained_projection_matches_dedupe_and_lru_result() {
    let dir = test_dir("retained-projection");
    let mut store = ArtifactStore::open(&dir, 5).await.unwrap();
    assert_eq!(store.used_bytes(), 0);
    assert_eq!(store.projected_used_bytes_after_put(b"1234").unwrap(), 4);
    let first = store.put("visual/png", b"1234").await.unwrap();
    assert_eq!(store.used_bytes(), 4);
    assert_eq!(store.projected_used_bytes_after_put(b"1234").unwrap(), 4);
    let duplicate = store.put("visual/png", b"1234").await.unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(store.used_bytes(), 4);
    assert_eq!(store.projected_used_bytes_after_put(b"5678").unwrap(), 4);
    store.put("visual/png", b"5678").await.unwrap();
    assert_eq!(store.used_bytes(), 4);
    assert_eq!(disk_bytes(&dir).await, 4);
    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn oversized_single_artifact_is_rejected_before_file_creation() {
    let dir = test_dir("oversized-prewrite");
    let mut store = ArtifactStore::open(&dir, 5).await.unwrap();
    let id = content_id(b"123456");
    assert!(store.put("visual/png", b"123456").await.is_err());
    assert!(!dir.join(id).exists());
    assert_eq!(store.used_bytes(), 0);
    let _ = tokio::fs::remove_dir_all(dir).await;
}
```

Add a third unit test that creates one indexed artifact, rewrites that entry's private `meta.path` to a directory path, pushes usage over the limit with a new file, invokes the GC path through `put`, and asserts the operation errors while the failed-to-delete entry's bytes remain included in `store.used_bytes()`.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-artifacts
```

Expected: compile/test failure because usage/projection APIs do not exist and oversized objects are currently written before GC.

- [ ] **Step 3: Implement deterministic projection**

Add:

```rust
pub fn used_bytes(&self) -> u64 {
    self.used
}

pub fn projected_used_bytes_after_put(&self, bytes: &[u8]) -> Result<u64> {
    let id = content_id(bytes);
    if self.index.contains_key(&id) {
        return Ok(self.used);
    }
    let incoming = bytes.len() as u64;
    anyhow::ensure!(incoming <= self.max_bytes, "artifact exceeds store byte budget");
    let mut projected = self.used.saturating_add(incoming);
    for id in &self.lru {
        if projected <= self.max_bytes {
            break;
        }
        if let Some(meta) = self.index.get(id) {
            projected = projected.saturating_sub(meta.bytes);
        }
    }
    Ok(projected)
}
```

At the start of the new-object branch in `put`, call `self.projected_used_bytes_after_put(bytes)?;` before `tokio::fs::write`.

- [ ] **Step 4: Make GC deletion accounting truthful**

In `gc`, do not remove the index entry or decrement `used` until backing-file deletion succeeds. Treat `std::io::ErrorKind::NotFound` as already removed. On any other delete error, leave the index entry and `used` unchanged and return the error.

- [ ] **Step 5: Run GREEN and regression**

```bash
cargo test -p localview-artifacts
```

Expected: PASS including existing dedupe/reopen tests.

- [ ] **Step 6: Commit**

Commit message: `feat(artifacts): expose truthful retained usage`.

---

### Task 3: VisualBaselineCache side-effect-free retained projection

**Files:**
- Modify: `crates/visual/src/baseline.rs`

**Interfaces:**
- Produce `VisualBaselineCache::projected_used_bytes_after_insert(session_id: SessionId, image_bytes: usize) -> Result<Option<usize>, VisualError>`.
- Projection does not mutate entries, clock, touch order, or used bytes.

- [ ] **Step 1: Add RED tests in a new `#[cfg(test)] mod tests` at the bottom of `baseline.rs`**

Use `uuid::Uuid::new_v4()` through the `SessionId` alias and tiny valid `RgbaImage` values. Add tests for:

1. empty-cache projection;
2. `image_bytes > byte_budget` returns `Ok(None)`;
3. same-session replacement projection subtracts old bytes;
4. max-entry LRU projection equals the next real insertion's `used_bytes()`;
5. projection is side-effect-free by comparing `len`, `used_bytes`, and the next real eviction result before/after projection.

For deterministic small images, use valid RGBA payloads such as `RgbaImage { width: 1, height: 1, data: vec![0; 4] }` and `RgbaImage { width: 2, height: 1, data: vec![0; 8] }`.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-visual
```

Expected: compile failure because projection method does not exist.

- [ ] **Step 3: Implement pure projection**

Add exactly:

```rust
pub fn projected_used_bytes_after_insert(
    &self,
    session_id: SessionId,
    image_bytes: usize,
) -> Result<Option<usize>, VisualError>
```

Return `Err(VisualError::InvalidBuffer)` when `image_bytes == 0`. Return `Ok(None)` when `image_bytes > self.byte_budget`.

Calculate base usage as `self.used_bytes - existing_session_bytes + image_bytes` using checked arithmetic. Build a local vector of all entries except the replaced session, sorted by `(touched_at, session_id)`. While projected bytes exceed `byte_budget` or projected entry count exceeds `max_entries`, subtract the oldest entry's bytes and decrement projected entry count. Do not call `next_tick`, `remove`, or mutate any field.

- [ ] **Step 4: Run GREEN and regression**

```bash
cargo test -p localview-visual
```

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat(visual): project baseline retained usage`.

---

### Task 4: Desktop owner-local retained authority

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/visual_capture.rs`
- Create: `apps/desktop/src-tauri/tests/retained_visual_resource_authority_contract.rs`

**Interfaces:**
- Consume the Task 1 ledger, Task 2 artifact projection, and Task 3 baseline projection.
- Add private `retained_resources: RetainedResourceLedger` to `VisualCaptureState`.

- [ ] **Step 1: Write RED desktop source contract**

Create `apps/desktop/src-tauri/tests/retained_visual_resource_authority_contract.rs`. It must read `../Cargo.toml` and `../src/visual_capture.rs`, assert the dependency and both retained kinds/projection methods/`used_bytes()`, assert no `/v1/runtime/resources/retained` string exists, and compare source positions inside `persist_and_register` and `commit_changed_baseline` to lock this order:

`retained synchronize` < `projected_used_bytes...` < `admit_projected` < owner `put/insert` < second `synchronize`.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-desktop --test retained_visual_resource_authority_contract
```

Expected: FAIL because the desktop does not depend on or own retained authority yet.

- [ ] **Step 3: Add dependency and explicit state default**

Add to desktop `Cargo.toml`:

```toml
localview-resource-governor = { path = "../../../crates/resource-governor" }
```

Import the retained types in `visual_capture.rs`. Remove `#[derive(Default)]` from `VisualCaptureState` and implement `Default` explicitly. Construct the ledger with `VISUAL_ARTIFACT_BUDGET_BYTES` and `VISUAL_BASELINE_BUDGET_BYTES as u64`; `expect("production retained visual resource budgets are nonzero")` is permitted because both limits are compile-time constants.

- [ ] **Step 4: Add deterministic retained-resource error helper**

Add a private helper that converts `RetainedResourceViolation` to a stable message containing only resource kind/current/observed-or-projected/limit bytes. Do not serialize internal mutex state or paths.

- [ ] **Step 5: Wire artifact reconcile → project → admit → mutate → reconcile**

Inside the existing artifact mutex block in `persist_and_register`:

1. initialize `ArtifactStore` if absent;
2. call `synchronize(CaptureStorage, store.used_bytes())`;
3. compute `projected_used_bytes_after_put(&png)`;
4. call `admit_projected(CaptureStorage, projected)`;
5. call `store.put("visual/png", &png).await`;
6. capture `store.used_bytes()` after the put attempt;
7. call `synchronize(CaptureStorage, actual)` before leaving the mutex block;
8. if put failed, return its error after reconciliation; if reconciliation reports over-limit, return the retained-resource error; only otherwise use the returned artifact ID.

- [ ] **Step 6: Wire baseline reconcile → project → admit → mutate → reconcile**

In `compatible_changed_baseline`, reconcile `Cache` from `cache.used_bytes()` after lazy initialization and again after `get_compatible`.

In `commit_changed_baseline`:

1. initialize/reconcile cache;
2. validate `image` with `image.validate()` and verify context dimensions exactly as existing `insert` already does;
3. call `projected_used_bytes_after_insert(session_id, image.data.len())`;
4. if `None`, return `Ok(false)` without mutation;
5. admit projected cache bytes;
6. call `cache.insert(...)`;
7. capture actual `cache.used_bytes()` and synchronize before leaving the mutex block;
8. return mutation error after reconciliation if insertion failed; return retained-resource violation if reconciliation is over-limit; otherwise return the insertion boolean.

- [ ] **Step 7: Run focused desktop and owner regressions**

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

- [ ] **Step 8: Commit**

Commit message: `feat(desktop): enforce retained visual resource authority`.

---

### Task 5: Named CI gates and status docs

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1: Add four additive CI commands to the `Tauri + frontend` job after the existing desktop/native visual contract commands**

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

Do not remove or rename any existing gate.

- [ ] **Step 2: Update implementation status precisely**

Add that capture-storage and visual baseline-cache enforcement now use owner-local retained-resource authority, exact owner projection/reconciliation, and no caller-writable counter. Keep browser-process, hidden-surface, and analysis-concurrency enforcement explicitly remaining.

- [ ] **Step 3: Update roadmap precisely**

Move capture-storage/cache from the remaining governor list into the landed Wave 2 runtime description, but leave browser-process/hidden-surface/analysis-concurrency as remaining. Do not claim the entire Runtime Resource Governor complete.

- [ ] **Step 4: Run compile/static regression**

```bash
cargo test -p localview-resource-governor
cargo test -p localview-artifacts
cargo test -p localview-visual
cargo check -p localview-desktop
```

Expected: PASS in an environment with the repository toolchain/dependencies; GitHub Actions remains canonical for the full cross-platform matrix and native GUI proof.

- [ ] **Step 5: Commit**

Commit message: `ci: gate retained visual resource authority`.

---

### Task 6: PR audit, exact-head verification, merge, and post-merge proof

**Files:** No planned production changes. Any failure-driven fix starts a new RED→GREEN sub-cycle and changes the exact head that must be verified.

- [ ] **Step 1: Create/update draft PR**

Title: `feat: enforce retained visual resource authority`.

Body must name Phase A and explicitly state browser-process, hidden-surface, and analysis-concurrency enforcement remain incomplete.

- [ ] **Step 2: Require exact final-head GitHub Actions**

Require full CI success on the exact final branch SHA. If Windows UIA Observe is triggered for the branch, require it green too. Never reuse an older green SHA.

- [ ] **Step 3: Audit final scope and review debt**

Fetch changed filenames, full patch, PR comments, reviews, and review threads. Confirm no caller-writable retained-resource endpoint, no Perception Budget dimension change, and no unrelated refactor.

- [ ] **Step 4: Mark ready and merge with expected-head locking**

Merge only with `expected_head_sha=<exact verified final SHA>`.

- [ ] **Step 5: Verify `main` exact merge commit**

Fetch `refs/heads/main` and require it to equal the merge result before treating the code as landed.

- [ ] **Step 6: Require post-merge push verification**

Require full CI on the exact merge commit. Require Windows UIA Observe too when the push workflow triggers. Only then mark Phase A complete and continue automatically to Runtime Resource Governor Phase B.