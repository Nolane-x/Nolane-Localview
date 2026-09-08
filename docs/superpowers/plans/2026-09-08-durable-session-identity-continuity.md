# Durable Session Identity Continuity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the exact `SessionId` for the same unambiguous localhost logical target across daemon restarts while keeping all runtime authority transient and fail-closed.

**Architecture:** `crates/sessions` gains a versioned canonical `SessionLineage`, a bounded durable lineage-to-UUID registry, and a resolver that commits new mappings atomically before reporting them durable. `SessionManager` performs whole-batch lineage analysis before mutation, prefers safe in-memory lineage continuity, consults the resolver only for genuinely new sessions, and fences ambiguous lineages. The daemon constructs the file-backed resolver from the existing LocalView state directory before discovery starts.

**Tech Stack:** Rust 2024, Tokio, Serde/serde_json, UUID, `atomic-write-file = 0.3.1`, existing GitHub Actions matrix.

**Spec:** `docs/superpowers/specs/2026-09-08-durable-session-identity-continuity-design.md`

## Global Constraints

- Persist identity only: never serialize or restore full `Session`, preview visibility, provider attachments, action authority, evidence freshness, resource leases, or verification state.
- Durable project identity is canonical normalized project path + `ServerKind`; endpoint fallback is exact `scheme + host + port`.
- Project-anchored lineage excludes port, scheme, PID, framework/title, and command text.
- Durable identity never uses `DefaultHasher` or another unspecified process-local hash contract.
- Whole discovery batch lineage grouping happens before session-map mutation.
- Ambiguous same-lineage targets are never assigned one shared durable UUID.
- New durable UUIDs are committed atomically before being published as durable identities.
- Corrupt/unknown/over-capacity registry state is preserved and degrades to volatile identity mode; it is never silently replaced with empty state.
- Session removal does not delete durable lineage mappings; D1 uses no identity TTL.
- No second governor, no Perception Budget change, no D2 owner-liveness/boot-epoch/recovery-debt implementation.
- Explicit limits: registry file bytes `1_048_576`, records `4_096`, normalized project path bytes `4_096`, endpoint host bytes `255`, scheme bytes `32`.
- Atomic file replacement uses workspace-pinned `atomic-write-file = 0.3.1`; do not hand-roll platform-specific overwrite semantics.

---

## File structure

- Create `crates/sessions/src/identity.rs` — canonical lineage, normalization, durable registry schema/validation, atomic store, resolver, durability diagnostics.
- Modify `crates/sessions/src/lib.rs` — batch lineage analysis and `SessionManager` integration only; keep protocol `Session` transient.
- Modify `crates/sessions/Cargo.toml` — sessions-only dependencies for serde, JSON, error typing, atomic writer.
- Modify workspace `Cargo.toml` — pin `atomic-write-file = "0.3.1"` once.
- Create `crates/sessions/tests/durable_identity_continuity.rs` — public restart/current-process/ambiguity acceptance contract.
- Create `crates/sessions/tests/durable_identity_registry.rs` — public on-disk validation/limits/atomic durability contract.
- Modify `apps/daemon/src/main.rs` — construct resolver from `<state_dir>/session-identities-v1.json` before creating `SessionManager`; log healthy/degraded mode.
- Create `apps/daemon/tests/session_identity_startup_contract.rs` — source/order contract proving resolver construction precedes discovery and no runtime authority is restored there.
- Modify `.github/workflows/ci.yml` — named `Durable session identity continuity contract` gate on all three Rust-core OSes.
- Modify `docs/IMPLEMENTATION_STATUS.md` — report only verified D1 behavior and leave D2 explicitly open.

---

### Task 1: Canonical versioned session lineage

**Files:**
- Create: `crates/sessions/src/identity.rs`
- Modify: `crates/sessions/src/lib.rs`
- Modify: `crates/sessions/Cargo.toml`
- Test: unit tests in `crates/sessions/src/identity.rs`

**Interfaces:**
- Consumes: `localview_protocol::{Endpoint, ProjectIdentity, ServerKind}`.
- Produces:
  - `pub enum SessionLineage { V1(SessionLineageV1) }`
  - `pub struct SessionLineageV1 { pub anchor: SessionLineageAnchorV1, pub server_kind: SessionServerKind }`
  - `pub enum SessionLineageAnchorV1 { Project { normalized_project_path: String }, Endpoint { scheme: String, host: String, port: u16 } }`
  - `pub enum SessionServerKind` mirroring protocol `ServerKind` with explicit stable serde names.
  - `pub fn session_lineage(project: &ProjectIdentity, endpoint: &Endpoint, kind: ServerKind) -> Result<SessionLineage, SessionIdentityError>`.

- [ ] **Step 1: Write failing pure-lineage tests**

Add tests that require project lineage to ignore port/scheme/command-derived `ProjectIdentity.key`, endpoint fallback to remain exact, different `ServerKind` to differ, and path normalization to reject unresolved `..`.

```rust
#[test]
fn project_lineage_ignores_endpoint_port_and_scheme() {
    let project = project("/work/app");
    let a = session_lineage(&project, &endpoint("http", "127.0.0.1", 5173), ServerKind::FrontendDevServer).unwrap();
    let b = session_lineage(&project, &endpoint("https", "127.0.0.1", 6200), ServerKind::FrontendDevServer).unwrap();
    assert_eq!(a, b);
}

#[test]
fn endpoint_fallback_is_exact() {
    let project = ProjectIdentity::default();
    let a = session_lineage(&project, &endpoint("http", "127.0.0.1", 5173), ServerKind::UnknownHttp).unwrap();
    let b = session_lineage(&project, &endpoint("http", "127.0.0.1", 5174), ServerKind::UnknownHttp).unwrap();
    assert_ne!(a, b);
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-sessions identity::tests -- --nocapture`

Expected: FAIL because `identity` module / lineage types do not exist.

- [ ] **Step 3: Implement the minimum lineage model**

Use explicit serde tags/versioning and a pure lexical path normalizer. Actual platform normalization must call `PathFlavor::current()`; tests exercise both `Unix` and `Windows` flavor helpers on every CI OS. Do not use filesystem `canonicalize()` and do not use `DefaultHasher`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "lineage_version", content = "value")]
pub enum SessionLineage {
    #[serde(rename = "localview_session_lineage_v1")]
    V1(SessionLineageV1),
}
```

- [ ] **Step 4: Run GREEN + lint**

Run:
- `cargo test -p localview-sessions identity::tests -- --nocapture`
- `cargo clippy -p localview-sessions --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

Commit: `feat: add canonical session lineage v1`

---

### Task 2: Bounded durable registry and atomic persistence

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/sessions/Cargo.toml`
- Modify: `crates/sessions/src/identity.rs`
- Create: `crates/sessions/tests/durable_identity_registry.rs`

**Interfaces:**
- Produces:
  - `pub const SESSION_IDENTITY_REGISTRY_FILE: &str = "session-identities-v1.json"`
  - `pub enum SessionIdentityHealth { Healthy, VolatileDegraded }`
  - `pub struct SessionIdentityResolver`
  - `pub async fn SessionIdentityResolver::open_file(path: PathBuf) -> Self`
  - `pub fn health(&self) -> SessionIdentityHealth`
  - `pub fn diagnostic(&self) -> Option<&str>`
  - internal registry schema `schema_version = 1`, unique lineage and unique UUID validation.

- [ ] **Step 1: Add RED registry contracts**

The integration contract creates a unique temp directory using `std::env::temp_dir().join(format!("localview-session-identity-{}", Uuid::new_v4()))` and cleans it after each test. Cover:

```rust
#[tokio::test]
async fn missing_registry_opens_healthy_without_runtime_session_state() { /* ... */ }

#[tokio::test]
async fn corrupt_registry_is_preserved_and_enters_volatile_mode() { /* write invalid JSON; open; assert bytes unchanged */ }

#[tokio::test]
async fn unknown_schema_version_is_not_rewritten() { /* schema_version: 999 */ }

#[tokio::test]
async fn duplicate_lineage_or_duplicate_uuid_is_rejected() { /* both invariant forms */ }

#[tokio::test]
async fn oversized_registry_is_rejected_without_eviction() { /* > 1_048_576 bytes */ }
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-sessions --test durable_identity_registry -- --nocapture`

Expected: FAIL because resolver/registry API is absent.

- [ ] **Step 3: Pin atomic writer + serialization dependencies**

Workspace `Cargo.toml`:

```toml
atomic-write-file = "0.3.1"
```

Sessions `Cargo.toml`:

```toml
atomic-write-file.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 4: Implement bounded load/validation and atomic commit primitive**

Use `tokio::task::spawn_blocking` for file load/commit so filesystem sync does not block the async runtime. `atomic-write-file::AtomicWriteFile` writes the complete serialized next state, `sync_all()` is called before `commit()`, and parent-directory durability is requested/surfaced according to the crate API. A failed commit must leave the in-memory authoritative registry unchanged.

Registry validation rejects nil UUIDs, duplicate lineages, same UUID on different lineages, noncanonical lineage payload, too many records, and size limit violations.

- [ ] **Step 5: Add internal fault-injection unit test**

Inside `identity.rs`, keep the file backend behind a private commit abstraction so a test backend can fail before commit. Assert previous registry bytes/state remain authoritative and no newly generated UUID is recorded as durable.

- [ ] **Step 6: Run GREEN**

Run:
- `cargo test -p localview-sessions --test durable_identity_registry -- --nocapture`
- `cargo test -p localview-sessions identity::tests -- --nocapture`
- `cargo clippy -p localview-sessions --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 7: Commit**

Commit: `feat: add durable session identity registry`

---

### Task 3: Resolver commit-before-publish and volatile degradation

**Files:**
- Modify: `crates/sessions/src/identity.rs`
- Test: unit tests in `crates/sessions/src/identity.rs`

**Interfaces:**
- Produces:
  - `pub enum SessionIdentityDurability { Durable, Volatile }`
  - `pub struct ResolvedSessionIdentity { pub session_id: SessionId, pub durability: SessionIdentityDurability }`
  - `pub async fn resolve_new(&self, lineage: &SessionLineage) -> ResolvedSessionIdentity`
  - `pub async fn existing(&self, lineage: &SessionLineage) -> Option<SessionId>` for healthy registry reads only.

- [ ] **Step 1: Write RED resolver tests**

Require:

```rust
#[tokio::test]
async fn existing_mapping_reuses_exact_uuid() { /* open resolver twice against same file */ }

#[tokio::test]
async fn new_uuid_is_durable_only_after_commit_success() { /* recording backend observes commit before return */ }

#[tokio::test]
async fn commit_failure_returns_volatile_uuid_and_does_not_mutate_mapping() { /* fault backend */ }

#[tokio::test]
async fn full_registry_returns_volatile_without_evicting_existing_records() { /* 4096 records */ }
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-sessions identity::tests::resolver -- --nocapture`

Expected: FAIL because resolver methods/durability result do not exist.

- [ ] **Step 3: Implement minimal resolver semantics**

`resolve_new` first checks healthy existing mapping. For a new lineage it generates UUID, clones next registry state, commits next state, and only after commit updates in-memory mapping and returns `Durable`. Any degraded state/capacity/commit failure returns a fresh `Volatile` UUID and keeps durable state unchanged.

- [ ] **Step 4: Run GREEN**

Run full sessions unit tests + Clippy.

- [ ] **Step 5: Commit**

Commit: `feat: resolve durable and volatile session identities`

---

### Task 4: SessionManager whole-batch reconciliation integration

**Files:**
- Modify: `crates/sessions/src/lib.rs`
- Create: `crates/sessions/tests/durable_identity_continuity.rs`

**Interfaces:**
- `SessionManager::new(grace)` remains available and uses volatile-only identity for compatibility/tests that do not opt into persistence.
- Add `pub fn SessionManager::with_identity_resolver(grace: Duration, resolver: SessionIdentityResolver) -> Self`.
- Manager owns `lineages: RwLock<HashMap<SessionId, SessionLineage>>` together with session state under one reconciliation authority; implementation may consolidate into one `RwLock<SessionState>` to make session+lineage mutation atomic.
- `reconcile` public signature remains unchanged.

- [ ] **Step 1: Add RED public continuity contract**

Cover all acceptance-critical flows:

```rust
#[tokio::test]
async fn same_project_reuses_uuid_across_fresh_manager_lifetimes() { /* same registry path */ }

#[tokio::test]
async fn project_port_and_scheme_changes_keep_uuid() { /* restart and same process */ }

#[tokio::test]
async fn command_and_port_change_together_keep_current_uuid() { /* same manager */ }

#[tokio::test]
async fn projectless_port_change_gets_new_uuid() { /* endpoint lineage */ }

#[tokio::test]
async fn same_root_different_server_kind_gets_distinct_ids() { /* ... */ }

#[tokio::test]
async fn ambiguous_same_lineage_targets_never_alias() { /* two discovered in one batch */ }

#[tokio::test]
async fn removed_session_keeps_durable_mapping_for_later_return() { /* remove after grace, new manager or later scan reuses */ }
```

Also assert a reused UUID starts with fresh runtime fields (`preview_visible == false`, fresh status/timestamps) rather than restoring an old `Session`.

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-sessions --test durable_identity_continuity -- --nocapture`

Expected: FAIL because `with_identity_resolver` and batch lineage reconciliation are absent.

- [ ] **Step 3: Implement batch analysis before mutation**

For each discovered target, compute `ProjectIdentity` + canonical lineage first, group by lineage, and mark ambiguity before touching session state.

Matching order:

Unambiguous lineage:
1. exact current canonical lineage;
2. compatibility `ProjectIdentity.key + ServerKind`;
3. exact endpoint;
4. durable resolver.

Ambiguous lineage:
1. exact endpoint/current safe match only;
2. otherwise new volatile UUID; never consume or overwrite the shared durable mapping.

Maintain exact `seen` semantics and remove session-lineage side state when the in-memory session expires, but never delete registry records.

- [ ] **Step 4: Add no-repeated-write oracle**

After first durable mapping commit, perform several healthy scans that match the current in-memory session and assert registry file bytes and modified timestamp remain unchanged. If filesystem timestamp resolution is coarse, also assert an internal test commit counter remains unchanged.

- [ ] **Step 5: Run GREEN + existing regressions**

Run:
- `cargo test -p localview-sessions --test durable_identity_continuity -- --nocapture`
- `cargo test -p localview-sessions`
- `cargo clippy -p localview-sessions --all-targets -- -D warnings`

Expected: existing `reconnects_same_project_when_port_changes` and removal tests remain green.

- [ ] **Step 6: Commit**

Commit: `feat: preserve durable session identity during reconciliation`

---

### Task 5: Daemon startup construction and degraded-mode diagnostics

**Files:**
- Modify: `apps/daemon/src/main.rs`
- Create: `apps/daemon/tests/session_identity_startup_contract.rs`

**Interfaces:**
- Daemon computes `let state_root = state_dir()?;` once near startup.
- It opens `SessionIdentityResolver::open_file(state_root.join(SESSION_IDENTITY_REGISTRY_FILE)).await` before constructing `SessionManager` and before `DiscoveryEngine::new`.
- It constructs `SessionManager::with_identity_resolver(config.disconnect_grace, resolver)`.
- Healthy mode logs continuity available; degraded mode warns continuity unavailable without terminating daemon.

- [ ] **Step 1: Add RED startup contract**

Source/order contract asserts the daemon source contains, in order:

```text
state_dir
SessionIdentityResolver::open_file
SessionManager::with_identity_resolver
DiscoveryEngine::new
```

It also asserts startup does **not** contain any D1 call that restores preview visibility, resource leases, provider attachments, or action permits.

- [ ] **Step 2: Run RED**

Run: `cargo test -p localview-daemon --test session_identity_startup_contract -- --nocapture`

Expected: FAIL because daemon still uses `SessionManager::new`.

- [ ] **Step 3: Implement startup wiring**

Reuse `state_root` for consequential-recovery path, Chromium runtime path, token path where practical without unrelated refactor. Open identity registry before discovery and log health/diagnostic. Do not abort daemon merely because resolver is degraded.

- [ ] **Step 4: Run GREEN + daemon compile**

Run:
- `cargo test -p localview-daemon --test session_identity_startup_contract -- --nocapture`
- `cargo check -p localview-daemon`
- `cargo clippy -p localview-daemon --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

Commit: `feat: load durable session identities at daemon startup`

---

### Task 6: Permanent CI gate and implementation-status closure

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`

**Interfaces:**
- Named Rust-core gate: `Durable session identity continuity contract`.

- [ ] **Step 1: Add named CI gate**

Place after `Tests` and before unrelated executor/perception gates:

```yaml
- name: Durable session identity continuity contract
  run: |
    cargo test -p localview-sessions --test durable_identity_registry
    cargo test -p localview-sessions --test durable_identity_continuity
    cargo test -p localview-daemon --test session_identity_startup_contract
```

This gate runs on Ubuntu, macOS, and Windows because it lives inside `rust-core` matrix.

- [ ] **Step 2: Update implementation status precisely**

Add only proven D1 facts: versioned lineage, atomic registry, ambiguity fence, durable/volatile modes, daemon startup integration, and explicit authority non-restoration. State that D2 owner-instance liveness / boot epoch / recovery debt remains open.

- [ ] **Step 3: Run local/package verification where available**

Run `cargo fmt --all -- --check` and the three named contract commands above.

- [ ] **Step 4: Commit**

Commit: `ci: gate durable session identity continuity`

---

### Task 7: PR exact-head verification and merge closure

**Files:**
- No new production files unless CI exposes a real defect.
- PR metadata only after tests are green.

**Interfaces:**
- PR must target `main` with expected exact head.

- [ ] **Step 1: Open/refresh draft PR after first RED commit**

PR title: `feat: preserve session identity across daemon restart`.

PR body records RED→GREEN evidence per task, explicit non-goals, and D2 boundary.

- [ ] **Step 2: Verify exact PR head**

Require:
- full `CI` matrix success;
- `Durable session identity continuity contract` success on Ubuntu/macOS/Windows;
- existing Tauri/frontend and three real GUI smoke jobs remain green;
- Windows UIA workflow remains green;
- no unexpected changed files outside D1 scope.

- [ ] **Step 3: Whole-PR scope audit**

Compare against `105f9d50a367a05fa6e0012398ebcd33fc321c5d`. Reject any Perception Budget, governor, desktop-surface, provider-authority, or unrelated runtime changes.

- [ ] **Step 4: Ready + expected-head merge**

Only after fresh exact-head verification, update PR body from RED status, mark ready, and merge with the exact verified head SHA.

- [ ] **Step 5: Post-merge verification**

Confirm `main` points to the merge commit and push-triggered CI + Windows UIA both complete successfully before declaring D1 closed.
