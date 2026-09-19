# Human-First Trusted Verify Change V2.6 — Engineering Specification

Status: canonical implementation specification for the Human-First Trusted Verify Change V2.6 wave.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Branch: `feat/human-first-trusted-verify-v26`

Base: `main@80a0dfac5c19963f505ac24c607429cbc232bc09`

Predecessors:
- Human-First UI/UX V2
- Trusted Capture V2.1
- Trusted Measure V2.2
- Trusted Open Source V2.3
- Trusted Ask AI V2.4
- Trusted Fix V2.5

---

## 0. Purpose

This document is the durable source of truth for Trusted Human-First Verify Change V2.6.

A future AI must be able to recover the authority model, baseline lifecycle, verification semantics, privacy boundaries, runtime states, tests and merge gates from this file without chat history.

The human-facing goal is:

> After a trusted Fix is applied, verify what actually changed in the managed UI and report objective evidence without silently editing anything else.

V2.6 must not turn “Apply succeeded” into “the UI is correct”.

Post-write byte verification and UI verification are different claims.

---

## 1. Product promise

After a successful V2.5 Apply, LocalView may offer **Verify change**.

Verify should answer:

- Did the applied source remain the exact postimage?
- Is the same managed session and canonical route still authoritative?
- Does the selected target still resolve exactly?
- Did the selected target or its local visual area observably change?
- Did new console/network regression signals appear?
- Is verification fully visual+semantic, semantic-only, or unavailable?
- If a connected AI provider is used, what is its advisory assessment of the before/after evidence?

Verify does not answer with absolute certainty that a product requirement is correct.

---

## 2. Non-negotiable no-write boundary

V2.6 is read-only.

Frontend Verify authority:

```
verificationId
```

Only.

React must not author:

- sessionId;
- reference;
- route;
- viewport;
- rect;
- baseline pixels;
- evidence IDs;
- source path;
- source contents;
- preimage/postimage;
- semantic snapshot;
- provider model;
- provider endpoint;
- provider headers;
- rollback command;
- patch;
- replacement text.

All verification authority is backend-owned and minted during the trusted Apply lifecycle.

---

## 3. No automatic rollback

V2.6 may report a regression signal or suggest rollback.

It must not:

- restore the old file automatically;
- invoke git;
- apply a reverse patch;
- write source;
- run shell commands;
- click UI controls;
- navigate the application;
- trigger another Fix automatically.

A future Trusted Rollback design, if desired, requires its own explicit write authority.

---

## 4. Why verification authority must be minted before write

Visual verification requires a trustworthy before-state.

A baseline created after Apply cannot prove how the UI changed.

Therefore V2.6 extends the backend Apply transaction with a **pre-Apply verification baseline minting phase** after V2.5 fresh authority revalidation but before source mutation.

The frontend still sends only `proposalId`.

---

## 5. Apply lifecycle extension

V2.6 Apply sequence:

1. consume backend-owned Fix proposal into Applying state;
2. acquire same-file Apply gate;
3. fresh-resolve session;
4. read canonical managed route;
5. fetch fresh semantic snapshot;
6. exact-resolve proposal reference;
7. exact-resolve trusted source mapping;
8. verify exact source preimage;
9. re-check route;
10. mint semantic verification baseline;
11. attempt bounded redacted visual baseline capture;
12. create backend-owned verification record;
13. perform V2.5 transactional source write;
14. exact postimage verification;
15. mark proposal Applied;
16. return Apply receipt containing opaque `verificationId`.

If the write fails, the verification record must be discarded.

---

## 6. Semantic baseline is mandatory

A trusted Verify record requires a semantic baseline.

The semantic baseline is created from the same fresh snapshot authority already used by Apply.

Minimum fields:

- session identity;
- stable reference;
- canonical route;
- snapshot version;
- project label;
- selected semantic projection;
- source locator;
- pre-Apply issue summary;
- verification context version.

If semantic baseline cannot be minted, Apply may fail closed rather than claim Verify availability.

V2.6 chooses correctness over a false verification affordance.

---

## 7. Visual baseline is bounded and may be unavailable

Visual baseline is valuable but platform capture may fail.

A semantic baseline may still support a limited verification result.

The Apply receipt must disclose whether a visual baseline was captured.

Verification scope:

- `semantic_visual`
- `semantic_only`

The UI must not render semantic-only verification as full visual proof.

---

## 8. Visual baseline authority

The visual baseline must come from the LocalView-managed surface.

Requirements:

- exact session;
- backend-derived viewport;
- native platform capture;
- capture settle;
- visual freeze;
- restore acknowledgement;
- private-pixel redaction before retention;
- route continuity;
- bounded dimensions;
- bounded bytes.

No DOM screenshot reconstruction.

No browser screenshot fallback.

No frontend viewport geometry.

---

## 9. Verification store

Introduce a backend-owned verification store.

Conceptual record:

```rust
struct FixVerificationRecord {
    verification_id: String,
    proposal_id: String,
    session_id: SessionId,
    reference: String,
    canonical_route: String,
    canonical_file: PathBuf,
    display_file: String,
    postimage: Vec<u8>,
    semantic_before: VerifySemanticBaseline,
    visual_before: Option<VerifyVisualBaseline>,
    instruction: String,
    created_at: Instant,
    expires_at: Instant,
    status: VerificationStatus,
}
```

Frontend never receives internal fields.

---

## 10. Opaque verification identity

Use a backend-generated unguessable opaque verification ID.

Frontend Apply receipt may receive:

```ts
interface HumanApplyFixReceipt {
  ...
  verificationId: string;
  verificationScope: 'semantic_visual' | 'semantic_only';
}
```

The ID is not source authority.

It only references backend-owned authority already minted before write.

---

## 11. Verification store bounds

Initial policy:

- max records: 16;
- TTL: 5 minutes;
- max visual baseline PNG bytes per record: 16 MiB;
- max total retained visual baseline bytes: 64 MiB;
- max semantic baseline serialized bytes: 48 KiB;
- oldest eligible records may be reaped;
- actively verifying records are not reaped mid-operation.

No unbounded baseline memory.

---

## 12. Verification record lifecycle

States:

- Pending
- Verifying
- Verified
- Expired
- Invalidated

Rules:

- one active Verify operation per record;
- duplicate submit suppressed;
- Verified is terminal;
- source/route authority failure invalidates the record;
- transient settle/provider failure may return inconclusive without inventing success;
- expired records cannot be revived.

---

## 13. One-shot versus retry

Verification facts should be immutable once successfully completed.

A transient failure before a trustworthy comparison may allow retry until TTL.

Once an objective verification receipt is committed, the active record becomes Verified and the same ID must not silently produce a different result later.

---

## 14. Post-Apply settle

Verify must not inspect immediately while HMR is mid-flight.

Use bounded existing capture settle authority.

Verification waits for:

- managed target available;
- bounded settle completion;
- fresh semantic snapshot;
- stable capture transaction when visual scope is available.

No infinite wait.

Initial overall Verify deadline: 15 seconds.

---

## 15. Fresh session and route authority

At Verify execution time:

1. resolve session from control authority;
2. read canonical managed route;
3. require equality with verification record route;
4. fetch fresh semantic snapshot;
5. canonicalize snapshot route;
6. require equality;
7. exact-resolve stored reference;
8. re-check route before returning result.

No fallback to another route/session.

---

## 16. Source postimage continuity

Before UI verification, re-read the trusted source target.

Require:

- same canonical file;
- same project root;
- same project-relative locator;
- same source mapping line when applicable;
- exact bytes equal V2.5 postimage.

If source changed after Apply:

`source_changed_after_apply`

Verify fails closed.

It must not judge UI evidence against unknown source state.

---

## 17. Exact target resolution

V2.6 initial scope requires the same stable LocalView reference to resolve exactly once after HMR.

Reject:

- zero matches;
- duplicate matches;
- malformed reference;
- route mismatch.

Do not fuzzy-match a nearby DOM node.

A valid fix that recreates the target under a new reference may therefore produce `target_unavailable` in V2.6.

This is conservative by design.

---

## 18. Semantic projection

Use a small deterministic selected-target projection.

Suggested fields:

- reference;
- role;
- name;
- tag;
- interactive;
- bounded rect;
- allowlisted attributes;
- project-relative source locator.

Never include raw input values or secret attributes.

Reuse V2.4 trusted context redaction where practical.

---

## 19. Semantic comparison

Compute objective before/after facts.

Possible facts:

- role changed;
- name changed;
- tag changed;
- interactive changed;
- geometry changed;
- safe attribute set changed;
- source locator changed.

Produce a bounded list of semantic change codes.

Do not turn those facts alone into “correct fix”.

---

## 20. Issue baseline

Before Apply baseline stores bounded issue fingerprints.

Console fingerprint:

- normalized level;
- bounded message;
- count.

Network fingerprint:

- method;
- sanitized route path;
- status/error class.

Do not retain:

- auth headers;
- bodies;
- cookies;
- URL query secrets.

---

## 21. Regression signal comparison

Post-Apply fresh snapshot is compared to baseline.

Objective regression signals may include:

- new console error fingerprint;
- increased console error count for existing error;
- new failed network request fingerprint;
- target disappeared;
- target became non-interactive when it was interactive;
- route changed;
- source changed.

Warnings alone should not automatically imply regression unless policy explicitly says so.

---

## 22. Redacted visual baseline representation

Do not keep raw unredacted frame data.

Recommended storage:

- redacted PNG bytes;
- pixel width/height;
- trusted viewport metadata;
- selected target rect in CSS coordinates;
- capture timestamp;
- revision if trustworthy.

Decode only during Verify.

---

## 23. Selected-target visual region

Whole-viewport diff is useful but can be noisy due unrelated animation.

V2.6 should compute two objective visual metrics where geometry permits:

1. viewport changed ratio;
2. target-local changed ratio.

The target-local region derives from backend semantic rect + trusted viewport scale.

React does not send the rect.

---

## 24. Visual comparison primitive

Reuse `localview_visual::pixel_diff` / bounded changed-region primitives.

Do not create a second ad-hoc pixel algorithm.

Initial threshold policy must be explicit and deterministic.

Suggested threshold:

- channel threshold = existing changed-region default where compatible;
- report ratio, do not hide the raw metric.

---

## 25. Geometry drift

If before/after viewport dimensions differ:

- do not rescale silently;
- visual comparison becomes unavailable/inconclusive;
- semantic verification may still proceed.

If target rect is outside viewport or non-finite:

- target-local visual comparison unavailable;
- whole viewport comparison may still be reported if dimensions match.

---

## 26. Visual baseline privacy

The provider does not receive image bytes in V2.6 by default.

Provider-assisted verification receives only bounded visual metrics and semantic/issue facts.

A future multimodal verification wave requires explicit image-sharing consent and privacy design.

---

## 27. Verification result semantics

Do not use a single overconfident boolean.

Backend result status:

- `change_observed`
- `no_observable_change`
- `regression_signal`
- `inconclusive`

Meaning:

### change_observed
At least one target-local semantic or visual fact changed, source/route/target authority remains valid, and no deterministic regression signal was detected.

This means **observable change exists without an obvious regression signal**.

It does not mean the user requirement is certainly satisfied.

### no_observable_change
Source was applied but neither semantic nor target-local visual facts show a relevant observable difference.

### regression_signal
One or more deterministic regression signals are present.

### inconclusive
Verification scope is insufficient or comparison cannot be trusted.

---

## 28. Provider-assisted assessment

Provider assessment is optional advisory output.

Provider receives:

- original bounded Fix instruction;
- bounded source change summary;
- before/after semantic facts;
- issue deltas;
- visual numeric metrics;
- deterministic result status.

Provider does not receive:

- filesystem path;
- full source contents;
- screenshot bytes;
- API secrets;
- mutation tools.

---

## 29. Provider cannot override deterministic facts

If deterministic status is `regression_signal`, provider cannot rewrite it to “verified”.

If deterministic status is `inconclusive`, provider cannot make LocalView claim certainty.

Provider output is stored separately:

```
assessment: {
  summary: string,
  confidenceLabel?: string
}
```

Human-facing copy must identify it as advisory.

---

## 30. No provider required

Verify works without an AI provider.

Core deterministic verification is LocalView-owned.

If provider unavailable:

- objective result still returns;
- advisory assessment omitted.

Do not disable Verify solely because AI is unavailable.

---

## 31. Verify command

Preferred Tauri command:

`verify_fix_change`

Frontend request:

```ts
interface VerifyFixChangeRequest {
  verificationId: string;
}
```

No generic verify payload.

---

## 32. Verify receipt

Conceptual receipt:

```ts
interface HumanVerifyChangeReceipt {
  verificationId: string;
  reference: string;
  displayFile: string;
  scope: 'semantic_visual' | 'semantic_only';
  status:
    | 'change_observed'
    | 'no_observable_change'
    | 'regression_signal'
    | 'inconclusive';
  semanticChanges: string[];
  regressionSignals: string[];
  viewportChangedRatio?: number;
  targetChangedRatio?: number;
  visualDiffEvidenceId?: string;
  snapshotVersion: number;
  providerLabel?: string;
  advisorySummary?: string;
  verifiedAtUnixMs: number;
}
```

Exact field names may evolve but authority semantics may not.

---

## 33. Evidence registration

When visual comparison succeeds, register bounded visual-diff evidence using existing LocalView evidence authority.

Do not let React mint evidence IDs.

Verification receipt may expose the backend-produced visual diff evidence ID.

---

## 34. Baseline capture evidence

The pre-Apply baseline itself does not need to be surfaced as a user artifact.

It exists to support Verify.

If persisted through existing artifact storage, its retention/budget must be explicit and it must remain redacted.

---

## 35. Apply receipt extension

V2.6 extends V2.5 Apply receipt with verification metadata.

It must not change Apply input authority.

Apply input remains opaque `proposalId` only.

Contract tests must lock this regression.

---

## 36. Failure to mint verification baseline

Because V2.6 makes Verify a first-class capability, semantic baseline minting failure should fail Apply before write.

Visual baseline failure alone may downgrade scope to semantic-only.

No source write should occur before the backend knows what verification scope it can honestly offer.

---

## 37. Failure after baseline but before write

If Apply transaction fails:

- remove/invalidate verification record;
- remove retained baseline bytes;
- no Verify affordance.

Do not retain an orphan record for a change that was never applied.

---

## 38. Selection/session UI isolation

After Apply success, the UI may show Verify for that applied change.

If current selection/session later changes:

- the verification record still belongs to original session/reference;
- UI must not present it as Verify for the new selection;
- active panel may retain a historical result only if clearly labeled.

Default V2.6 behavior: invalidate active Human-First verify state on session/selection change while keeping backend TTL cleanup.

---

## 39. Command palette

`COMMAND_IDS.aiVerifyChange` becomes real only when a current successful Fix state contains a valid verification ID.

Otherwise disabled with human guidance.

It must route through the same canonical Verify handler as the AI panel.

---

## 40. AI panel

After Fix Apply success:

- show Apply success;
- expose Verify Change action;
- explain Verify is read-only;
- show scope after completion;
- show objective facts separately from AI advisory text.

No auto-run unless explicitly designed later.

---

## 41. Inspector

Inspector may show a compact Verify action only when it refers to the currently applied change/reference.

Do not show a generic Verify button for arbitrary selection without a verification record.

---

## 42. Human wording

Prefer:

- “Verify change”
- “Checking the applied change…”
- “Observable change found”
- “No observable UI change”
- “Regression signal detected”
- “Verification inconclusive”
- “Semantic-only verification”
- “Visual + semantic verification”
- “AI assessment”

Avoid:

- “Fix confirmed”
- “Guaranteed correct”
- “100% resolved”

---

## 43. Accessibility

Verify button:

- semantic button;
- disabled state;
- `aria-busy`;
- accessible reason when unavailable.

Result:

- status announced once;
- objective facts in readable order;
- visual ratios readable as percentages;
- no color-only status.

---

## 44. Narrow viewport

Verification result must remain usable at narrow desktop widths.

Requirements:

- facts wrap;
- no horizontal overflow;
- ratios remain visible;
- provider advisory wraps;
- evidence ID truncates safely.

---

## 45. Reduced motion

Verification does not depend on animation.

Loading state remains clear under reduced-motion preference.

---

## 46. Verify state model

Conceptual frontend state:

```ts
type HumanVerifyState =
  | { status: 'idle' }
  | {
      status: 'ready';
      verificationId: string;
      scope: VerifyScope;
      reference: string;
      displayFile: string;
    }
  | {
      status: 'verifying';
      verificationId: string;
      reference: string;
    }
  | {
      status: 'success';
      receipt: HumanVerifyChangeReceipt;
    }
  | {
      status: 'failure';
      reason:
        | 'expired'
        | 'source_changed'
        | 'route_changed'
        | 'target_unavailable'
        | 'settle_failed'
        | 'failed';
    };
```

---

## 47. Duplicate suppression

While Verify is active:

- button disabled;
- `aria-busy=true`;
- repeated click/command does not duplicate backend request.

Backend store also prevents concurrent verification of one record.

---

## 48. Stale response isolation

Frontend uses generation + verification ID + original session/reference fences.

If active selection/session changes while Verify is in flight:

- completion must not attach to new selection;
- no stale result displayed as current.

---

## 49. Verification TTL

Initial TTL: 5 minutes from successful Apply baseline mint.

Expired Verify returns a humanized `expired` reason.

No silent reminting of a baseline after expiry.

The user may create a new Fix cycle if needed.

---

## 50. Route change

If route differs from pre-Apply canonical route:

- status does not become change_observed;
- return route_changed failure or regression signal depending exact phase;
- do not visually compare unrelated pages.

---

## 51. Source change after Apply

If source bytes no longer equal recorded postimage:

- return source_changed;
- do not provider-assess the old Fix against new code;
- do not mutate.

---

## 52. Target disappeared

If exact stable reference no longer resolves:

- deterministic regression signal `target_missing`, unless route/source authority itself already failed;
- no fuzzy remapping in V2.6.

---

## 53. New errors

A new console error or failed network fingerprint is a regression signal.

Existing unchanged issues are not counted as new regressions.

Issue comparison must be deterministic and bounded.

---

## 54. Visual unchanged

If target-local visual ratio is zero and semantic projection unchanged:

- `no_observable_change` unless regression signals exist.

If visual baseline unavailable:
- semantic equality alone may yield `no_observable_change` within semantic-only scope.

UI must state the scope.

---

## 55. Visual change outside target only

If viewport changed but target-local region did not and semantic target unchanged:

- default result is `inconclusive`, not change_observed.

This avoids crediting unrelated animation/layout elsewhere.

---

## 56. Target-local visual change

If target-local pixels changed above deterministic threshold and authority is otherwise healthy:

- this is an objective target-local change fact.

It may contribute to `change_observed`.

---

## 57. Geometry change

If selected target rect changes significantly but remains valid:

- record semantic geometry change;
- before/after target-local pixel comparison may need union bounding region if dimensions match;
- do not crop using frontend geometry.

---

## 58. Sensitive pixels

Visual baseline and post capture both pass private mask/redaction before comparison.

Verification must never compare an unredacted before image against redacted after image.

If redaction/freeze authority fails:
- visual verification unavailable/fail closed.

---

## 59. Verification context version

Introduce:

`VERIFY_CONTEXT_VERSION = 1`

Increment when semantics/privacy/result classification materially change.

---

## 60. Deterministic receipt

For identical trusted before/after inputs and policy version, deterministic fields must be identical.

Provider advisory text is explicitly outside deterministic result semantics.

---

## 61. Logging

Do not log:

- baseline image bytes;
- source file contents;
- full question/instruction;
- secrets;
- absolute paths.

May log:

- verification ID prefix;
- status code;
- scope;
- changed ratios;
- semantic change codes;
- regression code count;
- timing.

---

## 62. Backend module

Prefer:

`apps/desktop/src-tauri/src/trusted_verify.rs`

Responsibilities:

- verification record/store;
- TTL/capacity/resource bounds;
- semantic baseline;
- issue fingerprinting;
- comparison/classification;
- optional provider assessment envelope;
- receipt construction.

Visual capture plumbing remains in `visual_capture.rs`.

---

## 63. Visual capture internal API

Add a backend-internal capture primitive for Verify.

It should:

- derive viewport from managed freeze;
- capture native managed surface;
- verify route/geometry;
- restore state;
- redact private pixels;
- return bounded redacted PNG/frame for backend comparison.

It must not require frontend geometry.

Prefer factoring V2.1 capture code rather than duplicating it.

---

## 64. Resource accounting

Verification retained visual bytes must participate in explicit bounded resource policy.

Do not bypass existing resource governor concepts.

If dedicated retained kind is unavailable, enforce strict local store budget and document it.

---

## 65. Provider assessment envelope

If provider configured, send only:

- Fix instruction;
- changed source-line range and project-relative file label;
- deterministic semantic before/after summaries;
- regression signals;
- visual ratios;
- deterministic status;
- context version.

No source content by default.

No image bytes.

---

## 66. Provider response bounds

Reuse V2.4 provider answer safety bounds where practical.

Provider assessment is plain bounded text.

No HTML.

No executable action JSON.

---

## 67. No hidden second Fix

Provider Verify cannot return a patch that LocalView automatically applies.

If provider suggests another change, it is text only.

User must initiate a new Fix cycle.

---

## 68. Tauri permissions

Expose only exact Verify command(s).

No generic artifact read command.

No arbitrary baseline ID lookup from frontend beyond opaque verification ID.

---

## 69. API shape

Suggested frontend API:

```ts
verifyFixChange({ verificationId }): Promise<HumanVerifyChangeReceipt>
```

No route/reference/path/geometry parameters.

---

## 70. Localization

Add all Verify strings across every supported locale.

Minimum keys:

- verify.title
- verify.ready
- verify.action
- verify.inProgress
- verify.changeObserved
- verify.noObservableChange
- verify.regressionSignal
- verify.inconclusive
- verify.semanticVisual
- verify.semanticOnly
- verify.expired
- verify.sourceChanged
- verify.routeChanged
- verify.targetUnavailable
- verify.failed
- verify.objectiveFacts
- verify.aiAssessment
- verify.readOnlyDisclosure

---

## 71. RED contract

Add:

`apps/desktop/src-tauri/tests/human_first_trusted_verify_v26_contract.rs`

It must initially require:

- canonical V2.6 spec;
- Apply input still proposalId-only;
- Apply receipt carries opaque verification ID/scope;
- frontend Verify input is verificationId-only;
- no frontend path/reference/viewport/evidence authority;
- `trusted_verify.rs`;
- `VERIFY_CONTEXT_VERSION`;
- bounded verification store/TTL/visual bytes;
- pre-Apply baseline minted before source write;
- failed Apply discards verification baseline;
- Verify revalidates source postimage;
- fresh session/route/snapshot;
- exact reference resolution;
- deterministic semantic comparison;
- bounded issue delta;
- target-local + viewport visual metrics;
- private redacted pixels only;
- no automatic rollback;
- no source writes in Verify;
- provider advisory cannot override deterministic result;
- command palette shares canonical Verify handler;
- runtime/render evidence markers.

RED first.

---

## 72. Dedicated workflow

Add:

`.github/workflows/human-first-trusted-verify-v26.yml`

Minimum steps:

1. V2.6 contract;
2. trusted_verify focused unit tests;
3. V2.5 Fix regression;
4. V2.4 Ask regression;
5. V2.3 Open Source regression;
6. V2.2 Measure regression;
7. V2.1 Capture regression;
8. frontend build.

---

## 73. Dedicated render workflow

Add:

`.github/workflows/human-first-trusted-verify-v26-render.yml`

It must rerun whenever:

- V2.6 implementation/spec/contracts change;
- predecessor Human-First contracts change;
- visual capture primitives used by Verify change;
- render harness changes.

Exact-head evidence only.

---

## 74. Runtime/render matrix

Extend Human-First harness after V2.5 state 115.

Minimum states:

116. Verify ready after Apply.
117. semantic+visual scope disclosed.
118. semantic-only scope disclosed.
119. Verify in-flight.
120. duplicate Verify suppressed.
121. change_observed result.
122. no_observable_change result.
123. regression_signal result.
124. inconclusive result.
125. expired verification.
126. source changed after Apply.
127. route changed.
128. target unavailable.
129. new console regression.
130. new network regression.
131. provider advisory present.
132. provider unavailable but deterministic Verify succeeds.
133. raw provider error not leaked.
134. stale selection response isolated.
135. stale session response isolated.
136. narrow viewport result.
137. Vietnamese Verify ready.
138. Vietnamese change_observed.
139. Verify failure does not break Open Source/Measure/Capture/Ask.
140. command palette no verification disabled.
141. command palette Verify routes through same request.
142. Verify request is verificationId-only.
143. no caller path/reference/viewport/evidence authority.
144. no automatic rollback/write invoke.
145. Fix can start a new review after Verify.

The harness must assert behavior, not only screenshots.

---

## 75. Unit tests — store

Test:

- insert bounded record;
- max capacity;
- total visual-byte budget;
- TTL;
- Pending→Verifying;
- duplicate Verifying rejected;
- transient failure release policy;
- Verified terminal;
- expiry;
- invalidation;
- cleanup visual bytes;
- Apply failure cleanup.

---

## 76. Unit tests — semantic comparison

Test:

- unchanged target;
- role change;
- name change;
- interaction change;
- geometry change;
- safe attribute change;
- target missing;
- duplicate reference;
- deterministic ordering.

---

## 77. Unit tests — issues

Test:

- unchanged console issue;
- new error;
- increased error count;
- new network failure;
- URL query redacted;
- bodies/headers absent;
- max issue counts.

---

## 78. Unit tests — visual classification

Test with synthetic `RgbaImage`:

- identical images;
- target pixel change;
- outside-target-only change;
- whole viewport change;
- dimension mismatch;
- invalid target rect;
- threshold determinism;
- ratio bounds 0..=1.

Reuse `pixel_diff`.

---

## 79. Unit tests — deterministic status

Test:

- target-local change + no regressions → change_observed;
- no target change → no_observable_change;
- regression signal dominates → regression_signal;
- outside-target-only visual change → inconclusive;
- missing visual baseline + semantic change → change_observed with semantic_only;
- missing visual baseline + semantic unchanged → no_observable_change with semantic_only.

---

## 80. Apply regression

V2.6 must prove:

- frontend Apply remains proposalId-only;
- no frontend verification baseline authority;
- prebaseline mint happens before `apply_fix_transaction`;
- verification record removed on Apply failure;
- existing V2.5 transactional rollback still GREEN.

---

## 81. Ask regression

V2.4 Ask remains read-only.

Verify provider assessment must not make Ask gain write authority.

Keep contract scope precise.

---

## 82. Capture regression

V2.1 explicit Capture remains independent.

Verify's internal visual baseline must not fake a user Capture receipt.

User-facing Capture evidence and internal Verify baseline are distinct.

---

## 83. Changed-region baseline isolation

Do not depend on the session-global `VisualBaselineCache` as the sole Verify before-state.

V2.6 record owns its exact pre-Apply baseline.

Existing changed-region capture remains unchanged for other workflows.

---

## 84. HMR not guaranteed

If project does not HMR or the managed app does not settle in time:

- return settle_failed/inconclusive;
- do not claim no change simply because the UI did not refresh in time.

---

## 85. No source execution

Verify does not run build/test commands automatically in V2.6.

It observes managed UI + source continuity only.

A later Trusted Test Runner design can add explicit execution authority.

---

## 86. Truthful limitation

V2.6 verifies an observed local UI effect.

It does not prove:

- all application routes;
- all devices;
- accessibility conformance globally;
- business correctness;
- absence of hidden regressions.

UI wording must preserve that limitation.

---

## 87. PR merge gate

Do not mark V2.6 ready until one immutable exact head has GREEN:

- dedicated V2.6 contract;
- trusted_verify unit tests;
- V2.5 regression;
- V2.4 regression;
- V2.3 regression;
- V2.2 regression;
- V2.1 regression;
- frontend build;
- V2.6 render audit;
- full repository CI;
- native GUI smoke WebKitGTK/WKWebView/WebView2;
- Windows UIA Observe;
- Windows Real Provider Seeds.

If head changes, regenerate evidence.

---

## 88. PR body closure

Record:

- exact head SHA;
- render screenshot count;
- executable check count;
- artifact digest;
- verification store limits;
- dedicated gate result;
- full CI result;
- Windows results.

Do not write “complete” before exact-head closure.

---

## 89. Future rollback boundary

A future rollback wave may use a retained preimage only under a new explicit write transaction.

V2.6 must not retain hidden rollback authority in the frontend.

No “Undo automatically”.

---

## 90. Continuation protocol

A future AI resuming V2.6 must:

1. read this spec;
2. fetch current PR head;
3. inspect all V2.6 commits;
4. inspect exact-head workflow runs;
5. preserve `verificationId`-only frontend authority;
6. preserve proposalId-only Apply authority;
7. keep Verify read-only;
8. preserve pre-Apply baseline ordering;
9. keep visual pixels redacted before retention;
10. never use session-global baseline as proof of this Fix;
11. never let provider override deterministic facts;
12. run RED→GREEN;
13. keep PR Draft until exact-head closure;
14. never reuse GREEN from older SHA.

---

## 91. Definition of done

Trusted Verify Change V2.6 is complete only when LocalView can:

- mint a bounded trusted baseline before a V2.5 write;
- apply source transactionally;
- expose only an opaque verification ID;
- wait for bounded post-Apply settle;
- revalidate exact source/session/route/target authority;
- compare fresh semantic and, when available, redacted visual state;
- detect deterministic regression signals;
- report verification scope honestly;
- optionally add bounded provider advisory assessment;
- remain completely read-only during Verify;
- pass all exact-head Human-First, cross-platform, native GUI and Windows provider gates.
