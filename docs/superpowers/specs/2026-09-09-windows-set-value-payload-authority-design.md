# Windows UIA SetValue Payload Authority Design

## Status

Approved architectural direction for the next Windows verified-action slice after D2 native-surface restart reconciliation.

Base: `main@ebb024cfaa6bdde0c647c2f875671d756c54dbfa`.

Branch: `feat/v43-windows-set-value-payload-authority`.

This design extends the verified Windows consequential path from payload-free semantic actions (`Invoke`, `SelectionItem`, `Toggle`, `Expand`, `Collapse`) to the first payload-bearing semantic action: UI Automation `ValuePattern.SetValue`.

## Goal

Add a production-safe `set_value` action without weakening existing V4.3 authority, durability, privacy, crash-recovery, or postcondition rules.

The slice must guarantee all of the following:

- no SetValue plaintext becomes durable in the consequential journal, operation binding, payload binding, postcondition contract ref, semantic snapshot, evidence receipt, HTTP response, workflow log, or diagnostic text;
- the exact value authorized for one action cannot be substituted before dispatch;
- restart cannot reconstruct the value, confirmation, payload capability, or dispatch authority from durable state;
- `SetValue` is attempted exactly once only after fresh evidence, exact lineage, confirmation, authorization revalidation, durable PREPARED, execution arm, and final volatile-context fences pass;
- provider acknowledgement never becomes world-state proof;
- fresh post-dispatch UIA equality observation, not provider acknowledgement, is required before `VerifiedExpected` and durable commit;
- password/secure fields and unknown password state fail closed before any mutation;
- read-only and unknown-read-only targets fail closed before any mutation;
- no keyboard, clipboard, pointer, or other fallback is introduced by this slice.

## Existing architectural boundary

The current code deliberately keeps `CanonicalActionOperation` payload-free. `DurableCanonicalActionOperationBinding` records only the exact admitted action, intent journal sequence, and semantic operation class. Raw text, keys, and coordinates are explicitly excluded.

The current Windows provider path also has two distinct execution layers:

1. `windows-observe-runtime` mints the opaque, non-Clone `WindowsUiaProviderExecutionRequest` from an already-armed one-shot permit;
2. `windows-uia-provider` converts that request into an owned MTA worker command. Existing `WindowsUiaPatternDispatchRequest` is payload-free and `Clone`.

The SetValue design must preserve both facts. It therefore does **not** add plaintext to the durable operation binding and does **not** add plaintext to `WindowsUiaPatternDispatchRequest`.

## Non-goals

This slice does not add:

- keyboard typing or `SendInput` fallback;
- append/insert/range-edit semantics;
- clipboard-based text entry;
- password, secure-field, token, API-key, or credential entry;
- pointer fallback;
- cross-platform AX/AT-SPI text mutation;
- a generic payload framework for future key/pointer/drag operations;
- application-specific low-risk classification;
- automatic redispatch after restart;
- reversible encryption or machine-key storage of payload plaintext;
- durable expected-value plaintext for postcondition recovery.

Initial semantic modes are only:

```text
replace_value(value)
clear_value
```

`clear_value` is explicit. It is not inferred from an empty caller string.

## Selected architecture

The slice introduces these distinct concepts:

1. `CanonicalActionOperation::SetValue` — payload-free semantic operation identity;
2. `SetValueMode` — typed `ReplaceValue | ClearValue` semantics;
3. `SetValuePayloadRef` — random UUID identity for one process-local payload capability;
4. `ProcessLocalSetValuePayload` — the authoritative plaintext held only in process memory;
5. `DurableSetValuePayloadBinding` — immutable durable metadata with a keyed commitment, never plaintext;
6. `WindowsUiaSetValueExecutionPayload` — a borrowed runtime view of the consumed process-local payload;
7. `WindowsUiaSetValueDispatchRequest` — a dedicated non-Clone owned MTA command with a temporary zeroizing payload copy;
8. `WindowsUiaSetValueVerificationReceipt` — fresh provider-owned equality evidence that exposes no observed value.

Operation identity, payload authority, durable commitment, dispatch evidence, and verification evidence remain separate.

## Canonical operation and compatibility carrier

Add:

```text
CanonicalActionOperation::SetValue
```

`InputText` remains reserved for later insert-text / keyboard-style semantics. `SetValue` means replacement through a semantic provider value capability.

The exact semantic authority is the durably explicit `SetValue` operation binding, not the legacy carrier. For compatibility with the existing `CanonicalQueuedAction` shape, direct canonical binding may use an internal payload-free `BridgeActionKind::TypeText` placeholder whose text is empty.

That placeholder:

- never enters the legacy public action queue;
- never contains caller plaintext;
- never selects the provider verb;
- never decides Replace versus Clear;
- never authorizes the payload;
- is covered by a regression proving caller plaintext is absent from the compatibility carrier.

The product path must use:

```text
record_intent_operation_bound_explicit(..., CanonicalActionOperation::SetValue)
```

and must never derive SetValue authority from `BridgeActionKind::TypeText`.

The existing durable operation sidecar remains payload-free and unchanged in shape.

## Process-local payload owner

Planning creates exactly one process-local payload owner:

```text
ProcessLocalSetValuePayload {
  payload_ref: SetValuePayloadRef,
  action_id: UUID,
  mode: SetValueMode,
  utf8_bytes: Zeroizing<Vec<u8>>,
  consumed: bool
}
```

Requirements:

- use the Rust `zeroize` crate or an already-present equivalent with guaranteed drop-time clearing for LocalView-owned byte buffers;
- no derived `Clone`;
- no `Serialize`/`Deserialize`;
- no plaintext-bearing `Debug` output; any custom debug representation is redacted and metadata-only;
- one payload belongs to one action and one payload ref;
- payload authority is consumed exactly once with the exact confirmation;
- wrong confirmation does not consume it;
- detach, session removal, plan cleanup, or shutdown drops and zeroizes unconsumed payloads;
- provider error, ambiguous dispatch, mismatch, unknown verification, or confirmation consumption never recreates the payload capability;
- restart loses it by design.

HTTP JSON parsing necessarily creates transient plaintext allocations before LocalView can wrap the value. UI Automation conversion can also create temporary UTF-16/BSTR/HSTRING allocations outside the zeroizing Rust byte buffer. This design does **not** claim zero copies or perfect allocator-history erasure. The enforceable requirement is:

```text
no durable plaintext
+ no plaintext logs/responses/receipts
+ bounded lifetime of LocalView-owned plaintext copies
+ explicit zeroization of LocalView-owned secret buffers
```

## Payload limits and exact bytes

For the first slice:

- `replace_value` accepts UTF-8 JSON text up to 16 KiB measured in UTF-8 bytes;
- `clear_value` accepts no `value` field;
- embedded U+0000 is rejected;
- whitespace is significant;
- no trim, case folding, or Unicode normalization is performed;
- the exact UTF-8 string supplied by the caller is the authorized value.

The same exact string semantics are used for post-dispatch equality.

## Process commitment key

Each `WindowsConsequentialControlHandle` owns one 256-bit commitment key generated from an OS-backed cryptographically secure RNG for that exact process/control-handle lifetime.

The key:

- is never persisted;
- is never returned through HTTP;
- is never logged;
- is never copied into provider or postcondition receipts;
- disappears on process restart.

## Durable payload commitment

The durable payload binding uses HMAC-SHA256 with domain-separated canonical bytes:

```text
"localview:set-value-payload:v1\0"
+ action_id UUID bytes
+ payload_ref UUID bytes
+ mode tag
+ payload_utf8_len as fixed-width big-endian u64
+ exact UTF-8 payload bytes
```

Conceptual record:

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

Properties:

- stored as an immutable create-new companion tied to the exact admitted intent sequence;
- contains no plaintext;
- old action id, intent sequence, payload ref, mode, or length cannot be substituted;
- same-process confirmation recomputes and verifies the commitment using the HMAC library's constant-time verification primitive;
- after restart the key is gone, so the durable record cannot be used as a value oracle or dispatch capability;
- digest is correctness metadata only and is not exposed in normal HTTP/log output.

A durable orphan created after `IntentAdmitted` but before plan publication is diagnostic/reconciliation state only. It never creates process-local confirmation or payload authority.

## Plan protocol

Add:

```text
POST /v1/sessions/{id}/windows-observe/consequential/set-value/plan
```

Replace request:

```json
{
  "element_ref": { "...": "exact ProviderElementRef" },
  "mode": "replace_value",
  "value": "exact caller text"
}
```

Clear request:

```json
{
  "element_ref": { "...": "exact ProviderElementRef" },
  "mode": "clear_value"
}
```

Rules:

- unknown fields are rejected;
- Replace requires exactly one `value` field;
- Clear rejects any `value` field, including `""`;
- client cannot provide pattern, dispatch verb, risk/idempotency class, principals, authorization revision, provider/target incarnation, commitment, digest, or postcondition verdict.

Successful response is metadata-only:

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

It never echoes plaintext or digest.

## Planning order

Planning stays under the existing plan gate and proceeds in this exact order:

1. authenticate bearer;
2. require current `SessionId`;
3. validate mode/value structure, NUL rule, and 16 KiB bound before provider work;
4. refresh Windows provider observation and exact element rebind;
5. require complete fresh observation;
6. require exact `WindowsUiaPattern::Value` support;
7. require fresh `IsPassword == false` evidence;
8. require fresh `ValuePattern.IsReadOnly == false` evidence;
9. allocate confirmation ref and payload ref;
10. direct-bind the canonical action with the payload-free compatibility carrier;
11. durably record `IntentAdmitted`;
12. durably bind `CanonicalActionOperation::SetValue`;
13. create the process-local payload owner and compute HMAC commitment;
14. fsync immutable `DurableSetValuePayloadBinding`;
15. insert one pending confirmation + payload owner;
16. return metadata response.

If any durable step fails, no process-local confirmation is published.

## Risk and idempotency

Generic UIA Value capability does not prove application-level consequence semantics. Initial policy remains conservative:

```text
risk_class = s4_destructive_or_irreversible
idempotency_class = irreversible
```

Mechanical repeatability of `SetValue` is insufficient to claim business-level idempotency. A future application-specific semantic profile may lower the risk only with its own evidence/tests.

## Secure-field and read-only policy

Planning fails closed when any of these is true:

```text
ValuePattern unsupported/unknown
IsPassword == true
IsPassword unavailable/unknown
ValuePattern.IsReadOnly == true
ValuePattern.IsReadOnly unavailable/unknown
fresh exact element rebind unavailable
fresh observation incomplete
```

The normal semantic snapshot may expose only the non-sensitive capability facts needed for admission, for example:

```text
windows_uia.is_password = false
windows_uia.value.is_read_only = false
```

It must not read/store current text merely to plan SetValue.

The MTA worker repeats `IsPassword` and `IsReadOnly` immediately before the side effect. A property transition between plan and dispatch therefore fails closed.

Password/credential delivery needs a later explicit secret-delivery design and is not implied by this slice.

## Confirmation and payload consumption

The existing confirmation endpoint remains the only user-authority transition.

On exact confirmation:

1. resolve exact pending action + confirmation;
2. atomically remove the pending plan from process-local storage;
3. move, not clone, `ProcessLocalSetValuePayload` into the verified SetValue transaction;
4. load the exact durable payload binding;
5. recompute and constant-time verify the HMAC;
6. require exact action id, intent sequence, payload ref, mode, and byte length;
7. only then continue to authorization revalidation, PREPARED, arm, and execution.

Exact confirmation consumes confirmation and payload authority before consequential execution begins. Wrong confirmation does not consume them.

If payload verification fails after confirmation consumption, the payload is zeroized and no replacement confirmation/retry capability is minted.

## Runtime execution seam

Existing payload-free APIs remain source-compatible:

```text
WindowsUiaProviderExecutionRequest
WindowsUiaDispatchExecutor
execute_armed_uia_dispatch
```

They continue to serve Invoke/Select/Toggle/Expand/Collapse without plaintext payload fields.

SetValue gets a parallel, narrow runtime seam:

```text
WindowsUiaSetValueExecutionPayload<'a> {
  payload_ref,
  mode,
  utf8_bytes: &'a [u8]
}

execute_armed_uia_set_value_dispatch(..., payload, executor)
```

The SetValue coordinator reuses the same canonical-envelope checks, durable PREPARED state, exact retained lease, final context arm, and one-shot generic dispatch permit. It additionally requires:

- durably admitted operation is exactly `SetValue`;
- sealed pattern is exactly `WindowsUiaPattern::Value`;
- live payload ref/mode matches the exact durable payload binding;
- HMAC was successfully revalidated before the provider boundary.

The coordinator must not add a second confirmation or authorization path.

Shared internal helpers may be factored to avoid duplicating the existing lifecycle, but the public payload-free path must remain behaviorally unchanged.

## Runtime receipt hardening

Current `WindowsUiaProviderExecutionRequest` already carries `dispatch_operation`; current `WindowsUiaProviderExecutionReceipt` does not. This slice hardens the generic receipt by adding exact `dispatch_operation` and requiring `provider_receipt_matches_request` to compare it.

This is deliberately shared because pattern-only matching is insufficient for `Expand` versus `Collapse` and would also be insufficient for future Value-family verbs.

Existing payload-free operations must remain behaviorally unchanged by this hardening.

SetValue runtime receipt additionally binds opaque metadata only:

```text
payload_ref
mode
```

It does not contain plaintext or commitment key. The durable HMAC digest does not need to cross the provider boundary because runtime already verified it before dispatch.

## MTA worker boundary

Existing `WindowsUiaPatternDispatchRequest` remains payload-free and Clone. SetValue does **not** add plaintext to it.

Add a dedicated worker request:

```text
WindowsUiaSetValueDispatchRequest {
  exact existing action / PREPARED / cut / lineage / element / context fields,
  dispatch_operation = SetValue,
  required_pattern = Value,
  payload_ref,
  mode,
  utf8_bytes: Zeroizing<Vec<u8>>
}
```

Requirements:

- no `Clone`;
- no plaintext-bearing derived `Debug`;
- no serde;
- moved exactly once into `WorkerCommand::DispatchSetValue`;
- worker validates exact lineage, retained element, final context, Value support, `IsPassword == false`, and `IsReadOnly == false` immediately before mutation;
- worker performs exactly one `IUIAutomationValuePattern::SetValue(...)` call;
- Clear dispatches exactly one `SetValue("")` call but remains mode-distinct in authority metadata;
- the LocalView-owned command buffer is zeroized when dropped after the call.

Because `std::sync::mpsc` requires owned command data, the runtime's authoritative payload must be copied into one bounded temporary zeroizing worker buffer. This copy is allowed and must be explicit in tests/review. Generic payload-free worker requests remain unchanged.

UIA/Windows string conversion may create additional short-lived OS/library allocations. They must never be logged or persisted, and their lifetime must be kept adjacent to the single SetValue call. The design does not claim guaranteed zeroization of allocations owned by COM/windows-rs.

## Keeping the expected value alive for verification

The process-local transaction retains the authoritative `ProcessLocalSetValuePayload` after the worker dispatch call returns. The temporary MTA command copy is independent and is zeroized on drop.

Therefore:

- dispatch does not consume the only remaining expected-value copy;
- post-dispatch equality can still compare against the exact authorized bytes;
- the transaction drops/zeroizes the authoritative copy immediately after terminal verification/unknown handling;
- restart still loses the value by design.

No durable component can recreate this expected value.

## Provider dispatch receipt

The SetValue worker receipt contains only exact non-secret binding evidence:

```text
dispatch_attempt_ref
action_id
preparation_journal_sequence
preparation_receipt_ref
snapshot_cut_ref
provider_incarnation_ref
target_incarnation_ref
element_ref
required_pattern = Value
dispatch_operation = SetValue
payload_ref
mode
final_context
transport_result
dispatch_result
```

It contains no plaintext, observed value, HMAC key, or HMAC digest.

`DispatchResult::DispatchedFull` remains dispatch evidence only.

## Mandatory payload-equality postcondition contract

The server creates one mandatory opaque contract ref containing no plaintext:

```text
lvpc:payload-equality:v1:{"mode":"replace_value","payload_ref":"<uuid>"}
```

Clear uses `mode = clear_value`.

The shared postcondition registry owns strict parsing/versioning for this contract family. The contract contains only mode + opaque payload ref.

Normal semantic snapshot evaluation cannot prove it because normal snapshots deliberately lack raw value authority. Generic evaluation therefore remains `Unknown` unless a SetValue-specific provider verifier supplies the exact fresh equality receipt.

The first SetValue route does not accept arbitrary caller-selected postcondition contracts. The mandatory equality contract is server-owned and sufficient for this slice.

## Post-dispatch verification

Provider dispatch acknowledgement is not the postcondition.

After a possibly dispatched SetValue action:

1. durable dispatch linearization is recorded using the existing one-shot permit;
2. existing consequential observation authority is obtained;
3. a fresh exact provider read is performed on the same provider/target lineage and retained/reacquired exact element;
4. worker rechecks `IsPassword == false` and Value capability/readability conditions;
5. worker compares the freshly observed current value against a bounded temporary copy of the still-live authorized payload;
6. raw observed value is never inserted into the general semantic snapshot, evidence store, journal, HTTP response, or logs.

Typed receipt:

```text
WindowsUiaSetValueVerificationReceipt {
  action_id,
  payload_ref,
  mode,
  provider_incarnation_ref,
  target_incarnation_ref,
  element_ref,
  observation_cut_ref,
  equality = MATCH | MISMATCH | UNKNOWN
}
```

The comparison is exact; no trim, case-fold, or Unicode normalization.

Only `MATCH` bound to the exact action/payload/lineage/element and fresh observation cut may produce `VerifiedPass` for the mandatory payload-equality contract.

`MISMATCH` becomes a verified unexpected outcome. `UNKNOWN` remains unresolved/reconciliation-only. Neither can become committed expected-world success.

The observed UIA value and any LocalView-owned temporary comparison buffers have bounded lifetime and are never serialized/logged.

## Recovery behavior

After restart, the durable SetValue intent/operation/payload commitment may still exist, but the process-local commitment key and plaintext are gone.

A payload-equality contract with no live payload resolver must produce typed `Unknown` evidence, not a verifier infrastructure failure and never a pass.

Recovery must not:

- recreate plaintext;
- ask an unrelated caller for a replacement value and treat it as the old action;
- infer success from `DispatchedFull`;
- infer success from element presence;
- recreate confirmation;
- recreate payload capability;
- redispatch.

### Crash before confirmation

Durable descriptors may remain, but confirmation and plaintext capability are gone. No dispatch is possible.

### Crash after confirmation but before provider dispatch

Durable state may be Authorized or PREPARED, but payload authority is gone. Recovery is observe/reconcile only.

### Crash after possible dispatch but before equality verification

Current world may be observed, but the expected value is gone. Payload-equality evaluates `Unknown`; no blind retry occurs.

### Crash after durable VerifiedExpected receipt but before commit

Existing VerifiedUncommitted commit-only recovery remains valid because the durable postcondition receipt already proves the expected state. No plaintext or new provider dispatch is required.

### Already committed

Existing terminal semantics remain unchanged.

This asymmetry intentionally favors privacy and no-replay authority over automatic recovery of lost plaintext.

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

Raw HRESULT/provider diagnostics may be preserved separately. Human text must not drive recovery behavior. Plaintext, observed value, HMAC key, or HMAC digest must never be interpolated into error messages.

## Concurrency and cleanup

- pending capacity remains bounded by the existing Windows consequential pending-plan limit;
- one action owns one payload ref;
- one payload ref belongs to one action;
- planning remains serialized by the existing plan gate;
- exact confirmation atomically removes one pending plan before execution;
- no second confirmation can resolve the same payload;
- no durable sidecar recreates payload capability;
- provider uncertainty never grants retry;
- detach/session removal/control reconfiguration drops and zeroizes unconsumed payload owners;
- MTA SetValue commands are move-only and zeroize LocalView-owned command buffers on drop;
- existing payload-free worker commands remain unchanged.

## Required TDD gates

Implementation proceeds RED → GREEN in causal slices. Permanent tests must prove at least:

1. `CanonicalActionOperation::SetValue` exists and remains payload-free;
2. compatibility carrier contains no caller plaintext and never enters public queue;
3. durable payload binding contains no plaintext and is create-new/immutable;
4. wrong action, intent sequence, payload ref, mode, length, or HMAC fails closed;
5. commitment key is process-local and a new process key cannot validate an old binding;
6. restart cannot turn durable payload binding into dispatch authority;
7. plan rejects forged pattern/verb/risk/commitment fields;
8. plan response never echoes plaintext or digest;
9. Clear rejects any caller `value` field;
10. payload >16 KiB and embedded NUL fail before provider work;
11. password true or unknown fails before intent/confirmation publication;
12. read-only true or unknown fails before intent/confirmation publication;
13. exact confirmation consumes payload authority once; wrong confirmation does not;
14. substituted/missing live payload blocks before PREPARED/provider side effect;
15. `SetValue + Value` is the only operation/pattern mapping that can mint SetValue dispatch;
16. generic runtime receipt now binds exact `dispatch_operation` for all existing actions;
17. existing generic `WindowsUiaPatternDispatchRequest` never gains plaintext;
18. dedicated MTA SetValue request is non-Clone/non-serializable/redacted-debug and move-only;
19. real Win32 UIA SetValue changes one editable non-password fixture exactly once;
20. MTA worker rechecks password/read-only immediately before side effect;
21. provider dispatch acknowledgement alone cannot commit;
22. fresh equality MATCH can produce VerifiedExpected + durable commit;
23. MISMATCH and UNKNOWN cannot commit expected success;
24. current/expected plaintext is absent from durable files, general snapshots, receipts, HTTP responses, and test logs;
25. crash/reopen after PREPARED or possible dispatch cannot redispatch without live payload;
26. recovery without live payload returns `Unknown` evidence instead of verifier failure;
27. VerifiedUncommitted commit-only recovery still works;
28. cleanup zeroizes pending owner buffers and worker command buffers owned by LocalView;
29. existing Invoke/Select/Toggle/Expand/Collapse regressions remain green.

## Windows workflow gates

Add explicit named Windows gates for:

- SetValue server-owned HTTP contract;
- payload commitment/privacy contract;
- real ValuePattern capability + password/read-only preflight;
- real SetValue MTA dispatch worker smoke;
- real HTTP SetValue plan → confirm → one SetValue → fresh equality verification → durable commit;
- restart/no-payload/no-redispatch recovery contract.

Normal Ubuntu/macOS/Windows workspace Check + Clippy + tests remain mandatory because shared canonical/journal/postcondition changes are platform-neutral correctness code.

## Expected production touch points

Limited to:

- `crates/live-bridge`: canonical SetValue operation, typed mode/ref, immutable payload binding storage/readback;
- `crates/postcondition-contracts`: strict opaque payload-equality v1 schema/registry support;
- `crates/windows-uia-provider`: Value capability state, dedicated SetValue worker command, one-shot dispatch, fresh equality read;
- `crates/windows-observe-runtime`: SetValue-specific verified execution/verification seam plus generic dispatch-operation receipt hardening;
- `crates/control`: server-owned SetValue plan, process-local payload/key lifecycle, confirmation integration;
- focused tests and Windows workflow gates.

Out of scope:

- Perception Budget;
- D2 surface ownership;
- Runtime Resource Governor semantics;
- Chromium authority;
- generic keyboard/pointer input;
- macOS AX;
- Linux AT-SPI.

The only deliberate shared hardening is adding exact `dispatch_operation` to the generic Windows runtime provider receipt/matcher. This closes an existing pattern-only receipt gap without changing behavior of existing semantic actions.

## Definition of done

This slice is complete only when one exact PR head proves all of the following:

- no raw SetValue payload is durable or product-visible through logs/responses/receipts/general snapshots;
- exact payload substitution is fenced before PREPARED/dispatch;
- password/read-only/unknown states fail closed;
- real ValuePattern SetValue succeeds exactly once on an editable non-secure fixture;
- fresh independent equality MATCH, not provider ack, is what permits VerifiedExpected;
- MISMATCH/UNKNOWN remain non-committed expected outcomes;
- restart cannot recreate payload, confirmation, or dispatch authority;
- unknown post-crash payload outcome remains reconciliation-only with no blind retry;
- VerifiedUncommitted commit-only recovery remains valid;
- all existing Windows semantic actions remain green;
- Rust workspace Check + Clippy + tests pass on Ubuntu/macOS/Windows;
- Tauri/frontend and WebKitGTK/WKWebView/WebView2 rendered-pixel smokes pass;
- Windows UIA workflow passes the new real SetValue gates;
- whole-PR scope audit shows no unrelated authority widening;
- final PR head is exact-SHA GREEN before merge;
- post-merge `main` is verified again with CI + Windows UIA on the exact merge SHA.
