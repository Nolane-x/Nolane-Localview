# Wave 5 React Live Component Ownership — Engineering Design

## Status

Canonical implementation specification for the first React-specific live ownership slice.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@d62df4c85d5eaae6b802c9b086f220fdd46526df`

Branch: `feat/wave5-react-live-ownership`

## Goal

Add bounded React component ownership evidence to LocalView-managed semantic snapshots without inventing ownership from DOM tag/class/depth heuristics, without mutating React, and without retaining component props, state, context, hooks or arbitrary runtime objects.

This slice is intentionally conservative: React ownership exists only when a host DOM element is demonstrably attached to a React fiber and a bounded component ancestor exposes development source metadata.

## Existing authority

Already landed:

- bounded semantic snapshots with stable element refs;
- explicit `data-source` / `data-component-source` source hints;
- fresh snapshot projection into bounded `SourceLocation`;
- progressive component targeting that consumes corroborated `source.component` ancestry only;
- bounded Source Map v3 parsing;
- project-owned Source Map resolution;
- trusted RuntimeError generated-position → project-source correlation.

Explicit application-provided source hints remain higher-authority than framework introspection.

## React runtime observation

The instrumentation may perform a read-only React fallback only after no explicit `data-component-source` or `data-source` hint exists.

A candidate host fiber must satisfy all of the following:

1. discovered only from an own property of the exact DOM element;
2. property name begins with `__reactFiber$` or legacy `__reactInternalInstance$`;
3. own-property inspection is capped at 64 keys;
4. the exact element also exposes the paired `__reactProps$<same-suffix>` own-property marker used by React DOM, but LocalView never reads its value;
5. the fiber is read only from an own data-property descriptor, never through an arbitrary getter;
6. the candidate fiber is an object;
7. `fiber.stateNode === element` exactly, proving the fiber belongs to that host element;
8. ancestor traversal follows only `return`;
9. traversal depth is capped at 32.

The adapter must not install or replace `__REACT_DEVTOOLS_GLOBAL_HOOK__`, patch React APIs, mutate fibers, traverse arbitrary child/sibling graphs, or create a second framework runtime.

## Component evidence

A component ancestor is accepted only when a bounded component identity can be derived from its React type and one bounded development source signal is available.

Supported development source signals are:

- React <=18-style `_debugSource`, with bounded `fileName`, positive `lineNumber` and optional positive `columnNumber`;
- React 19-style `_debugStack`, parsed from at most 16 KiB / 24 lines and admitted only when one HTTP(S) frame belongs to the exact current `location.origin`.

For `_debugStack`, LocalView skips React/ReactDOM/JSX runtime/Vite-internal/dependency frames and rejects `/@fs/`, percent-encoded paths, protocol-relative paths and explicit `..` path components. It retains only the bounded same-origin path plus line/column; the raw stack is never retained. The host fiber's source signal is preferred, because it identifies the JSX/source position that created the DOM host node; a component ancestor source signal is only a bounded fallback.

Component identity may use:

- `type.displayName`;
- `type.name`;
- one bounded wrapper level through `type.render` or `type.type`.

Anonymous/unresolved types are skipped rather than named heuristically.

Retained payload:

```text
origin = "react-dev-fiber"
file
line
column?
component
```

The adapter never retains:

- props;
- state;
- memoizedState;
- pendingProps;
- memoizedProps;
- context;
- hooks;
- keys;
- refs;
- rendered text;
- source contents;
- arbitrary fiber fields.

## Resource bounds

Per semantic-tree snapshot:

- at most 256 React ownership probes;
- at most 64 own-property names scanned per probed element;
- at most 32 fiber ancestors traversed;
- component display identity capped at 96 UTF-8 bytes;
- React source file capped at the existing 260-byte source-file limit;
- source line <= 1,000,000;
- source column <= 10,000,001;
- React 19 debug-stack inspection capped at 16 KiB and 24 lines;
- only exact same-origin HTTP(S) debug-stack frames may become source evidence.

An exhausted ownership probe budget produces no React ownership hint for later nodes. It does not make the semantic snapshot fail.

## Source projection

`fresh_snapshot::project_source` gains `react-dev-fiber` as an allowed source origin.

For this origin:

- file and positive line are mandatory;
- component is mandatory and bounded;
- a deterministic component ownership identity is formed from framework + source file + component name; source line/column remain location evidence and do not split one component into separate ownership identities across JSX lines;
- generic `data-source` still never becomes component ownership;
- explicit `data-component-source` retains its existing authority unchanged.

This keeps framework evidence inside the existing `SourceLocation` and progressive-targeting path without widening the protocol with an unrelated ownership model.

## Precedence

Source-hint precedence is:

1. explicit `data-component-source`;
2. explicit `data-source`;
3. bounded React development-fiber evidence;
4. no source hint.

React runtime evidence must never overwrite an explicit application source declaration.

## Browser proof

A real Chromium + React development fixture must prove:

1. LocalView bootstrap is injected before the React app;
2. the repository's React 19.2.8 development runtime can produce `react-dev-fiber` ownership through bounded `_debugStack` evidence (while the adapter keeps React <=18 `_debugSource` compatibility);
3. the retained hint contains only bounded file/line/column/component/signal data;
4. no props/state/source contents appear in the semantic snapshot JSON;
5. an explicit `data-component-source` on the same node overrides React introspection;
6. a plain non-React DOM node produces no React ownership;
7. invalid/synthetic React-shaped properties that fail the `stateNode === element` anchor produce no ownership.

If a future React runtime exposes neither an admissible bounded `_debugSource` nor an admissible same-origin `_debugStack`, the adapter must return no ownership rather than fabricate it. React private fields remain version-sensitive evidence, never a guaranteed public API.

## Verification gates

### Gate 1 — instrumentation contract

Prove generated bootstrap contains:

- bounded React property prefixes;
- paired fiber/props host marker with matching suffix, without reading props;
- exact host `stateNode` anchor;
- bounded key/depth/probe limits;
- bounded React 19 debug-stack bytes/lines;
- same-origin debug-stack source fencing;
- explicit-source precedence;
- `react-dev-fiber` output;
- no React mutation hook.

### Gate 2 — fresh snapshot projection

Prove:

- valid React source hint projects to bounded component ownership;
- missing/invalid component metadata fails closed;
- generic `data-source` remains non-component;
- explicit `data-component-source` behavior is unchanged.

### Gate 3 — existing progressive targeting

Preserve existing progressive target tests so React ownership cannot weaken explicit-source component resolution or silently widen missing ownership.

### Gate 4 — browser runtime

Run the real Chromium + React development fixture described above.

### Gate 5 — repository regression

Minimum exact-head closure:

```text
cargo fmt --check
cargo test -p localview-instrumentation
cargo test -p localview-control --lib fresh_snapshot
cargo test -p localview-control --test fresh_semantic_snapshot
cargo test -p localview-capture --test progressive_target_resolution
cargo check -p localview-control --all-targets
cargo check --workspace --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- React production/minified builds always expose ownership metadata;
- React Server Component ownership;
- arbitrary source-map lookup or remote/bundled source reconstruction from React ownership itself;
- Vue/Svelte ownership;
- CSS ownership;
- component props/state inspection;
- hook inspection;
- component tree replay;
- root-cause proof;
- stable public React internals;
- arbitrary framework DevTools replacement.

React fiber fields used here are private runtime evidence and therefore remain fail-closed and version-sensitive by design.

## Completion definition

The slice is complete when a LocalView-managed React development page can attach bounded, privacy-safe, read-only component ownership evidence to semantic nodes only when the host fiber and development source metadata are strongly validated, while explicit application source hints remain authoritative and unsupported/ambiguous React runtime shapes produce no fabricated ownership.
