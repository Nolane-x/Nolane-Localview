# V4.3 L7 Windows W03/W04/W05 Real-Provider Closure — Design

Date: 2026-09-11

Status: Approved for implementation

Branch: `feat/v43-l7-windows-w03-w04-w05-closure`

Base: `main@e698043f0d6371124e43d7889b1af7832d0b5b9f`

## 1. Purpose and bounded scope

This slice extends the merged Windows L7 real-provider program from W01/W02/W06 to exactly three additional seed identities from the V4.3 Windows provider matrix:

- **W03 — virtualized item realization**;
- **W04 — unsupported Invoke pattern**;
- **W05 — UIA provider hang**.

It does not claim full Windows provider completion, full W01–W15 closure, or full V4.3 completion. Existing W01/W02/W06 evidence remains authoritative and must not be weakened or rewritten by this slice.

The design follows the V4.3 specification requirements that Windows UIA work be isolated from the LocalView UI thread, unsupported control patterns remain typed rather than silently promoted, virtualized items require an explicit realization lifecycle, and provider hangs must not freeze LocalView.

## 2. Critical semantic distinction

The seed matrix numbers W03/W04/W05 are seed identities, while the later failure-taxonomy identifiers V43-W03/V43-W04/V43-W05 describe failure classes. They are related by required defense, not by number equality.

This slice therefore binds Lab observations to exact seed IDs and explicit defense semantics rather than inferring behavior from matching numeric suffixes.

## 3. Selected architecture

The selected design uses two test-only Windows fixtures and the existing production UIA worker/runtime:

```text
A. Existing Win32 seed (W04)
   real BUTTON / ordinary HWND
   independent JSON-line oracle
          |
          v
   existing Windows UIA snapshot/preflight/provider path

B. New WPF edge seed (W03 + W05)
   virtualized ItemsControl + custom hangable AutomationPeer
   independent JSON-line oracle
          |
          v
   existing Windows UIA MTA worker
      + item-container lookup / virtualized-item realization
      + timeout poison/quarantine boundary
          |
          v
   Windows observe runtime
          |
          v
   provider-neutral Validation Lab adapter
```

No shipping crate may depend on either seed fixture. Ground-truth channels are test-only and are not visible to production LocalView code.

## 4. W03 — virtualized item realization

### 4.1 Why tree traversal alone is insufficient

Microsoft UI Automation virtualization semantics require clients to use the ItemContainer control pattern to retrieve a placeholder for an item that is not present in the normal UIA tree. The placeholder supports VirtualizedItem; calling `Realize` makes the item fully accessible. Therefore the existing `ControlViewWalker` snapshot traversal cannot by itself discover all virtualized items.

The production API must model this explicitly instead of pretending an unseen virtual item is a normal realized tree node.

### 4.2 Provider API

Add a narrow item-container query API to `localview-windows-uia-provider`:

```rust
pub enum WindowsUiaItemLookupProperty {
    Name,
    AutomationId,
}

pub struct WindowsUiaVirtualizedItemQueryRequest {
    pub snapshot_cut_ref: String,
    pub container_element_ref: ProviderElementRef,
    pub property: WindowsUiaItemLookupProperty,
    pub value: String,
}

pub struct WindowsUiaVirtualizedItemQueryReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub container_element_ref: ProviderElementRef,
    pub placeholder_element_ref: ProviderElementRef,
}

pub struct WindowsUiaVirtualizedItemRealizeRequest {
    pub snapshot_cut_ref: String,
    pub placeholder_element_ref: ProviderElementRef,
}

pub struct WindowsUiaVirtualizedItemRealizeReceipt {
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub previous_placeholder_ref: ProviderElementRef,
}
```

The query command must:

1. bind the exact retained container from the current snapshot cut;
2. require ItemContainer support;
3. call `FindItemByProperty` using only the explicitly supported lookup properties;
4. require the returned item to expose VirtualizedItem support before labeling it virtual;
5. retain the placeholder COM element only on the owning MTA worker;
6. return a `ProviderElementRef` whose realization is `RealizationRequired`, never `RealizedCurrent`.

The placeholder identity is freshness-scoped. It is not durable and cannot itself become action authority.

### 4.3 Realization command

The realize command must:

1. bind the exact retained placeholder and exact provider/target incarnation;
2. transition only the provider-side operation, not mutate a cached semantic node into freshness;
3. call `IUIAutomationVirtualizedItemPattern::Realize()` on the owning MTA;
4. return only an execution receipt for the realization call;
5. require the runtime to acquire a **fresh snapshot cut** before any action preflight can succeed.

A pre-realization placeholder must continue to fail `preflight_uia_action` through the existing `ElementNotRealized` gate. No direct mutation from `RealizationRequired` to `RealizedCurrent` is allowed without fresh observation.

### 4.4 W03 seed fixture

Create a test-only WPF fixture under:

```text
tools/provider-seeds/windows-uia-edge-seed/
```

Use an SDK-style `net8.0-windows` WPF application with no external NuGet package dependency. The fixture exposes a virtualizing `ListBox`/`ItemsControl` with enough items that a deterministic tail item is outside the realized visual range. The harness looks the tail item up by Name through the container pattern, proves the returned element is a VirtualizedItem placeholder, realizes it, obtains a fresh LocalView snapshot, and proves the fresh snapshot contains a `RealizedCurrent` element corresponding to the oracle item.

The independent oracle reports the logical item ID/name and whether the WPF container has generated a visual container for that item before and after realization. LocalView production code cannot read this oracle.

## 5. W04 — unsupported Invoke pattern

W04 uses the existing Win32 seed because an ordinary non-invokable control is sufficient to exercise real UIA capability evidence without introducing another provider fixture.

Extend the seed with a deterministic child control whose UIA provider reports Invoke unavailable. The oracle reports that invoking it is unsupported and tracks a mutation counter that must remain unchanged.

The real-provider test must prove:

1. the LocalView snapshot records `WindowsUiaPattern::Invoke` as `Unsupported` for the target;
2. `preflight_uia_action` returns `PatternUnsupported { Invoke }`;
3. no pattern executor is reached;
4. no pointer/keyboard fallback is introduced by this slice;
5. the seed oracle mutation counter remains zero.

W04 must not be satisfied by a fake provider or by directly inspecting seed implementation details.

## 6. W05 — UIA provider hang

### 6.1 Existing strength

The current provider already places UIA work on a dedicated MTA thread and each caller-side worker command has a bounded `recv_timeout`. `Drop` intentionally does not join the MTA thread, so a hung provider cannot block LocalView cleanup.

### 6.2 Existing gap

A command timeout currently returns `WindowsUiaWorkerError::CommandTimeout`, but the same worker object remains reusable even though its MTA thread may still be permanently blocked inside a hostile provider call. Subsequent commands can queue behind a poisoned worker and repeatedly timeout. The provider incarnation therefore remains misleadingly live after an unrecoverable command timeout.

### 6.3 Poison/quarantine rule

After the first command timeout, that worker instance is **poisoned**:

- no later command may be sent to it;
- later calls return a typed `WorkerPoisoned`/equivalent unavailable error immediately rather than waiting another timeout;
- the worker's provider incarnation can never become current authority again;
- runtime recovery must acquire a fresh Windows UIA worker/provider incarnation rather than reusing the poisoned one.

The low-level `WindowsUiaWorker` may implement poisoning with an atomic/shared health state that is checked before every send and set on timeout. The hung MTA thread is detached and left for process teardown; LocalView does not attempt unsafe cancellation of a third-party COM call.

### 6.4 Side-effect timeout semantics

A timeout during a potentially side-effecting provider operation is never translated into known failure. The execution coordinator already treats provider execution errors as reconciliation-only rather than blind retry authority; this slice must preserve that rule.

W05 must distinguish:

- responsiveness evidence: caller returns within the configured timeout budget;
- provider health: the timed-out worker is poisoned;
- authority: old provider-incarnation evidence is stale after reacquire;
- outcome knowledge: dispatch timeout remains unknown/reconciliation-required where a side effect may have crossed the boundary.

### 6.5 W05 seed fixture

The WPF edge seed exposes a custom control with a custom `AutomationPeer`. A test-only command arms a deterministic blocking gate. The next selected UIA property/pattern request blocks inside the provider until process shutdown, emulating a hostile third-party provider without blocking the seed UI thread globally.

The oracle reports when the gate is armed and when the provider method has entered the blocked state. The real-provider test measures only bounded correctness facts, not scheduler timing beyond a conservative upper bound.

## 7. Runtime recovery boundary

Keep worker construction authority outside generic fake-provider traits. For the production Windows implementation, add a narrow reacquire path that replaces the real `WindowsUiaObserveProvider` after a poisoned-worker failure and forces all attached session authority to be re-established under a new provider incarnation.

Do not silently swap the worker underneath an existing `ProviderElementRef`. Recovery requires reattach/reobserve; any stale element ref from the old provider incarnation must fail exact binding.

If implementation shows that automatic in-manager replacement would broaden lifetime semantics too far, the minimum acceptable W05 closure is an explicit typed poison signal plus the existing release/new-manager reacquire path used by W06, proven end-to-end in the W05 real-provider harness. The PR must not claim transparent automatic recovery unless that path is actually implemented and tested.

## 8. Validation Lab additions

Extend `RealProviderCaseKind` with exact variants:

```rust
W03VirtualizedItemRealization {
    placeholder_blocked_before_realization: bool,
    fresh_cut_after_realization: bool,
    realized_current_after_fresh_cut: bool,
}

W04UnsupportedInvoke {
    invoke_support_unsupported: bool,
    dispatch_attempted: bool,
    side_effect_observed: bool,
}

W05ProviderHang {
    caller_returned_bounded: bool,
    poisoned_worker_reused: bool,
    provider_reacquired: bool,
    stale_authority_survived_reacquire: bool,
}
```

Every asserted oracle comparison remains RPOMR-eligible. Existing metrics are reused only where their denominator predicate is truly present:

- W03: RPOMR; stale-cache authority only if old pre-realization authority survives a required fresh cut;
- W04: RPOMR; no synthetic metric is invented for typed unsupported behavior;
- W05: RPOMR, SCAR when stale provider authority survives reacquire, UOBRR only if a timeout creates blind retry authority, CBFR only when cleanup/reacquire baseline is explicitly measured.

Counterexamples outrank passes. Unknown/inconclusive/unsupported observations cannot mint a real-provider integration pass.

## 9. L7 campaign extension

The existing prospective L7 campaign must be extended only after standalone W03/W04/W05 tests are green.

The required seed set becomes exactly:

```text
W01-missing-uia-property-event
W02-recreated-uia-element
W03-virtualized-item-realization
W04-unsupported-invoke-pattern
W05-uia-provider-hang
W06-windows-uia-provider-reacquire
```

The campaign must preserve:

- exact candidate SHA binding;
- environment manifest;
- seed binary/project digests;
- persisted preregistration before execution;
- fail-closed exact required-seed coverage;
- provider-backed evidence refs;
- clean measured RPOMR for every asserted comparison;
- publication of nonempty L7 artifacts.

A W03/W04/W05 failure must block the campaign; the earlier W01/W02/W06 records are not downgraded or rewritten.

## 10. CI and shipping isolation

Update the dedicated `windows-real-provider-seeds.yml` workflow to:

1. build/test the existing Rust Win32 seed;
2. build the new WPF edge seed using the hosted Windows .NET SDK;
3. run standalone W03, W04 and W05 real-provider tests fail-closed by exact test name;
4. run the extended prospective L7 campaign;
5. upload the exact-head L7 artifact bundle.

The normal Windows UIA workflow must also compile/run the new production tests necessary for provider query/realization and worker poison behavior.

A final compare against `main@e698043f0d6371124e43d7889b1af7832d0b5b9f` must confirm:

- edge seed and oracle code are test-only;
- no desktop/daemon/CLI/MCP shipping dependency is added on the seed projects;
- no broad provider support claim is introduced;
- production changes are limited to the Windows UIA provider/runtime semantics necessary for W03/W05.

## 11. Acceptance boundary

This slice is complete only when all of the following are true at one exact PR head:

- W03 real Windows provider test proves placeholder -> Realize -> fresh cut -> `RealizedCurrent`, while pre-realization action authority is blocked;
- W04 real Windows provider test proves Invoke unsupported remains typed unsupported with zero fallback/side effect;
- W05 real Windows provider test proves bounded return, poisoned-worker non-reuse, fresh provider incarnation on reacquire, and stale authority rejection;
- Validation Lab records all three through the independent oracle adapter with no mismatch/failure;
- the prospective six-seed campaign records clean RPOMR for all required asserted comparisons;
- dedicated Windows L7 workflow and normal Windows UIA workflow are green at the exact head;
- the published artifact contains the environment manifest, preregistration, receipt and result bound to the exact candidate;
- shipping isolation compare is clean.

Only then may the PR be marked ready and merged with an exact expected-head SHA. The merge message must remain bounded to W03/W04/W05 real-provider authority.