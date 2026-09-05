# Attachment-Bound Consequential Recovery Debt Design

## Goal

Connect the durable recovery inventory introduced by V4.3 boot recovery to the exact provider/target incarnation that becomes attached after restart, without recreating dispatch authority and without fabricating postcondition evidence.

## Constraints

- Durable journal history outranks process-local queue state after restart.
- Recovery never recreates a dispatch permit or accepts an executor.
- Correctness ordering uses `journal_sequence`, never wall-clock time.
- A provider attachment may only act on recovery debt whose admitted envelope matches the exact session, provider incarnation, and target incarnation of the attached semantic snapshot.
- `VerifiedUncommitted` may be committed from its durable `VerifiedExpected` postcondition receipt without a fresh provider observation.
- `DispatchPrepared`, `PossiblyDispatched`, and `OutcomeObservedUnverified` require a verifier before any world-success claim. Until a production verifier exists for their opaque postcondition contract refs, they remain explicitly verifier-required.
- `Admitted`, `AuthorizedNotDispatched`, and `KnownNotDispatched` must not be treated as provider-reconciliation debt.
- `Committed` is historical terminal state.
- `write_actions` / `input_dispatch` remains disabled.

## Architecture

### 1. Journal lineage query

`ConsequentialJournal` will expose a typed attachment-bound recovery query. The query accepts an exact `SessionId`, `ProviderIncarnationRef`, and `TargetIncarnationRef`, then derives candidates only from durable `IntentAdmitted` envelopes whose lineage matches all three values. The returned candidates retain `action_id`, recovery state, latest monotonic journal sequence, and expected postcondition contract refs, sorted by latest journal sequence.

This query is generic journal functionality: it does not know about UIA, HWNDs, or daemon routing.

### 2. Commit-only recovery primitive

The Windows verified-execution module will expose a commit-only recovery function that accepts only `&ConsequentialJournal` and `action_id`. It may:

- return historical `AlreadyCommitted` from a durable verified receipt;
- transition `VerifiedUncommitted -> Committed` from that same durable verified receipt;
- return a typed `NotCommitReady` state for everything else.

It accepts no runtime, verifier, executor, dispatch permit, or bridge, making blind redispatch structurally impossible.

### 3. Daemon attachment planner

The daemon recovery module will expose a pure planner over attachment-bound journal candidates. It classifies each candidate as:

- `CommitOnly` for `VerifiedUncommitted`;
- `HistoricalCommitted` for `Committed`;
- `VerifierRequired` for `DispatchPrepared`, `PossiblyDispatched`, or `OutcomeObservedUnverified`;
- `NoProviderRecovery` for pre-dispatch/known-not-dispatched states.

The Windows observe drain loop already sees concrete attached sessions. On the first observation of an attached session incarnation, daemon code reads the current immutable semantic snapshot, queries exact lineage-bound recovery debt, commits `CommitOnly` entries, and logs `VerifierRequired` entries without mutating them. Repeated loop ticks must not repeatedly process the same unchanged attachment incarnation; a provider/target incarnation change forms a new recovery opportunity.

### 4. No fake verifier

Postcondition contract refs are currently opaque references. This slice deliberately does not infer predicates from strings and does not synthesize `VerifiedPass`. A later verifier-registry slice will resolve supported contract revisions to independent verifier implementations and feed only those typed verifiers into `recover_consequential_uia_action`.

## Error handling

- Missing current semantic snapshot after an attached-session report is fail-closed and leaves durable debt unchanged.
- Lineage mismatch means the action is excluded rather than coerced onto the new target.
- Invalid/missing durable verified receipt makes commit-only recovery fail without appending `Committed`.
- Provider-dependent debt without a registered verifier is surfaced as `VerifierRequired`; no observation receipt is minted merely to make progress.
- Resource/transient runtime failures leave the durable journal unchanged and are retryable only as observation/recovery work, never as dispatch.

## Verification

TDD acceptance coverage will prove:

1. journal attachment query rejects same-session wrong-provider and wrong-target history and preserves journal-sequence ordering;
2. commit-only recovery transitions only `VerifiedUncommitted` with a durable `VerifiedExpected` receipt and accepts no executor/runtime/verifier;
3. daemon planner classifies all consequential recovery states without laundering verifier-required states into success;
4. attachment processing uses the runtime's exact current snapshot lineage and does not process mismatched durable actions;
5. existing verified execution, restart recovery, Windows real-provider smoke, full cross-platform CI, and legacy route fail-closed behavior remain green.
