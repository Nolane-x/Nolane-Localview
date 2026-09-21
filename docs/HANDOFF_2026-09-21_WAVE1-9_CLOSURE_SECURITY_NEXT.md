# Nolane LocalView — Wave 1–9 Closure / Security-Hardening Handoff

Date: 2026-09-21  
Repository: `Nolane-x/Nolane-Localview`  
Primary branch: `main`

## 1. Read this first

This handoff exists so another AI can resume from the repository state without reconstructing the previous multi-agent sessions.

The bounded **Wave 1–9 software roadmap is closed**. Do not restart or rebuild Waves 1–9 from scratch. The next program is **production/security hardening + incomplete-surface discovery**, followed later by deliberately deferred product/research work.

Do **not** touch, merge, rewrite or claim closure for PR #116 / V4.3 W10 mixed-DPI physical hardware proof unless the user explicitly reopens that task.

## 2. Exact closure state

Wave 4–5 closure was already landed before this cycle.

This cycle closed:

- Wave 7 — PR #194 — Visual Critic + Design Grammar — merged as `7210dcbcb00c617c5772565e2e5c7a8291ca09ba`.
- Wave 6 — PR #195 — Accessibility + Interaction — merged as `3201b0d51e44ca7137c6f5524dc63c8a3b764772`.
- Wave 8 — PR #196 — Headless / CI / Reports / Baselines / Attestation — merged as `3b4608d6e827459137ce73dff4afe34bb7a9eae5`.
- Wave 9 original PR #197 was closed as superseded.
- Wave 9 final integration — PR #198 — merged as `bded849d7fdb4a640b4cd12c802381b783bc42c2`.
- PR #198 exact head `e5f59b074fa2979b2e91f919e0e4258a8ccd1087` passed **35/35 GitHub Actions workflows** with zero failed runs before merge.

The final integration branch for Wave 9 was deliberately created from the then-current `main` after Waves 6–8 landed. Its final diff contained exactly the Wave 9 ownership set and did not overwrite Wave 6/7/8 files.

## 3. What Waves 6–9 now provide

### Wave 6

Live bounded accessibility + interaction intelligence:

- local/bundled axe evidence;
- deterministic LocalView accessibility checks;
- privacy-safe native AX enrichment/discrepancy evidence;
- keyboard/focus journeys;
- transient focus-path overlay;
- effective hitbox evidence;
- dead-click/feedback-latency observation;
- bounded interaction graph discovery;
- deterministic replay receipts bound to stable refs and exact state.

Truth boundary: this does not claim automated accessibility completeness.

### Wave 7

Live visual critic + design grammar:

- project-pattern extraction from live evidence;
- spacing/type/control/etc. observed families when evidence exists;
- density/balance/hierarchy features;
- explicit deterministic / heuristic / subjective classes;
- structured critic findings;
- critic overlay model;
- serializable design-regression baseline/diff.

Truth boundary: inferred patterns are not official design tokens without source authority; subjective findings do not become deterministic facts or automatic CI failures.

### Wave 8

Headless / CI / reports / baseline execution:

- real headless LocalView execution;
- deterministic fixture/state adapters;
- JSON / Markdown / HTML reports;
- bounded artifact/baseline handling;
- content-addressed identity;
- Git-aware local annotations;
- deterministic CI exit policy;
- digest attestation.

Truth boundary: a digest envelope is not a trusted cryptographic signature unless a real signer/key exists.

### Wave 9

Autonomous candidate verification without uncontrolled editing:

- affected-state compilation;
- project-contained isolated shadow worktrees;
- hard/soft contracts with Pass/Fail/Excepted/Unknown;
- mutation challenges with killed/survived/skipped/invalid;
- predicted-vs-actual impact;
- unexpected-impact evidence;
- partial revalidation with escalation on incomplete authority;
- proof receipts;
- cleanup/resource proof;
- Trusted Fix/Verify handoff.

Truth boundary: Wave 9 can prove/reject/inconclusive a candidate, but it does **not** silently apply a winning patch to the user's real working tree. Unknown is never Pass.

## 4. Integration bugs found and fixed during closure

Do not regress these:

1. Wave 9 had Wave 5 regression-gate formatting failures in `trusted_verify.rs`.
   - Fixed the Wave 9 trusted-verification handoff formatting.
2. Wave 9 shadow-worktree test failed on Windows because Git checkout produced CRLF while the assertion required LF.
   - The assertion was made line-ending portable.
3. Dedicated Wave 9 formatting was strengthened for owned/new Wave 9 surfaces.
   - Do not reformat the entire legacy `trusted_fix.rs` merely to remove unrelated formatting debt.
4. A post-merge Windows UIA Observe run timed out once at the exact attach step.
   - The same run was retried and passed deep provider/action tests, confirming the previous result as a hosted-runner flake rather than a justified semantic code change.
   - Never “fix” provider code merely from one isolated hosted timeout without repeatable evidence.

## 5. Remaining open work is not Waves 6–9

The following are intentionally still open or broader than the closed software waves:

- native workspace composition/focus/crash/DPI policy before child WebView becomes default;
- PR #116 / W10 physical mixed-DPI proof, requiring real distinct-DPI display evidence;
- analysis-concurrency authority only when a concrete concurrent owner exists;
- Partial umbrella capabilities still explicitly listed in `docs/SPEC_COVERAGE.md`;
- broad framework parity beyond currently proven ownership families;
- broader diagnostics/network/performance depth where the coverage document still says Partial;
- rich desktop state-timeline UI even though deterministic replay itself is now implemented;
- later expanded V2/V3 causal / proof-carrying / multi-agent / content-addressed / attested-proof vertical slices.

Do not turn these into “bugs” unless their current documented contract says they should already exist.

## 6. Next mission from the user: strongest practical security / defect hardening

The next AI will receive a security skill bundle from the user.

The next AI must **first read the skill bundle and the real repo**, then build a new security/hardening wave plan based on evidence.

Do not assume a vulnerability exists. Do not claim a vulnerability without a reproducible path, failing test, unsafe invariant, or sufficiently concrete code/data-flow evidence.

The program should search for:

- correctness bugs;
- incomplete production paths;
- security vulnerabilities;
- privilege/authority confusion;
- auth/session ownership bypass;
- cross-session or stale-generation leakage;
- race/TOCTOU problems;
- unsafe filesystem/path/symlink handling;
- unsafe process execution or command/argument construction;
- temporary-file/worktree cleanup failures;
- secret/token/cookie/form-value leakage;
- cross-origin boundary violations;
- unsafe artifact/report/baseline persistence;
- content-addressed integrity mistakes;
- shadow candidate isolation escapes;
- trusted Fix/Verify bypass;
- native-provider unsafe authority or stale handles;
- malformed protocol/evidence parsing;
- resource-exhaustion / unbounded retention;
- supply-chain/dependency/build-security issues;
- insecure defaults;
- security-relevant platform differences between Windows/macOS/Linux.

## 7. How the next AI should create hardening waves

Do not invent a giant feature roadmap first.

Start with a **Security Discovery / Threat Model wave** and produce an attack-surface inventory.

A useful initial structure, to be revised from evidence, is:

### Security Wave S0 — Threat model + attack-surface inventory

Map trust boundaries and concrete attack surfaces:

- localhost discovery/classification;
- authenticated daemon/control plane;
- Tauri IPC;
- WebView bridge;
- MCP/CLI/headless surfaces;
- native capture/providers;
- artifact/report/baseline stores;
- source-map/source ownership;
- trusted Fix/Verify;
- Wave 9 shadow worktrees/candidate execution;
- temporary files/processes;
- project filesystem containment;
- network-fault authority;
- Chromium escalation;
- Git-aware/report paths.

Deliver findings with severity, exploit/precondition, affected authority, proof/reproducer and proposed regression test.

### Security Wave S1 — Control-plane / session / origin authority

Audit auth, exact-session ownership, route/origin checks, stale generation, replay, cancellation, request correlation, cross-session leakage and public/private action boundaries.

### Security Wave S2 — Filesystem / mutation / process isolation

Audit traversal, symlink/reparse points, canonicalization, shadow worktrees, temporary files, patch application, trusted Fix, cleanup/rollback, process launch, argv/shell handling and project-root containment.

### Security Wave S3 — Privacy / evidence / persistence

Audit redaction, secret-bearing fields, report escaping, artifact/baseline retention, content-addressed integrity, evidence taint/provenance, screenshots/private masks and accidental path/token leakage.

### Security Wave S4 — Native/platform boundaries

Audit WebView2/WKWebView/WebKitGTK, Windows UIA/macOS AX/Linux AT-SPI integration, handle/session lifetime, platform-specific path semantics, CRLF/encoding, process cleanup and platform permission boundaries.

### Security Wave S5 — Adversarial/fuzz/property campaign

Use fuzzing/property/adversarial tests where useful for protocol parsers, report rendering, content-addressed objects, evidence envelopes, patch inputs, path handling, session/state transitions and cleanup/rollback.

### Security Wave S6 — Production closure

Only after S0–S5 evidence:

- fix reproducible defects;
- add permanent regression gates;
- rerun exact-head dedicated security CI;
- rerun full cross-platform CI;
- update security/coverage/status docs truthfully;
- list surviving/inconclusive risks rather than hiding them.

The next AI may split or rename these waves after inspecting the security skill bundle and repository. Evidence owns the roadmap, not this suggested numbering.

## 8. Hard rules for the next AI

- Always begin from latest `main`.
- Never branch from an unmerged worker branch.
- Keep separate non-overlapping branches if multiple AIs are used.
- Do not modify PR #116/W10.
- Do not weaken existing security, privacy or fail-closed assertions merely to make CI green.
- Do not disable tests because they expose a real bug.
- Do not call heuristic/subjective findings security facts.
- Do not expose secrets in logs/tests/reports.
- Do not introduce arbitrary internet browsing into LocalView.
- Do not introduce a mandatory cloud/account dependency.
- Do not make Chromium permanent by default.
- Preserve local-first authority.
- Preserve human/trusted authority for real project mutation.
- Treat Unknown/Inconclusive as explicit states, never implicit success.
- When a hosted-runner failure appears flaky, reproduce/rerun before changing correctness-sensitive code.
- Every accepted security fix should gain a regression test whenever feasible.

## 9. Documents to read before security work

Read at minimum:

- `docs/ROADMAP.md`
- `docs/SPEC_COVERAGE.md`
- `docs/IMPLEMENTATION_STATUS.md`
- this handoff
- all Wave 6–9 design specs under `docs/superpowers/specs/`
- control/auth/session/bridge code
- filesystem/source-map/source ownership code
- artifacts/content-addressed/reports/attestation
- trusted Fix/Verify
- contracts/verification/mutation/counterfactual/state-space
- native providers
- CI workflows
- the user-supplied security skill bundle.

## 10. Definition of success for the next phase

The next phase is not successful because “security code was added”.

It is successful when:

1. the attack surface is explicitly mapped;
2. concrete defects/risks are reproducible or evidence-backed;
3. fixes preserve existing authority/truth boundaries;
4. permanent regression tests exist;
5. dedicated security workflows are exact-head green;
6. full cross-platform CI is green;
7. unresolved risks and unsupported claims are listed honestly;
8. `main` remains local-first, bounded and fail-closed.

## 11. Final closure evidence note

Before treating this handoff itself as final, verify the post-merge GitHub Actions suite for main commit:

`bded849d7fdb4a640b4cd12c802381b783bc42c2`

The exact-head pre-merge evidence is already closed: PR #198 head `e5f59b074fa2979b2e91f919e0e4258a8ccd1087` passed 35/35 workflows.

This file should be updated to state post-merge main is green only after all push-triggered workflows on `bded849d...` complete successfully.
