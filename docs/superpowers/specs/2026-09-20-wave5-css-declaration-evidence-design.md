# Wave 5 Bounded CSS Declaration Evidence — Engineering Design

## Status

Canonical design for the first live CSS declaration-attribution slice.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `feat/wave5-vue-absolute-path-authority@1e4692af2409de618b4dd414d95c85bf6a4bbca7`

Branch: `feat/wave5-css-declaration-evidence`

## Goal

Let LocalView answer a narrow, evidence-backed question for one stable element reference:

> Which bounded author declarations directly match this element for the fixed LocalView style-property set, and what computed values does the browser currently expose?

This slice is deliberately **not** a full CSS cascade debugger. It establishes truthful live declaration evidence without inventing source coordinates, without reading arbitrary stylesheet text, and without claiming a winning rule when cascade/layer/inheritance authority has not yet been proven.

## Existing authority

Already available below this branch:

- stable LocalView element references;
- on-demand `inspect(reference)` resolution inside the exact managed page;
- bounded computed-style packets;
- read-only public Snapshot and Measure action authority;
- same-session action/result correlation and storage sanitization;
- project/source authority for explicit, React, Svelte and Vue ownership;
- `localview-inspector::CssCause` as an offline primitive, but no live CSSOM attribution path.

The existing `winning_css_cause` helper is not treated as browser cascade ground truth by this slice.

## Page-side evidence

Add an on-demand `inspectCss(reference)` capability. It must never run as part of every semantic snapshot.

The page-side collector may inspect only:

- the exact resolved element;
- its inline `CSSStyleDeclaration`;
- readable `document.styleSheets`;
- direct top-level `CSSStyleRule` entries;
- the fixed LocalView property allowlist.

Hard bounds:

- at most 64 stylesheets considered;
- at most 512 top-level rules considered across the document;
- at most 64 matched rules;
- at most 128 retained declarations;
- selector <= 320 UTF-8 bytes after redaction;
- value <= 180 UTF-8 bytes after redaction;
- stylesheet path <= 260 UTF-8 bytes;
- route <= 2,048 UTF-8 bytes.

Unreadable/cross-origin stylesheet rule lists are never bypassed; they increment an aggregate `opaque_stylesheets` count only.

## Fixed property allowlist

The first slice covers the existing bounded visual/layout family rather than arbitrary custom properties:

`display`, `position`, `overflow-x`, `overflow-y`, `box-sizing`, `z-index`,
`flex-direction`, `flex-wrap`, `justify-content`, `align-items`, `gap`,
`row-gap`, `column-gap`, `grid-template-columns`, `grid-template-rows`,
padding/margin/border widths, `font-size`, `font-weight`, `font-family`,
`line-height`, `color`, `background-color`, `opacity`,
`pointer-events`, and `visibility`.

No CSS custom property enumeration is allowed in this slice.

## Declaration record

Each retained declaration contains only:

```text
property
value
important
origin = inline | author_stylesheet
selector?              # absent for inline
stylesheet_path?       # same-origin URL pathname only
rule_index?            # bounded top-level CSSOM index
```

The selector is evidence, not source code authority. It is redacted and bounded.

For a stylesheet URL:

- parse against the current document URL;
- require exact same origin;
- remove query and fragment;
- retain pathname only;
- never retain credentials, host, query tokens or fragment data.

A stylesheet with no safe same-origin pathname may still contribute a matched declaration with `stylesheet_path = null`; this does not become source-file authority.

## Computed values

Return a bounded computed map for the same fixed allowlist.

Computed values are browser-observed final values. They are not proof that a particular retained declaration won the cascade.

## Cascade honesty

This slice does **not** calculate or claim:

- numeric selector specificity;
- cascade layer order;
- animation/transition precedence;
- author/user/user-agent origin ordering;
- inheritance provenance;
- `:is()`, `:where()`, `:not()`, `:has()` specificity semantics;
- a winning declaration;
- a CSS source line/column.

Rules nested under conditional group rules are not traversed in this first slice. Their absence is represented by `conditional_rules_omitted = true` when such groups are observed.

A later slice may add a proven specificity/cascade model or Tier-3 DevTools-backed exact source coordinates.

## Control authority

Add a read-only `BridgeActionKind::StyleInspect`.

Public queue policy:

- requires one valid LocalView `@e...` reference;
- is admitted beside Snapshot and Measure as read-only;
- remains session-scoped;
- cannot modify the page.

The managed preview executes only `window.__LOCALVIEW__.inspectCss(reference)`.

Result storage must fail closed through a dedicated sanitizer. Raw page payload is never trusted merely because instrumentation produced it.

## Backend sanitization

The live bridge validates and reconstructs the retained payload:

- exact action/reference match;
- loopback HTTP(S) route, query/fragment removed;
- bounded computed property/value map using the fixed allowlist;
- bounded declarations using the same allowlist;
- known origin enum only;
- booleans and bounded non-negative counters only;
- selector/value/path length and control-character bounds;
- same-origin path is path-only, never a URL;
- unknown fields are discarded;
- failed/malformed result becomes `ok = false`, `payload = null`, generic error.

The control plane stores the sanitized style result as Interaction evidence and, on success, also stores a bounded `EvidenceKind::Source` object with uncertainty `Observed`. This means “source-related runtime evidence”, not proven source coordinate.

## Verification

Required gates:

1. instrumentation contract proves the on-demand CSS collector and hard bounds exist;
2. real browser fixture proves inline + same-origin stylesheet matching;
3. unreadable stylesheet access is fail-closed/aggregate-only;
4. query/fragment never survive stylesheet path projection;
5. CSS custom properties are not enumerated;
6. conditional-group rules are not silently presented as complete coverage;
7. public control queue requires a valid element ref and admits StyleInspect as read-only;
8. live-bridge sanitizer rejects wrong ref, remote route, unknown property/origin, oversize selector/value/path and excess declarations;
9. exact successful payload retains only bounded fields;
10. existing Snapshot/Measure/consequential-action authority remains unchanged;
11. full workspace and repository regression remain green.

## Explicit non-claims

This slice does not claim:

- complete CSS cascade reconstruction;
- specificity;
- cascade layers;
- inheritance provenance;
- pseudo-element rules;
- Shadow DOM stylesheet ownership;
- adoptedStyleSheets ownership;
- exact stylesheet source line/column;
- CSS source-map resolution;
- source editing;
- root-cause proof.

## Completion definition

Complete when one exact LocalView element can request bounded live CSS declaration evidence through the authenticated read-only action path, the managed page returns fixed-property inline/top-level same-origin CSSOM matches, the live bridge reconstructs a privacy-safe bounded result, and the control plane retains that sanitized result without any claim of cascade winner or exact CSS source coordinates.
