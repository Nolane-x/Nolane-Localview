# V4.3 L7 Windows W03/W04/W05 Real-Provider Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the bounded Windows L7 W03 virtualized-item, W04 unsupported-Invoke, and W05 provider-hang real-provider seeds without weakening the merged W01/W02/W06 authority.

**Architecture:** Keep W04 on the existing Win32 seed, add a separate test-only WPF edge seed for deterministic virtualization/hang behavior, extend the production Windows UIA MTA worker with explicit ItemContainer/VirtualizedItem commands and poison-on-timeout semantics, then feed all three independent-oracle comparisons through the provider-neutral Validation Lab and the existing prospective L7 campaign.

**Tech Stack:** Rust 1.85+/edition 2024, `windows` Win32/UIA bindings, Tokio runtime manager, .NET 8 WPF test fixture, GitHub Actions Windows hosted runner, `localview-validation-lab`.

**Spec:** `docs/superpowers/specs/2026-09-11-v43-l7-windows-w03-w04-w05-closure-design.md`

## Global Constraints

- Base authority is `main@e698043f0d6371124e43d7889b1af7832d0b5b9f`.
- Scope is exactly W03, W04 and W05 real-provider seeds; do not claim full Windows provider or full V4.3 completion.
- W01/W02/W06 existing tests and campaign evidence must stay green and semantically unchanged.
- Ground truth is test-only and production LocalView must not read it.
- Real-provider pass requires prospective preregistration, exact candidate/environment/seed binding, exact required-seed coverage and clean measured RPOMR.
- Potentially side-effecting timeout never becomes known failure or blind retry authority.
- Heavy seed/Lab code must not become a shipping dependency.

---

### Task 1: Establish draft PR and clean baseline

**Files:**
- Existing: `.github/workflows/ci.yml`
- Existing: `.github/workflows/windows-uia-observe.yml`
- Existing: `.github/workflows/windows-real-provider-seeds.yml`

**Interfaces:**
- Consumes: exact base `e698043f0d6371124e43d7889b1af7832d0b5b9f`.
- Produces: draft PR and baseline workflow evidence before production code changes.

- [ ] **Step 1: Open the draft PR from `feat/v43-l7-windows-w03-w04-w05-closure` to `main`.**

Body must state exact W03/W04/W05 scope and explicitly deny a broad support claim.

- [ ] **Step 2: Record the plan-only head SHA.**

Use GitHub commit metadata; do not infer it from local state.

- [ ] **Step 3: Run/observe pull-request workflows at the plan-only head.**

Expected: all existing CI/Windows workflows pass because only docs changed. If an existing baseline job fails, diagnose before attributing later failures to this slice.

---

### Task 2: W04 RED — real unsupported Invoke must stay typed and side-effect-free

**Files:**
- Modify: `tools/provider-seeds/windows-uia-seed/src/control.rs`
- Modify: `tools/provider-seeds/windows-uia-seed/src/state.rs`
- Modify: `tools/provider-seeds/windows-uia-seed/src/main.rs`
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w04.rs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml`

**Interfaces:**
- Consumes: existing seed JSON-lines control/oracle protocol and `WindowsUiaActionPreflightError::PatternUnsupported`.
- Produces: deterministic real control identity plus an oracle mutation counter proving zero Invoke side effect.

- [ ] **Step 1: Add seed protocol fields/command required to expose W04 ground truth.**

Extend ground truth with a non-invokable control handle/incarnation and `unsupported_invoke_side_effect_count: u64`. The fixture should use an ordinary Win32 `STATIC` child or equivalent UIA control that does not expose Invoke.

- [ ] **Step 2: Write the ignored Windows real-provider test before changing production behavior.**

Core assertion shape:

```rust
let result = manager
    .preflight_uia_action(
        session_id,
        WindowsUiaActionPreflightRequest {
            authority,
            element_ref: unsupported_ref,
            required_pattern: WindowsUiaPattern::Invoke,
        },
    )
    .await;

assert_eq!(
    result,
    Err(WindowsUiaActionPreflightError::PatternUnsupported {
        pattern: WindowsUiaPattern::Invoke,
    })
);
assert_eq!(seed.ground_truth()?.unsupported_invoke_side_effect_count, 0);
```

- [ ] **Step 3: Commit RED-only W04 test/seed contract and run exact Windows test in CI.**

Expected RED reason must be missing/incorrect fixture or real-provider evidence, not a fabricated test failure.

- [ ] **Step 4: Make the minimum seed/harness correction until W04 is genuinely green.**

No pointer/keyboard fallback may be added. Production action-preflight semantics should remain unchanged unless the real test exposes an actual bug.

- [ ] **Step 5: Run existing action-preflight/provider dispatch tests plus W04.**

Expected: W04 passes and all previous unsupported-pattern tests remain green.

- [ ] **Step 6: Commit GREEN W04.**

Suggested message: `test(v43): close real-provider W04 unsupported Invoke`.

---

### Task 3: W03 RED — provider-level ItemContainer lookup and placeholder authority

**Files:**
- Create: `crates/windows-uia-provider/src/virtualized_item.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Create: `crates/windows-uia-provider/tests/virtualized_item_contract.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs` Windows `WorkerCommand`/worker methods.

**Interfaces:**
- Consumes: exact retained snapshot element lease and `ProviderElementRealization` lifecycle.
- Produces:

```rust
pub enum WindowsUiaItemLookupProperty { Name, AutomationId }
pub struct WindowsUiaVirtualizedItemQueryRequest { /* design fields */ }
pub struct WindowsUiaVirtualizedItemQueryReceipt { /* design fields */ }
pub struct WindowsUiaVirtualizedItemRealizeRequest { /* design fields */ }
pub struct WindowsUiaVirtualizedItemRealizeReceipt { /* design fields */ }
```

- [ ] **Step 1: Write compile-time/data-contract tests for request validation.**

Tests must reject empty lookup value, stale snapshot cut, cross-incarnation container refs and non-`RealizationRequired` realize requests.

- [ ] **Step 2: Write Windows-only provider test for the semantic invariant before implementation.**

A placeholder receipt must have:

```rust
assert_eq!(
    receipt.placeholder_element_ref.realization,
    ProviderElementRealization::RealizationRequired
);
assert_eq!(
    receipt.placeholder_element_ref.acquisition_cut_ref,
    snapshot_cut_ref
);
```

- [ ] **Step 3: Commit RED and prove failure/compile gap on CI.**

Expected RED: new types/methods absent or provider command unimplemented.

- [ ] **Step 4: Implement `virtualized_item.rs` data contracts and validation.**

Keep this file data-only and platform-neutral within the crate; no test-fixture knowledge.

- [ ] **Step 5: Implement MTA ItemContainer query.**

Use `IUIAutomationItemContainerPattern` and `FindItemByProperty` on the exact retained container. Support only UIA Name and AutomationId property IDs. The returned item must support `IUIAutomationVirtualizedItemPattern`; otherwise return a typed non-virtual-item error.

Generate the placeholder `ProviderElementRef` with the current provider/target lineage, current acquisition cut, semantic lookup hint, and `ProviderElementRealization::RealizationRequired`.

- [ ] **Step 6: Retain placeholder COM interfaces separately from normal realized element leases.**

A second lookup may invalidate a prior provider placeholder; keep retention bounded by target/snapshot and replace the old set rather than accumulating unbounded placeholders.

- [ ] **Step 7: Implement MTA realization command.**

Bind exact placeholder/cut/lineage, require VirtualizedItem support, call `Realize()`, return only `WindowsUiaVirtualizedItemRealizeReceipt`. Do not mutate the placeholder ref into `RealizedCurrent`.

- [ ] **Step 8: Run provider unit/Windows contract tests.**

Expected: query/realize data path is green; no action has yet been granted from the placeholder.

- [ ] **Step 9: Commit GREEN provider primitive.**

Suggested message: `feat(v43): add bounded UIA virtualized-item realization primitive`.

---

### Task 4: W03 runtime authority — fresh cut required after realization

**Files:**
- Modify: `crates/windows-observe-runtime/src/runtime_manager.rs`
- Modify: `crates/windows-observe-runtime/src/action_preflight.rs` only if error mapping is required, not to weaken the gate.
- Create: `crates/windows-observe-runtime/tests/virtualized_item_authority_contract.rs`

**Interfaces:**
- Consumes: provider query/realize receipts from Task 3.
- Produces runtime methods that query and realize a virtualized item while preserving exact session/provider/target authority.

- [ ] **Step 1: Write fake-provider/runtime RED proving pre-realization action is blocked.**

Use a node/ref with `ProviderElementRealization::RealizationRequired`; expected error remains:

```rust
WindowsUiaActionPreflightError::ElementNotRealized {
    realization: ProviderElementRealization::RealizationRequired,
}
```

- [ ] **Step 2: Write RED proving a realization receipt alone cannot satisfy action preflight.**

The old placeholder acquisition cut must remain unusable. A successful path requires a new snapshot cut whose newly observed element is `RealizedCurrent`.

- [ ] **Step 3: Add narrow runtime provider traits/methods for item lookup and realization.**

Do not add these methods to providers that do not implement the capability; use a capability subtrait analogous to `WindowsObserveActionLeaseProvider`.

- [ ] **Step 4: Implement runtime orchestration.**

Serialized under `operation_gate`: bind session, query placeholder, realize it, perform a fresh reconciliation/observation snapshot under a new cut, then return the fresh snapshot/receipt. Do not resolve a final actionable ref by fuzzy name alone in production authority; the caller/harness may compare the fresh snapshot with oracle and exact provider identity evidence.

- [ ] **Step 5: Run runtime contract tests.**

Expected: old cut fails; fresh cut is required; existing preflight behavior is unchanged for normal `RealizedCurrent` elements.

- [ ] **Step 6: Commit.**

Suggested message: `feat(v43): require fresh observation after UIA realization`.

---

### Task 5: W03 WPF real-provider seed and oracle

**Files:**
- Create: `tools/provider-seeds/windows-uia-edge-seed/LocalView.WindowsUiaEdgeSeed.csproj`
- Create: `tools/provider-seeds/windows-uia-edge-seed/Program.cs`
- Create: `tools/provider-seeds/windows-uia-edge-seed/EdgeWindow.cs`
- Create: `tools/provider-seeds/windows-uia-edge-seed/OracleProtocol.cs`
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w03.rs`
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/Cargo.toml`

**Interfaces:**
- Consumes: Task 3/4 production virtualized-item methods.
- Produces a test-only WPF `ListBox` with built-in virtualization and independent JSON-line oracle.

- [ ] **Step 1: Create a package-free .NET 8 WPF project.**

Project properties:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net8.0-windows</TargetFramework>
    <UseWPF>true</UseWPF>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
  </PropertyGroup>
</Project>
```

- [ ] **Step 2: Build deterministic virtualized UI.**

Create a `ListBox` with virtualization/recycling enabled and at least 256 named string items. Fix the viewport height so the tail item `LocalView Virtual Item 255` has no generated `ListBoxItem` container at startup.

- [ ] **Step 3: Implement independent oracle commands.**

JSON-lines commands: `get_ground_truth`, `get_virtual_item_state`, `shutdown`. Ground truth reports process/run IDs, window handle, target logical item name, and whether `ItemContainerGenerator.ContainerFromIndex(255)` is non-null.

- [ ] **Step 4: Write the ignored W03 real-provider harness test.**

Test sequence:

```text
launch edge seed
attach LocalView real Windows UIA runtime
snapshot container
query tail item by Name through ItemContainer
assert placeholder RealizationRequired
prove preflight cannot accept placeholder
Realize placeholder
capture fresh snapshot cut
oracle proves item container is now generated
provider evidence proves fresh realized element exists
adapt to RealProviderCaseKind::W03VirtualizedItemRealization
```

- [ ] **Step 5: Run exact W03 on hosted Windows.**

If WPF virtualization behavior differs on the runner, fix the fixture deterministically; do not weaken assertions or replace the real provider with a fake.

- [ ] **Step 6: Commit green W03 real-provider seed.**

Suggested message: `test(v43): close real-provider W03 virtualized realization`.

---

### Task 6: W05 RED — poison the worker after command timeout

**Files:**
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Create: `crates/windows-uia-provider/tests/worker_timeout_poison_contract.rs`
- Modify: `crates/windows-observe-runtime/src/runtime_manager.rs` only for typed propagation/reacquire boundary.

**Interfaces:**
- Consumes: `WindowsUiaWorkerConfig::command_timeout` and all worker command send/receive paths.
- Produces a worker-health state and typed poison error that prevents reuse after timeout.

- [ ] **Step 1: Write RED unit contract around a timeout-capable worker abstraction.**

Required behavior:

```text
first command -> CommandTimeout
worker health -> poisoned
second command -> WorkerPoisoned immediately, without another timeout wait
```

- [ ] **Step 2: Add `WorkerPoisoned` error and shared atomic health.**

Use an `Arc<AtomicBool>` or a small explicit state object. Every public worker method must check health before sending. `recv_command`/caller wrapper must poison on timeout but not on ordinary typed provider errors.

- [ ] **Step 3: Preserve Drop semantics.**

Do not join the worker. Sending `Shutdown` to a poisoned/hung queue is best-effort only.

- [ ] **Step 4: Run worker/provider tests.**

Expected: one timeout poisons exactly that worker instance; a newly spawned worker has a distinct provider incarnation and healthy state.

- [ ] **Step 5: Commit.**

Suggested message: `fix(v43): quarantine Windows UIA worker after timeout`.

---

### Task 7: W05 WPF hostile-provider seed and real provider evidence

**Files:**
- Modify: `tools/provider-seeds/windows-uia-edge-seed/EdgeWindow.cs`
- Create: `tools/provider-seeds/windows-uia-edge-seed/HangableControl.cs`
- Modify: `tools/provider-seeds/windows-uia-edge-seed/OracleProtocol.cs`
- Create: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w05.rs`

**Interfaces:**
- Consumes: Task 6 poison semantics and existing W06 release/reacquire patterns.
- Produces deterministic real UIA provider hang plus independent oracle evidence.

- [ ] **Step 1: Implement a custom WPF control/AutomationPeer whose selected UIA read blocks on an unsignaled gate when armed.**

The hang must occur in the UIA provider call path while the seed process and oracle reader remain responsive. The oracle records `hang_armed` and `provider_call_entered` separately.

- [ ] **Step 2: Add JSON-line command `arm_provider_hang`.**

Command returns only after the fixture is armed, not after the provider has entered the block.

- [ ] **Step 3: Write ignored W05 real-provider test.**

Test sequence:

```text
launch edge seed
spawn real Windows UIA manager with a short but nonzero test timeout
attach/snapshot normally
arm hang
issue exact UIA call expected to enter custom peer
assert caller returns CommandTimeout within conservative upper bound
assert oracle provider_call_entered
assert next call returns WorkerPoisoned without another timeout budget
release/drop manager without joining hung MTA
spawn new real manager while seed remains alive
assert provider incarnation changed
assert old ref/authority cannot bind in new manager
```

- [ ] **Step 4: Add side-effect-uncertainty regression if hang target is dispatch-capable.**

If the blocked method can occur after a side-effect boundary, assert the coordinator leaves PREPARED/reconciliation state and never grants blind retry. If the fixture hangs only an observation read, do not fabricate a dispatch uncertainty claim; record provider-unresponsive evidence only.

- [ ] **Step 5: Run exact W05 on hosted Windows and existing W06.**

Expected: W05 and W06 both green; the new poison rule does not break normal release/reacquire.

- [ ] **Step 6: Commit.**

Suggested message: `test(v43): close real-provider W05 hang quarantine`.

---

### Task 8: Extend provider-neutral Lab adapter for W03/W04/W05

**Files:**
- Modify: `crates/validation-lab/src/real_provider_adapter.rs`
- Create: `crates/validation-lab/tests/real_provider_w03_w04_w05.rs`

**Interfaces:**
- Consumes: case facts from standalone real-provider harnesses.
- Produces exact `RealProviderCaseKind` variants and metric/failure eligibility.

- [ ] **Step 1: Write RED adapter tests for each new case.**

Cases must include one clean/pass path and one unsound/counterexample path.

- [ ] **Step 2: Add enum variants exactly as specified in the design.**

- [ ] **Step 3: Implement case semantics.**

W03 fails if placeholder action authority was not blocked, no fresh cut followed realization, or fresh evidence does not establish `RealizedCurrent`.

W04 fails if support was not typed unsupported, dispatch was attempted, or oracle observed a side effect.

W05 fails if caller was not bounded, a poisoned worker was reused, required reacquire did not occur, or stale old-incarnation authority survived. Reuse existing failure flags only when their established metric meaning matches; otherwise rely on RPOMR/counterexample without inventing a new metric.

- [ ] **Step 4: Run validation-lab tests and commit.**

Suggested message: `feat(v43): adapt W03 W04 W05 real-provider evidence`.

---

### Task 9: Extend prospective six-seed L7 campaign

**Files:**
- Modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_campaign.rs`
- Modify: `.github/workflows/windows-real-provider-seeds.yml`
- Modify: `.github/workflows/windows-uia-observe.yml`

**Interfaces:**
- Consumes: standalone green W03/W04/W05 records and existing W01/W02/W06 campaign.
- Produces exact required seed coverage W01–W06 for this bounded six-seed campaign and published artifacts.

- [ ] **Step 1: RED-expand preregistration required cases to six exact IDs.**

Expected failure until campaign executes/appends W03/W04/W05.

- [ ] **Step 2: Add W03/W04/W05 execution to integrated campaign.**

Use logical sequence numbers after the existing W01/W02/W06 records without changing old record semantics. RPOMR denominator must equal six for six asserted comparisons and numerator must be zero for pass.

- [ ] **Step 3: Update artifact seed digest binding.**

Environment/campaign evidence must bind both the existing Win32 seed executable digest and the WPF edge-seed build/project digest; do not collapse them into an unlabeled aggregate.

- [ ] **Step 4: Update dedicated Windows workflow.**

Add fail-closed exact test registration/execution for W03/W04/W05 and build the WPF edge seed before tests.

- [ ] **Step 5: Update normal Windows UIA workflow with production contract tests.**

Run provider virtualized-item and timeout-poison contracts even when L7 fixture execution is not requested.

- [ ] **Step 6: Run fresh exact-head workflows.**

Required: CI, Windows UIA Observe, Windows Real Provider Seeds all complete success.

- [ ] **Step 7: Download/inspect published L7 artifact.**

Must contain nonempty:

```text
environment-manifest.json
LAB-PREREGISTRATION.json
LAB-PREREGISTRATION-RECEIPT.json
LAB-RESULT.json
```

Result must bind the exact PR head and six required seed identities.

- [ ] **Step 8: Commit.**

Suggested message: `test(v43): extend Windows L7 campaign through W05`.

---

### Task 10: Final isolation audit, PR review and exact merge

**Files:**
- No new implementation files expected.
- Update PR body only.

**Interfaces:**
- Consumes: final exact-head diff/workflow/artifact evidence.
- Produces merge-ready bounded PR.

- [ ] **Step 1: Compare exact base to exact final head.**

Confirm no seed project became a shipping dependency and no unrelated daemon/desktop/CLI/MCP behavior changed.

- [ ] **Step 2: Run fresh exact-head verification after the last code/doc change.**

Do not reuse earlier green runs from a different SHA.

- [ ] **Step 3: Inspect review threads/comments.**

Resolve only after implementing or technically refuting each relevant issue.

- [ ] **Step 4: Update PR body with TDD lineage and exact run/artifact IDs.**

Explicitly state that this closes W03/W04/W05 only and does not claim full Windows provider/V4.3 completion.

- [ ] **Step 5: Mark PR ready only after all required exact-head checks are green.**

- [ ] **Step 6: Merge using `expected_head_sha` equal to the verified final head.**

- [ ] **Step 7: Verify resulting `main` commit/workflows before claiming completion.**
