# Windows UIA SetValue Payload Authority Design

**Status:** architecture approved in chat; self-reviewed design for written-spec review

**Base:** `main@ebb024cfaa6bdde0c647c2f875671d756c54dbfa`

**Scope:** first payload-bearing consequential Windows semantic action: UI Automation `ValuePattern.SetValue`, with exact payload authority, plaintext-free durable state, process-epoch keyed commitments, fresh provider verification, and no blind retry.

## 1. Context

LocalView already has a verified Windows consequential chain for payload-free semantic operations:

```text
Invoke -> SelectionItem -> Toggle -> Expand/Collapse
```

The provider capability model already contains `WindowsUiaPattern::Value`, but the trusted execution path does not yet dispatch `IUIAutomationValuePattern::SetValue` and does not yet publish ValuePattern state suitable for exact post-dispatch verification.

The current architecture intentionally separates canonical operation identity, durable consequential intent, process-local confirmation authority, exact provider/target incarnation, exact fresh observation cut, provider dispatch evidence, and independent post-dispatch world verification.

The existing durable canonical-operation sidecar is deliberately payload-free. SetValue must preserve that invariant. Raw text must never be added to the operation record, consequential journal, semantic snapshot, evidence store, receipt, log, or HTTP response.

The product spec requires semantic value mutation before keyboard fallback, distinguishes `set_value` from `insert_text`, and requires secure/password handling to be conservative.

## 2. Goal

Add one server-owned Windows route that can replace the value of one exact current non-sensitive UIA ValuePattern element while proving:

1. the exact plaintext authorized at plan time is the plaintext presented to the provider at dispatch time;
2. plaintext remains process-local only;
3. durable state binds the admitted action to an opaque payload commitment before authorization advances;
4. durable commitment material is not usable for offline guessing of low-entropy plaintext without the process-epoch secret key;
5. daemon/provider restart cannot reconstruct plaintext, confirmation authority, commitment-verification authority, or dispatch authority;
6. ValuePattern capability/read-only/password state is revalidated at the final provider boundary;
7. provider dispatch success is not world success;
8. a fresh post-dispatch provider cut independently proves the exact current value against the same process-epoch commitment;
9. incomplete, ambiguous, stale, sensitive, read-only, unsupported, or unknown state fails closed;
10. existing Invoke/Select/Toggle/Expand/Collapse behavior remains unchanged.

## 3. Non-goals

This slice does not add keyboard text injection, `insert_text`, append, range replacement, rich-text semantics, clipboard mutation, pointer fallback, password/secure-field mutation, cross-platform AX/AT-SPI value execution, application-specific risk inference, automatic retry after an unknown outcome, plaintext persistence for recovery, post-restart exact-value verification when the original process commitment key has been lost, or a generic arbitrary payload framework for every future action class.

The only text mutation mode in this slice is exact **replace**. Empty string is valid and means clear the value.

## 4. Chosen Architecture

Use a **volatile plaintext capability + durable opaque keyed commitment**.

```text
server-owned canonical operation = SetValue
                |
                v
exact admitted intent + immutable payload commitment sidecar
                |
                |   commitment = HMAC-SHA256(process-epoch key, exact UTF-8 bytes)
                |   process-epoch key is NOT persisted
                v
process-local confirmation + plaintext capability
                |
                v
final UIA worker recomputation + one-shot SetValue
                |
                v
fresh provider snapshot publishes only keyed commitment metadata
                |
                v
server-owned exact-node postcondition -> VerifiedPass/Fail/Unknown
```

The operation identity says what side-effect class was authorized. The durable commitment binds which opaque payload commitment belongs to the exact admitted intent. The process-local plaintext capability carries the value needed to execute. Fresh postcondition evidence proves current world state without persisting plaintext.

## 5. Canonical Operation and Shared Commitment Shape

Add a distinct canonical operation:

```rust
CanonicalActionOperation::SetValue
```

`CanonicalActionOperation::InputText` remains unchanged for legacy `BridgeActionKind::TypeText` and future keyboard/insert-text work. The SetValue product route binds `SetValue` explicitly; it never derives it from the legacy TypeText carrier.

Add the provider-neutral opaque commitment types to `crates/protocol/src/correctness.rs` and re-export them from `crates/protocol/src/lib.rs`, because both live-bridge durability and the Windows provider already depend on this lower layer:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalTextPayloadCommitment {
    pub scheme: CanonicalTextPayloadCommitmentScheme,
    pub epoch_ref: Uuid,
    pub tag_hex: String,
    pub utf8_bytes: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CanonicalTextPayloadCommitmentScheme {
    WindowsUiaProcessHmacSha256V1,
}
```

`localview-protocol` owns only the serializable shape and structural validation. It does not own the secret key and does not compute provider-specific HMACs. The Windows provider owns the algorithm implementation and process-epoch key.

## 6. Commitment Cryptography and Lifetime

The Windows UIA worker attempts to create at startup:

```text
commitment_epoch_ref = random UUID
commitment_key       = random 256-bit secret
```

The secret key lives only in provider worker process memory and is never serialized, returned by an API, logged, persisted in journal/evidence state, or copied into a dispatch/postcondition receipt.

Use HMAC-SHA256 with domain separation:

```text
"localview/windows-uia/value/v1\0"
+ u32 big-endian UTF-8 byte length
+ exact UTF-8 bytes
```

The resulting commitment contains scheme, epoch ref, lowercase-hex HMAC tag, and UTF-8 byte length.

This avoids public unsalted SHA-256, which would allow offline guessing of low-entropy values. A durable-only attacker who lacks the process-epoch key cannot test guesses against the stored tag.

Add focused workspace dependencies:

```text
hmac = "0.12"
sha2 = "0.10"
getrandom = "0.3"
```

Only `localview-windows-uia-provider` uses the cryptographic implementation dependencies. `localview-protocol` and `localview-live-bridge` consume only the opaque commitment type.

If secure randomness/key initialization fails, the worker must keep unrelated observe-only UIA functionality available. SetValue commitment generation becomes unavailable and fails closed with a stable typed error; ValuePattern commitment evidence is unavailable/incomplete, but unrelated semantic observation must not be disabled merely because SetValue cryptographic setup failed.

A provider/daemon restart creates a new epoch and key. Old commitments remain durable history but are not re-computable under the new process. That loss is intentional and must produce `Unknown`/`ReconciliationRequired`, never a guessed pass and never redispatch authority.

## 7. Text Payload Domain

The plan request is exactly:

```json
{
  "element_ref": { "...": "..." },
  "value": "exact replacement text"
}
```

Rules:

- mutation mode is fixed to server-owned `replace`;
- maximum decoded UTF-8 payload size is **16 KiB**;
- empty string is valid;
- embedded `U+0000` is rejected;
- no trimming, Unicode normalization, line-ending conversion, case folding, or other rewriting occurs before commitment or dispatch;
- commitment is over the exact UTF-8 bytes after successful JSON decoding;
- if an application normalizes/transforms the value, postcondition verification may fail. This slice prefers conservative false negatives over silently changing text semantics.

## 8. Durable Payload Commitment Sidecar

Add `crates/live-bridge/src/consequential_journal/payload_commitment.rs` with an immutable record beside the existing payload-free operation binding:

```rust
pub struct DurableCanonicalActionPayloadCommitment {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub operation: CanonicalActionOperation,
    pub payload_kind: CanonicalPayloadKind,
    pub mutation_mode: CanonicalTextMutationMode,
    pub commitment: CanonicalTextPayloadCommitment,
}

pub enum CanonicalPayloadKind {
    Utf8Text,
}

pub enum CanonicalTextMutationMode {
    Replace,
}
```

Rules:

- action must still be exactly `Admitted`;
- exact `IntentAdmitted` envelope and action ID must match;
- operation must be exactly `SetValue` for this slice;
- sidecar path is bound to action ID and intent journal sequence;
- write uses create-new semantics, flush, and `sync_all()`;
- a second commitment cannot overwrite the first;
- corrupt/missing/mismatched sidecar fails closed;
- plaintext is never present.

The commitment tag does not itself create confirmation, authorization, PREPARED, execution, or recovery-dispatch authority.

## 9. Process-local Plaintext Capability

The control layer keeps plaintext only inside the pending process-local SetValue plan:

```rust
struct PendingWindowsSetValuePayload {
    action_id: Uuid,
    commitment: DurableCanonicalActionPayloadCommitment,
    plaintext: String,
}
```

Requirements:

- no `Serialize` implementation;
- no plaintext-bearing `Debug` output;
- response/error/log messages never include plaintext;
- exact confirmation consumption atomically removes the pending plaintext capability before verified execution;
- wrong confirmation neither exposes nor consumes the exact payload;
- detach, session removal, runtime replacement, or daemon shutdown drops pending payloads;
- if provider worker replacement changes commitment epoch while a plan is pending, confirmation fails closed on epoch mismatch; it never re-commits the old plaintext under the new provider and continues silently;
- restart begins with no plaintext capabilities and no old process commitment key;
- this slice claims process-local lifetime/no-persistence, not compiler-guaranteed memory zeroization.

## 10. Server-owned Planning Route

Add:

```text
POST /v1/sessions/{id}/windows-observe/consequential/set-value/plan
```

Accepted request fields are exactly `element_ref` and `value`. Unknown fields are rejected. The caller cannot provide or override canonical operation, ValuePattern requirement, mutation mode, commitment scheme/epoch/tag, risk/idempotency class, decision/acting principal, authorization revision, provider/target incarnation, or expected exact-value postcondition contract.

Planning order:

```text
authenticate bearer
-> require live session/runtime/journal
-> validate payload bounds/NUL
-> existing plan serialization gate
-> fresh provider observation + exact element rebind
-> require ValuePattern supported
-> require password/read-only state known
-> reject password/sensitive/read-only targets
-> ask the exact current Windows provider worker to commit the plaintext
-> bind direct canonical SetValue action
-> record durable IntentAdmitted
-> fsync payload-free SetValue operation binding
-> fsync immutable payload commitment sidecar
-> derive server-owned exact-value postcondition contract from exact target + commitment
-> create process-local confirmation + plaintext capability
-> return only non-secret plan metadata
```

No durable authorization/PREPARED state may advance before operation binding and payload commitment are durable.

## 11. Conservative Risk and Retry Policy

Generic ValuePattern support does not prove application-level consequence semantics. Initial SetValue keeps the same conservative floor as existing generic semantic writes:

```text
risk_class        = S4 destructive_or_irreversible
idempotency_class = irreversible
```

Explicit process-local confirmation remains required.

No automatic retry is permitted after confirmation consumption or any possibly-dispatched outcome. A new attempt requires a new action ID, fresh provider evidence, fresh commitment, fresh plaintext capability, and fresh confirmation.

## 12. Provider Value Observation and Privacy

The provider already records ValuePattern capability. Extend semantic observation with:

```text
windows_uia.value.is_read_only = true|false
windows_uia.value.is_password  = true|false
```

For a non-password ValuePattern node whose current value is readable and whose commitment key is available, publish only:

```text
windows_uia.value.commitment_scheme = windows_uia_process_hmac_sha256_v1
windows_uia.value.commitment_epoch  = <current provider epoch UUID>
windows_uia.value.commitment_tag    = <HMAC tag>
windows_uia.value.utf8_bytes        = <decimal byte length>
```

Never publish `CurrentValue` plaintext.

Password/secure behavior:

- read `IsPassword` before attempting to read CurrentValue;
- if password/sensitive, do not read CurrentValue for commitment generation;
- publish sensitivity state only;
- SetValue planning and final dispatch are unsupported in this slice.

Read-only behavior:

- `ValuePattern.CurrentIsReadOnly == true` blocks planning and dispatch;
- unknown/unreadable read-only state blocks planning/dispatch.

If ValuePattern is supported on a non-password node but required current-value or commitment observation fails, publish explicit provider debt and mark the Value correctness observation incomplete. It must not authorize SetValue or satisfy its postcondition. Unrelated non-Value observation remains available.

## 13. Provider Dispatch Payload

Add:

```rust
WindowsUiaPatternDispatchOperation::SetValue
```

with `required_pattern() == WindowsUiaPattern::Value`.

Extend the dispatch request with a typed non-serializable process-local payload:

```rust
pub enum WindowsUiaPatternDispatchPayload {
    SetValue {
        plaintext: String,
        commitment: CanonicalTextPayloadCommitment,
    },
}
```

Rules at the final MTA worker boundary:

- SetValue requires exactly one SetValue payload;
- payload-free operations reject any payload;
- operation/pattern mismatch is rejected;
- commitment epoch must equal the worker's current commitment epoch;
- worker recomputes HMAC + UTF-8 length from plaintext and requires exact commitment equality;
- exact current live element must still support ValuePattern;
- password/sensitive state is checked again before CurrentValue/SetValue use;
- read-only state is checked again;
- existing provider/target/snapshot/element/context fences remain mandatory;
- only then call `IUIAutomationValuePattern::SetValue` exactly once.

Dispatch receipt echoes only opaque commitment metadata, never plaintext or secret key. Provider return success remains dispatch evidence only.

## 14. Server-owned Exact-node Postcondition

SetValue cannot accept a caller-selected weak contract such as “an Edit node exists”. The server derives the required postcondition from the exact refreshed provider identity and exact opaque commitment.

Add a new immutable provider-neutral registry schema:

```text
native-semantic:v3
```

V1 and V2 remain byte-for-byte and semantically frozen.

V3 initial type:

```rust
pub struct NativeSemanticExactNodeAttributesPostconditionV3 {
    pub provider_family: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub opaque_provider_element_id: String,
    pub lifetime_profile_revision: String,
    pub required_attributes: BTreeMap<String, String>,
}
```

SetValue's server-owned required attributes are:

```text
windows_uia.value.commitment_scheme = expected scheme
windows_uia.value.commitment_epoch  = expected epoch
windows_uia.value.commitment_tag    = expected tag
windows_uia.value.utf8_bytes        = expected byte length
windows_uia.value.is_password       = false
windows_uia.value.is_read_only      = false
```

Evaluation:

- snapshot must be complete for the required Value correctness evidence;
- provider/target incarnation must equal contract lineage; lineage mismatch -> `Unknown`;
- provider family + opaque element identity + lifetime profile must identify exactly one current node;
- ambiguous identity -> `Unknown`;
- zero matching node in an otherwise complete same-lineage snapshot -> `VerifiedFail`;
- exact unique node + all required readable attributes equal -> `VerifiedPass`;
- exact unique node + complete readable attributes differ -> `VerifiedFail`;
- missing correctness attributes or incomplete observation -> `Unknown`.

The post-dispatch acquisition cut is intentionally new, so V3 does not require the old `acquisition_cut_ref`. Observation-cut authority remains carried by the journal-minted fresh observation receipt.

Because the commitment key is not persisted, a post-restart provider epoch cannot reproduce the old commitment tag. Recovery of a pre-crash unverified SetValue remains `Unknown/ReconciliationRequired`; V3 must not turn an epoch mismatch into fail/pass certainty.

## 15. Confirmation and Verified Execution Order

Confirmation continues through:

```text
POST /v1/sessions/{id}/windows-observe/consequential/{action_id}/confirm
```

Required order:

```text
validate bearer/session/action/confirmation identity
-> resolve exact current runtime/journal
-> atomically remove exact pending plaintext capability
-> read exact durable operation + payload commitment
-> require current provider commitment epoch == admitted commitment epoch
-> recompute plaintext commitment through the exact provider worker
-> require exact durable commitment equality
-> create one-shot independent authorization revalidator
-> existing semantic preflight
-> exact live-element lease
-> canonical SetValue operation revalidation
-> volatile dispatch-context seal
-> durable PREPARED
-> second dispatch-time context arm
-> one-shot ValuePattern.SetValue
-> provider dispatch receipt without plaintext
-> journal-minted fresh post-dispatch semantic snapshot
-> V3 exact-node commitment verification
-> durable postcondition reconciliation receipt
-> commit only on VerifiedExpected
```

Any failure after confirmation consumption does not restore plaintext or confirmation authority.

## 16. Crash / Restart Semantics

**Crash before confirmation:** durable intent/operation/opaque commitment may exist; plaintext, confirmation, and process HMAC key disappear; no dispatch can be reconstructed.

**Crash after PREPARED or possibly-dispatched:** existing consequential journal state remains authoritative about what is known/unknown; no blind redispatch is allowed; old payload commitment cannot be recomputed under a new process epoch; exact value outcome remains `Unknown/ReconciliationRequired` unless an independent application-specific durable postcondition is introduced in a future slice.

**Crash after VerifiedExpected receipt:** existing durable verified-receipt/commit-only recovery semantics remain valid; plaintext is not needed.

This is intentionally less convenient than persisting a guessable plaintext digest and is the required security trade-off for the initial generic SetValue path.

## 17. Stable Fail-closed Errors

Add stable errors at the appropriate layer:

```text
windows_set_value_payload_too_large
windows_set_value_payload_contains_nul
windows_set_value_sensitive_field_blocked
windows_set_value_read_only
windows_set_value_value_state_unknown
windows_set_value_commitment_unavailable
windows_set_value_commitment_epoch_mismatch
windows_set_value_payload_commitment_missing
windows_set_value_payload_commitment_mismatch
windows_set_value_payload_capability_missing
windows_set_value_pattern_unavailable
```

Raw HRESULT/provider diagnostics may be retained separately. No error string may contain plaintext, HMAC secret key material, or a serialized pending payload object.

## 18. TDD / Verification Strategy

All production changes use explicit RED -> GREEN lineage.

### Layer 1 — protocol commitment shape + durable sidecar

RED first, then prove the shared commitment type is structurally validated; an exact admitted SetValue action binds one immutable sidecar; reopen preserves only opaque commitment metadata; wrong intent sequence/action/operation/corrupt sidecar fails closed; second binding cannot overwrite the first; and payload-free operation binding remains unchanged.

### Layer 2 — native-semantic V3

RED first, then prove V3 symbols are absent before implementation; V1/V2 golden references remain unchanged; exact unique matching node passes; exact unique mismatch fails; zero exact node on complete same-lineage snapshot fails; ambiguous identity is Unknown; lineage mismatch is Unknown; incomplete/missing correctness attributes are Unknown; and non-canonical/unknown fields fail closed.

### Layer 3 — provider keyed commitment + Value observation

Prove process epoch/key is created once per worker lifetime; same exact value in same epoch gives same tag; different value gives a different tag; new worker epoch cannot reproduce old commitment identity; ValuePattern editable non-password node publishes opaque commitment attributes only; plaintext is absent from semantic snapshot serialization; read-only/password states are explicit; password CurrentValue is not read for commitment generation; entropy/key initialization failure disables SetValue without disabling unrelated observation; unreadable required Value state makes Value correctness observation incomplete; and SelectionItem/Toggle/ExpandCollapse observations remain unchanged.

### Layer 4 — provider SetValue dispatch

RED first, then prove SetValue dispatch operation/payload types are missing before implementation; missing payload is rejected; payload on existing payload-free operations is rejected; pattern/operation mismatch is rejected; epoch/tag mismatch is rejected before side effect; final password/read-only transition rejects dispatch; exact COM SetValue executes once; and receipt contains commitment metadata but no plaintext.

### Layer 5 — HTTP planning authority

RED route first, then prove the route exists only after implementation; request accepts exactly element_ref + value; caller cannot forge operation/pattern/mode/commitment/risk/idempotency/postcondition; NUL/over-limit/sensitive/read-only/unknown target fails before authorization; successful plan returns action ID + confirmation ref + opaque commitment metadata only; and journal/sidecars/responses/loggable debug values contain no plaintext.

### Layer 6 — real Win32 SetValue

Use a retained Win32 Edit fixture with the real UIA worker:

```text
fresh snapshot
-> plan SetValue
-> explicit confirmation
-> exact ValuePattern.SetValue
-> fresh provider snapshot
-> V3 exact commitment VerifiedPass
-> durable VerifiedExpected -> COMMITTED
```

Also prove wrong confirmation leaves exact confirmation usable; exact confirmation is one-shot; read-only Edit fails closed; password Edit fails closed without CurrentValue export; unexpected provider/application transformation prevents VerifiedExpected; same action cannot dispatch twice; and provider-worker replacement between plan and confirm fails closed on commitment epoch mismatch.

### Layer 7 — permanent workflow gates

Add named Windows UIA gates for payload commitment authority, native-semantic V3 exact-node attributes, ValuePattern commitment/privacy observation, SetValue dispatch worker, SetValue HTTP authority, and real HTTP SetValue -> verified durable commit.

Final merge requires exact-head Rust Check + Clippy + full workspace Tests on Ubuntu/macOS/Windows, Tauri + frontend, WebKitGTK/WKWebView/WebView2 rendered-pixel smoke, Windows UIA Observe including SetValue, no unresolved review blocker/head drift, and post-merge CI + Windows UIA green on the exact merge SHA.

## 19. File / Responsibility Boundaries

Expected focused changes:

- `Cargo.toml` — workspace `hmac`, `sha2`, `getrandom` versions.
- `crates/protocol/src/correctness.rs` — opaque commitment shape and structural invariants.
- `crates/protocol/src/lib.rs` — re-export commitment types.
- `crates/live-bridge/src/action_envelope.rs` — `CanonicalActionOperation::SetValue`.
- `crates/live-bridge/src/consequential_journal/payload_commitment.rs` — immutable exact-intent payload commitment sidecar.
- `crates/live-bridge/src/consequential_journal.rs` — focused module/export.
- `crates/postcondition-contracts/src/lib.rs` — frozen V1/V2 plus exact-node-attribute V3.
- `crates/windows-uia-provider/Cargo.toml` — provider cryptographic dependencies.
- `crates/windows-uia-provider/src/value_commitment.rs` — process-epoch key generation/HMAC implementation and typed unavailable state; no persistence.
- `crates/windows-uia-provider/src/action_capability.rs` — no new Value capability enum; existing Value pattern remains authoritative.
- `crates/windows-uia-provider/src/pattern_dispatch.rs` — typed SetValue dispatch payload/operation/receipt metadata.
- `crates/windows-uia-provider/src/lib.rs` — Value state observation, final privacy/read-only/password fence, exact SetValue COM method.
- `crates/windows-observe-runtime/src/...` — route-independent commitment-aware preflight/executor/postcondition plumbing while preserving existing verified-action coordinator trust boundaries; implementation plan must name exact files after reading the coordinator/executor seams.
- `crates/control/src/windows_consequential.rs` — server-owned SetValue planning and process-local plaintext capability.
- focused tests plus `.github/workflows/windows-uia-observe.yml`.

Do not refactor unrelated provider/runtime/control subsystems merely to make this slice aesthetically cleaner.

## 20. Safety Invariants

Implementation is unacceptable if any become false:

1. `SessionId` continuity is never execution authority.
2. ValuePattern capability is never action authority.
3. canonical operation identity and payload commitment remain distinct.
4. plaintext is never durable correctness state.
5. process HMAC key is never persisted or returned.
6. durable commitment alone cannot be used to test plaintext guesses without the missing process key.
7. restart cannot recreate confirmation, plaintext, key, or dispatch authority.
8. exact payload commitment is revalidated immediately before provider side effect.
9. exact provider/target/element/context fences remain mandatory.
10. password/secure target mutation is unsupported in this slice.
11. provider dispatch acknowledgement is not world proof.
12. world proof comes from a journal-minted fresh provider observation.
13. new process epoch cannot launder an old unverified SetValue into success.
14. unknown/incomplete/ambiguous evidence fails closed.
15. unknown outcome never enables blind retry.
16. V1/V2 postcondition wire semantics remain frozen.
17. existing payload-free Windows actions keep current behavior.
18. SetValue cryptographic initialization failure cannot disable unrelated observe-only UIA capability.

## 21. Definition of Done

The slice is complete only when one exact final head proves the server-owned semantic SetValue route exists; SetValue is distinct from InputText; exact opaque payload commitment is durable before authorization advances; plaintext and commitment key remain process-local only; durable/evidence outputs resist direct/offline low-entropy plaintext guessing without the process key; ValuePattern observation publishes only opaque keyed commitment metadata plus read-only/password state; password/read-only targets fail closed; worker revalidates epoch + HMAC + plaintext immediately before one-shot SetValue; fresh exact-node postcondition independently proves current world commitment in the same process epoch; crash/restart cannot reconstruct dispatch authority and cannot falsely verify an old unverified SetValue; real Win32 SetValue reaches durable `VerifiedExpected` only after fresh proof; existing Windows semantic action regressions remain green; exact-head CI + Windows UIA are green; and exact merge SHA is post-merge green before this slice is declared closed.
