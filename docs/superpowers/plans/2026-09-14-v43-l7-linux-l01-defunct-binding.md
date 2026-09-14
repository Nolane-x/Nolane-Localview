# V4.3 Linux L01 — AT-SPI DEFUNCT Binding Invalidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a production Linux AT-SPI provider boundary that terminally invalidates a binding after a real `State::Defunct` observation and proves the defense with an exact-head Ubuntu AT-SPI oracle.

**Architecture:** Introduce `localview-linux-atspi-provider` as a sibling of the Windows UIA and macOS AX providers. Keep binding identity/lifecycle and opaque action-eligibility authority inside that crate; live Linux builds query `atspi` 0.30.0, while unit contracts use a validation-only typed-state injection surface that is absent from shipping builds. A standalone Ubuntu L7 harness drives a real GTK3/ATK/AT-SPI accessible through the shipping provider path and refuses closure unless the old accessible reference itself reports `DEFUNCT`.

**Tech Stack:** Rust 2024 / rust-version 1.85; exact `atspi = 0.30.0`; zbus through the `atspi` re-export; existing `localview-protocol` provider/target incarnation refs; GTK3 + ATK bridge + AT-SPI2 + D-Bus + Xvfb for the real Ubuntu oracle; GitHub Actions exact-SHA workflows.

**Spec:** `docs/superpowers/specs/2026-09-14-v43-l7-linux-l01-defunct-binding-design.md`

## Global Constraints

- Scope is V43-L01 only: `AT-SPI DEFUNCT still actionable -> invalid binding`.
- Base authority is `main@fb8c0b21c4a56369cd22312253c708ea5630c7dd`.
- Workspace Rust floor remains exactly `1.85`; edition remains `2024`.
- The production dependency target is exact `atspi = "=0.30.0"`; if Rust 1.85 cannot resolve/check it, stop and amend the approved design before choosing another version.
- `crates/native-provider` must not gain `atspi`, zbus, portal, or PipeWire transport ownership.
- Only a typed `State::Defunct` membership observed from AT-SPI may create `InvalidDefunct`.
- Transport failure/unavailable must fail closed but must not be relabeled DEFUNCT.
- `State::Stale` is not L01 and must not be treated as DEFUNCT.
- Once one binding revision becomes `InvalidDefunct`, that exact binding can never return to live/actionable state.
- Same bus name/object path/role/name reuse never revives the old binding; reacquisition creates a new binding revision.
- The action permit is an L01 eligibility token only; L01 does not add a general Linux action executor.
- Validation state injection is unit-test-only and must be absent from the default shipping API.
- The real-provider gate must observe platform `DEFUNCT`; object disappearance/unavailable is not a substitute.
- No existing Windows/macOS gate may be weakened, skipped, `continue-on-error`, or conditionally bypassed.

---

## File Structure

Create:

```text
crates/linux-atspi-provider/
  Cargo.toml
  src/
    lib.rs          # exports; shipping validation-surface compile-fail sentinel
    binding.rs      # endpoint, binding revision, terminal lifecycle, opaque permit/errors
    provider.rs     # Linux live AT-SPI connection/state read + authorization
  tests/
    l01_defunct_binding_contract.rs

.github/workflows/
  v43-l01-contract.yml
  v43-l01-real-provider.yml

tools/provider-seeds/linux-atspi-defunct-seed/
  seed.py

tools/validation-lab/linux-l7-real-provider-harness/
  Cargo.toml
  tests/v43_real_provider_l01.rs
```

Modify:

```text
Cargo.toml
```

Do not modify Windows UIA or macOS AX implementation files for L01.

---

### Task 1: Exact AT-SPI Dependency / Rust 1.85 Preflight

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/linux-atspi-provider/Cargo.toml`
- Create: `crates/linux-atspi-provider/src/lib.rs`

**Interfaces:**
- Produces workspace package `localview-linux-atspi-provider`.
- No production L01 behavior is implemented in this task.

- [ ] **Step 1: Register the crate after `crates/macos-ax-provider`**

Add:

```toml
"crates/linux-atspi-provider",
```

Create `crates/linux-atspi-provider/Cargo.toml`:

```toml
[package]
name = "localview-linux-atspi-provider"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]
path = "src/lib.rs"

[features]
default = []
validation-state-injection = []

[dependencies]
localview-native-provider = { path = "../native-provider" }
localview-protocol = { path = "../protocol" }
thiserror.workspace = true
uuid.workspace = true

[target.'cfg(target_os = "linux")'.dependencies]
atspi = { version = "=0.30.0", features = ["zbus"] }

[dev-dependencies]
tokio.workspace = true
```

Create `src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

- [ ] **Step 2: Prove exact dependency resolution under the repository Rust floor**

Run:

```bash
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 check -p localview-linux-atspi-provider --all-targets
cargo +1.85.0 tree -p localview-linux-atspi-provider | grep '^atspi v0.30.0'
```

Expected: both commands succeed and dependency tree contains exact `atspi v0.30.0` on Linux. If Cargo reports an MSRV conflict, do not change the version; stop and amend/re-review the design.

- [ ] **Step 3: Commit dependency preflight scaffold**

```bash
git add Cargo.toml crates/linux-atspi-provider
git commit -m "build(v43): preflight Linux AT-SPI provider dependency"
```

---

### Task 2: Permanent L01 RED Contract + Exact-SHA Workflow

**Files:**
- Create: `crates/linux-atspi-provider/tests/l01_defunct_binding_contract.rs`
- Create: `.github/workflows/v43-l01-contract.yml`

**Interfaces required by the RED test:**

```rust
AtspiEndpoint::new(bus_name, object_path)
LinuxAtspiProvider::new_for_validation(provider_ref, target_ref)
LinuxAtspiProvider::bind(endpoint, acquisition_cut_ref)
LinuxAtspiProvider::authorize_from_state_set_for_validation(&binding, StateSet)
AtspiBindingLifecycle::{Live, InvalidDefunct}
AtspiActionEligibilityError::{Defunct, AlreadyInvalidDefunct, ObservationUnavailable, ProviderIncarnationMismatch, TargetIncarnationMismatch}
AtspiActionEligibilityPermit::binding_revision()
AtspiElementBinding::{binding_revision, lifecycle, endpoint}
```

- [ ] **Step 1: Write the initial RED**

Use a Linux-only integration test:

```rust
#![cfg(target_os = "linux")]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiBindingLifecycle, AtspiEndpoint, LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider() -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from("provider:linux-atspi:test:1"),
        TargetIncarnationRef::from("target:linux-atspi:test:1"),
    )
}

#[test]
fn explicit_defunct_observation_terminally_blocks_action_eligibility() {
    let provider = provider();
    let binding = provider.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/7"),
        "cut:l01:1",
    );

    let live = provider
        .authorize_from_state_set_for_validation(&binding, StateSet::empty())
        .expect("non-defunct state may pass only the L01 liveness gate");
    assert_eq!(live.binding_revision(), binding.binding_revision());
    drop(live);

    let defunct = StateSet::new(State::Defunct);
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&binding, defunct),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(binding.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);
}
```

- [ ] **Step 2: Commit RED before implementing the API**

```bash
git add crates/linux-atspi-provider/tests/l01_defunct_binding_contract.rs .github/workflows/v43-l01-contract.yml
git commit -m "test(v43): require Linux L01 DEFUNCT invalidation"
```

The workflow must checkout `${{ github.event.pull_request.head.sha || github.sha }}`, verify `git rev-parse HEAD` equals that SHA, then run:

```bash
cargo test -p localview-linux-atspi-provider --features validation-state-injection --test l01_defunct_binding_contract -- --nocapture
```

- [ ] **Step 3: Verify RED**

Expected failure: unresolved Linux L01 provider/binding/permit API. A YAML, dependency-download, runner, or unrelated compile failure does not count as semantic RED.

---

### Task 3: GREEN-A Provider-Owned Binding Lifecycle and Opaque Permit

**Files:**
- Create: `crates/linux-atspi-provider/src/binding.rs`
- Create: `crates/linux-atspi-provider/src/provider.rs`
- Modify: `crates/linux-atspi-provider/src/lib.rs`
- Extend: `crates/linux-atspi-provider/tests/l01_defunct_binding_contract.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtspiEndpoint { bus_name: String, object_path: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiBindingLifecycle { Live, InvalidDefunct }

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiActionEligibilityError {
    #[error("AT-SPI binding is DEFUNCT")]
    Defunct,
    #[error("AT-SPI binding was already invalidated as DEFUNCT")]
    AlreadyInvalidDefunct,
    #[error("AT-SPI state observation is unavailable")]
    ObservationUnavailable,
    #[error("AT-SPI binding provider incarnation mismatch")]
    ProviderIncarnationMismatch,
    #[error("AT-SPI binding target incarnation mismatch")]
    TargetIncarnationMismatch,
}
```

`AtspiElementBinding` stores provider/target refs, endpoint, acquisition cut, monotonically generated `u64` binding revision, and an `Arc<Mutex<AtspiBindingLifecycle>>`. Clones share the lifecycle so no stale clone can outlive terminal invalidation.

`AtspiActionEligibilityPermit` has private fields and no public constructor; expose read-only `binding_revision()`, `provider_incarnation_ref()`, `target_incarnation_ref()` and `acquisition_cut_ref()`.

- [ ] **Step 1: Implement minimal lifecycle and validation-only typed state path**

Core decision function:

```rust
fn authorize_from_observed_state(
    binding: &AtspiElementBinding,
    states: atspi::StateSet,
) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
    if binding.lifecycle() == AtspiBindingLifecycle::InvalidDefunct {
        return Err(AtspiActionEligibilityError::AlreadyInvalidDefunct);
    }
    if states.contains(atspi::State::Defunct) {
        binding.invalidate_defunct();
        return Err(AtspiActionEligibilityError::Defunct);
    }
    Ok(AtspiActionEligibilityPermit::mint(binding))
}
```

Expose it only through:

```rust
#[cfg(all(target_os = "linux", feature = "validation-state-injection"))]
pub fn authorize_from_state_set_for_validation(...)
```

- [ ] **Step 2: Add negative contract cases**

Tests must prove:

```text
State::Stale without Defunct does not become Defunct
State::Visible without Defunct does not become liveness/occlusion proof beyond the L01 permit
provider lineage mismatch is typed ProviderIncarnationMismatch
target lineage mismatch is typed TargetIncarnationMismatch
```

- [ ] **Step 3: Verify GREEN**

```bash
cargo test -p localview-linux-atspi-provider --features validation-state-injection --test l01_defunct_binding_contract -- --nocapture
cargo check -p localview-linux-atspi-provider --all-targets
cargo clippy -p localview-linux-atspi-provider --all-targets -- -D warnings
```

- [ ] **Step 4: Add a shipping-surface compile-fail sentinel**

In `lib.rs`, document that `new_for_validation` / `authorize_from_state_set_for_validation` are unavailable without the feature using a `compile_fail` doctest. This prevents synthetic state injection from becoming ordinary product authority.

- [ ] **Step 5: Commit GREEN-A**

```bash
git add crates/linux-atspi-provider
git commit -m "feat(v43): add Linux L01 DEFUNCT binding authority"
```

---

### Task 4: RED-B / GREEN-B Terminal Reuse and Reacquisition

**Files:**
- Extend: `crates/linux-atspi-provider/tests/l01_defunct_binding_contract.rs`
- Modify: `crates/linux-atspi-provider/src/binding.rs`
- Modify: `crates/linux-atspi-provider/src/provider.rs`

**Interfaces:**

```rust
pub fn reacquire(
    &self,
    endpoint: AtspiEndpoint,
    acquisition_cut_ref: impl Into<String>,
) -> AtspiElementBinding;
```

Reacquisition always mints a new binding revision; it never mutates an old binding.

- [ ] **Step 1: Add RED asserting same endpoint cannot revive old binding**

```rust
#[test]
fn same_endpoint_reuse_requires_a_new_binding_revision() {
    let provider = provider();
    let endpoint = AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/7");
    let old = provider.bind(endpoint.clone(), "cut:l01:old");
    let old_revision = old.binding_revision();

    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, StateSet::new(State::Defunct)),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, StateSet::empty()),
        Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
    );

    let fresh = provider.reacquire(endpoint, "cut:l01:new");
    assert_ne!(fresh.binding_revision(), old_revision);
    assert!(provider
        .authorize_from_state_set_for_validation(&fresh, StateSet::empty())
        .is_ok());
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);
}
```

- [ ] **Step 2: Run RED before adding `reacquire`**

```bash
cargo test -p localview-linux-atspi-provider --features validation-state-injection --test l01_defunct_binding_contract same_endpoint_reuse_requires_a_new_binding_revision -- --exact --nocapture
```

Expected: missing `reacquire` API.

- [ ] **Step 3: Implement reacquisition as a fresh binding mint**

Use one process-wide `AtomicU64` binding sequence. Never reset lifecycle on the old binding.

- [ ] **Step 4: Run full L01 contract GREEN and commit**

```bash
cargo test -p localview-linux-atspi-provider --features validation-state-injection --test l01_defunct_binding_contract -- --nocapture
git add crates/linux-atspi-provider
git commit -m "feat(v43): make Linux DEFUNCT invalidation terminal"
```

---

### Task 5: Shipping Live AT-SPI State Observation

**Files:**
- Modify: `crates/linux-atspi-provider/src/provider.rs`
- Extend: `crates/linux-atspi-provider/src/lib.rs`

**Interfaces:**

```rust
#[cfg(target_os = "linux")]
pub async fn connect(
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
) -> Result<LinuxAtspiProvider, AtspiProviderConnectionError>;

#[cfg(target_os = "linux")]
pub async fn authorize_action(
    &self,
    binding: &AtspiElementBinding,
) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError>;
```

- [ ] **Step 1: Implement live connection using `atspi::AccessibilityConnection::new()`**

Keep the connection in the provider. No D-Bus handle is stored in `AtspiElementBinding`.

- [ ] **Step 2: Build an `AccessibleProxy` from the binding endpoint and call `get_state().await`**

The live path must feed the returned typed `StateSet` into the same terminal decision used by validation. Any proxy construction / D-Bus / state-read error maps to `ObservationUnavailable`; do not inspect human-readable error text to infer DEFUNCT.

- [ ] **Step 3: Recheck provider/target lineage before each state read**

The binding provider/target refs must exactly equal the provider instance refs. Mismatch returns the typed lineage error before any permit is minted.

- [ ] **Step 4: Run compile gates**

```bash
cargo +1.85.0 check -p localview-linux-atspi-provider --all-targets
cargo clippy -p localview-linux-atspi-provider --all-targets -- -D warnings
cargo test -p localview-linux-atspi-provider --features validation-state-injection --test l01_defunct_binding_contract -- --nocapture
```

- [ ] **Step 5: Commit live transport**

```bash
git add crates/linux-atspi-provider
git commit -m "feat(v43): read live Linux AT-SPI binding state"
```

---

### Task 6: Real GTK3/ATK/AT-SPI Seed + Harness

**Files:**
- Create: `tools/provider-seeds/linux-atspi-defunct-seed/seed.py`
- Create: `tools/validation-lab/linux-l7-real-provider-harness/Cargo.toml`
- Create: `tools/validation-lab/linux-l7-real-provider-harness/tests/v43_real_provider_l01.rs`

**Seed contract:**
- GTK3 window contains one button with accessible name exactly `LocalView L01 Defunct Button`.
- Keep a Python reference to `button.get_accessible()` after widget destruction so the old accessible lifetime can expose ATK/AT-SPI DEFUNCT instead of being garbage-collected immediately.
- Button click callback increments an in-process `press_count`.
- stdin commands are exactly `destroy`, `status`, `quit`; stdout emits line-delimited JSON acknowledgements. The control channel never reports provider state/DEFUNCT.

Minimal GTK control shape:

```python
accessible = button.get_accessible()
accessible.set_name("LocalView L01 Defunct Button")

def on_command(_source, _condition):
    line = sys.stdin.readline().strip()
    if line == "destroy":
        button.destroy()
        emit({"event": "destroyed"})
    elif line == "status":
        emit({"event": "status", "press_count": press_count})
    elif line == "quit":
        Gtk.main_quit()
        return False
    return True
```

**Harness Cargo.toml:** standalone `[workspace]`, Rust 1.85, Linux target dependencies on `localview-linux-atspi-provider`, exact `atspi = "=0.30.0"` with `zbus`, `serde_json`, `tokio` macros/rt/time/process.

- [ ] **Step 1: Write ignored real-provider test before workflow wiring**

The test must:

1. launch `seed.py`;
2. connect to AT-SPI via `AccessibilityConnection`;
3. traverse the registry tree until exact accessible name is found;
4. extract destination + object path only as provider-local endpoint coordinates;
5. construct shipping `LinuxAtspiProvider` and bind the endpoint;
6. call `authorize_action` and require live success;
7. use the permit as the condition for exactly one pre-DEFUNCT `ActionProxy::do_action(0)`; require seed `press_count == 1`;
8. send `destroy` while retaining the old accessible proxy/reference;
9. poll `AccessibleProxy::get_state()` for at most 5 seconds until the **real returned state set** contains `State::Defunct`;
10. call shipping `authorize_action` on the old binding and require typed DEFUNCT denial;
11. query seed status and require press count remains 1, proving post-DEFUNCT dispatch delta is zero;
12. call authorization again and require `AlreadyInvalidDefunct`;
13. write exact-SHA evidence JSON.

- [ ] **Step 2: Run the real test locally/CI-equivalent and require real DEFUNCT**

Environment packages:

```bash
sudo apt-get update
sudo apt-get install -y dbus-x11 xvfb at-spi2-core python3-gi gir1.2-gtk-3.0 libgtk-3-0 libatk-adaptor
```

Run inside a fresh session/X display:

```bash
dbus-run-session -- xvfb-run -a env NO_AT_BRIDGE=0 GTK_MODULES=gail:atk-bridge \
  cargo test --manifest-path tools/validation-lab/linux-l7-real-provider-harness/Cargo.toml \
  --test v43_real_provider_l01 \
  linux_real_provider_l01::l01_real_defunct_terminally_invalidates_old_binding \
  -- --ignored --exact --nocapture --test-threads=1
```

Expected: the old proxy's AT-SPI state set contains `State::Defunct`. If it only disappears/becomes unavailable, stop; do not weaken the oracle and do not claim L01 closure.

- [ ] **Step 3: Commit seed + harness only after the real protocol path works**

```bash
git add tools/provider-seeds/linux-atspi-defunct-seed tools/validation-lab/linux-l7-real-provider-harness
git commit -m "test(v43): add real Linux L01 DEFUNCT oracle"
```

---

### Task 7: Exact-Head Linux L01 Real-Provider Workflow + Evidence Verification

**Files:**
- Create: `.github/workflows/v43-l01-real-provider.yml`
- Extend only if needed: `tools/validation-lab/linux-l7-real-provider-harness/tests/v43_real_provider_l01.rs`

**Evidence schema:** `localview-v43-l01-real-provider-record-v1` with fields:

```text
schema
case_id = "L01"
candidate_sha
provider_family = "linux_atspi"
initial_defunct_observed = false
final_defunct_observed = true
live_authorization_succeeded = true
pre_defunct_press_count = 1
post_defunct_press_count = 1
post_defunct_dispatch_delta = 0
defunct_denial = "defunct"
old_binding_terminal_denial = "already_invalid_defunct"
old_binding_revival_succeeded = false
fresh_binding_revision_greater_than_old = true   # only when replacement/reacquire subcase is exercised
ground_truth_source = "real_gtk_atk_atspi_state"
```

Do not persist raw bus unique names or object paths in the artifact.

- [ ] **Step 1: Add exact-SHA workflow**

Workflow rules:

```yaml
name: V4.3 L01 Real Provider Seed
on:
  pull_request:
  push:
    branches: [main]
concurrency:
  group: localview-v43-l01-real-provider-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
```

Use `ubuntu-latest`, install the exact packages from Task 6, bind checkout SHA, create `$RUNNER_TEMP/localview-v43-l01`, set `LOCALVIEW_CANDIDATE_SHA` and `LOCALVIEW_L7_ARTIFACT_DIR`, require the exact test registration with `cargo test ... -- --list`, then execute inside `dbus-run-session` + `xvfb-run`.

- [ ] **Step 2: Add machine verifier before artifact upload**

Python verifier must require all schema fields above, candidate SHA equality, `final_defunct_observed is True`, zero post-defunct dispatch delta, and both typed denial classes.

- [ ] **Step 3: Upload artifact even on failure**

Use `actions/upload-artifact@v4`, `if: always()`, `if-no-files-found: error`, retention 30 days.

- [ ] **Step 4: Commit workflow**

```bash
git add .github/workflows/v43-l01-real-provider.yml tools/validation-lab/linux-l7-real-provider-harness/tests/v43_real_provider_l01.rs
git commit -m "ci(v43): verify real Linux L01 DEFUNCT invalidation"
```

---

### Task 8: PR, Exact-Head Retained Sweep, Audit, Merge

**Files:** no new behavior unless a retained gate exposes a real regression.

- [ ] **Step 1: Open one draft PR from the implementation branch to `main`**

Title:

```text
feat(v43): close Linux L01 DEFUNCT binding invalidation
```

Body must state the L01 authority boundary, RED/GREEN lineage, real GTK/AT-SPI oracle requirement, and retained-gate requirement.

- [ ] **Step 2: Freeze one candidate SHA**

Do not treat earlier-head green runs as evidence for the final candidate.

- [ ] **Step 3: Require exact-head success for the complete retained matrix**

At minimum:

```text
V4.3 L01 DEFUNCT Binding Contract
V4.3 L01 Real Provider Seed
V4.3 M01-M10 contract/real-provider workflows
V4.3 W13/W14/W15 real-provider workflows
Windows Real Provider Seeds
Windows UIA Observe
CI
```

Also require the cross-platform workspace/native-provider tests already included in CI. Clippy remains `-D warnings`.

- [ ] **Step 4: Audit PR immediately before merge**

Require:

```text
head SHA == the 100%-green candidate
main SHA == PR base SHA (no base drift)
changed files are only design/plan + Cargo workspace + linux-atspi-provider + L01 workflows + Linux seed/harness
0 unresolved conversation comments
0 reviews requiring action
0 unresolved review threads
mergeable == true
```

- [ ] **Step 5: Mark Ready and merge with expected-head guard**

If any head changes after the green sweep, repeat the exact-head sweep.

- [ ] **Step 6: Post-merge verification**

Fetch `main` and require the returned merge SHA is the new main head and has the expected candidate as a parent. Only then record V43-L01 closed and move to L02.

---

## Self-Review Record

- **Spec coverage:** every design requirement maps to Tasks 1-8; no L02-L10 behavior is included.
- **Dependency boundary:** `atspi` is Linux-target-only in the production crate; `native-provider` remains transport-neutral.
- **Type consistency:** binding revision is `u64`; provider/target lineage uses existing `ProviderIncarnationRef` / `TargetIncarnationRef`; validation and live paths share the same state decision.
- **Oracle integrity:** validation-only state injection cannot satisfy the real-provider gate; GTK/ATK must return real `State::Defunct` for the old accessible reference.
- **Terminality:** all clones share lifecycle; reacquisition creates a new binding revision and never revives the old one.
- **No placeholders:** implementation stop conditions are explicit: MSRV failure requires design amendment; no real DEFUNCT means no merge.
