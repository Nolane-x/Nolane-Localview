# Attachment-Bound Consequential Recovery Debt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bind durable consequential recovery debt to the exact attached provider/target incarnation, safely finish commit-only recovery, and expose provider-dependent debt as verifier-required without fabricating evidence or dispatch authority.

**Architecture:** Extend the generic consequential journal with an exact-lineage recovery query, add a no-runtime/no-executor commit-only Windows recovery primitive, then let daemon attachment processing classify and drain only states that are safe without a verifier. Provider-dependent states remain explicit durable debt until a later verifier registry resolves their opaque postcondition contracts.

**Tech Stack:** Rust 2024, Tokio, LocalView live-bridge journal, Windows observe runtime, daemon runtime, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-05-attachment-bound-recovery-debt-design.md`

## Global Constraints

- No production code before a failing test proves the missing behavior.
- No recovered dispatch permit or executor in any restart/attachment recovery API.
- Match exact `SessionId` + `ProviderIncarnationRef` + `TargetIncarnationRef` before provider-bound recovery.
- Use `journal_sequence` for recovery ordering; never use wall-clock time for correctness.
- Never synthesize postcondition evidence from opaque contract-ref strings.
- `write_actions` / `input_dispatch` stays disabled.
- Exact-head full CI and Windows UIA real-provider gates must pass before merge.

---

### Task 1: Exact attachment-bound journal recovery debt

**Files:**
- Modify: `crates/live-bridge/src/consequential_journal.rs`
- Test: `crates/live-bridge/tests/v43_attachment_recovery_debt.rs`

**Interfaces:**
- Consumes: durable `IntentAdmitted` envelope lineage and `ConsequentialRecoveryState`.
- Produces: `ConsequentialAttachmentRecoveryDebt` and `ConsequentialJournal::recovery_debt_for_attachment(session_id, provider_ref, target_ref)`.

- [ ] **Step 1: Write the failing test**

Create durable actions spanning: exact lineage, same-session wrong provider, same-session wrong target, and a second exact-lineage action with a later journal sequence. Reopen the journal and call `recovery_debt_for_attachment`. Assert that only exact-lineage actions are returned and they are ordered by latest `journal_sequence`.

- [ ] **Step 2: Run the RED test**

Run:

```bash
cargo test -p localview-live-bridge --test v43_attachment_recovery_debt -- --nocapture
```

Expected: compile failure because `ConsequentialAttachmentRecoveryDebt` and/or `recovery_debt_for_attachment` do not exist.

- [ ] **Step 3: Implement minimal journal query**

Add a typed in-memory recovery-debt struct with:

```rust
pub struct ConsequentialAttachmentRecoveryDebt {
    pub action_id: Uuid,
    pub session_id: SessionId,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
}
```

Implement an async query that derives lineage from durable `IntentAdmitted` entries, derives current state from the full journal, filters exact session/provider/target equality, and sorts by `latest_journal_sequence`.

- [ ] **Step 4: Run GREEN + journal regressions**

```bash
cargo test -p localview-live-bridge --test v43_attachment_recovery_debt -- --nocapture
cargo test -p localview-live-bridge --test v43_recovery_inventory -- --nocapture
cargo test -p localview-live-bridge --test v43_consequential_journal -- --nocapture
```

Expected: all PASS.

---

### Task 2: Commit-only recovery without runtime/verifier/executor

**Files:**
- Modify: `crates/windows-observe-runtime/src/verified_execution.rs`
- Modify: `crates/windows-observe-runtime/src/lib.rs` if export wiring is required
- Test: `crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs`

**Interfaces:**
- Consumes: `&ConsequentialJournal`, `action_id`.
- Produces: `recover_consequential_uia_commit_only(journal, action_id)` and `WindowsUiaCommitOnlyRecoveryOutcome`.

- [ ] **Step 1: Write failing behavior tests**

Add tests proving:

```text
VerifiedUncommitted + durable VerifiedExpected receipt -> Committed
Committed + durable VerifiedExpected receipt -> AlreadyCommitted
PossiblyDispatched -> NotCommitReady and journal state unchanged
VerifiedUncommitted without valid durable receipt -> error and no commit
```

The function signature must have no runtime, bridge, executor, permit, or verifier parameter.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior commit_only -- --nocapture
```

Expected: compile failure because the commit-only API/types do not exist.

- [ ] **Step 3: Implement minimal commit-only state machine**

Reuse durable receipt validation semantics already present in `recover_consequential_uia_action`. Only `VerifiedUncommitted` may append `Committed`; `Committed` is a historical read; every other state returns `NotCommitReady` without mutation.

- [ ] **Step 4: Run GREEN + coordinator regressions**

```bash
cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior -- --nocapture
cargo check -p localview-windows-observe-runtime --all-targets
```

Expected: all PASS.

---

### Task 3: Daemon attachment-bound planner and safe drain

**Files:**
- Modify: `apps/daemon/src/consequential_recovery.rs`
- Modify: `apps/daemon/src/main.rs`
- Test: `apps/daemon/src/consequential_recovery.rs` module tests

**Interfaces:**
- Consumes: exact current semantic snapshot lineage plus `ConsequentialJournal::recovery_debt_for_attachment`.
- Produces: typed `AttachmentRecoveryDisposition` and one attachment-processing function that commits only commit-ready debt and reports verifier-required debt.

- [ ] **Step 1: Write failing planner tests**

Assert mappings:

```text
VerifiedUncommitted -> CommitOnly
Committed -> HistoricalCommitted
DispatchPrepared -> VerifierRequired
PossiblyDispatched -> VerifierRequired
OutcomeObservedUnverified -> VerifierRequired
Admitted -> NoProviderRecovery
AuthorizedNotDispatched -> NoProviderRecovery
KnownNotDispatched -> NoProviderRecovery
```

Also create journal debt for a wrong provider/target and prove attachment processing never returns it for the current snapshot lineage.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-daemon consequential_recovery::tests:: -- --nocapture
```

Expected: compile failure for missing disposition/planner/processing API.

- [ ] **Step 3: Implement minimal daemon orchestration**

Extend the existing Windows observe drain task to track last seen `(session_id, provider_incarnation_ref, target_incarnation_ref)` attachment incarnations. On a new exact incarnation, call the recovery processor. Execute commit-only recovery for `CommitOnly`; log `VerifierRequired` with action id/state/expected contract refs; never begin postcondition observation without a verifier.

- [ ] **Step 4: Run daemon GREEN**

```bash
cargo test -p localview-daemon consequential_recovery::tests:: -- --nocapture
cargo check -p localview-daemon --all-targets
cargo clippy -p localview-live-bridge -p localview-windows-observe-runtime -p localview-daemon --all-targets --no-deps -- -D warnings
```

Expected: all PASS.

---

### Task 4: Release verification and merge hygiene

**Files:**
- Modify only CI workflow if a permanent focused gate is justified; otherwise no production changes.

**Interfaces:**
- Consumes: clean PR head.
- Produces: merge evidence on the exact clean SHA.

- [ ] **Step 1: Ensure diff hygiene**

Confirm no temporary scripts/workflows, no `Cargo.lock` churn, and no repo-wide rustfmt noise.

- [ ] **Step 2: Run official exact-head gates**

Require the repository's standard full `CI` workflow and `Windows UIA Observe` workflow on the same clean head SHA.

- [ ] **Step 3: Review PR hygiene**

Confirm no unresolved review threads/comments and changed files match the intended slice.

- [ ] **Step 4: Squash merge only after both official gates are green**

After merge, verify `main` points at the resulting squash commit and keep `write_actions` / `input_dispatch` disabled.
