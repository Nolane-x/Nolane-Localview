# Windows UIA SetValue Payload Authority Design

## Status

Approved architectural direction for the next Windows verified-action slice after D2 native-surface restart reconciliation.

Base: `main` at `ebb024cfaa6bdde0c647c2f875671d756c54dbfa`.

Branch: `feat/v43-windows-set-value-payload-authority`.

This slice extends the verified Windows consequential path from payload-free semantic actions (`Invoke`, `SelectionItem`, `Toggle`, `Expand`, `Collapse`) to the first payload-bearing semantic action: UI Automation `ValuePattern.SetValue`.

## Goal

Add a production-safe `set_value` action without weakening any existing V4.3 authority, durability, privacy, crash-recovery, or postcondition rules.

The result must guarantee that:

- the value to write never becomes durable plaintext in the consequential journal, operation binding, postcondition contract ref, semantic snapshot, diagnostic response, or provider receipt;
- the exact value authorized for one action cannot be substituted before dispatch;
- a crash or daemon restart cannot reconstruct the value or dispatch authority from durable state;
- `SetValue` is attempted exactly once after the existing fresh-evidence, exact-lineage, confirmation, PREPARED, execution-arm, and volatile-context fences pass;
- provider acknowledgement is still not world-state proof;
- a fresh post-dispatch UIA read must independently prove that the target value equals the authorized value before the action can become `VerifiedExpected` and `Committed`;
- password/secure fields are not supported by this first slice and fail closed before any value mutation;
- no keyboard fallback is introduced. Verified keyboard input remains a later, separately designed escalation path.

## Existing boundary

The current Windows verified-action path deliberately keeps `CanonicalActionOperation` payload-free. The durable operation companion records only the semantic operation class and exact admitted intent sequence. Raw text, keys, and coordinates are explicitly excluded from that authority record.

The current product routes are also server-owned. Clients cannot choose UIA pattern, provider verb, risk class, idempotency class, principal, provider incarnation, or target incarnation. Planning refreshes provider evidence first and confirmation is process-local and one-shot.

That boundary must remain intact. This design therefore does **not** add `value: String` to the existing durable operation binding and does **not** reuse legacy `BridgeActionKind::TypeText` plaintext as correctness authority.

## Non-goals

This slice does not add:

- keyboard typing or `SendInput` fallback;
- append/insert/range-edit semantics;
- clipboard-based text entry;
- password or secure-field mutation;
- credential entry;
- pointer fallback;
- cross-platform AX/AT-SPI text mutation;
- application-specific low-risk classification;
- automatic redispatch after restart;
- durable plaintext, reversible encryption, or machine-key secret storage;
- a generic payload framework for every future key/pointer/drag action.

The initial semantic modes are only:

```text
replace_value(value)
clear_value
```

`clear_value` is represented explicitly rather than smuggling semantic meaning through an empty caller string.

## Selected architecture

The design introduces five narrowly separated concepts:

1. `CanonicalActionOperation::SetValue` — payload-free semantic operation identity;
2. `ProcessLocalSetValuePayload` — the exact plaintext value held only in process memory;
3. `SetValuePayloadRef` — random UUID identity for one process-local payload capability;
4. `DurableSetValuePayloadBinding` — durable metadata binding the admitted action to an opaque keyed commitment, never plaintext;
5. `WindowsUiaSetValueVerificationReceipt` — fresh provider-owned post-dispatch equality evidence that exposes only equality/lineage metadata, never the observed value.

These are intentionally distinct. Operation identity does not contain payload. Durable metadata does not recreate plaintext. Provider dispatch evidence does not imply verification.

## Canonical operation

Add a new enum value:

```text
CanonicalActionOperation::SetValue
```

`InputText` remains reserved for later insert-text / keyboard-style semantics. `SetValue` means replacement through a semantic provider value capability.

The direct canonical compatibility carrier remains payload-free. The product path must not put the requested value into a legacy public queue or any durable envelope field.

The durable existing operation sidecar remains unchanged in shape and records only `SetValue`.

## Process-local payload capability

Planning creates one process-local payload object:

```text
ProcessLocalSetValuePayload {
  payload_ref: UUID,
  action_id: UUID,
  mode: Replace | Clear,
  utf8_bytes: secret process-local buffer,
  commitment: opaque keyed digest,
  consumed: bool
}
```

Properties:

- created only after fresh exact target evidence and `ValuePattern` preflight pass;
- bound to exactly one action id;
- stored only in the same process-local pending plan that owns the confirmation capability;
- not serializable;
- not cloneable as authority;
- consumed exactly once by the exact confirmation path;
- never reinserted after confirmation consumption, provider error, unknown dispatch, or verification failure;
- erased on normal drop using a zeroizing memory wrapper where supported by the Rust dependency surface;
- lost on process/daemon restart by design.

HTTP JSON parsing necessarily materializes caller text briefly in memory. The implementation must minimize additional copies and move the parsed value into the zeroizing payload owner as early as practical.

## Payload limits and canonical bytes

For the first slice:

- `replace_value` accepts UTF-8 JSON text up to 16 KiB;
- `clear_value` accepts no value field;
- embedded U+0000 is rejected because provider/string-boundary behavior is not part of this slice's conformance proof;
- whitespace is significant;
- no trimming is performed;
- no Unicode normalization is performed;
- the exact UTF-8 string supplied by the caller is the authorized payload.

The commitment canonical encoding is domain-separated and includes at least:

```text
version
+ action_id
+ payload_ref
+ mode
+ exact UTF-8 payload bytes
```

## Opaque keyed commitment

Each `WindowsConsequentialControlHandle` owns one random 256-bit payload commitment key for that process/control-handle lifetime.

The durable binding stores an HMAC-SHA256 commitment produced with that process-local key. The key itself is never persisted.

Conceptual durable record:

```text
DurableSetValuePayloadBinding {
  action_id,
  intent_journal_sequence,
  payload_ref,
  mode,
  payload_utf8_len,
  commitment_algorithm = "hmac-sha256-process-v1",
  commitment_digest
}
```

Security properties:

- the durable record contains no plaintext;
- after process death the commitment key is gone, so the record is not a reusable password/value oracle and cannot reconstruct dispatch authority;
- before dispatch the same live process can recompute the commitment and reject payload substitution;
- payload bindings are immutable `create_new` companions tied to the exact admitted intent sequence, following the existing durable operation-binding pattern;
- an old/stale companion file cannot authorize another action because action id + intent sequence + payload ref must all match.

A restarted process may read this record for diagnostics/recovery classification, but cannot resolve it into plaintext or a dispatch payload.

## Plan protocol

Add:

```text
POST /v1/sessions/{id}/windows-observe/consequential/set-value/plan
```

Request, replace mode:

```json
{
  "element_ref": { "...": "exact ProviderElementRef" },
  "mode": "replace_value",
  "value": "exact caller text"
}
```

Request, clear mode:

```json
{
  "element_ref": { "...": "exact ProviderElementRef" },
  "mode": "clear_value"
}
```

Unknown fields are rejected. The client cannot provide:

- UIA pattern;
- provider dispatch verb;
- risk/idempotency class;
- principals;
- authorization revision;
- provider/target incarnation;
- payload commitment/digest;
- postcondition verdict.

The successful plan response returns metadata only:

```text
action_id
confirmation_ref
payload_ref
mode
payload_utf8_len
planning_reconciliation_receipt_ref
operation = set_value
risk_class
idempotency_class
```

It never echoes the requested value or commitment key.

## Plan ordering

Planning is serialized by the existing plan gate and must proceed in this order:

1. authenticate control bearer;
2. require current `SessionId`;
3. capture fresh Windows provider evidence and exact element rebind;
4. require complete current observation;
5. preflight exact `ValuePattern` capability;
6. require `IsPassword == false` from fresh provider evidence;
7. require `ValuePattern.IsReadOnly == false` from fresh provider evidence;
8. allocate process-local confirmation and payload refs;
9. bind direct canonical action with `CanonicalActionOperation::SetValue` semantics and no plaintext carrier authority;
10. durably record `IntentAdmitted`;
11. durably record payload-free canonical operation `SetValue`;
12. compute process-local keyed payload commitment;
13. fsync immutable `DurableSetValuePayloadBinding`;
14. insert the process-local pending confirmation + payload capability;
15. return the plan response.

If any durable step fails, no process-local confirmation is published. A partially admitted durable action remains non-dispatchable and reconciliation/diagnostic only.

## Risk and idempotency

Generic UIA capability does not prove application-level consequences. The first SetValue slice therefore preserves the existing conservative generic-write floor:

```text
risk_class = S4 destructive_or_irreversible
idempotency_class = irreversible
```

This may be relaxed later only by a stronger application-specific semantic profile and corresponding tests. UIA `SetValue` appearing mechanically repeatable is not enough to claim business-level idempotency.

## Confirmation and payload consumption

The existing confirmation endpoint remains the only user-authority transition.

On exact confirmation:

1. resolve exact pending action + confirmation;
2. atomically remove the pending plan from process-local storage;
3. move, not clone, the `ProcessLocalSetValuePayload` into the verified execution transaction;
4. recompute and compare the HMAC commitment against the durable payload binding;
5. require exact action id, intent sequence, payload ref, mode, and byte length;
6. only then allow the existing authorization revalidation / PREPARED / arm sequence to continue.

A wrong confirmation ref does not consume the exact pending payload. An exact confirmation consumes both confirmation and payload authority before consequential execution begins, preserving the existing no-blind-retry rule.

## Pre-dispatch authority

The existing sequence remains mandatory:

```text
fresh planning evidence
→ exact capability preflight
→ canonical intent
→ durable operation + payload binding
→ explicit confirmation
→ independent authorization revalidation
→ exact retained element lease
→ volatile foreground/focus/modal seal
→ durable PREPARED
→ second volatile context arm
→ one-shot provider execution
```

Immediately before `SetValue`, the Windows MTA worker must revalidate:

- exact provider incarnation;
- exact target incarnation;
- exact retained element;
- exact current `ValuePattern` availability;
- `IsPassword == false`;
- `ValuePattern.IsReadOnly == false`;
- final volatile context requirements;
- exact dispatch operation `SetValue`;
- exact payload ref/commitment binding supplied by the consumed process-local capability.

No semantic failure escalates automatically to keyboard input.

## Provider dispatch request and receipt

`WindowsUiaProviderExecutionRequest` gains a non-serializable optional payload-bearing variant used only for `SetValue`:

```text
SetValue {
  payload_ref,
  mode,
  secret text buffer
}
```

Existing payload-free operations carry `None`.

The request remains borrowed/move-constrained so external code cannot manufacture a replayable provider request.

The provider performs exactly one UIA `ValuePattern.SetValue(...)` call for the exact consumed request.

Provider receipts must not contain plaintext. As part of this slice, the runtime/provider receipt chain must explicitly bind the exact `dispatch_operation`; the receipt matcher must compare it. This closes any pattern-only ambiguity and keeps shared `Expand/Collapse` and the new `SetValue` verb exact.

For SetValue, the receipt additionally binds `payload_ref` and the opaque commitment digest. It does not contain the value.

`DispatchResult::DispatchedFull` remains dispatch evidence only.

## Secure-field policy

This first SetValue slice rejects a target when:

```text
IsPassword == true
IsPassword unavailable/unknown
ValuePattern.IsReadOnly == true
ValuePattern.IsReadOnly unavailable/unknown
ValuePattern unsupported
fresh exact element binding unavailable
```

The semantic snapshot may expose non-sensitive capability state such as:

```text
windows_uia.is_password = false
windows_uia.value.is_read_only = false
```

but must not expose the current text value merely to support this action.

Password/credential mutation requires a later explicit secret-delivery policy. This slice does not claim it.

## Post-dispatch verification

Provider dispatch success is not the postcondition.

After a possibly-dispatched SetValue action, LocalView must obtain fresh observation authority through the existing consequential observation permit, then perform a fresh exact UIA value read on the same provider/target lineage and exact element.

The comparison happens inside the trusted provider/runtime boundary against the still-live process-local payload. Raw observed text is not inserted into the general semantic snapshot, evidence store, journal, log, or HTTP response.

The provider returns a typed receipt conceptually containing:

```text
WindowsUiaSetValueVerificationReceipt {
  action_id,
  payload_ref,
  payload_commitment,
  mode,
  provider_incarnation_ref,
  target_incarnation_ref,
  element_ref,
  observation_cut_ref,
  is_password = false,
  is_read_only = false,
  equality = MATCH | MISMATCH | UNKNOWN
}
```

The equality comparison uses exact string semantics. No trim/case-fold/Unicode normalization is allowed.

Only `MATCH`, bound to the exact action/payload/lineage/element and a fresh post-dispatch observation cut, may contribute `VerifiedPass` for the mandatory SetValue payload postcondition.

## Postcondition contract representation

The admitted envelope must contain a server-owned mandatory opaque payload-equality contract reference that contains no plaintext, for example:

```text
lvpc:payload-equality:v1:{"mode":"replace_value","payload_ref":"<uuid>"}
```

`clear_value` uses its own mode value.

The shared postcondition registry owns parsing/versioning of this contract family. Generic native-semantic snapshot evaluation returns `Unknown` for it because a normal snapshot deliberately lacks raw value authority.

The SetValue verifier can resolve it only when it has:

- the exact durable payload binding;
- the matching live process-local payload capability/commitment context;
- a fresh `WindowsUiaSetValueVerificationReceipt`.

This preserves one shared correctness-schema registry while preventing normal semantic snapshots from becoming plaintext stores.

Additional caller-selected arbitrary postcondition contracts are out of scope for the first SetValue route. The mandatory equality contract is server-created and sufficient for initial product correctness.

## Crash and restart semantics

### Crash before confirmation

Durable intent/operation/payload descriptor may remain, but confirmation and plaintext capability are gone. No dispatch is possible.

### Crash after confirmation but before provider dispatch

Durable state may be `AUTHORIZED` or `PREPARED`, but the payload capability is gone. Recovery may observe/reconcile only; it cannot recreate payload or redispatch.

### Crash after possible dispatch but before value verification

Recovery may capture current world state, but the process-local commitment key/plaintext no longer exists. The payload-equality contract therefore evaluates `Unknown` and the action remains `RECONCILIATION_REQUIRED` / postcondition-not-verified. There is no blind retry.

### Crash after durable VerifiedExpected receipt but before commit

Existing `VerifiedUncommitted` commit-only recovery remains valid because the durable receipt already proves the exact postcondition. No plaintext or new provider dispatch is needed.

### Already committed

Existing durable terminal semantics remain unchanged.

This deliberate asymmetry favors privacy and no-replay authority over automatic recovery of a lost plaintext payload.

## Error taxonomy

Add stable typed errors for at least:

```text
set_value_payload_too_large
set_value_payload_invalid_nul
set_value_mode_invalid
set_value_password_field_blocked
set_value_password_state_unavailable
set_value_read_only
set_value_read_only_state_unavailable
set_value_payload_binding_missing
set_value_payload_binding_mismatch
set_value_payload_capability_missing
set_value_payload_already_consumed
set_value_verification_mismatch
set_value_verification_unknown
```

Raw UIA/HRESULT diagnostics may be preserved separately, but human error text must not determine recovery behavior.

## Concurrency and one-shot rules

- pending capacity remains bounded by the existing Windows consequential pending-plan limit;
- one action owns one payload ref;
- one payload ref belongs to one action;
- confirmation and payload consumption are serialized under the existing pending-plan/plan gate boundaries;
- no second confirmation path may resolve the same payload;
- no payload capability is recreated from durable sidecars;
- exact confirmation does not grant retry after provider uncertainty;
- cleanup/detach/session removal drops and zeroizes unconsumed payloads together with pending confirmations.

## Required TDD gates

Implementation must proceed RED → GREEN in small causal slices.

At minimum, permanent tests must prove:

1. `CanonicalActionOperation::SetValue` exists and remains payload-free;
2. durable payload binding contains no plaintext and is immutable/create-new;
3. wrong action/intent sequence/payload ref/commitment fails closed;
4. daemon restart cannot resolve a durable payload binding into dispatch authority;
5. HTTP plan rejects client-forged pattern/verb/risk/commitment fields;
6. HTTP plan never echoes plaintext;
7. password target is rejected before intent/confirmation publication;
8. read-only target is rejected before intent/confirmation publication;
9. exact confirmation consumes payload exactly once;
10. missing/substituted payload blocks before PREPARED/provider side effect;
11. canonical SetValue + ValuePattern is the only mapping that mints provider `SetValue`;
12. provider request/receipt bind exact dispatch operation and payload ref;
13. real Win32 UIA SetValue changes a non-password editable fixture exactly once;
14. provider dispatch acknowledgement alone cannot commit;
15. fresh post-dispatch equality `MATCH` can produce VerifiedExpected + durable commit;
16. fresh post-dispatch `MISMATCH`/`UNKNOWN` cannot commit;
17. crash/reopen after PREPARED or PossiblyDispatched cannot redispatch without payload capability;
18. VerifiedUncommitted commit-only recovery remains valid;
19. cleanup removes/zeroizes pending payload state;
20. existing Invoke/Select/Toggle/Expand/Collapse regressions remain green.

## Windows workflow gate

The Windows UIA workflow must gain explicit named gates for:

- SetValue server-owned HTTP contract;
- payload-binding/privacy contract;
- real retained Win32 UIA ValuePattern capability/read-only/password preflight;
- real SetValue dispatch worker smoke;
- real HTTP SetValue plan → confirm → fresh equality verification → durable committed result;
- restart/no-payload/no-redispatch recovery contract.

The normal cross-platform CI matrix must remain green because the shared canonical/journal/postcondition changes are platform-neutral correctness code even though real UIA execution is Windows-only.

## Scope boundary

Expected production touch points are limited to:

- `crates/live-bridge` canonical operation + durable opaque payload binding;
- `crates/postcondition-contracts` opaque payload-equality schema/parser;
- `crates/windows-uia-provider` ValuePattern capability/dispatch/fresh equality observation;
- `crates/windows-observe-runtime` payload-aware execution/verification plumbing;
- `crates/control` server-owned SetValue plan + process-local payload lifecycle;
- Windows workflow gates and focused tests.

No Perception Budget, native-surface D2 ownership, Chromium governor, generic keyboard/pointer injection, macOS AX, or Linux AT-SPI authority is changed by this slice.

## Definition of done

This slice is complete only when all of the following are true on one exact PR head:

- no raw SetValue payload appears in durable files, generic semantic snapshots, receipts, logs, or HTTP responses;
- exact payload substitution is fenced before dispatch;
- password/read-only/unsupported targets fail closed;
- one-shot real UIA SetValue succeeds on an editable non-secure fixture;
- fresh independent equality verification, not provider ack, is what permits `VerifiedExpected`;
- restart cannot recreate payload or dispatch authority;
- unknown post-crash payload outcome remains reconciliation-only with no blind retry;
- all existing Windows semantic actions remain green;
- Rust workspace Check + Clippy + tests pass on Ubuntu/macOS/Windows;
- Tauri/frontend and WebKitGTK/WKWebView/WebView2 rendered-pixel smokes pass;
- Windows UIA workflow passes the new real SetValue gates;
- whole-PR scope audit shows no unrelated authority widening;
- final PR head is exact-SHA GREEN before merge and post-merge `main` is verified again.