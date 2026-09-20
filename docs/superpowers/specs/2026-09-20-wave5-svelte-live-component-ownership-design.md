# Wave 5 Svelte Live Component Ownership — Engineering Design

## Status

Canonical implementation specification for the first Svelte-specific live ownership slice.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@665b9fdf1b678b57bbf3d553caac7eb91514b4ab`

Branch: `feat/wave5-svelte-live-ownership`

## Goal

Add bounded, privacy-safe Svelte development ownership evidence to LocalView-managed semantic snapshots without inventing ownership from DOM structure, without reading component props/state/context, and without treating an absolute compiler filename as safe project identity.

This slice follows the same fail-closed rule as the landed React adapter: framework runtime evidence is optional enrichment underneath explicit application-provided source declarations.

## Existing authority

Already landed:

- bounded fresh semantic snapshots with stable element refs;
- explicit `data-component-source` / `data-source` source hints;
- progressive component targeting through bounded `SourceLocation`;
- bounded React live ownership;
- bounded Source Map v3 parsing;
- project-owned Source Map loading/containment;
- trusted RuntimeError generated-position → project-source correlation.

Explicit application source hints remain higher authority than framework introspection.

## Upstream Svelte development evidence

Current Svelte development runtime assigns source metadata to generated DOM elements through an own `__svelte_meta` value whose bounded location shape is:

```text
{
  parent: DevStackEntry | null,
  loc: {
    file,
    line,
    column
  }
}
```

Current Svelte `DevStackEntry` carries:

```text
file
type = component | if | each | await | key | render
line
column
parent
componentTag?
```

Svelte source locations are real compiler-produced coordinates. The runtime metadata is still private development evidence, not a stable public API, so LocalView must fail closed when the shape is absent or ambiguous.

## Exact-element runtime observation

The adapter runs only after no explicit `data-component-source` or `data-source` hint exists and after React ownership produces no admissible result.

A Svelte candidate is accepted only when all of the following hold:

1. the exact DOM element has an **own** property descriptor named `__svelte_meta`;
2. that descriptor is a data property with its own `value`; accessors/getters are never invoked;
3. the value is a plain runtime object containing a bounded `loc` object;
4. `loc.file`, `loc.line` and `loc.column` pass the source rules below;
5. the normalized source file must end in `.svelte`;
6. a bounded component identity is derived only from that source file's basename.

Although current Svelte metadata also carries a `parent` development stack, LocalView deliberately does **not** traverse or retain it in this slice. Upstream Svelte runtime tests show root component elements may legitimately have `parent = null`, while nested parent entries describe block/call context rather than a universal owner record. Source-file identity is therefore the smaller and more reliable ownership authority.

The adapter never enumerates arbitrary Svelte runtime state, never reads `meta.parent`, and never installs a Svelte DevTools hook.

## Framework probe budget

Semantic snapshots share one framework-ownership probe budget:

- at most 256 marker-bearing framework probes per snapshot;
- React spends a probe only after an exact React host marker is found;
- Svelte spends a probe only after an own `__svelte_meta` data descriptor exists;
- invalid/fake marker shapes still consume the probe once identified;
- exhausting the budget yields no framework source hint for later nodes rather than failing the whole semantic snapshot.

This prevents one absent framework from consuming the other framework's budget on every ordinary DOM node.

## Privacy-safe source-file admission

This first Svelte slice accepts only already-project-relative compiler file identity.

A candidate Svelte file must:

- be UTF-8 bounded to 260 bytes after normalization;
- use slash-normalized separators;
- not begin with `/` or `//`;
- not contain a Windows drive prefix such as `C:/`;
- not contain a URI scheme;
- not contain `%`, `?` or `#`;
- not contain an explicit `..` path segment;
- drop empty and `.` path segments deterministically;
- remain non-empty after normalization.

This deliberately rejects the absolute filesystem filename commonly supplied by default Vite/Svelte compilation. Instrumentation does not know the backend-owned project root and therefore must not strip an absolute prefix heuristically. Project-root canonicalization for absolute Svelte compiler filenames is a later backend-owned slice.

## Coordinate rules

Accepted element source coordinates:

- line: integer in `1..=1_000_000`;
- column: integer in `0..=10_000_000`.

The zero column is intentional: Svelte development metadata uses zero-based source columns while lines are positive.

No synthetic line or column may be created.

## Component identity

A valid Svelte element source file must end in `.svelte`. The component identity is the basename of that same normalized source file with the `.svelte` extension removed.

This deliberately avoids `meta.parent` and `componentTag`: a root component can have no parent stack, while nested component entries represent invocation/block context. The source file that Svelte itself attached to the exact element is the direct source-backed authority.

The component name is capped at 96 UTF-8 bytes and must not contain control characters.

Retained instrumentation payload:

```text
origin = "svelte-dev-meta"
file
line
column
component
signal = "element_meta"
```

The adapter does not retain:

- props;
- state;
- context;
- reactive values/signals;
- component instances;
- the Svelte parent/dev stack;
- rendered text as ownership evidence;
- source contents;
- absolute project paths.

## Source precedence

Source-hint precedence becomes:

1. explicit `data-component-source`;
2. explicit `data-source`;
3. bounded React ownership;
4. bounded Svelte ownership;
5. no source hint.

Framework evidence never overwrites explicit application-provided declarations.

## Fresh snapshot projection

`fresh_snapshot::project_source` admits `svelte-dev-meta` as a bounded framework origin.

For Svelte:

- file is mandatory and already privacy-safe project-relative;
- line must be positive;
- column is mandatory and may be zero;
- component is mandatory and bounded;
- component ownership identity is deterministic:
  `svelte:<file>:<component>`;
- source line/column remain location evidence and do not split one component into multiple identities.

The existing protocol remains unchanged: Svelte can safely reuse `SourceLocation` because the runtime provides true coordinates.

## Browser proof

The focused browser proof uses the real Svelte compiler/runtime, not synthetic Svelte-shaped markers.

The job installs a pinned Svelte 5 development runtime, compiles a real `.svelte` component with:

- `dev: true`;
- a deliberately project-relative compiler filename;
- real client generation;

then serves the generated module under Vite, injects the exact LocalView bootstrap before application startup and mounts the component in Chromium.

It proves:

1. the real rendered Svelte element carries admissible `svelte-dev-meta` evidence;
2. retained file/line/column are compiler-produced;
3. component identity is bounded and source-backed;
4. a secret component prop is absent from the semantic snapshot;
5. explicit `data-component-source` outranks Svelte introspection;
6. plain DOM does not fabricate Svelte ownership;
7. an accessor-backed fake `__svelte_meta` is never invoked;
8. absolute/traversal-like/non-`.svelte` fake source files fail closed;
9. root-component metadata with `parent = null` remains valid because LocalView does not depend on the parent stack.

## Verification gates

### Gate 1 — instrumentation contract

Prove:

- one shared 256 framework marker budget;
- exact own-data-descriptor Svelte access;
- no getter invocation;
- no Svelte parent-stack traversal;
- relative-path privacy fencing plus mandatory `.svelte` identity;
- real line/zero-based-column validation;
- component identity derived only from the exact element's source file;
- explicit-source and React precedence;
- no Svelte state/props/context collection.

### Gate 2 — fresh projection

Prove:

- valid Svelte ownership projects to `SourceLocation`;
- zero-based column 0 remains valid;
- component identity is stable across multiple element lines;
- missing component, invalid line/column or malformed origin fails closed;
- existing explicit and React projection remains unchanged.

### Gate 3 — progressive targeting

Preserve existing progressive component resolution; Svelte ownership reaches that path only through the same bounded `SourceLocation.component` contract.

### Gate 4 — real browser runtime

Run the pinned real Svelte 5 compiler/runtime + Vite + Chromium proof described above.

### Gate 5 — repository regression

Minimum exact-head closure:

```text
cargo fmt --check (slice files)
cargo test -p localview-instrumentation
cargo test -p localview-control --lib
cargo test -p localview-control --test fresh_semantic_snapshot
cargo test -p localview-capture --test progressive_target_resolution
cargo check -p localview-control --all-targets
cargo check --workspace --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- Svelte production builds always expose `__svelte_meta`;
- arbitrary future Svelte private runtime shapes are compatible;
- default Vite absolute compiler filenames can be safely converted inside page instrumentation;
- backend project-root canonicalization of Svelte absolute filenames;
- Svelte props/state/context/reactivity inspection;
- Vue component ownership;
- CSS declaration ownership;
- source editing or source-content return;
- component replay;
- DevTools replacement;
- root-cause proof.

## Completion definition

The slice is complete when a real Svelte 5 development component can attach bounded read-only source/component ownership to its exact semantic node only from admissible compiler/runtime metadata, with true coordinates, exact source-file-backed component identity, explicit-source precedence, no parent-stack or secret/runtime-state retention, deterministic hard bounds and fail-closed behavior for absolute, malformed, synthetic or ambiguous metadata.
