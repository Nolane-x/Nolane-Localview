# Whole-Cycle Perception Budget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce one four-dimensional Perception Budget across repeated `diagnose → plan → execute → re-plan` steps, including cumulative reservations and measured whole-cycle latency, without creating caller-controlled escalation state.

**Architecture:** Add a cumulative planner admission primitive that evaluates `spent + next` against the original contract while preserving planner-owned escalation authority. Add one authenticated `/v1/sessions/{id}/perception/cycle` coordinator that owns cycle state inside a single request, repeatedly re-plans from retained evidence, executes only currently supported typed actions, updates cumulative reservations and measured elapsed latency, and stops on no-op, a fail-closed executor boundary, an unauthorized budget overrun, or a hard internal step bound. Existing `/perception/plan` and `/perception/step` remain backward-compatible single-step surfaces.

**Tech Stack:** Rust, Axum, Tokio, serde, `localview-token-budget`, `localview-planner`, `localview-control`, GitHub Actions.

**Spec:** `docs/ROADMAP.md` and `docs/IMPLEMENTATION_STATUS.md`, implementing the next listed Active Perception slice after the retained semantic feedback loop.

## Global Constraints

- Perception Budget dimensions remain exactly `latency_ms`, `text_tokens`, `image_regions`, `chromium_spawns`.
- CPU/RAM/capture storage/browser process count/hidden surfaces/concurrency/cache remain separate Runtime Resource Governor concerns.
- Public callers never submit `budget_escalation_reason`, a pre-authorized plan, cumulative spent usage, or cycle authority state.
- Chromium remains planner-authorized only when browser-specific suspicion exists; this slice does not add actual Chromium process execution.
- The live control-plane executor remains fail-closed for action kinds without a dedicated executor.
- No cloud/model/provider dependency is introduced.
- All arithmetic on cumulative usage is saturating and deterministic.
- One HTTP cycle is bounded independently of the Perception Budget by a small fixed maximum step count to prevent pathological infinite re-planning even when an escalation reason authorizes an overrun.
- Whole-cycle latency is measured from coordinator entry and re-evaluated at planner/executor boundaries; the current bridge action itself keeps its existing bounded timeout and is not force-cancelled mid-action in this slice, avoiding orphaned action state.

---

### Task 1: Cumulative planner admission authority

**Files:**
- Modify: `crates/planner/src/lib.rs`
- Modify: `crates/planner/tests/perception_budget_authority.rs`

**Interfaces:**
- Consumes: `PerceptionBudgetContract`, `PerceptionBudgetUsage`, `PerceptionCycleSignals`, existing `BudgetedPerceptionCandidate`.
- Produces: `plan_budgeted_perception_cycle_with_usage(candidates, budget, spent, signals)`; existing `plan_budgeted_perception_cycle` remains a zero-spent wrapper.

- [ ] **Step 1: Write failing cumulative-usage tests**

Add tests that require: (a) `spent.text_tokens + next.text_tokens` to reject an otherwise individually-in-budget action without escalation; (b) insufficient-evidence may authorize the same cumulative overrun and the returned budget decision contains cumulative usage plus `insufficient_evidence`; (c) cumulative Chromium spawns are counted after normalization; (d) input ordering remains deterministic under nonzero spent usage.

```rust
let spent = usage(100, 750, 0, 0);
let plan = plan_budgeted_perception_cycle_with_usage(
    &[candidate("region", PerceptionActionKind::RegionCapture, 1.0, usage(100, 100, 1, 0))],
    &budget(),
    &spent,
    &PerceptionCycleSignals::default(),
);
assert!(plan.actions.is_empty());
assert_eq!(plan.rejected[0].reason,
    PerceptionPlanRejectionReason::BudgetExceededWithoutAuthorizedEscalation);
```

- [ ] **Step 2: Run the focused test and observe RED**

Run: `cargo test -p localview-planner --test perception_budget_authority`

Expected: compile failure because `plan_budgeted_perception_cycle_with_usage` does not exist.

- [ ] **Step 3: Implement cumulative admission**

Implement saturating usage addition inside planner authority, evaluate the original contract against cumulative usage, score candidates against remaining budget where useful, and keep Chromium normalization/browser-specific authorization centralized. Keep the existing function as:

```rust
pub fn plan_budgeted_perception_cycle(
    candidates: &[BudgetedPerceptionCandidate],
    budget: &PerceptionBudgetContract,
    signals: &PerceptionCycleSignals,
) -> BudgetedPerceptionPlan {
    plan_budgeted_perception_cycle_with_usage(
        candidates,
        budget,
        &PerceptionBudgetUsage { latency_ms: 0, text_tokens: 0, image_regions: 0, chromium_spawns: 0 },
        signals,
    )
}
```

Expose one planner-owned helper for the escalation reason applicable to a selected action so the live coordinator does not duplicate escalation-priority policy.

- [ ] **Step 4: Run focused planner tests GREEN**

Run: `cargo test -p localview-planner --test perception_budget_authority`

Expected: all planner authority tests pass.

- [ ] **Step 5: Commit**

Commit message: `feat: admit perception steps against cumulative budget`

---

### Task 2: Live whole-cycle coordinator

**Files:**
- Create: `crates/control/tests/perception_cycle_budget.rs`
- Create: `crates/control/src/perception_cycle.rs`
- Modify: `crates/control/src/lib.rs`
- Modify: `crates/control/src/perception.rs`
- Reuse: `crates/control/src/fresh_snapshot.rs`

**Interfaces:**
- Consumes: shared `LivePerceptionPlanRequest`, `build_live_perception_plan_with_usage`, planner-owned escalation-reason helper, `acquire_fresh_semantic_snapshot`.
- Produces: authenticated `POST /v1/sessions/{id}/perception/cycle` returning bounded step receipts, cumulative usage, final budget decision and completion reason.

- [ ] **Step 1: Write RED integration contracts**

Create tests for the real HTTP control plane requiring:

1. Unknown request fields such as `spent`, `plan`, or `budget_escalation_reason` are rejected with `422`.
2. A normal cycle with sufficient budget queues one semantic snapshot, accepts the real authenticated action-result path, consumes retained evidence, re-plans to no-op, and returns one execution step with cumulative nonzero usage.
3. The cycle response reports measured elapsed latency and reserved non-latency usage under the original contract, not a fresh budget per re-plan.
4. A candidate that would exceed cumulative non-latency budget without an authorized signal is not executed.
5. Post-execution actual-latency overrun is re-evaluated through planner-owned escalation reason rather than silently accepted; the typed receipt records the resulting decision.
6. Unsupported visual/Chromium action remains fail-closed and no generic bridge action is queued.

The semantic success fixture should claim the queued Snapshot and POST its result through `/v1/sessions/{id}/actions/results`, exactly like the retained-feedback test.

- [ ] **Step 2: Run the focused control test and observe RED**

Run: `cargo test -p localview-control --test perception_cycle_budget`

Expected: 404 for `/perception/cycle` or compile failure for the not-yet-existing cumulative planning interface.

- [ ] **Step 3: Implement the coordinator**

Add `perception_cycle.rs` with a fixed `MAX_PERCEPTION_CYCLE_STEPS` safety bound. The handler must:

```rust
let started_at = Instant::now();
let mut spent = PerceptionBudgetUsage::zero();
let mut steps = Vec::new();

for _ in 0..MAX_PERCEPTION_CYCLE_STEPS {
    spent.latency_ms = elapsed_ms(started_at);
    let planned = build_live_perception_plan_with_usage(&state, id, &request, &spent).await?;
    if planned.plan.actions.is_empty() { return completed_noop(...); }
    let selected = planned.plan.actions[0].clone();
    execute only the typed supported action;
    spent = planned.plan.budget_decision.usage;
    spent.latency_ms = elapsed_ms(started_at);
    re-evaluate the original contract using the planner-owned reason for this selected action/signals;
    append a bounded receipt and continue;
}
return a typed step-limit conflict;
```

Do not accept cumulative usage from JSON. Do not persist cycle state in `ControlState`. Do not change the existing `/perception/step` behavior.

- [ ] **Step 4: Run focused control and regression tests GREEN**

Run:
- `cargo test -p localview-control --test perception_cycle_budget`
- `cargo test -p localview-control --test perception_retained_feedback_loop`
- `cargo test -p localview-control --test live_perception_execution`
- `cargo test -p localview-control --test live_perception_plan`
- `cargo test -p localview-planner --test perception_budget_authority`

Expected: all pass.

- [ ] **Step 5: Commit**

Commit message: `feat: enforce whole-cycle perception budget`

---

### Task 3: Cross-platform gate, docs and final verification

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: `docs/ROADMAP.md`

**Interfaces:**
- Consumes: completed whole-cycle runtime path.
- Produces: explicit hosted regression gate and truthful project status.

- [ ] **Step 1: Add explicit CI contract**

Add after the retained feedback gate:

```yaml
- name: Whole-cycle perception budget contract
  run: cargo test -p localview-control --test perception_cycle_budget
```

- [ ] **Step 2: Update status/roadmap without overclaiming**

Record cumulative planner admission and the live single-request whole-cycle coordinator as landed. Explicitly state that the existing bridge action keeps its own bounded timeout and is not force-cancelled mid-action; dedicated native visual/Chromium executors and Runtime Resource Governor enforcement remain incomplete.

- [ ] **Step 3: Run/review exact final-head CI**

Require `completed/success` for the exact final head across:
- Rust core Ubuntu/macOS/Windows;
- Tauri + frontend;
- WebKitGTK/WKWebView/WebView2 rendered-pixel smokes;
- planner, Tier-3, live planning, execution, retained-feedback and whole-cycle budget explicit gates.

- [ ] **Step 4: Review the PR diff manually**

Check for caller-controlled escalation, second execution authorities, CPU/RAM conflation, unbounded loops, duplicate screenshot/Chromium paths, and docs overclaims.

- [ ] **Step 5: Mark ready and squash-merge with expected-head lock**

Only merge after exact-head CI is successful and the diff review is clean.
