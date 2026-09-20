# Wave 5 Vue Live Ownership Foundation — Engineering Design

## Status

Canonical design for the first bounded Vue 3 ownership foundation.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@67bc243c165acdd842bc128575552dcff55704ca`

Branch: `feat/wave5-vue-live-ownership-main`

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

Absolute compiler filenames are rejected. Page instrumentation does not own project-root canonicalization and must not guess a filesystem prefix.

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

A follow-up protocol slice must add a dedicated bounded component-ownership representation before Vue ownership can participate in fresh `PageSnapshot`, progressive component targeting or source opening without fake coordinates.

That later representation should be independent of `SourceLocation` and contain only:

```text
framework
file
component
signal
```

No sentinel source line is permitted.

## Browser proof

Use a pinned Vue 3 SFC/compiler/runtime + Vite + Chromium fixture.

Prove:

1. a real Vue-rendered element has an own `__vueParentComponent` data descriptor;
2. the real plugin-vue development fixture exposes an absolute `__file` and page instrumentation rejects it fail-closed;
3. a bounded exact-element project-relative Vue marker is admitted as component/file ownership evidence;
4. explicit `data-component-source` outranks Vue introspection;
5. no secret prop/setup-state value appears in serialized semantic evidence;
6. plain DOM does not fabricate Vue ownership;
7. accessor-backed `__vueParentComponent`, `type` and `__file` are never invoked;
8. absolute/traversal/encoded/non-`.vue` file identities fail closed;
9. the adapter does not read `instance.parent`;
10. no line/column is fabricated.

## Verification gates

- instrumentation source-contract test;
- real Vue 3 browser proof;
- React and Svelte ownership regression;
- workspace check;
- full repository CI.

Fresh snapshot/progressive targeting must remain unchanged in this foundation because those surfaces require truthful source coordinates today.

## Explicit non-claims

This slice does not claim:

- production Vue always exposes private markers;
- arbitrary future Vue private marker compatibility;
- Vue props/setupState/context/proxy inspection;
- component parent-tree reconstruction;
- exact source line/column;
- fresh PageSnapshot component ownership;
- progressive component targeting from Vue ownership;
- conversion of absolute compiler paths inside page instrumentation;
- CSS ownership;
- DevTools replacement;
- root-cause proof.

## Completion definition

The foundation is complete when a real pinned Vue 3 SFC development fixture proves the genuine exact-element runtime marker and the current absolute `__file` truth boundary, while page instrumentation rejects that absolute identity fail-closed; the same adapter must separately admit a bounded project-relative exact-element Vue marker under the shared framework probe budget, with explicit-source precedence, no getter invocation, no state leakage, strict path fencing and no fabricated source coordinates.

It remains a foundation, not full end-to-end Vue source correlation, until a dedicated component-ownership protocol field is landed.
