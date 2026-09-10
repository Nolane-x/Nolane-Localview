# V4.3 Validation Lab Task 5 Authority Correction

Date: 2026-09-10

Status: Normative implementation-plan correction

Scope: Task 5 of `2026-09-10-v43-validation-lab-authority-core.md`

## Reason

RED-3B established a capability-boundary requirement that the original Task 5 interface did not encode strongly enough: `ValidatedPreregistrationReceipt` is a live authority capability minted only by `validate_persisted_receipt`. It must not implement `Deserialize`, because arbitrary serialized input must not be able to reconstruct an already-validated prospective-execution capability.

A second lifecycle review identified an ordering requirement implicit in prediction-before-test: the persisted preregistration receipt must have a nonzero logical sequence strictly before the run's declared `start_sequence`. A receipt that is valid for the bytes but was persisted at or after execution start cannot authorize a prospective result.

The original Task 5 sketch placed `ValidatedPreregistrationReceipt` directly inside a `Serialize + Deserialize` `ExecutionMode` and did not make the persistence-before-start ordering check explicit. That sketch is superseded by this correction.

## Correct authority split

### Live admission capability

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabRunAdmission {
    Prospective {
        preregistration: LabPreregistration,
        receipt: ValidatedPreregistrationReceipt,
    },
    Exploratory {
        revision_context: LabRevisionContext,
        seed_identities: Vec<LabSeedIdentity>,
        assumptions: BTreeSet<String>,
        downgrade_reason: DowngradeReason,
    },
}
```

`LabRunAdmission` is an in-process admission object. It is not a durable artifact and does not implement `Deserialize`.

Prospective admission requires possession of a real `ValidatedPreregistrationReceipt` together with the exact preregistration it authorizes. A `PreparedPreregistration`, `PersistedPreregistrationReceipt`, receipt projection, or arbitrary JSON value is insufficient.

Exploratory admission deliberately carries the minimum run context directly instead of requiring a valid `LabPreregistration`. This keeps the approved downgrade semantics representable when preregistration creation or persistence failed before prospective authority existed.

### Durable receipt projection

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreregistrationReceiptProjection {
    pub digest: CanonicalDigest,
    pub logical_sequence: u64,
    pub persistence_ref: String,
}
```

The projection is data, not a capability. It is created from `&ValidatedPreregistrationReceipt` after validation and may be serialized into canonical result artifacts.

No public API may accept `PreregistrationReceiptProjection` as sufficient input to mint `LabRunAdmission::Prospective` or otherwise recreate `ValidatedPreregistrationReceipt`.

### Durable result mode

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum ExecutionMode {
    Prospective {
        receipt: PreregistrationReceiptProjection,
    },
    Exploratory {
        downgrade_reason: DowngradeReason,
    },
}
```

`ExecutionMode` is a result/artifact projection of how a run was admitted. Deserializing it reconstructs evidence data only; it never reconstructs live prospective authority.

## Prospective admission validation

A prospective `LabRunBuilder` must, before admitting the run:

1. re-canonicalize the supplied `LabPreregistration`;
2. require the live validated receipt digest to equal that exact preregistration digest;
3. require `receipt.logical_sequence < revision_context.start_sequence` so persistence is provably before execution start;
4. require `ActualExecutionAuthority` to match the preregistered seed-catalog digest, comparison profile revision, random source profile, and model bound exactly;
5. return a typed hard authority error on any digest, ordering, or actual-authority drift;
6. only after all checks convert the validated receipt into its durable projection stored in the builder/result payload.

There is no automatic downgrade from a broken prospective authority chain to exploratory mode.

Exploratory admission remains explicit and requires a typed `DowngradeReason`. Stronger `ResultEvidence` supplied to an exploratory run may not escalate the final result above `ExploratoryObservation`.

## Result and finalization authority

`LabRunBuilder` is append-only before finalization. It stores canonical observation digests, not mutable observation objects. `finalize()` canonicalizes the result payload first, computes `result_artifact_digest`, then creates `CompletedLabRunIdentity { revision_context, result_artifact_digest }`. The identity is not included in the payload it hashes.

The builder flips to finalized only after validation and digest computation succeed. A second finalization or any later append is `AlreadyFinalized`.

## Permanent verification requirements

Task 5 RED/GREEN tests must prove:

- prepared or merely persisted preregistration data cannot construct live prospective admission;
- validated receipt is not deserializable;
- receipt projection is serializable/deserializable but cannot mint live authority;
- receipt persisted at or after `start_sequence` cannot authorize prospective execution;
- prospective admission rejects preregistration digest drift and every `ActualExecutionAuthority` field drift;
- explicit exploratory admission works without requiring a valid persisted preregistration and always finalizes as `ExploratoryObservation`;
- result payload serialization contains the projection, not a deserializable live capability;
- completed identity binds the already-computed result payload digest without circular hashing;
- second finalization and post-finalization append are rejected.

This correction narrows the implementation plan to the already-approved two-phase authority model. It does not expand PR #104 into campaign execution or L2-L9 orchestration.
