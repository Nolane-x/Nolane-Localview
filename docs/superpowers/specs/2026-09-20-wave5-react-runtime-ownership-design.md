# Wave 5 React Runtime Ownership — Engineering Design

## Status

Canonical implementation specification for the first framework-native React component ownership slice.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@93ebf3cc0d93befef5837f224ba01bb5eaf3844c`

Branch: `feat/wave5-react-runtime-ownership`

## Goal

Attach one bounded React component-owner identity to semantic DOM nodes when the already-running managed page exposes a strong React Fiber binding for that exact DOM element.

This slice establishes **component ownership only**. It does not claim source-file ownership, JSX call-site resolution, root-cause attribution, props/state inspection, or React DevTools replacement.

## Existing authority

Already landed:

- managed-page semantic-tree instrumentation with stable element refs;
- bounded semantic node/depth retention;
- fresh exact-session semantic snapshot projection;
- explicit `data-component-source` ownership hints;
- project-owned Source Map runtime authority;
- live RuntimeError → project source correlation is being closed separately.

The React adapter must not weaken any of those authorities or turn inferred component names into source-file claims.

## React runtime signal

The adapter may inspect only own properties on the concrete DOM node to locate a React host Fiber binding with one of these bounded prefixes:

- `__reactFiber$`
- `__reactInternalInstance$`

No global DevTools hook is installed, replaced, patched or required.

If exactly one supported Fiber binding is found, the adapter may walk the Fiber `return` chain up to a hard depth of 32 and select the nearest composite owner carrying a bounded component display name.

A component name may be read only from inert identity metadata:

- function/class `displayName`;
- function/class `name`;
- wrapper object `displayName`;
- bounded inner `type` / `render` displayName/name.

No component function is invoked.

## Retained ownership packet

The semantic packet may retain only:

```text
framework = "react"
component = bounded non-empty component name
origin = "react_fiber"
depth = bounded owner distance
```

Hard bounds:

- component name: 128 UTF-8 bytes after existing secret redaction;
- owner walk: 32 fibers;
- supported Fiber keys inspected per DOM node: 8;
- framework ownership enrichments per snapshot: configurable but hard-clamped to 256.

## Privacy contract

The adapter must never retain or serialize:

- props;
- state;
- hooks;
- context;
- Fiber IDs/keys;
- Fiber object contents;
- DOM private-property names/suffixes;
- `_debugStack`;
- `_debugSource`;
- source paths;
- source code;
- component function bodies;
- event handlers;
- React DevTools payloads.

This first slice deliberately keeps React 18/19 source-location differences out of the ownership packet. React 19 removed the older `_debugSource` path and exposes different development debug metadata, so source correlation remains a later project-contained slice rather than being smuggled into this adapter.

## Fail-closed semantics

Return no React ownership when:

- framework ownership collection is disabled;
- the per-snapshot ownership budget is exhausted;
- zero or multiple supported Fiber host keys are present;
- the binding is null/non-object;
- the return chain cycles;
- no bounded composite display name is found within 32 levels;
- the component name is empty or exceeds the byte cap after redaction.

Do not fall back to tag, class name, DOM depth, test id, CSS class, filename guesses or text content.

## Protocol projection

Add an optional `FrameworkOwnership` to `SemanticNode`:

```rust
pub struct FrameworkOwnership {
    pub framework: String,
    pub component: String,
    pub origin: String,
    pub depth: u8,
}
```

The field is serde-defaulted and omitted when absent so previously serialized snapshots remain readable.

Fresh snapshot projection accepts only the exact bounded packet:

- framework == `react`;
- origin == `react_fiber`;
- component <= 128 bytes and non-empty;
- depth <= 32.

Malformed ownership metadata is dropped without invalidating the rest of the semantic node.

## Delta behavior

Framework ownership participates in semantic signatures. If a stable DOM ref moves between component owners, the semantic delta must mark that ref changed.

## Verification gates

### Gate 1 — instrumentation source contract

Prove generated bootstrap contains:

- the two supported Fiber key prefixes;
- exact-one-key requirement;
- 32-level return-chain cap;
- 256-node snapshot enrichment hard cap;
- bounded component name;
- no serialization of props/state/hooks/debugStack/debugSource.

### Gate 2 — browser behavior

In a deterministic browser fixture, prove:

- a DOM node with one React-like host Fiber binding resolves the nearest composite owner;
- nested composite owners choose the nearest owner;
- host-only/no-owner nodes emit no ownership;
- multiple matching Fiber keys fail closed;
- cyclic return chains terminate without emission;
- unrelated private properties are ignored;
- no props/state/debug source marker escapes the ownership packet.

The browser fixture validates LocalView's bounded Fiber-reader behavior; this slice does not claim every React version/runtime exposes these private bindings.

### Gate 3 — fresh snapshot projection

Prove:

- valid React ownership is projected into `SemanticNode.framework_owner`;
- malformed framework/origin/name/depth is dropped;
- explicit `SourceLocation` remains independent;
- generic DOM/source heuristics cannot create framework ownership.

### Gate 4 — compatibility

All existing `SemanticNode` constructors/tests are updated, and old serialized nodes lacking `framework_owner` still deserialize successfully.

### Gate 5 — repository regression

Minimum exact-head closure:

```text
cargo fmt --check
cargo test -p localview-instrumentation
cargo test -p localview-control fresh_snapshot
cargo test -p localview-protocol
cargo check --workspace --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- React source file or JSX line resolution;
- React 18 `_debugSource` support;
- React 19 `_debugStack` source mapping;
- production-build React ownership availability;
- React Server Component ownership;
- portal ownership completeness;
- Suspense/Offscreen ownership semantics;
- props/state/hooks/context inspection;
- Vue/Svelte ownership;
- component → project source containment;
- automatic source editing;
- root-cause proof.

## Completion definition

The slice is complete when LocalView can carry a bounded React component-owner identity from a strongly bound DOM/Fiber relationship into the fresh semantic snapshot without retaining React application state or source/debug payloads, while missing/ambiguous/private-runtime cases fail closed instead of fabricating ownership.
