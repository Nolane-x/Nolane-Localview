# Tier-3 Chromium Rendered Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect planner-authorized Tier-3 Chromium screenshots to bounded local artifact persistence and trusted Visual evidence without exposing raw pixels or creating a second Chromium authority.

**Architecture:** Add a distinct `ChromiumRenderedCapture` planner action alongside the existing compatibility probe. The planner chooses exactly one Tier-3 mode per browser-specific step; the control runtime resolves the route, executes the already-bounded screenshot primitive, stores PNG bytes in a lazy local ArtifactStore, retains metadata-only Visual evidence, and re-plans from that evidence.

**Tech Stack:** Rust, Tokio, Axum, LocalView planner/engine/control/evidence/artifacts/resource-governor/chromium crates, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-01-tier3-chromium-rendered-evidence-design.md`

## Global Constraints

- Tier-3 Chromium is admitted only by planner-owned `browser_specific_suspicion`.
- Exactly one Chromium spawn is charged for either Chromium action kind.
- Rendered capture additionally charges exactly one image region.
- PNG bytes remain local and never enter Evidence payloads or public JSON.
- Artifact filesystem paths remain private.
- Chromium rendered evidence records scale factor `1.0`; no native/Chromium pixel-equivalence claim is allowed.
- Existing native WebView capture and compatibility Contract evidence remain unchanged authorities.

---

### Task 1: Planner and engine rendered-Chromium authority

**Files:**
- Modify: `crates/planner/src/lib.rs`
- Modify: `crates/planner/tests/perception_budget_authority.rs`
- Modify: `crates/engine/src/lib.rs`
- Modify: `crates/engine/tests/tier3_perception_authority.rs`
- Modify: `crates/control/src/perception.rs`
- Create: `crates/control/tests/chromium_rendered_planning.rs`

**Interfaces:**
- Produces enum variant `PerceptionActionKind::ChromiumRenderedCapture`.
- Produces helper semantics where both Chromium action kinds force `chromium_spawns = 1`, while rendered capture also forces `image_regions = 1`.
- Produces live planning behavior: browser suspicion + viewport + remaining image budget => rendered capture; otherwise compatibility probe.

- [ ] **Step 1: Write RED planner/engine/control contracts**

Add tests asserting:

```rust
assert_eq!(selected.action.kind, PerceptionActionKind::ChromiumRenderedCapture);
assert_eq!(plan.budget_decision.usage.chromium_spawns, 1);
assert_eq!(plan.budget_decision.usage.image_regions, 1);
```

and a zero-image-budget case asserting:

```rust
assert_eq!(selected.action.kind, PerceptionActionKind::ChromiumEscalation);
assert_eq!(plan.budget_decision.usage.image_regions, 0);
```

Engine tests must build one-action authorized plans for both kinds and require `EngineTier::Chromium`; an unplanned Chromium request must remain rejected.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-planner --test perception_budget_authority
cargo test -p localview-engine --test tier3_perception_authority
cargo test -p localview-control --test chromium_rendered_planning
```

Expected: compile/test failure because `ChromiumRenderedCapture` and rendered candidate derivation do not exist.

- [ ] **Step 3: Implement minimal authority**

In planner, introduce a private predicate equivalent to:

```rust
fn is_chromium_action(kind: PerceptionActionKind) -> bool {
    matches!(kind, PerceptionActionKind::ChromiumEscalation | PerceptionActionKind::ChromiumRenderedCapture)
}
```

`BudgetedPerceptionCandidate::effective_usage()` must force one Chromium spawn for both kinds and force one image region for rendered capture. `perception_escalation_reason()` and rejection logic must require `BrowserSpecificSuspicion` for both.

In `control::perception`, pass remaining-image availability into candidate derivation. If browser-specific suspicion is active and `request.viewport.is_some()` and `spent.image_regions < request.budget.image_regions`, derive only `ChromiumRenderedCapture`; otherwise derive only `ChromiumEscalation`.

In engine, Tier-3 plan validation accepts either supported Chromium kind while still requiring exactly one action and one charged Chromium spawn.

- [ ] **Step 4: Run GREEN**

Run the three focused commands above plus:

```bash
cargo test -p localview-planner
cargo test -p localview-engine
```

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: authorize rendered Chromium perception`

---

### Task 2: Local artifact and Visual evidence authority

**Files:**
- Modify: `crates/control/Cargo.toml`
- Modify: `crates/control/src/chromium_runtime.rs`
- Create: `crates/control/tests/chromium_rendered_evidence.rs`

**Interfaces:**
- Consumes `localview_chromium::execute_rendered_screenshot` and `ChromiumScreenshotRequest`.
- Produces `execute_rendered_capture(state, session_id, revision, region, viewport, timeout_cap) -> Result<ChromiumRenderedReceipt, ChromiumRuntimeError>`.
- Produces metadata-only `ChromiumRenderedReceipt` with `target`, `artifact_id`, `bytes`, CSS viewport, pixel dimensions, stdout/stderr byte metadata and `evidence_id`.

- [ ] **Step 1: Write RED evidence tests**

Drive a configured fake Chromium executable and real `ControlState`. Assert successful capture retains one Visual evidence object satisfying:

```rust
assert_eq!(evidence.kind, EvidenceKind::Visual);
assert_eq!(evidence.provenance.source, "chromium-rendered");
assert_eq!(evidence.provenance.engine.as_deref(), Some("chromium"));
assert_eq!(evidence.payload["backend"], "chromium-headless");
assert_eq!(evidence.payload["viewport"]["device_scale_factor"], 1.0);
assert!(evidence.payload.get("path").is_none());
assert!(evidence.payload.get("png").is_none());
```

Also assert canonical target has no query/fragment, cross-origin/non-loopback target cannot execute, and invalid/nonzero/timeout cases insert no new Visual evidence.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-control --test chromium_rendered_evidence
```

Expected: compile failure because live rendered runtime authority is absent.

- [ ] **Step 3: Add lazy bounded ArtifactStore**

Add `localview-artifacts` to control dependencies. Extend `ChromiumExecutorConfig` with an `Arc<tokio::sync::Mutex<Option<ArtifactStore>>>` plus artifact root `<temp_root>/rendered-artifacts`. Keep configuration synchronous; open the store lazily during the first rendered execution with a 128 MiB budget.

Persist only after a successful zero-exit screenshot:

```rust
let artifact = store.put("visual/png", &execution.png).await?;
```

Never serialize `artifact.path`.

- [ ] **Step 4: Implement rendered runtime execution**

Validate CSS width/height are nonzero. Build `ChromiumScreenshotRequest` with `pixel_width = viewport.css_width` and `pixel_height = viewport.css_height`; the live Chromium evidence scale is explicitly `1.0`.

Use the existing server-owned `resolve_target`, private-safe route identity, timeout cap and `ResourceWorkKind::Chromium` reservation. Reject nonzero exit before artifact persistence. Insert one Visual evidence object only after artifact persistence succeeds.

- [ ] **Step 5: Run GREEN**

Run:

```bash
cargo test -p localview-control --test chromium_rendered_evidence
cargo test -p localview-chromium --test rendered_pixel_contract
cargo test -p localview-control --test chromium_perception_cycle
```

Expected: PASS.

- [ ] **Step 6: Commit**

Commit message: `feat: retain Chromium rendered evidence locally`

---

### Task 3: Whole-cycle rendered execution and retained feedback

**Files:**
- Modify: `crates/control/src/perception_cycle.rs`
- Modify: `crates/control/src/perception.rs`
- Create: `crates/control/tests/chromium_rendered_perception_cycle.rs`
- Modify: `crates/control/tests/chromium_evidence_freshness.rs`

**Interfaces:**
- Consumes `execute_rendered_capture` from Task 2.
- Produces `PerceptionCycleExecutionReceipt::ChromiumRendered` metadata-only result.
- Makes current trusted rendered Visual evidence satisfy browser-specific observation for exact route/revision.

- [ ] **Step 1: Write RED cycle contracts**

Test a browser-specific plan with viewport/image budget and assert:

```rust
assert_eq!(response["steps"][0]["execution"]["kind"], "chromium_rendered");
assert_eq!(response["usage"]["chromium_spawns"], 1);
assert_eq!(response["usage"]["image_regions"], 1);
assert!(response.to_string().find("png").is_none());
assert!(response.to_string().find("rendered-artifacts").is_none());
```

Prove there is exactly one retained Chromium Visual evidence object and no `page_load_dump_dom` Contract is created in the same step. Then re-plan and assert no second Chromium action while route/revision are unchanged. Change route or revision and assert Chromium becomes eligible again.

- [ ] **Step 2: Run RED**

Run:

```bash
cargo test -p localview-control --test chromium_rendered_perception_cycle
cargo test -p localview-control --test chromium_evidence_freshness
```

Expected: failure because the cycle only executes compatibility probes.

- [ ] **Step 3: Implement cycle execution**

Add metadata-only receipt fields and execute `ChromiumRenderedCapture` through Task 2. Actual usage is exactly:

```rust
PerceptionBudgetUsage {
    latency_ms: 0,
    text_tokens: 0,
    image_regions: 1,
    chromium_spawns: 1,
}
```

The existing cycle boundary replaces latency with measured elapsed time. Set `visual_satisfied = true` after success.

Update Chromium satisfaction logic so trusted current `chromium-rendered` Visual evidence or current `chromium-compatibility` Contract evidence can satisfy browser-specific observation, each requiring current canonical route/revision.

- [ ] **Step 4: Run GREEN and regression suite**

Run:

```bash
cargo test -p localview-control --test chromium_rendered_perception_cycle
cargo test -p localview-control --test chromium_evidence_freshness
cargo test -p localview-control --test perception_cycle_budget
cargo test -p localview-control --test native_visual_perception_cycle
cargo test -p localview-control --test action_cancellation
```

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: execute rendered Chromium perception`

---

### Task 4: Explicit CI, roadmap and exact-head proof

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/IMPLEMENTATION_STATUS.md`
- Modify: this plan, marking completed steps only after proof.

**Interfaces:**
- Produces named gates `Tier-3 Chromium rendered evidence contract`, `Tier-3 Chromium rendered planning contract`, and `Tier-3 Chromium rendered perception cycle contract`.

- [ ] **Step 1: Add explicit cross-platform CI commands**

Add to Rust core jobs:

```yaml
- name: Tier-3 Chromium rendered-pixel executor contract
  run: cargo test -p localview-chromium --test rendered_pixel_contract
- name: Tier-3 Chromium rendered planning contract
  run: cargo test -p localview-control --test chromium_rendered_planning
- name: Tier-3 Chromium rendered evidence contract
  run: cargo test -p localview-control --test chromium_rendered_evidence
- name: Tier-3 Chromium rendered perception cycle contract
  run: cargo test -p localview-control --test chromium_rendered_perception_cycle
```

- [ ] **Step 2: Update roadmap/status truthfully**

Document that Tier-3 rendered pixels are local Visual evidence with Chromium scale 1.0, not deterministic native-equivalent pixels. Keep responsive sweep, stitching and broader Resource Governor gaps open.

- [ ] **Step 3: Run full exact-head CI**

Require on one SHA:

- Ubuntu/macOS/Windows Rust core check + Clippy + full workspace tests;
- Tauri/frontend and all desktop contracts;
- WebKitGTK/WKWebView/WebView2 rendered-pixel smoke;
- all Chromium rendered planning/evidence/cycle contracts.

- [ ] **Step 4: Merge only after exact-head success**

Fast-forward is acceptable only when the current base is the exact merge-base/ancestor of the tested head. Never force-update `main`.

## Self-review

- Spec coverage: planner authority, engine admission, single-spawn selection, local artifact retention, metadata-only evidence, route/revision freshness, budget accounting, failure semantics and cross-platform proof all map to Tasks 1–4.
- Placeholder scan: no TODO/TBD implementation placeholders.
- Type consistency: rendered action name is `ChromiumRenderedCapture`; runtime function is `execute_rendered_capture`; execution receipt is `ChromiumRendered`; evidence source is `chromium-rendered`.
- Scope: native capture, full-page stitching, responsive sweeps and cross-engine deterministic pixel equality remain out of scope.
