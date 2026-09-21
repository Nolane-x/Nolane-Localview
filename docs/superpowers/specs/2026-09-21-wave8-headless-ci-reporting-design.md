# Wave 8 — Headless / CI / Reports / Baselines / Attestation

Date: 2026-09-21  
Branch: `feat/wave8-headless-ci-reporting`

## Scope

This lane closes Wave 8 only:

`exact LocalView session -> fresh bounded state -> selected analysis -> optional governed visual/Chromium work -> verification -> report bundle -> content-addressed baseline -> digest attestation -> deterministic CI status`.

It does not own accessibility/interaction discovery (Wave 6), visual-critic/design-grammar logic (Wave 7), or autonomous mutation/shadow-patch verification (Wave 9). The daemon/control engine remains an API authority and is consumed rather than refactored.

## Headless authority

`localview headless` reuses the authenticated localhost control plane. Session resolution is exact:

- an explicit SessionId must exist;
- zero sessions fail;
- one implicit session is accepted;
- multiple implicit sessions fail as ambiguous;
- only HTTP(S) loopback targets are admitted.

The runner requests a fresh semantic snapshot before work and another fresh semantic snapshot after work. Route, viewport, and semantic-state drift make the run inconclusive and withhold baseline authority.

Native visual verification is requested only through `/verify/visual/capture`, preserving the existing Runtime Resource Governor and native capture transaction. Chromium compatibility work is requested only through the planner-owned `/perception/cycle` path with one bounded Chromium spawn. There is no permanent Chromium pool and no arbitrary external navigation path.

## Deterministic fixture/state adapter

A fixture is bounded JSON with schema version, local route, viewport/device scale, stable-state identifier, explicit visual/Chromium permissions, and optional setup/cleanup commands.

Commands are structured executable + args, never shell strings. Shell interpreters are rejected, executable paths are not caller-selected, cwd must remain project-contained, arguments and timeout are bounded, stdin is closed, stdout/stderr are discarded, and timeout kills the child.

The fixture has a canonical SHA-256 identity. State identity also binds project key, route, viewport, fixture identity and stable-state identifier.

## Reports

Wave 8 produces JSON, Markdown and HTML from one report model. The report contains bounded:

- project/session/state identity;
- revision when known;
- route without query/fragment;
- viewport;
- evidence classes and IDs;
- deterministic/heuristic/subjective diagnostic counts;
- verification payload/result;
- baseline comparison;
- artifact references;
- Git annotation;
- incomplete/inconclusive reasons.

Report normalization strips absolute source paths, bounds text/list sizes, redacts sensitive nested keys, and escapes project/user content for Markdown/HTML. Control tokens, cookies, raw input values and raw secret-tainted evidence are never report inputs.

## Baseline authority and retention

Physical artifact storage ID and canonical content identity are deliberately separate:

- `lv-*` remains the legacy physical bounded ArtifactStore locator;
- `sha256:*` is the canonical content/proof authority.

A baseline envelope binds schema version, state identity, route, viewport, safe evidence semantic hashes, optional Wave 7 design-baseline hash, created revision and bounded provenance. Its canonical hash comes from `localview-content-addressed`; dependency closure includes evidence/design hashes.

Evidence IDs are report references only. Baseline comparison uses safe evidence semantics rather than volatile evidence IDs. Secret-tainted evidence is excluded.

Baseline retention is bounded by ArtifactStore. Retained bytes are revalidated against the canonical baseline hash before comparison. A run with state drift cannot create/update baseline authority.

## Git annotation

The runner uses the existing local `project-state` control endpoint. It may record HEAD commit, branch, dirty state, bounded project-relative changed files and bounded relevant source files. No GitHub API, push or commit is performed. Non-Git projects remain valid and report `git unavailable`.

## CI policy

Exit codes are stable:

- `0`: passed;
- `2`: hard deterministic/project-policy gate failed;
- `3`: inconclusive;
- `4`: infrastructure failure.

Heuristic findings do not fail by default. They become gates only with explicit project/CLI policy. GitHub Actions annotations, when enabled by the environment, are stdout protocol only and require no GitHub token/API.

## Attestation

Wave 8 emits a `digest_attestation` binding report hash, revision, state identity, safe evidence/proof hashes, gate status and a bounded environment fingerprint.

This is a digest envelope, not a cryptographic signature. Existing signed receipt APIs remain separate and are not impersonated.

## Privacy/resource invariants

- authenticated control plane only;
- exact loopback session only;
- no arbitrary Internet navigation;
- no control token in outputs;
- no secret-tainted evidence payload in outputs/baselines;
- no absolute secret paths in reports;
- bounded control/fixture/baseline/report data;
- bounded artifact retention;
- native visual and Chromium work remain under existing governor authorities;
- no Wave 6/7/9 execution authority is duplicated.

## Gates

Wave 8 exact-head gates cover session ambiguity/auth transport, deterministic fixture identity/timeout, route/state drift policy, visual/Chromium unavailability and governor denial classification, JSON/Markdown/HTML safety, canonical hashing/dedupe/retention, baseline dependency closure, Git annotations, deterministic CI policy, heuristic default behavior, digest-attestation stability, and secret/path redaction.
