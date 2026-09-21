# Wave 9 Autonomous Verification Design

Date: 2026-09-21  
Lane: Worker AI-4  
Branch: `feat/wave9-autonomous-verification`

## Purpose

Wave 9 verifies a candidate change without granting the verifier authority to edit the user's real project.

The correctness pipeline is:

`exact base revision -> affected-state plan -> disposable candidate -> isolated shadow state -> contracts/postconditions -> mutation challenges -> predicted-vs-actual impact -> partial/escalated revalidation -> proof receipt`.

A successful receipt is evidence that a specific candidate was verified against a bounded, explicit state set. It is **not** permission to mutate the real working tree. Real Apply remains the existing Trusted Fix, human-reviewed authority.

## Non-goals

Wave 9 does not persist reports/artifacts, replace Wave 7 visual criticism, discover Wave 6 accessibility/flows, rewrite source ownership, or turn LocalView into an uncontrolled autonomous editor.

It does not infer root cause from impact correlation and does not claim complete coverage when a denominator is unknown.

## Trust boundaries

### Real working tree

The real working tree is read-only to Wave 9 candidate execution. Dirty user state is allowed and must remain byte/status equivalent across shadow preparation and cleanup.

Candidate execution must never implement "edit real file, test, restore". Failure to create safe isolation is terminal for that candidate.

### Candidate identity

A candidate is bound to:

- an exact full git object id;
- a candidate UUID;
- a deterministic patch digest;
- project-relative file paths;
- a SHA-256 base hash for every overlay;
- bounded file count, patch bytes, and source bytes.

The patch digest includes file path, base hash and patch bytes. The Trusted Fix bridge independently binds that digest back to the pending proposal preimage and review diff.

### Project containment

Absolute paths, parent traversal, symlink tree entries, binary sources, sensitive paths, missing files and files over the size bound fail closed.

Tracked secret-bearing paths are not copied into the shadow checkout. This intentionally prefers false-negative execution over exposing credentials.

## Affected-state compilation

`AffectedStateInput` is compiled by `localview-state-space` using the existing bounded state-space compiler.

The resulting `AffectedStatePlan` includes:

- base revision and change identity;
- impacted routes, regions, stable refs and contracts;
- state dimensions and selected product states;
- bounded risk score;
- evidence provenance;
- eligible-state denominator when known;
- pair coverage only when the denominator is known;
- explicit truncation/incomplete reasons.

Unknown denominator, dependency-graph incompleteness, compiler truncation, or missing provenance prevents a complete-coverage claim.

## Isolated shadow candidate

Git projects use a detached temporary worktree at the exact candidate revision.

Preparation order is security-sensitive:

1. validate exact repository root and HEAD;
2. validate full object id;
3. reject tracked secret paths;
4. validate every overlay path/hash/type/size and patch path;
5. snapshot the real worktree status;
6. create detached disposable worktree;
7. checkout exact base;
8. run `git apply --check`;
9. apply patch only in the shadow;
10. assert the shadow diff contains only expected paths.

Cleanup removes the registered worktree and proves the directory is absent. Drop performs best-effort cleanup but a verification receipt is not considered clean unless explicit cleanup proof succeeds.

## Candidate launch

A dynamic local port can be reserved on `127.0.0.1:0`.

Launching a shadow app is separately authorized and fails closed unless all of these are proven:

- loopback-only bind;
- network isolation;
- production/external-service isolation;
- bounded startup timeout;
- bounded lifetime.

No external-host navigation is authorized by this lane. Network fault mutation uses synthetic/loopback authority only.

## Live contracts and postconditions

The existing `localview-contracts` registry remains authoritative for hard/soft UX contracts.

Wave 9 adds:

- compilation of an impacted/effective contract subset;
- inheritance-cycle rejection;
- effective-scope conflict rejection;
- runtime facts compiled only from exact-revision, non-secret-tainted, observed/derived evidence;
- explicit fact-domain completeness;
- four-valued verdicts: Pass, Fail, Excepted, Unknown.

Absence is never inferred from an incomplete enumeration. Examples:

- observing a forbidden issue is enough to Fail even if issue enumeration is incomplete;
- not observing an issue is Pass only with a complete issue-code domain;
- an absent required selector is Unknown until selector enumeration is complete.

Hard Fail rejects the candidate. Hard Unknown makes proof inconclusive. Soft Fail/Unknown is retained as a warning and never masquerades as a hard invariant.

Registered native semantic postcondition results are projected into the same proof summary. `Unknown` remains `Unknown`.

## Mutation challenge

Wave 9 mutation execution starts from a cloned synthetic/shadow state. It does not mutate the user's live state.

Supported operators reuse `localview-mutation`: layout shifts/resizes, hide, accessible-name removal, tab-order breakage, handler disabling, feedback delay, loopback HTTP fault/timeout, content replacement and visual token override.

Each challenge records:

- safety decision;
- before/after state digest;
- whether mutation executed in isolation;
- triggered detector set;
- evidence id;
- Killed, Survived, SkippedUnsafe or Invalid verdict.

A skipped or invalid mutation is not silently removed from the proof population. A surviving mutation remains a first-class proof finding.

External side effects are forbidden by the Wave 9 execution policy.

## Predicted versus actual impact

Prediction and observation use typed targets:

- route;
- region;
- stable reference;
- contract;
- issue class;
- visual region.

Comparison emits:

- predicted and observed;
- predicted but not observed;
- unexpected observed impact;
- inconclusive.

When observation scope is incomplete, a missing predicted impact becomes Inconclusive rather than "not observed".

Unexpected impact is retained prominently. The receipt explicitly states that this comparison records correlation and does not establish root cause.

## Partial revalidation

The planner extracts the affected routes, regions, refs, contracts, responsive widths, flow checkpoints, visual baselines and source/semantic checks.

Partial revalidation is allowed only when affected-state evidence is complete and its denominator is known.

If dependencies are incomplete, the state-space is truncated, or the denominator is unknown, the planner switches to `Escalated`. A supplied known universe is merged into the plan. If no complete universe exists, the result remains escalated but cannot claim complete coverage.

Every planned state must be either revalidated or explicitly skipped with a reason.

## Proof receipt

`AutonomousVerificationReceipt` is serializable and content-addressable. It contains:

- base revision;
- candidate id;
- patch digest;
- isolation type;
- affected-state-plan hash;
- evaluated contracts and hard/soft classification;
- mutation results;
- predicted and actual impact;
- unexpected impact;
- evidence ids;
- stale evidence ids;
- revalidated state set;
- skipped states and reasons;
- resource-budget receipt;
- cleanup proof;
- final verdict and reasons.

Final verdicts are:

- `verified`: all hard proof obligations resolved, isolation/cleanup complete, budget satisfied, no surviving/skipped-invalid mutation challenge, no unresolved unexpected/inconclusive impact, and revalidation may claim complete coverage;
- `rejected`: identity binding fails, the real worktree changed, a hard contract fails, or cleanup/isolation proof fails;
- `inconclusive`: hard facts are unknown, dependency/coverage remains incomplete, mutation survives/is skipped/invalid, stale evidence exists, resource admission fails, impact is unresolved, or planned states are unaccounted.

Soft warnings are reported but alone do not convert an otherwise complete proof into rejection.

## Trusted Fix / Verify handoff

`trusted_fix.rs` can compile a pending human Fix proposal into a disposable counterfactual candidate. The overlay is bound to the proposal's exact preimage SHA-256 and review diff.

`trusted_verify.rs` accepts a Wave 9 handoff only when the receipt is Verified at the caller-supplied exact current revision and all cleanup/resource/hard-contract/mutation/impact/freshness checks remain clean.

The final Trusted Fix handoff revalidates:

- pending/non-expired proposal;
- candidate UUID;
- base revision;
- patch digest;
- exact proposal path;
- preimage hash;
- exact proposal diff;
- project containment;
- current real file still equals the proposal preimage.

The handoff helper intentionally does not call `begin_apply`, `apply_fix_transaction`, `fs::write`, commit, push, or any equivalent mutation authority.

## Resource and privacy policy

All dimensions are bounded. Oversized patches/files and resource-budget denials are explicit proof outcomes.

Secrets, credentials and environment secret stores are not copied into shadow state. Secret-tainted evidence is ignored for autonomous contract facts. The shadow launcher is denied unless external-service isolation can be proven.

## Adversarial coverage

The Wave 9 tests cover or gate:

- exact base mismatch;
- dirty real worktree remains untouched;
- path traversal;
- symlink escape;
- oversize patch;
- binary source;
- sensitive source;
- non-git project;
- loopback-only launch policy;
- startup/lifetime bounds;
- state-space truncation;
- incomplete dependency graph;
- unknown contract facts;
- contract conflict/inheritance cycles;
- hard failure and soft warning;
- unsafe mutation skip, kill and survival;
- predicted-only and unexpected impact;
- partial-revalidation escalation;
- stale evidence;
- cleanup failure;
- resource denial;
- absence of external mutation side effects;
- a real deterministic git-worktree candidate fixture.

## Known inconclusive boundaries

Wave 9 deliberately returns Inconclusive rather than guessing when:

- the impacted-state denominator cannot be established;
- evidence is stale or heuristic-only;
- a required shadow process/network/service isolation property cannot be proven;
- a relevant mutation cannot safely execute;
- a mutation survives;
- a required revalidation state is skipped/unaccounted;
- observation scope cannot distinguish "not observed" from "not inspected";
- dependency information is incomplete.

These are proof boundaries, not hidden score penalties.

## Persistence boundary

This lane returns the receipt in memory/serialization form only. Wave 8/integration owns report and artifact persistence. Wave 9 does not edit `crates/reports`, `crates/artifacts`, `crates/content-addressed` or `crates/attestation`.
