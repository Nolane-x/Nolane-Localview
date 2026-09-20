# Wave 5 Component Ownership Protocol — Engineering Design

## Status

Canonical design for bounded component ownership independent of exact source coordinates.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@93b86ee77df90cf4ba3c1360cdbf25e1fe03fc08`

Branch: `feat/wave5-component-ownership-protocol`

## Goal

Let LocalView carry truthful framework component ownership through fresh semantic snapshots and progressive component targeting even when a runtime can prove component/file identity but cannot prove an exact source line.

This closes the protocol gap exposed by Vue 3 development metadata without weakening `SourceLocation`, inventing line 0/1, or storing framework runtime state.

## Existing authority

Already landed:
- explicit `data-source` / `data-component-source`;
- bounded React ownership with exact source evidence;
- bounded Svelte ownership with compiler file/line/column evidence;
- bounded Source Map v3 consumer;
- project-owned Source Map runtime authority;
- trusted RuntimeError -> project source correlation.

Vue foundation is developed separately and emits bounded `vue-dev-instance` file/component evidence without line/column.

## Protocol

Add:

```text
ComponentOwnership {
  framework: Option<String>,
  file: String,
  component: String,
  signal: String,
}
```

and:

```text
SemanticNode.ownership: Option<ComponentOwnership>
```

`SourceLocation` remains unchanged and continues to require a real positive line.

No sentinel coordinates are permitted.

## Projection policy

One raw `sourceHint` may project to:
- source only;
- ownership only;
- both source and ownership;
- neither, if invalid.

Rules:
- `data-source`: source only.
- `data-component-source`: source + explicit component ownership.
- `react-dev-fiber`: source + React ownership.
- `svelte-dev-meta`: source + Svelte ownership.
- `vue-dev-instance`: Vue ownership only.

Vue projection accepts only bounded project-relative `.vue` identity and exact bounded component/signal metadata. Absolute paths, URI schemes, encoded/traversal paths, malformed values and unknown signals fail closed.

## Progressive targeting

The component target resolver prefers `SemanticNode.ownership`.

A component target is emitted only when:
- the target has bounded ownership;
- an ancestor has the same framework/file/component ownership key;
- ancestor geometry is valid and contains the target.

The returned provenance uses the bounded component name.

For compatibility with already-landed snapshots/tests, the previous `SourceLocation.component` path remains a fallback when no structured ownership is present. It must not override conflicting structured ownership.

## Privacy

Component ownership contains only:
- framework identity when known;
- project-relative source file;
- bounded component name/identity;
- bounded signal classification.

It never contains props, state, context, hooks, setup state, DOM text, source contents, absolute project root, tokens or runtime object payloads.

## Bounds

- file <= 260 UTF-8 bytes;
- component <= 96 UTF-8 bytes;
- signal <= 64 UTF-8 bytes;
- framework is absent or one of `react`, `svelte`, `vue`;
- no control characters;
- framework-specific relative file validation remains fail-closed.

## Verification

Required focused gates:
- protocol serialization/backward-deserialization;
- fresh projection for explicit/React/Svelte/Vue;
- Vue ownership proves no fabricated `SourceLocation`;
- malformed/absolute/traversal Vue evidence fails closed;
- progressive resolver emits component target from ownership-only evidence;
- mismatched ownership does not fabricate a component target;
- existing source-based progressive resolution remains compatible;
- workspace all-target check;
- full repository CI.

Formatting closure note:
- the exact Rust surfaces named by the ownership-focused, React and Svelte gates were normalized with canonical `rustfmt --edition 2024` before final exact-head verification;
- formatting changes are non-semantic and exist only to unblock the focused contract matrix.

## Explicit non-claims

This slice does not claim:
- exact Vue source line/column;
- backend canonicalization of absolute Vue compiler paths;
- CSS ownership;
- component state inspection;
- arbitrary framework private-runtime compatibility;
- source editing;
- root-cause proof.

## Completion definition

Complete when a fresh snapshot can retain bounded Vue component ownership with `source = None`, React/Svelte/explicit ownership can retain structured ownership alongside their existing source evidence, and progressive component targeting consumes the structured ownership without fake coordinates while all previous source-based paths remain compatible.
