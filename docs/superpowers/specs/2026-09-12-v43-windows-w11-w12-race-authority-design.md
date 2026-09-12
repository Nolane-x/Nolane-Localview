# V4.3 Windows Input/Action Race Authority — W11/W12 Design

Date: 2026-09-12

Status: Approved continuation of the V4.3 Windows L7 provider program

Branch: `feat/v43-l7-windows-w11-w12-race-authority`

Base: `main@16c08afbc4db90bb17ab1312594147a08111ff82`

## 1. Purpose

This slice closes the remaining Windows V4.3 **input/action race** seeds adjacent to the already-implemented W07/W08/W09 verified-input authority:

- **W11 — modal before dispatch**;
- **W12 — target restart after authorization**.

The slice deliberately reuses the existing consequential-action journal, one-shot execution permit, retained provider/target/element lineage, and immediate Windows dispatch-context fence. It does not create a second action authority stack, a new public raw-input endpoint, or a broad Windows automation claim.

W10 mixed DPI is intentionally not included. The existing Windows real-provider design classifies W10 under geometry/accessibility/privacy, while W11/W12 share the same input/action-race authority family as W07/W08/W09.

## 2. Source-of-truth requirements

This design continues the V4.3 requirements already enforced by the W07/W08/W09 slice:

- semantic provider actions remain preferred over raw input;
- volatile dispatch facts are re-observed immediately before consequential side effect;
- modal state is part of dispatch eligibility;
- target/provider/element lineage is incarnation-bound rather than name/AutomationId-bound;
- dispatch authority is one-shot and journal-backed;
- a target restart cannot silently transfer authority from one target incarnation to another;
- a blocked pre-dispatch attempt produces zero input insertion;
- unknown/possibly-dispatched world outcomes are not blindly retried;
- real-provider support claims require independent seed/oracle evidence.

## 3. Selected architecture

The selected architecture is **reuse, not expansion**:

```text
canonical consequential action authority
    |
    +--> PREPARED journal state
    +--> one-shot DispatchExecutionPermit
    +--> exact target/provider/element lineage
    +--> exact verified key batch
             |
             v
WindowsUiaWorker::dispatch_verified_input
    |
    +--> revalidate retained target/provider/element
    +--> observe final dispatch context
    |      +--> foreground/focus
    |      +--> owned modal blocker
    +--> snapshot keyboard state
    +--> SendInput only if all fences pass
             |
             v
InputDispatchReceipt + durable reconciliation semantics
```

W11 extends the real seed so the existing `require_no_modal_blocker` fence is exercised against an actual owned modal/popup window.

W12 exercises lifecycle invalidation: an execution request minted for target incarnation A must not dispatch after A exits and a visually/semantically similar seed process B appears.

## 4. W11 — modal before dispatch

### Goal

Prove that a modal blocker appearing after authorization but before the immediate side-effect boundary prevents verified input from reaching the target.

### Seed sequence

1. launch the Windows edge seed and prepare the existing verified-input target;
2. attach LocalView to the exact seed window/control and mint journal-backed verified-input authority;
3. prove no modal blocker exists at preparation time;
4. arm/open a deterministic **owned modal window** after authority is prepared;
5. invoke `WindowsUiaWorker::dispatch_verified_input` with `require_no_modal_blocker = true`;
6. require the worker-owned final context observation to report the modal HWND;
7. assert `WindowsUiaDispatchContextBlocker::ModalBlockerPresent` before insertion;
8. independently assert the seed target observed zero key effect;
9. close the modal and prove test-owned resources return to baseline.

### Authority rule

The earlier clean preflight is not reusable evidence for the immediate dispatch boundary. The worker must observe modal state again immediately before any insertion attempt.

### Pass boundary

W11 passes only when:

- a real owned modal is present at the final boundary;
- the attempt is blocked with typed modal evidence;
- requested key events are not inserted;
- the oracle reports zero target effect;
- no retry is automatically authorized from the same consumed authority.

## 5. W12 — target restart after authorization

### Goal

Prove that authorization for one target/provider incarnation cannot migrate to a restarted process that presents the same window/control identity labels.

### Seed sequence

1. launch seed process **A** and prepare the verified-input target;
2. attach LocalView and retain exact A lineage: PID/HWND, target incarnation, provider incarnation, element reference, snapshot cut;
3. create PREPARED journal state and mint a one-shot verified-input request for A;
4. terminate A before dispatch;
5. launch seed process **B** from the same seed binary with the same AutomationId/name but a new process/window/provider incarnation;
6. attempt execution using the already-minted A request;
7. require fail-closed rejection before any input side effect;
8. independently assert B observed zero verified-input effect;
9. separately prove that B can be reacquired only through a **new** attachment/snapshot/authority flow.

### Identity rule

Matching human-readable title, AutomationId, role, location, or executable path is insufficient to transfer authority. The request remains bound to A's exact provider/target/element lineage and preparation journal sequence.

### Pass boundary

W12 passes only when:

- stale A authority cannot dispatch after A is gone;
- B is not treated as A merely because semantic labels match;
- B receives zero effect from the stale request;
- reacquiring B yields fresh incarnation evidence;
- no blind replay of A's request is authorized.

## 6. Production changes

Production code changes are allowed only where tests expose a real authority gap.

Expected default outcome:

- W11 should reuse `WindowsUiaDispatchContextRequirements::require_no_modal_blocker` and `ModalBlockerPresent` unchanged;
- W12 should reuse existing target/provider/element lineage checks in the worker/runtime path.

If either real seed reveals that production code observes the wrong boundary or accepts stale lineage, the implementation may add the narrowest typed validation needed. It must not add a second dispatcher, public raw inserter, automatic target rebinding, or retry authority.

## 7. Seed and oracle extensions

Extend `tools/provider-seeds/windows-uia-edge-seed` with test-only W11/W12 controls.

### W11 fixture

Add an owned modal/popup with:

- deterministic title;
- deterministic owner = main seed window;
- stable queryable HWND while open;
- explicit open/close commands;
- cleanup on shutdown.

Oracle state must expose only synthetic facts needed for validation:

```text
modal_window_handle
modal_is_open
modal_owner_window_handle
verified_input_effect_count
```

### W12 fixture

No privileged restart hook is added to production. The harness owns process lifecycle. Oracle records each seed's independent `seed_run_id`, PID, main HWND and effect count so process A and B cannot be conflated.

## 8. Validation Lab semantics

Extend `RealProviderCaseKind` with exact W11/W12 cases:

```rust
W11ModalBeforeDispatch {
    modal_blocker_observed: bool,
    input_inserted: bool,
    target_effect_observed: bool,
}

W12TargetRestartAfterAuthorization {
    original_target_gone: bool,
    replacement_target_present: bool,
    stale_authority_rejected: bool,
    replacement_effect_observed: bool,
    fresh_reacquire_required: bool,
}
```

No synthetic metric is invented merely to count these cases. RPOMR remains eligible for completed independent provider/oracle comparisons.

A blocked result is not a success unless the independent oracle agrees that the unsafe side effect did not occur.

## 9. Campaign shape

The prospective real-provider campaign after this slice contains exactly:

```text
W01 W02 W03 W04 W05 W06 W07 W08 W09 W11 W12
```

It intentionally does **not** claim W10 was measured. Evidence metadata must make the non-contiguous case set explicit rather than calling it “W01-W12 complete”.

The existing W01-W09 evidence remains individually addressable and unchanged.

## 10. Failure handling

- modal open command fails -> W11 unmeasured/fail; never convert to pass;
- modal exists but final worker evidence does not observe it -> counterexample;
- any W11 target effect occurs -> counterexample;
- process A does not terminate cleanly -> W12 unmeasured/fail;
- process B is ambiguous or cannot be independently identified -> W12 unmeasured/fail;
- stale A request reaches any insertion attempt after restart -> counterexample;
- B receives an effect from A authority -> counterexample;
- request/receipt lineage mismatch -> dispatch-uncertain/reconciliation semantics, no retry;
- cleanup cannot prove modal/process baseline -> explicit cleanup failure.

Retries are limited to clearly pre-dispatch test-infrastructure operations and may not erase the first failed evidence record.

## 11. Security and privacy

- no arbitrary external HWND transport is introduced;
- no pointer/mouse injection;
- no Unicode/arbitrary text typing;
- no clipboard path;
- no password/secret-field input;
- no privileged helper;
- no automatic modal dismissal;
- no automatic target restart/rebind;
- no automatic replay of stale authority;
- oracle data is synthetic test metadata only.

## 12. CI and evidence gates

### Cross-platform pure gates

- Validation Lab W11/W12 typed semantics;
- stale-authority/no-retry reducer semantics where OS-independent;
- campaign exact-case-set authority;
- no shipping dependency on seed/oracle code.

### Windows provider/runtime gates

- modal blocker remains part of immediate dispatch context;
- stale target/provider/element lineage fails closed;
- no raw `SendInput` public bypass regresses.

### Windows L7 real-provider gate

- W11 real owned-modal race with zero target effect;
- W12 real process A->B restart with stale authority rejection and zero B effect;
- fresh B reacquisition demonstrated separately;
- prospective exact case set W01-W09 + W11 + W12;
- artifact lineage binds exact candidate SHA, seed digest and environment metadata.

## 13. Non-goals

This slice does not close:

- W10 mixed DPI;
- W13 sensitive text/redaction;
- W14 owner-drawn weak accessibility;
- W15 low-resource degradation;
- pointer input;
- arbitrary text input;
- broad Windows provider `SUPPORTED` status.

## 14. Completion boundary

This slice is complete only when:

1. W11 opens a real owned modal after preparation and the immediate worker fence blocks with zero insertion/effect;
2. W12 proves stale A authority cannot dispatch to restarted B and B receives zero effect;
3. B can only be acted upon after fresh reacquisition and new authority;
4. no automatic retry/rebind/dismiss behavior is introduced;
5. Validation Lab records exact W11/W12 independent oracle semantics;
6. the prospective campaign contains exactly W01-W09 + W11 + W12 and does not claim W10;
7. existing W01-W09, UIA semantic action, verified-input and full CI gates remain green;
8. no public raw-input bypass or shipping dependency on seed/oracle code exists.

Passing this slice permits only the bounded claim that W11/W12 are implemented and measured at their declared evidence levels. It does not permit a broad Windows provider support claim.