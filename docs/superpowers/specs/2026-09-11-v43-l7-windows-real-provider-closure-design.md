# V4.3 L7 Windows Real-Provider Closure — Design

Date: 2026-09-11

Status: Approved for implementation

Branch: `feat/v43-l7-windows-real-provider-closure`

Base: `main@f638a7ea15c598e375c98a17043d96752dbfb263`

## 1. Purpose

V4.3 cannot promote provider support from pure reducers, fake providers, or cross-platform compilation alone. The next correctness boundary is to run tiny deterministic applications against the real Windows UI Automation provider/runtime path, compare what LocalView observes or proves with independently exposed test ground truth, and feed that evidence into the existing Validation Lab authority without creating a second production provider stack.

This design closes the first executable Windows **real-provider seed** slice. It deliberately starts with provider freshness and identity semantics because those are the direct real-world counterparts of the fake-provider campaign already present on the base branch and because they exercise V4.3 acceptance conditions around event completeness, reconciliation, provider incarnation, stale identity, and Windows UIA lifetime semantics.

The first implementation slice covers three Windows seeds:

- **W01 — missing UIA property event**;
- **W02 — runtime-ID reuse / recreated element**;
- **W06 — COM apartment teardown / provider reacquire**.

Those three seeds establish the reusable real-provider harness. The remaining Windows seed matrix is intentionally outside this implementation slice and remains a separate follow-on program; this slice must not claim full Windows provider support.

## 2. Source-of-truth requirements

The design follows the V4.3 product specification requirements for:

- Lab Layer 7 real provider seed applications (§1034);
- provider-backed revision identity and `platform_profile` (§1007);
- seed environment manifests (§1119);
- Windows provider seed applications (§1249);
- real-provider seed app principle and test-only ground-truth side channel (§1321–§1322);
- Windows provider seed matrix W01–W15 (§1323);
- V4.3 validation pyramid (§1344);
- platform-provider CI tier (§1353);
- Definition of Done — V4.3 Lab (§1384);
- V4.3 acceptance rule that real-provider support claims require real-provider seeds (§1399).

A simulator can validate semantic discrimination. It cannot establish that real Windows UIA behaves as assumed. Real-provider observations therefore have stronger environment and provenance requirements than fake-provider observations.

## 3. Layer-number reconciliation

The specification contains two explicit layer descriptions:

1. the detailed Lab Layer sequence around §1027–§1037, where L6 is the Provider Contract Simulator and L7 is Real Provider Seed Apps;
2. the later V4.3 recommended validation pyramid in §1344, where L6 is the fake provider simulator and L7 is real provider seed applications.

Both descriptions agree on the meanings needed by this design:

- **L6 = fake/provider-contract simulator**;
- **L7 = real provider seed applications**.

Historical repository labels are not rewritten. In particular, merged commit subjects remain immutable provenance even where a human-facing layer label drifted. PR #110 metadata has been reconciled to L6. New V4.3 artifacts MUST prefer the semantic campaign name and, when a numeric layer is written, use the specification meaning above for provider campaigns.

The implementation adds a testable semantic campaign guard around provider campaign construction so that fake-provider work is bound to L6 and real-provider work is bound to L7. It must not alter the serialized `CampaignLayer::{L0..L9}` wire representation or rewrite old result artifacts.

For differential campaigns, existing historical names remain provenance. This slice does not attempt a global renumbering migration because the earlier detailed Lab Layer sequence and the later recommended pyramid organize some non-provider layers differently. A later documentation/ledger cleanup may map those semantic campaign kinds explicitly without changing historical identities.

## 4. Selected architecture

The selected architecture is a **test-only seed process + independent oracle channel + existing Windows provider runtime + provider-neutral Lab adapter**.

```text
Windows hosted CI / developer test

  test harness
      |
      +--> start purpose-built seed process
      |        |
      |        +--> real Win32/UIA surface
      |        |
      |        +--> test-only ground-truth channel
      |                    |
      |                    +--> exact seed state / incarnation facts
      |
      +--> existing LocalView Windows UIA observe/runtime path
      |        |
      |        +--> provider receipts / snapshots / reconciliation
      |
      +--> compare declared oracle property
               |
               v
       provider-neutral real-provider adapter
               |
               v
       localview-validation-lab
         - provider-backed observation
         - RPOMR + applicable typed metrics
         - preregistration/platform profile
         - evidence refs
         - bounded result strength
```

The seed process is not linked into LocalView. The LocalView runtime is not given access to the ground-truth channel. Only the test harness can read the oracle side channel.

## 5. Why this architecture

### 5.1 Reuse the real provider path

The repository already has substantial Windows provider/runtime coverage, including real provider dispatch, reconciliation, action capability, fresh evidence, postcondition verification, prepared dispatch, set-value execution/recovery, and real Win32/UIA smoke paths. The L7 harness must exercise those same product-facing provider APIs rather than introduce a parallel “lab provider.”

A test that calls a fake adapter and labels the result “real provider” is forbidden.

### 5.2 Keep Lab semantics provider-neutral

`localview-validation-lab` remains pure research authority. It may accept a typed record describing a completed provider-vs-oracle comparison, but it must not depend on `windows-uia-provider` or `windows-observe-runtime`.

Windows-specific tests own translation from real Windows receipts into the portable comparison input. This preserves the existing one-way boundary:

```text
production provider/runtime -> test harness -> validation-lab

validation-lab -X-> shipping provider/runtime
```

### 5.3 Keep ground truth independent

The oracle must not be derived from the same UIA observation that LocalView is being asked to validate. The seed process owns hidden exact state and exposes it only through a test-only channel. This gives the harness an independent comparator for provider-observed state.

## 6. Test-only Windows seed application

Create a purpose-built Windows test artifact under a non-shipping tools/test surface. The preferred shape is a tiny standalone executable with no dependency from daemon, desktop, CLI, provider, or runtime crates.

A concrete repository location for implementation is:

```text
tools/provider-seeds/windows-uia-seed/
  Cargo.toml
  src/main.rs
  src/state.rs
  src/control.rs
```

The package is built explicitly by the Windows provider-seed CI job rather than becoming a production dependency. If Cargo workspace nesting requires isolation, the seed package owns its own local workspace boundary; the implementation must prove that normal release binaries do not depend on it.

The first seed executable must be intentionally small. It needs only enough UI to expose deterministic controls and lifecycle transitions for W01, W02 and W06.

### 6.1 Seed control protocol

The test harness launches the seed with a unique run identity and a test-only local control/oracle endpoint. The endpoint may be a restricted named pipe or another local IPC primitive with equivalent process-local/test-only isolation.

The protocol supports bounded commands such as:

```text
READY
GET_GROUND_TRUTH
SET_PROPERTY_WITHOUT_EXPECTED_PROVIDER_EVENT
RECREATE_TARGET_CONTROL
RESTART_PROVIDER_SURFACE
SHUTDOWN
```

The exact command spelling is not correctness authority. The resulting typed state is.

### 6.2 Ground-truth state

Ground truth must include the minimum independent facts needed by the three seeds, for example:

```text
seed_run_id
seed_case_id
seed_process_incarnation
window_incarnation
control_incarnation
logical_value/state
recreation_generation
expected semantic identity relation
logical_sequence
```

The test harness canonicalizes and digests ground-truth records before placing their evidence reference into Lab observations. Raw pipe names, random channel tokens, process secrets, or sensitive text are not written into Lab artifacts.

### 6.3 Production isolation

Production LocalView MUST NOT know the ground-truth endpoint. The endpoint is passed only to the test seed/harness environment. A permanent dependency-exclusion test must prove that shipping crates do not depend on the seed package or a test-oracle client.

## 7. Provider-neutral real-provider comparison adapter

Add a narrow adapter inside `crates/validation-lab` for completed real-provider comparisons. Suggested types:

```text
RealProviderCaseInput
RealProviderCaseKind
RealProviderObservedOutcome
RealProviderGroundTruth
RealProviderLabRecord
```

The adapter receives already-collected provider evidence and independent oracle truth. It does not call Windows, UIA, IPC, the seed process, browser, network, or model.

### 7.1 Required authority fields

Every input must bind at least:

```text
case_id
seed_identity / seed_app_digest
platform_profile_revision
environment_artifact_digest
provider_evidence_refs[]
ground_truth_digest
comparison_profile_revision
logical_sequence
```

Whitespace/empty authority identifiers fail closed before an observation is minted.

### 7.2 Metric semantics

Every completed independent provider-vs-oracle comparison is eligible for **RPOMR**.

- provider result agrees with independently established ground truth -> RPOMR denominator +1, numerator unchanged;
- provider result asserts a conflicting world fact -> RPOMR denominator +1, numerator +1 through a typed `RealProviderOracleMismatch` failure flag;
- the provider result is explicitly `UNKNOWN`, `INCONCLUSIVE`, `UNSUPPORTED`, or conservatively blocked and therefore does not assert a conflicting fact -> it must not be counted as oracle mismatch, but it also cannot mint a successful real-provider case result.

This distinction is required so conservative behavior is not punished as silent unsoundness while incomplete evidence cannot masquerade as a passing provider integration.

The adapter may additionally mark a case eligible for an existing metric only when the exact seed creates that metric’s denominator opportunity. For the first three seeds:

- W01: EOFFR and RMR where the harness presents a real event-gap/reconciliation opportunity;
- W02: PIAER where an old provider-local identity can be confused with a new incarnation;
- W06: provider-lifetime/reconciliation evidence; RPOMR is always measured for a completed oracle comparison, while any additional metric eligibility must correspond to an existing typed failure predicate rather than be inferred from prose.

### 7.3 Result strength

A `REAL_PROVIDER_INTEGRATION_PASS` result is permitted only when:

- the run is prospective under a valid persisted preregistration;
- `platform_profile` is present and matches the executed environment family;
- the required case set for this declared campaign was actually executed;
- every required case produced an independent oracle comparison;
- no real-provider oracle mismatch or applicable typed failure occurred;
- no required case is unknown/inconclusive/unsupported;
- the seed app digest and environment manifest are bound to the run.

A mismatch produces `COUNTEREXAMPLE_FOUND`, preserving exact evidence references. Incomplete execution does not become a pass.

## 8. Environment binding

Provider-backed L7 evidence is invalid without an environment manifest. The harness records the specification’s required dimensions:

```text
OS build
architecture
LocalView build digest
provider/API capability revisions
display topology
DPI scale
locale/input method
permission state
seed app digest
```

For hosted Windows CI, fields unavailable with trustworthy precision must be represented as explicit unknown/unsupported metadata under the environment schema, not silently omitted while claiming a broader result.

The canonical environment artifact digest is included in the Lab run/result and in every real-provider case lineage that depends on it.

## 9. Seed W01 — Missing UIA property event

### Goal

Prove that missing event delivery cannot silently establish “nothing changed,” and that targeted reconciliation can restore current state.

### Seed sequence

1. launch seed and establish provider/target/control incarnation;
2. obtain a real UIA observation and oracle ground truth at state A;
3. mutate the seed to state B through a path designed not to rely on the expected property notification;
4. retain event-continuity evidence from the real Windows path;
5. require reconciliation before treating absence of notification as complete current-state evidence;
6. compare the reconciled LocalView state with independent seed ground truth B.

### Pass boundary

The case passes only if LocalView either observes the real change through trustworthy evidence or marks continuity/currentness insufficient and reconciles to B before making a current-state assertion.

If LocalView accepts stale A as current solely because no event arrived, the case creates a typed false-freshness counterexample. If reconciliation is claimed complete while the resulting state conflicts with ground truth, RPOMR records a mismatch.

## 10. Seed W02 — Runtime-ID reuse / recreated element

### Goal

Prove that provider-local identity is bound to provider/target incarnation and that a recreated control cannot resurrect an old semantic identity merely because an opaque provider identifier or similar fingerprint is reused.

### Seed sequence

1. launch target control incarnation A and bind provider evidence;
2. record an element reference plus independent control incarnation A;
3. destroy/recreate the logical control as incarnation B in the same deterministic location/role;
4. where feasible, make provider-level identity/fingerprint reuse pressure explicit;
5. ask the existing provider/runtime path to reacquire current state;
6. compare LocalView’s current element relation with ground truth incarnation B.

### Pass boundary

LocalView must invalidate or revalidate the old binding and create a new current observation. Accepting A as current B without sufficient reacquisition is a PIAER failure and a real-provider counterexample.

If Windows does not reuse the exact UIA runtime ID in a given hosted environment, the seed must retain the observed provider identity relation and may still test recreated-element invalidation. It must not fabricate an ABA success claim that did not occur. Exact runtime-ID reuse coverage then remains not measured for that environment.

## 11. Seed W06 — COM apartment teardown / provider reacquire

### Goal

Prove that UIA worker/apartment lifetime is part of current provider validity and that teardown/reacquisition does not preserve stale element authority.

### Seed sequence

1. attach through the real Windows provider runtime and establish provider/worker incarnation;
2. observe deterministic target state and ground truth;
3. force the test-owned provider worker/apartment lifecycle transition through an existing supported test boundary;
4. require the runtime to reacquire provider/element authority rather than reusing stale references;
5. obtain a fresh observation;
6. compare it against independent seed ground truth and record cleanup state.

### Pass boundary

A new usable observation must be bound to the current provider/worker incarnation. A stale pre-teardown element cannot become authoritative after the transition. Cleanup must return the test-owned provider resources to the declared baseline or report cleanup failure explicitly.

## 12. Evidence lineage

Each real-provider case retains references for:

```text
preregistration digest
seed identity + seed app digest
environment artifact digest
provider/target incarnation evidence
provider observation/reconciliation receipt(s)
ground-truth digest
comparison result
metric observation id
counterexample/minimized-seed ref when applicable
```

The oracle side-channel transport itself is not evidence authority. Canonical oracle state and its digest are.

Evidence from W01/W02/W06 must remain individually addressable. A campaign-level Boolean `success=true` is forbidden.

## 13. Failure handling

The harness fails closed under uncertainty.

- seed process fails to start -> case not measured; no pass;
- target/window selection is ambiguous -> case not measured; no pass;
- oracle channel unavailable or malformed -> hard case failure/incomplete; no RPOMR comparison and no pass;
- provider attach/reconciliation times out -> explicit inconclusive/unsupported state; no pass;
- required provider evidence lacks current incarnation binding -> no current-state success;
- provider and oracle disagree -> counterexample, not retry-until-green;
- cleanup cannot prove process/worker baseline -> record cleanup failure and retain diagnostics;
- a crash leaves action/world outcome unknown -> do not blindly retry a non-idempotent action.

Automatic retries are allowed only for clearly pre-dispatch/test-infrastructure operations whose semantics cannot duplicate a consequential external side effect. Retry policy itself must not erase the first failed evidence record.

## 14. CI topology

### 14.1 Cross-platform authority tests

Pure `localview-validation-lab` adapter tests remain in the ordinary Rust matrix on Ubuntu, macOS and Windows. These tests validate:

- fail-closed authority fields;
- RPOMR denominator/numerator accounting;
- conservative unknown/inconclusive handling;
- campaign-layer semantic guards for fake-provider versus real-provider cases;
- result-strength gating;
- provider-backed platform-profile requirement;
- shipping dependency exclusion.

They do not claim real-provider execution.

### 14.2 Windows real-provider gate

Extend the existing **Windows UIA Observe** workflow with a named gate, for example:

```text
V4.3 L7 Windows real-provider seeds W01/W02/W06
```

The job must:

1. build the exact LocalView candidate;
2. build the exact seed executable;
3. compute/bind their digests;
4. launch each seed case in a bounded subprocess;
5. exercise the real UIA/provider runtime path;
6. collect independent ground truth;
7. finalize a prospective provider-backed Lab run;
8. fail if RPOMR or an applicable safety metric records a failure;
9. fail if a required case is unmeasured/inconclusive when the environment claims the needed capability;
10. terminate seed/provider resources and verify cleanup baseline.

The job should publish bounded Lab result/environment/counterexample artifacts on failure, without publishing secrets or test-channel tokens.

## 15. Shipping isolation

The implementation must permanently prove all of the following:

- daemon does not depend on `localview-validation-lab`;
- desktop does not depend on `localview-validation-lab`;
- CLI and MCP do not depend on provider seed code;
- Windows provider/runtime does not depend on the test seed or oracle client;
- release packaging excludes `tools/provider-seeds/**`;
- the seed app and heavy Lab artifacts are not started during normal LocalView use.

A development/CI workspace may build heavy validation tools. Resident LocalView remains light.

## 16. Scope and decomposition

This design establishes the reusable Windows L7 harness and closes W01/W02/W06 only.

The remainder of the Windows matrix is deliberately decomposed into later slices because the failure surfaces require different authorities:

- **semantic/capability/resource:** W03 virtualized realization, W04 unsupported Invoke, W05 provider hang, W15 low-resource degradation;
- **input/action race:** W07 foreground steal, W08 partial input dispatch, W09 modifier interference, W11 modal race, W12 process restart after authorization;
- **geometry/accessibility/privacy:** W10 mixed DPI, W13 sensitive text/redaction, W14 owner-drawn weak accessibility.

W13 may be implemented early if it can reuse an existing secret-redaction fixture without expanding the first harness’s authority. It is not part of this slice’s pass requirement.

macOS AX/capture/input seeds and Linux AT-SPI/portal/PipeWire seeds require their own platform designs. L8 long stress/crash/restart and L9 holdout/challenger/security review remain later validation layers under §1344 and are not collapsed into this L7 slice.

## 17. Alternatives considered

### Alternative A — label existing Windows smoke tests as L7 real-provider evidence

Rejected. Existing smoke tests are valuable product verification, but without a purpose-built independent seed oracle, provider-backed preregistration, environment binding, and RPOMR accounting they do not satisfy the complete L7 research-evidence contract.

### Alternative B — make `localview-validation-lab` directly call Windows UIA

Rejected. That would couple research authority to one platform, risk a reverse dependency into production semantics, and make macOS/Linux reuse harder.

### Alternative C — build one giant Windows seed application covering W01–W15 now

Rejected for the first slice. It would combine event continuity, identity, raw input, DPI, resource, security, and action-lifecycle authorities before the harness itself is proven. Small seed families make failures local, keep oracle state understandable, and allow exact result-strength claims.

### Selected approach

Build the narrow reusable harness first, prove it with W01/W02/W06, then add independent seed families without changing the fundamental Lab/provider boundary.

## 18. Verification strategy

Implementation follows permanent RED -> minimal GREEN -> adversarial hardening.

Required permanent tests include:

1. fake-provider case cannot be preregistered as provider-real L7;
2. real-provider case cannot mint `REAL_PROVIDER_INTEGRATION_PASS` without `platform_profile`;
3. zero completed oracle comparisons leaves RPOMR `NOT_MEASURED`;
4. provider/oracle mismatch increments RPOMR exactly once;
5. explicit conservative unknown does not increment RPOMR and cannot mint pass;
6. missing ground-truth digest fails before observation creation;
7. missing environment digest fails before observation creation;
8. seed/app/profile identity drift fails closed;
9. W01 stale-currentness shortcut produces the expected counterexample path;
10. W02 stale identity acceptance produces PIAER and counterexample evidence;
11. W06 stale worker/provider authority cannot survive reacquisition;
12. shipping dependency exclusion remains true;
13. hosted Windows W01/W02/W06 execute against the real provider path and produce a non-zero RPOMR denominator;
14. exact-head CI and Windows provider-seed gate succeed before merge.

The test harness must retain the first divergence/counterexample rather than rerun until only a clean result remains.

## 19. Definition of Done for this slice

This slice is complete only when all of the following are true:

- provider campaign layer semantics are explicit and new fake-provider work is identified as L6 while real-provider work is identified as L7;
- historical commit names remain unchanged provenance; PR #110 metadata records L6;
- the Windows seed executable is test-only and independently controllable;
- production LocalView cannot read the oracle side channel;
- W01, W02 and W06 run against the real Windows UIA/provider runtime path;
- each required case binds independent ground truth and provider evidence;
- provider-backed preregistration binds a Windows `platform_profile`;
- the seed environment manifest binds OS/build/provider/display/DPI/locale/permission/seed-app identity at the available declared precision;
- RPOMR is actually measured with denominator greater than zero;
- a clean candidate records RPOMR numerator zero for the exact executed seed set;
- W01 measures the applicable freshness/reconciliation distinction;
- W02 measures stale identity/ABA behavior when the environment presents the relevant identity condition, without fabricating reuse that did not occur;
- W06 proves provider/worker reacquisition and cleanup behavior for the implemented boundary;
- any mismatch is retained as a counterexample rather than converted to a retry success;
- unknown/inconclusive/unsupported required cases cannot produce `REAL_PROVIDER_INTEGRATION_PASS`;
- no shipping binary gains a Validation Lab or seed-app dependency;
- cross-platform pure Lab tests remain green;
- the named hosted Windows L7 real-provider seed gate is green on the exact candidate SHA;
- the result claim is scoped to **Windows W01/W02/W06 real-provider seed coverage only** and does not claim universal Windows, macOS, Linux, or V4.3 completion.

## 20. Non-goals

This slice does not:

- merge all W01–W15 into one campaign;
- claim `SUPPORTED` for every Windows target family;
- implement macOS AX seed applications;
- implement Linux AT-SPI/portal/PipeWire seed applications;
- add raw `SendInput` authority;
- expand consequential action permissions;
- change production UIA dispatch semantics unless a real seed exposes a specific bug requiring a separately reviewed fix;
- add L8 long stress/crash/restart campaigns;
- add L9 holdout/challenger/security review;
- run fuzz/model-check infrastructure in resident LocalView;
- rewrite historical Lab artifacts or commit names.

## 21. Expected implementation sequence after design approval

The implementation plan orders work as follows:

1. reconcile provider campaign semantic-layer guards and permanent RED tests;
2. add provider-neutral real-provider comparison authority + RPOMR/result-strength tests;
3. build the test-only Windows seed process and independent oracle protocol;
4. wire W01 through the existing real observation/reconciliation path;
5. wire W02 through real recreated-element/provider identity handling;
6. wire W06 through real worker/provider lifetime teardown and reacquisition;
7. bind environment/platform/seed digests into a prospective Lab run;
8. add the named Windows hosted provider-seed CI gate;
9. run exact-head cross-platform Lab regression plus Windows real-provider verification;
10. self-review counterexample retention, cleanup baseline and shipping dependency exclusion before merge.

Each step must preserve the exact current production authority boundaries until a failing real-provider seed supplies evidence for a narrower corrective production change.
