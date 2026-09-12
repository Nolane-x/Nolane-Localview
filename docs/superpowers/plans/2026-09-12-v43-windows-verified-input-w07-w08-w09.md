# V4.3 Windows Verified Input W07/W08/W09 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one bounded verified Windows keyboard fallback with immediate foreground/input-state fences and dedicated partial-dispatch receipts, then close W07, W08 and W09 at their declared evidence levels.

**Architecture:** Keep verified keyboard fallback separate from semantic UIA pattern execution. Reuse the consequential journal and existing dispatch-context authority, add pure input-state/result classifiers, then place the platform API behind a provider-owned one-shot executor and feed exact receipts into runtime + Validation Lab.

**Tech Stack:** Rust edition 2024, `windows` 0.61, existing LocalView Windows runtime/provider, .NET 8 edge seed, GitHub Actions Windows runner.

**Spec:** `docs/superpowers/specs/2026-09-12-v43-windows-verified-input-w07-w08-w09-design.md`

## Global Constraints

- Base: `main@c357f172df93b869a87583a0fd9be469326ea020`.
- Scope is W07/W08/W09 only; no pointer, clipboard, arbitrary-text, secret-field or privileged automation.
- Semantic UIA actions stay preferred and unchanged.
- Final foreground/focus/modal + keyboard-state checks occur adjacent to platform dispatch.
- Never release user-held modifiers automatically.
- Partial dispatch is a real external side effect with unknown world outcome and no blind retry.
- Full platform acceptance is not world verification.
- W01–W06 evidence must remain green.

---

### Task 1: Baseline PR

**Files:** existing workflows only.

- [ ] Open draft PR from this branch to `main`; state exact W07/W08/W09 scope and link spec/plan.
- [ ] Record exact docs-only head and observe baseline workflows before production changes.

### Task 2: Pure verified-input contracts — RED then GREEN

**Files:**
- Create: `crates/windows-uia-provider/src/verified_input.rs`
- Create: `crates/windows-uia-provider/tests/verified_input_contract.rs`
- Modify: `crates/windows-uia-provider/src/event_buffer_lib.rs`

**Produces:** bounded key-event/batch types, `WindowsKeyboardStateSnapshot`, `WindowsInputDispatchBlocker`, insertion classification.

- [ ] RED: reject empty batch, >32 events, and virtual key 0; preserve order.
- [ ] RED: held Shift/Ctrl/Alt/Windows state yields `InputStateConflict`; evaluator creates no compensating key events.
- [ ] RED: classify 4/4 as fully inserted, 2/4 as partial+unknown+reconcile, 0/4 as blocked/unknown-cause, 5/4 as invalid backend result.
- [ ] Commit RED.
- [ ] Implement pure types/evaluators only; no platform call in this step.
- [ ] Run tests and commit GREEN.

### Task 3: Provider-owned platform boundary — RED then GREEN

**Files:**
- Modify: `crates/windows-uia-provider/src/verified_input.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Create: `crates/windows-uia-provider/tests/verified_input_worker_contract.rs`

**Produces:** move-only provider execution request, exact `WindowsInputDispatchReceipt`, and a narrow platform inserter abstraction.

- [ ] RED: if final dispatch-context observation differs from earlier arm evidence, platform inserter call count stays zero.
- [ ] RED: if final keyboard snapshot has conflicting modifier, platform inserter call count stays zero.
- [ ] RED: receipt must match action/preparation/provider/target/element/batch identity exactly.
- [ ] Implement worker ordering: exact lease/incarnation check -> final context -> input-state snapshot -> conflict check -> one platform dispatch -> result classification -> receipt.
- [ ] Add Windows production backend using the existing keyboard/input Win32 feature set; preserve raw inserted count/diagnostic without inventing a specific policy cause.
- [ ] Run provider tests and commit.

### Task 4: Runtime/journal authority

**Files:**
- Modify: `crates/windows-observe-runtime/src/execution_arm.rs`
- Modify: `crates/windows-observe-runtime/src/runtime_manager.rs`
- Create: `crates/windows-observe-runtime/tests/verified_input_execution_contract.rs`

- [ ] RED: caller cannot construct/replay raw execution request; runtime mints it from one-shot PREPARED authority.
- [ ] RED: partial platform dispatch durably remains possibly/partially dispatched, requires reconciliation, and cannot grant automatic retry.
- [ ] RED: full platform dispatch still requires postcondition verification and does not directly mint world success.
- [ ] Implement separate verified-keyboard coordinator without changing semantic UIA executor semantics.
- [ ] Validate exact receipt binding before durable append; mismatch becomes dispatch-uncertain.
- [ ] Run runtime/journal regressions and commit.

### Task 5: Validation Lab W07/W08/W09

**Files:**
- Modify: `crates/validation-lab/src/real_provider.rs`
- Create: `crates/validation-lab/tests/v43_windows_input_cases.rs`

- [ ] RED W07: foreground theft not detected, or any later input effect, is a counterexample.
- [ ] RED W08: partial count treated as success or retry-authorized is a counterexample.
- [ ] RED W09: conflicting modifier not blocked, or any later input effect, is a counterexample.
- [ ] Implement provider-neutral case variants; keep RPOMR as the ordinary independent-oracle metric and invent no synthetic metric.
- [ ] Run Lab tests and commit.

### Task 6: Windows seed evidence for W07/W09

**Files:**
- Modify edge seed + oracle under `tools/provider-seeds/windows-uia-edge-seed/`.
- Create `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w07.rs`.
- Create `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w09.rs`.

- [ ] Add synthetic key-effect counter and deterministic secondary foreground window controlled only by harness.
- [ ] W07: authorize against target, switch foreground before final boundary, assert LocalView blocks and oracle effect count remains zero.
- [ ] Add test-owned modifier fixture with guaranteed cleanup.
- [ ] W09: establish real conflicting modifier state, assert `InputStateConflict`, zero platform insertion and zero target effect; LocalView does not release the modifier.
- [ ] Run exact hosted-Windows tests and commit.

### Task 7: W08 partial wrapper + production backend smoke

**Files:**
- Create `tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w08.rs`.
- Create/modify `crates/windows-uia-provider/tests/windows_verified_input_smoke.rs`.

- [ ] Deterministic wrapper returns exactly 2 accepted of 4 requested through the production classification/receipt path; assert partial+unknown+reconcile+no retry.
- [ ] Separate Windows smoke proves the production backend uses the same receipt path for an ordinary full dispatch against synthetic target.
- [ ] Artifacts must distinguish wrapper evidence from real platform full-dispatch evidence; do not claim hosted Windows naturally produced a partial result.
- [ ] Commit.

### Task 8: Prospective campaign W01–W09

**Files:**
- Modify L7 campaign test and Windows workflows.

- [ ] RED campaign if any W07/W08/W09 evidence is absent at its declared evidence level.
- [ ] Add exact W07/W08/W09 workflow gates while leaving W01–W06 gates intact.
- [ ] Require exactly nine bound observation digests in the prospective campaign.
- [ ] Run existing + new gates and commit.

### Task 9: Exact-head completion

- [ ] Observe exact-head CI, Windows UIA Observe and Windows Real Provider Seeds.
- [ ] Diagnose failures root-cause first; do not force-green, weaken assertions, inflate timeouts, or replace real-provider requirements with fakes.
- [ ] Verify no public raw-input bypass and no seed/Lab shipping dependency.
- [ ] Mark PR ready only when required exact-head gates are green.
- [ ] Merge with expected head SHA and confirm `main` merge commit.

Completion claim stays bounded to W07/W08/W09 at their declared evidence levels.