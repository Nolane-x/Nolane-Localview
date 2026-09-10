# V4.3 Validation Lab Authority Core — Execution Order Correction

Date: 2026-09-10

Status: Normative execution-order correction for `2026-09-10-v43-validation-lab-authority-core.md`.

The implementation plan's type review correctly notes that `LabPreregistration` consumes `LabSeedIdentity`, while the numbered text introduces seed identity in Task 4 after Task 3.

To avoid a temporary second seed-identity type, execute the existing tasks in this dependency order:

```text
Task 1 -> Task 2 -> Task 4 -> Task 3 -> Task 5 -> Task 6 -> Task 7 -> Task 8
```

No task contract changes otherwise.

Consequences:

- Task 4 GREEN establishes the one canonical `LabSeedIdentity` and `LabSeedCatalog` types.
- Task 3 then uses `Vec<LabSeedIdentity>` directly in `LabPreregistration`.
- No string placeholder seed identity is permitted in GREEN production code.
- All permanent RED->GREEN commit boundaries described by the main plan remain required.

This correction resolves the only type-order concern found in plan self-review.