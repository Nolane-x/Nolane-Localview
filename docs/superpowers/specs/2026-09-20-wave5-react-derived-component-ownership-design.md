# Wave 5 React Derived Component Ownership — Engineering Design

## Status

Canonical implementation specification for the first live React component-ownership adapter.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@93ebf3cc0d93befef5837f224ba01bb5eaf3844c`

Branch: `feat/wave5-react-derived-ownership`

## Goal

Add bounded React component ownership evidence to LocalView-managed semantic snapshots without pretending private React Fiber internals are explicit source truth.

The adapter may identify a React component that owns a DOM node and may use that identity for lower-confidence progressive component targeting when target and ancestor independently corroborate the same derived owner.

It must not convert React-private metadata into `SourceLocation.component`, must not expose props/state/hooks/raw Fiber objects, and must fail soft when React internals are absent or incompatible.

## Trust model

LocalView has two distinct component-ownership classes:

1. **Explicit source ownership**
   - origin: application-provided `data-component-source`;
   - existing `SourceLocation.component`;
   - confidence: 0.95 in progressive targeting;
   - remains the highest-priority component authority.

2. **Derived framework ownership**
   - origin: passive React Fiber observation;
   - represented separately from `SourceLocation`;
   - basis: `react_fiber_private`;
   - confidence: capped at 0.65;
   - never presented as a source-file mapping or deterministic application assertion.

A derived React owner cannot overwrite, upgrade, or fabricate explicit source ownership.

## Instrumentation contract

Add an instrumentation configuration switch:

`include_framework_ownership: true` by default.

For one DOM element, the adapter may inspect only bounded own-property names looking for React host-instance keys with known prefixes:

- `__reactFiber$`
- `__reactInternalInstance$`

Resource bounds:

- inspect at most 64 own-property names;
- inspect at most one accepted React host Fiber per DOM element;
- walk at most 32 `return` links;
- maintain a visited-set bound of 32 to fail on cycles;
- component name maximum 120 UTF-8 bytes after redaction;
- no second history/ring buffer is introduced.

Host candidate validation:

- candidate must be object-like;
- its `stateNode` must be the exact DOM element before it is trusted as the host Fiber;
- arbitrary keys that merely share a prefix but do not bind `stateNode === element` are ignored.

## Component-name extraction

Starting at the host Fiber's parent chain, select the nearest bounded composite owner whose type exposes a stable diagnostic name.

Allowed name sources, in priority order:

- `type.displayName`;
- function/class `type.name`;
- `elementType.displayName`;
- function/class `elementType.name`;
- bounded wrapper lookup such as `type.render.displayName/name` for common ForwardRef-style wrappers.

String host types such as `"div"` are never component owners.

Names are diagnostic identity only. They are redacted, trimmed and bounded. Empty names are ignored.

## Retained schema

Semantic node raw packet may contain:

```json
{
  "frameworkOwnerHint": {
    "framework": "react",
    "component": "SettingsCard",
    "basis": "react_fiber_private",
    "confidence_milli": 650
  }
}
```

It must never contain:

- Fiber keys;
- Fiber tags;
- raw Fiber object data;
- props;
- state;
- hooks;
- context;
- component keys;
- raw debug stacks;
- source-code snippets;
- absolute file paths;
- React DevTools payloads.

## Protocol projection

Add a separate bounded protocol type:

```rust
enum FrameworkOwnershipBasis {
    ReactFiberPrivate,
}

struct FrameworkOwnershipHint {
    framework: String,
    component: String,
    basis: FrameworkOwnershipBasis,
    confidence_milli: u16,
}
```

`SemanticNode` gains optional `framework_ownership` with serde default for wire compatibility.

Fresh-snapshot projection accepts only:

- framework exactly `react`;
- basis exactly `react_fiber_private`;
- non-empty bounded component name;
- confidence in `1..=650`.

Malformed framework ownership invalidates only the hint, not the entire semantic node; the adapter is supplementary evidence.

## Progressive targeting

Component target resolution order:

1. existing explicit `SourceLocation.component` corroboration;
2. otherwise derived React ownership corroboration.

Derived React component target requirements:

- target node has `framework_ownership`;
- a semantic ancestor has the exact same framework, component and basis;
- ancestor geometry is valid and contains the target element;
- nearest qualifying ancestor wins.

Derived target provenance is distinct:

```text
framework_component {
  framework,
  component,
  basis,
  owner_ref
}
```

Confidence is capped by the hint and never exceeds 650.

Explicit source component targeting remains at 950 and always wins when available.

## Failure semantics

- no React host key -> no framework hint;
- malformed/spoofed prefix whose `stateNode` is not the exact element -> ignored;
- Fiber cycle/depth overflow -> no framework hint;
- anonymous/unnameable owner -> no framework hint;
- instrumentation exception -> ordinary semantic snapshot continues without React ownership;
- React version changes -> fail soft rather than breaking application behavior.

## Verification gates

### Gate 1 — instrumentation contract

Prove generated bootstrap includes:

- default-enabled framework ownership switch;
- bounded own-property scan;
- accepted React host-key prefixes;
- exact `stateNode === element` binding;
- 32-hop return bound;
- bounded component-name extraction;
- `frameworkOwnerHint` packet;
- no props/state/hooks/debug-stack retention.

### Gate 2 — fresh snapshot projection

Prove:

- valid React derived hint projects to separate `framework_ownership`;
- explicit `SourceLocation.component` remains unchanged;
- malformed/over-confidence framework hints are discarded without creating source ownership.

### Gate 3 — progressive resolver

Prove:

- explicit source component outranks React derived ownership;
- matching target + ancestor React ownership emits derived component target;
- one-sided/mismatched/spoofed ownership emits no component target;
- derived confidence never exceeds 650.

### Gate 4 — real browser + React proof

Use deterministic Chromium with a pinned React development fixture to prove:

1. a rendered DOM descendant of a named React component receives a bounded derived owner;
2. a nested DOM descendant and component boundary corroborate the same owner where expected;
3. ordinary non-React DOM receives no React owner;
4. a spoofed `__reactFiber$...` property whose `stateNode` is not the element is ignored;
5. drained semantic JSON contains no React props/state/hooks/debug stack or arbitrary fixture secret.

The proof must use the production instrumentation bootstrap, not a hand-copied detector.

### Gate 5 — regression

Minimum exact-head closure:

```text
cargo test -p localview-instrumentation
cargo test -p localview-control --test fresh_semantic_snapshot
cargo test -p localview-capture --test progressive_target_resolution
cargo check -p localview-protocol --all-targets
cargo check -p localview-control --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- React source-file mapping;
- React 19 debug-stack source symbolication;
- source-map correlation for component call sites;
- React Server Component ownership completeness;
- Suspense/Offscreen ownership completeness;
- React Native;
- Vue/Svelte ownership;
- props/state/hooks inspection;
- React DevTools replacement;
- deterministic security authority from private Fiber internals;
- component-to-source root-cause proof.

## Completion definition

The slice is complete when a real React development page observed through the production LocalView bootstrap can add bounded, separately-classified derived React component ownership to semantic nodes and use corroborated ownership for lower-confidence progressive component targeting, while explicit application source ownership remains higher authority and private React/application payloads are never retained.
