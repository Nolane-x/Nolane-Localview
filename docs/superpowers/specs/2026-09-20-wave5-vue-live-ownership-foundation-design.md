# Wave 5 Vue Live Ownership Foundation — Engineering Design

## Status

Canonical design for the first bounded Vue 3 ownership foundation.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@f85684a7b446cab9921f11544bb53d67813970db`

Branch: `feat/wave5-vue-ownership-reconciled`

## Goal

Add privacy-safe, exact-element Vue 3 development ownership evidence to LocalView instrumentation without fabricating source coordinates and without reading Vue component state.

This slice deliberately separates **component ownership** from **source location**. Current `SourceLocation` requires a positive line, while Vue's bounded runtime evidence reliably proves a component file but does not, by itself, prove an exact source line for the rendered DOM element. LocalView must not invent line 1 or use line 0 as a sentinel.

## Existing authority

Already landed on `main` below this branch:

- explicit `data-component-source` / `data-source` source hints;
- bounded React ownership with true source evidence;
- bounded Svelte ownership with compiler-produced file/line/column;
- bounded Source Map v3 consumer;
- project-owned Source Map loading and containment;
- trusted RuntimeError generated-position correlation.

Explicit application source declarations remain the highest authority.

## Upstream Vue 3 evidence

Current Vue 3 runtime-core development rendering assigns own DOM markers including:

- `__vnode`;
- `__vueParentComponent`.

The marker is installed only under development or production-devtools feature authority.

Current Vue SFC compiler output also carries component `__file` metadata. This gives a bounded component/file identity when present, but no exact rendered-node source line is guaranteed by that surface.

All of these are private development details. Missing or changed shapes fail closed.

## Exact-element admission

Vue inspection runs only after explicit source, React and Svelte produce no admissible ownership result.

A candidate is considered only when:

1. the exact DOM element has an **own data property descriptor** named `__vueParentComponent`;
2. accessors/getters are never invoked;
3. the descriptor value is a bounded object candidate;
4. the instance exposes an own data `type` value;
5. the type is an object or function;
6. the type exposes an own data `__file` string;
7. the normalized file is an admissible project-relative `.vue` identity.

The marker consumes one shared framework probe once found, even if the candidate later fails validation.

## Shared framework probe budget

React, Svelte and Vue share one marker-bearing probe budget:

- 256 probes maximum per semantic snapshot;
- ordinary DOM nodes with no framework marker spend zero probes;
- a discovered React/Svelte/Vue private marker spends one probe;
- malformed marker shapes still spend that probe;
- exhaustion yields no later framework ownership hint instead of failing the snapshot.

## Privacy boundary

The Vue adapter may read only:

- exact-element `__vueParentComponent` own data value;
- instance `type` own data value;
- component type `__file` own data value.

It must not read or retain:

- props;
- attrs;
- slots;
- setupState;
- ctx;
- proxy;
- exposed;
- emit state;
- provides;
- vnode children;
- subTree;
- parent component chain;
- rendered text as ownership evidence;
- Vue DevTools global state;
- source contents.

## File identity

The first Vue foundation accepts only normalized project-relative `.vue` identity.

Rules:

- UTF-8 bounded to 260 bytes;
- slash-normalized;
- non-empty;
- no leading root;
- no Windows drive prefix;
- no URI scheme;
- no `%`, `?`, `#` or `:`;
- no explicit `..` segment;
- mandatory `.vue` extension.

Project-relative identities are retained directly. A bounded absolute compiler filename may transit only inside the authenticated Snapshot completion path so the control plane can reconcile it against the exact session project root. Before any evidence or action-result persistence, the control plane canonicalizes the candidate, requires a real regular `.vue` file contained by canonical `git_root`/`cwd`, rewrites it to project-relative form, or scrubs the ownership hint fail-closed. Agent-facing storage must never retain the absolute project path. Filesystem authority is bounded by a 150 ms total I/O budget per Snapshot; when that budget is exhausted, unresolved candidates are scrubbed rather than delaying the broader fresh-snapshot deadline. Sanitization applies to successful and failed Snapshot results before live result history is retained. If bounded node/depth traversal cannot prove complete tree coverage, the semantic tree is dropped fail-closed rather than persisting an unsanitized remainder. Absolute-path canonicalization itself is capped to a bounded number of unique candidates; additional absolute Vue candidates are scrubbed individually rather than dropping an otherwise valid tree.

## Component identity

Component name is derived only from the normalized `.vue` basename.

Example:

```text
file = src/components/UserCard.vue
component = UserCard
framework = vue
signal = element_parent_component
```

The component name is bounded to 96 UTF-8 bytes.

No component identity is inferred from tag names, classes, DOM depth, rendered text, props or registry lookup.

## Instrumentation payload

Raw semantic instrumentation may retain:

```text
origin = "vue-dev-instance"
file
component
signal = "element_parent_component"
```

It must not include a line or column unless a future upstream-backed exact-coordinate surface is independently proven.

## Projection boundary

Current `localview_protocol::SourceLocation` requires a positive line. Therefore this foundation **must not** project Vue file-only ownership into `SourceLocation`.

The first implementation remains truthful by keeping Vue ownership in the bounded instrumentation evidence layer only.

The dedicated bounded `ComponentOwnership` protocol is now landed on `main`. Vue file/component evidence therefore projects into `SemanticNode.ownership` while `SemanticNode.source` remains absent unless real source coordinates exist.

The representation is independent of `SourceLocation` and contains only:

```text
framework
file
component
signal
```

No sentinel source line is permitted. Progressive component targeting consumes this structured ownership and refuses conflicting legacy source-component fallback.

## Browser proof

Use a pinned Vue 3 SFC/compiler/runtime + Vite + Chromium fixture.

Prove:

1. a real Vue-rendered element has an own `__vueParentComponent` data descriptor;
2. the real plugin-vue development fixture exposes an absolute `__file` and page instrumentation transports it only as bounded raw ownership evidence without source coordinates;
3. backend authority canonicalizes an in-project absolute `.vue` file to project-relative identity before persistence and scrubs outside-project/unavailable candidates;
4. the fixture can still rewrite only the genuine Vue component type's `__file` field to a bounded project-relative identity and the same real Vue element/instance is admitted directly;
5. explicit `data-component-source` outranks Vue introspection;
6. no secret prop/setup-state value appears in serialized semantic evidence;
7. plain DOM does not fabricate Vue ownership;
8. accessor-backed `__vueParentComponent`, `type` and `__file` are never invoked;
9. traversal/encoded/non-`.vue` identities fail closed before or during backend authority;
10. the adapter does not read `instance.parent`;
11. no line/column is fabricated;
12. absolute filesystem paths never survive into retained evidence/action results.

## Verification gates

- instrumentation source-contract test;
- real Vue 3 browser proof;
- fresh semantic Vue ownership projection on the landed protocol;
- progressive component targeting regression on the landed protocol;
- React and Svelte ownership regression;
- workspace check;
- full repository CI.

Fresh snapshot and progressive targeting are now regression-gated against the landed coordinate-independent ownership protocol. The instrumentation slice must preserve the exact `vue-dev-instance` payload contract consumed by those surfaces.

## Explicit non-claims

This slice does not claim:

- production Vue always exposes private markers;
- arbitrary future Vue private marker compatibility;
- Vue props/setupState/context/proxy inspection;
- component parent-tree reconstruction;
- exact source line/column;
- conversion of absolute compiler paths inside page instrumentation;
- CSS ownership;
- DevTools replacement;
- root-cause proof.

## Completion definition

The foundation is complete when a real pinned Vue 3 SFC development fixture proves the genuine exact-element runtime marker and the current absolute `__file` truth boundary, while page instrumentation rejects that absolute identity fail-closed; after that proof, the fixture may normalize only the genuine component type's `__file` field to a bounded project-relative fixture identity and the same real Vue element/instance must be admitted under the shared framework probe budget, with explicit-source precedence, no getter invocation, no state leakage, strict path fencing and no fabricated source coordinates.

With the dedicated component-ownership protocol now landed, this reconciliation closes the bounded Vue runtime-evidence → fresh ownership → progressive component-targeting contract for project-relative `.vue` identities. Backend-safe reconciliation of upstream absolute compiler filenames is part of this closure: raw absolute identity is temporary transport only, while retained ownership is project-relative or absent.


## Exact-head closure verification

The absolute-path authority branch was retargeted directly to `main@a7e0ba22d42c67e8d3f582d85d95d5797f3aaea2` after the reconciled Vue ownership slice landed.

A one-shot repository formatter closed the Rust formatting gate at `e0b40d630acb2f4a1dc761a4e0013611aa71bbfa`; the temporary workflow self-removed in the same commit. This formatting closure changes no authority or privacy claim. The exact post-format branch head must still pass the focused Vue browser/authority contracts, React/Svelte regressions, workspace check and full repository CI before merge.
