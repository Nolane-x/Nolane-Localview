# V4.3 Validation Lab Authority Core — Persistence Boundary Clarification

Date: 2026-09-10

Status: Normative clarification to `2026-09-10-v43-validation-lab-authority-core-design.md`

## Purpose

The approved design requires prediction-before-test preregistration while also keeping the authority core free of filesystem/network side effects. A direct `LabPreregistration::receipt()` API would be too weak because an in-memory object could mint a receipt before any durable research record exists.

This clarification makes preregistration explicitly two-phase and supersedes only the receipt-minting wording in Design §§9 and 14.1. All other design requirements remain unchanged.

## Two-phase contract

### Phase A — Prepare canonical preregistration

The authority core validates and canonicalizes `LabPreregistration`, then produces:

```rust
pub struct PreparedPreregistration {
    pub canonical_bytes: Vec<u8>,
    pub digest: CanonicalDigest,
}
```

`PreparedPreregistration` is not sufficient to start a prospective run.

### Phase B — Acknowledge durable persistence

The persistence boundary stores the exact `canonical_bytes` and returns evidence that binds the same digest and a monotonic logical sequence:

```rust
pub struct PersistedPreregistrationReceipt {
    pub digest: CanonicalDigest,
    pub logical_sequence: u64,
    pub persistence_ref: String,
}
```

The authority core exposes a validation constructor/function that accepts `PreparedPreregistration` plus persistence evidence and refuses to mint/accept a prospective execution authority unless:

- the persisted digest equals the prepared digest exactly;
- `logical_sequence > 0`;
- `persistence_ref` is non-empty;
- the receipt is bound to the exact preregistration used to start the run.

The authority core does not claim that bytes were persisted by itself. The external research artifact boundary owns the I/O and must supply the persistence receipt. Future adapters may use `localview-artifacts` or a dedicated lab artifact store, but the authority core does not acquire a shipping-runtime dependency to do so.

## Failure semantics

- Missing persistence evidence before execution => caller may only start `Exploratory { downgrade_reason: MissingPersistedPreregistration }`.
- Digest mismatch between prepared artifact and persistence evidence => hard `PreregistrationPersistenceMismatch`; do not downgrade a tampered prospective run.
- Empty `persistence_ref` or zero logical sequence => hard invalid-persistence-receipt error.
- A valid persisted receipt may not be reused with different preregistration canonical bytes/digest.

## TDD consequence

Permanent tests must prove:

1. prepared-only preregistration cannot start a prospective run;
2. exact persisted receipt can start a prospective run;
3. mismatched digest is a hard error;
4. missing persistence can continue only as exploratory and cannot later regain a prospective result class;
5. changing preregistration content changes the prepared digest and invalidates an older persistence receipt.

This clarification strengthens, rather than relaxes, the Prediction-Before-Test rule.