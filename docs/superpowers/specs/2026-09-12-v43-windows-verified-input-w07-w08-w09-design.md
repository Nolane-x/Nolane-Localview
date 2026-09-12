# V4.3 Windows Verified Input Authority — W07/W08/W09 Design

Date: 2026-09-12

Status: Approved continuation of the V4.3 Windows L7 provider program

Branch: `feat/v43-l7-windows-w07-w08-w09-input-authority`

Base: `main@c357f172df93b869a87583a0fd9be469326ea020`

## 1. Purpose

This slice adds the first production raw-keyboard fallback authority and closes three Windows V4.3 input/action-race seeds:

- **W07 — foreground stolen between preflight and input**;
- **W08 — partial input dispatch**;
- **W09 — user-held modifier interference**.

The slice is intentionally narrower than “Windows input automation”. It proves one bounded keyboard-dispatch primitive whose correctness can be audited end-to-end. It does not add pointer injection, clipboard typing, secret-field typing, privileged helpers, broad text synthesis, or generic “type anything anywhere” capability.

The implementation must preserve the existing semantic-provider preference. UIA Value/Invoke/Selection actions remain stronger and preferred; raw input is an explicit fallback path with its own authorization, volatile-state dependencies, receipt semantics, and post-dispatch reconciliation obligation.

## 2. Source-of-truth requirements

This design implements the V4.3 requirements around:

- explicit escalation from provider actions to verified OS input (§1231–§1232);
- fallible foreground acquisition and immediate dispatch fence (§1233–§1235);
- distinct `InputDispatchReceipt` (§1236);
- partial `SendInput` semantics (§1237);
- conservative zero-insert diagnosis (§1238);
- keyboard/modifier state as dispatch dependency (§1239–§1241);
- Windows integrity/protected-surface boundaries (§1242–§1243);
- Windows seed matrix W07/W08/W09 (§1323);
- input invariants I-V43-018..022 (§1364);
- Windows verified-action migration ordering (§1373);
- Windows provider Definition of Done fields for foreground, partial dispatch and modifier interference (§1380).

## 3. Selected architecture

The selected architecture is a **separate verified-input primitive** adjacent to the existing Windows UIA dispatch stack, not a mutation of `WindowsUiaPatternDispatchOperation`.

```text
existing consequential action authority
    |
    +--> semantic UIA pattern executor (preferred)
    |
    +--> verified keyboard fallback admission
             |
             +--> bind exact target/provider/action lineage
             +--> bind exact intended key batch
             +--> snapshot keyboard/input state
             +--> final foreground/focus/modal recheck
             +--> SendInput wrapper
             +--> InputDispatchReceipt
             +--> durable dispatch linearization
             +--> mandatory post-dispatch reconciliation/verification
```

The raw-input backend is isolated in a focused provider module. The existing `dispatch_context` evaluator remains the canonical foreground/focus/modal gate and is reused rather than copied. The new input-state evaluator is pure and OS-independent so W09 semantics can be unit/mutation tested without Windows.

## 4. Why raw input is a separate authority surface

`WindowsUiaPatternDispatchOperation` represents semantic provider operations such as Invoke or SetValue. A raw keyboard batch has materially different correctness semantics:

- foreground state matters at the exact insertion boundary;
- user keyboard state can alter the meaning of the batch;
- the backend can insert only a prefix/subset of requested events;
- API acceptance is not application/world success;
- post-dispatch reconciliation is required even when every event was inserted.

Therefore the new path must not overload `DispatchResult::Succeeded` or an existing UIA pattern receipt to represent input insertion.

## 5. Canonical input batch

The first production batch form is intentionally small:

```rust
pub struct WindowsVerifiedKeyEvent {
    pub virtual_key: u16,
    pub transition: WindowsKeyTransition,
}

pub enum WindowsKeyTransition {
    KeyDown,
    KeyUp,
}

pub struct WindowsVerifiedKeyboardBatch {
    pub events: Vec<WindowsVerifiedKeyEvent>,
}
```

Constraints:

- batch size is bounded to 32 events;
- virtual key `0` is rejected;
- no Unicode/text expansion in this slice;
- no caller-supplied raw Win32 `INPUT` flags;
- no mouse events;
- the batch is canonicalized exactly as ordered; reordering is forbidden.

This is enough to test verified chords and navigation keys while keeping the semantic surface auditable.

## 6. Input-state snapshot and W09

Create a pure input-state model:

```rust
pub struct WindowsKeyboardStateSnapshot {
    pub shift_down: bool,
    pub control_down: bool,
    pub alt_down: bool,
    pub left_windows_down: bool,
    pub right_windows_down: bool,
    pub caps_lock_on: bool,
    pub num_lock_on: bool,
    pub scroll_lock_on: bool,
    pub layout_identity: Option<String>,
}
```

The first authorization profile is conservative: any physically-held Shift/Ctrl/Alt/Windows modifier that is not explicitly represented as already-held input state in the authorized batch profile blocks dispatch with `InputStateConflict`.

LocalView MUST NOT synthesize key-up events to release user-held modifiers. This slice has no “normalize keyboard state” behavior.

Lock keys are recorded in evidence but do not independently block a non-text virtual-key batch unless the action profile declares them relevant. Layout identity is recorded when available and remains explicit unknown otherwise.

## 7. Foreground/focus dispatch fence and W07

The existing `WindowsUiaDispatchContextRequirements` / `evaluate_windows_uia_dispatch_context` path remains authoritative for:

- target HWND/PID identity;
- current foreground HWND/PID;
- exact focused element where required;
- modal blocker state.

The verified-input executor MUST collect and evaluate a fresh context **inside the same worker command immediately before `SendInput`**. A context receipt obtained earlier during preflight or execution-arm is insufficient on its own.

If foreground/focus/modal state changes between planning and the final worker fence, no input is emitted and the result is a typed blocker. `SetForegroundWindow()` success/request is never treated as proof that the target is foreground.

## 8. `InputDispatchReceipt`

A dedicated receipt records insertion separately from world outcome:

```rust
pub struct WindowsInputDispatchReceipt {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub foreground_before_dispatch: WindowsUiaDispatchContextObservation,
    pub input_state_before_dispatch: WindowsKeyboardStateSnapshot,
    pub requested_event_count: u32,
    pub inserted_event_count: u32,
    pub api_status: WindowsInputApiStatus,
    pub blocker: Option<WindowsInputDispatchBlocker>,
    pub reconciliation_required: bool,
}
```

The receipt is provider evidence, not final verification. `inserted_event_count == requested_event_count` means the OS API reported all requested input events inserted; it does not prove the application performed the intended action.

## 9. W08 partial and zero dispatch classification

A tiny wrapper boundary makes the Win32 API result independently testable:

```rust
pub trait WindowsInputInserter {
    fn insert(&self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult;
}
```

Production implementation calls `SendInput`. Test implementations can return exact inserted counts without faking provider semantics.

Classification:

```text
inserted == requested > 0
  -> FULLY_INSERTED
  -> reconciliation_required = true

0 < inserted < requested
  -> PARTIAL_DISPATCH
  -> UNKNOWN_OUTCOME
  -> reconciliation_required = true
  -> no automatic retry

inserted == 0
  -> INPUT_DISPATCH_BLOCKED / UNKNOWN_CAUSE
  -> preserve raw diagnostic code when available
  -> MUST NOT claim UIPI specifically without independent evidence

inserted > requested
  -> malformed/impossible backend result
  -> fail closed as dispatch-uncertain
```

A partial dispatch is a real external side effect. The action attempt is durably linearized with partial/unknown semantics and cannot be silently replayed.

## 10. Authority and lifecycle integration

The first slice reuses the existing consequential-action journal and one-shot execution permit. It adds a **keyboard fallback execution request** that is minted only after:

1. canonical action authority is still current;
2. durable state is `PREPARED`;
3. exact target/provider/element lineage is current;
4. the fallback path was explicitly admitted for the action;
5. the exact key batch is bound to that admitted action;
6. a final context/input-state fence passes.

No public transport client may construct a raw-input execution request directly.

The provider receipt must bind the same action ID, preparation sequence, target/provider incarnation and exact batch digest. Receipt mismatch becomes dispatch-uncertain and leaves the journal in reconciliation semantics rather than granting retry authority.

## 11. Seed strategy

### W07 — foreground stolen

Use the existing Windows edge seed plus a second deterministic helper/top-level window controlled only by the test harness.

Sequence:

```text
authorize keyboard fallback for seed target
prove target foreground/focused
arm foreground thief
cross final-preflight boundary
steal foreground before insertion fence
execute
```

Pass only if final worker fence observes the mismatch and `requested_event_count > 0` while `inserted_event_count == 0`.

The seed oracle independently reports which window owns foreground and whether the target observed any key effect.

### W08 — partial dispatch

The real `SendInput` API does not provide a deterministic hosted-CI mechanism to force an exact partial count. Therefore W08 uses the production classification path with a test-only inserter wrapper that returns `0 < inserted < requested`, plus a separate Windows real-provider smoke proving the production inserter is wired through the same receipt path for ordinary full insertion.

W08 evidence must be labeled accordingly: the partial-count semantics are wrapper/property evidence, not a claim that hosted Windows naturally produced a partial `SendInput` result.

### W09 — user-held modifier interference

The Windows seed/harness physically sets a real modifier state using a test-owned helper before LocalView's input-state snapshot. The LocalView batch itself does not contain authority to release that user-held modifier.

Pass only if the final input-state fence returns `InputStateConflict` before `SendInput` and the oracle confirms the target saw zero key effect.

## 12. Validation Lab integration

Extend `RealProviderCaseKind` with:

```rust
W07ForegroundStolen {
    final_foreground_mismatch_detected: bool,
    input_inserted: bool,
}
W08PartialInputDispatch {
    requested_event_count: u32,
    inserted_event_count: u32,
    unknown_outcome_preserved: bool,
    blind_retry_authorized: bool,
}
W09ModifierInterference {
    conflicting_modifier_observed: bool,
    input_state_conflict_blocked: bool,
    input_inserted: bool,
}
```

No synthetic metric should be invented merely to count these seeds. RPOMR remains eligible for completed independent provider/oracle comparisons. Existing action-safety metrics may be used only if their denominator opportunity exactly matches the seed semantics.

The prospective L7 campaign expands from W01–W06 to exact W01–W09 evidence only after the three new cases are independently green.

## 13. Security and privacy

- No arbitrary external target HWND is accepted from public transport.
- No elevated helper or privilege-bypass mechanism is added.
- No secure/password field input is enabled.
- No clipboard content is used.
- Keyboard-state evidence contains key-state booleans/layout metadata only, never typed text.
- Test seed data is synthetic.
- A protected/inaccessible surface remains typed unsupported/blocked.

## 14. Resource and failure behavior

The new path is on-demand only. It creates no resident poller, no new long-lived thread beyond the existing Windows provider worker model, and no continuous keyboard hook.

Failure rules:

- stale target/provider/element -> block before input;
- foreground/focus/modal mismatch -> block before input;
- conflicting modifier -> block before input;
- inserter transport/backend error before known insertion -> typed blocked/unknown cause;
- partial insertion -> unknown outcome + reconciliation required + no blind retry;
- full insertion -> reconciliation required before world-success claim;
- receipt mismatch -> dispatch uncertain, no blind retry;
- crash after possible insertion -> recover from durable journal as possibly dispatched.

## 15. CI and evidence gates

Cross-platform pure tests:

- batch validation/canonicalization;
- input-state conflict evaluator;
- insertion-count classifier;
- receipt exact-binding validation;
- Validation Lab W07/W08/W09 semantics.

Windows-only provider tests:

- final foreground fence adjacent to inserter;
- modifier snapshot adjacent to inserter;
- real production `SendInput` wrapper full-insertion smoke against the deterministic seed target;
- no input on W07/W09 blocker paths.

Windows L7 gate:

- W07 real foreground-steal seed;
- W08 deterministic production-classifier wrapper campaign + production inserter smoke, with evidence class clearly labeled;
- W09 real modifier-interference seed;
- prospective campaign requiring exact W01–W09 observation digests.

## 16. Non-goals

This slice does not add:

- pointer/mouse injection;
- Unicode or arbitrary text typing;
- clipboard input;
- secret/password-field input;
- privileged/elevated automation;
- automatic modifier release;
- IME composition control;
- arbitrary public raw-input RPC;
- full Windows provider `SUPPORTED` claim;
- W10/W11/W12/W13/W14/W15 closure.

## 17. Completion boundary

This slice is complete only when:

1. W07 foreground theft is blocked at the immediate input boundary with zero insertion;
2. W08 partial insertion produces a dedicated partial/unknown receipt and cannot authorize blind retry;
3. W09 real conflicting modifier state blocks before insertion without releasing the user's key;
4. full insertion remains distinct from verified world outcome;
5. exact W01–W09 prospective campaign passes on the declared Windows environment;
6. existing W01–W06 and semantic UIA action paths remain green;
7. no raw-input public bypass or heavy resident path is introduced.

Passing this slice permits the repository to claim the bounded W07/W08/W09 contracts are implemented and measured at their declared evidence levels. It does not permit a broad “Windows input automation is complete” claim.