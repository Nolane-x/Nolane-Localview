# Wave 5 CSS Author Cascade Authority — Engineering Design

## Status

Canonical design for a bounded, fail-closed author-cascade winner slice built on the landed CSS declaration trace.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@98672046ef00e44d463eaa5b5422a4efa9a7ccce`

Branch: `feat/wave5-css-author-cascade`

## Goal

Extend LocalView from:

```text
element -> computed style -> selector-matching declaration evidence
```

to a narrowly truthful:

```text
element -> active author rules -> bounded specificity/source-order comparison
        -> proven author-cascade winner for a safe property subset
```

without claiming to reproduce the entire browser cascade.

The authority is deliberately narrower than "final computed CSS winner". It proves only the winning **observable author declaration** inside the supported domain described below.

## Immediate correctness repair

The first declaration-trace slice traversed nested CSS rules generically. That is insufficient because a selector can match while its enclosing `@media` or `@supports` condition is inactive.

This slice repairs that boundary:

- active `CSSMediaRule`: traverse only when `matchMedia(conditionText).matches`;
- active `CSSSupportsRule`: traverse only when `CSS.supports(conditionText)`;
- inactive media/supports blocks contribute no declaration evidence;
- unsupported conditional/cascade containers are not traversed as if active.

This repair applies to declaration evidence as well as winner authority.

## Supported cascade domain

Winner authority is emitted only for direct declarations of this conservative property set:

- `display`
- `position`
- `box-sizing`
- `z-index`
- `opacity`
- `pointer-events`
- `visibility`
- `color`

These properties were chosen because this slice can compare their direct author declarations without having to expand a common shorthand that can silently set the same property. `align-items` and `justify-content` remain ordinary trace properties but are intentionally excluded from winner authority because `place-items` and `place-content` can set them indirectly.

A property outside this set may still appear in ordinary declaration trace evidence, but it receives no cascade-winner claim.

## Stylesheet coverage

A cascade proof requires complete observable author-style coverage for the managed document.

Coverage is incomplete if any relevant stylesheet cannot be inspected safely, including:

- `cssRules` throws;
- linked stylesheet identity is cross-origin, `/@fs/`, encoded or otherwise outside the existing bounded path policy;
- an `@import` rule is encountered;
- an active nested at-rule with cascade semantics not supported by this slice is encountered, including cascade layers, container queries, scope and other unknown grouping rules.

When global coverage is incomplete, no author winner is emitted.

Inactive `@media` and `@supports` blocks do not make coverage incomplete because they cannot contribute to the current cascade.

Keyframes/font-face/page/property rules are not element selector declarations and are ignored for author winner comparison. This does not make any animation/transition claim.

## Selector authority

A selector rule is relevant only if an individual selector-list arm matches the exact element.

LocalView implements a bounded specificity parser for an intentionally small, auditable selector grammar:

- ASCII type/universal selectors;
- IDs;
- classes;
- attribute selectors as one class-specificity unit;
- non-functional pseudo-classes as one class-specificity unit;
- descendant and `>`, `+`, `~` combinators.

This slice rejects for winner authority:

- escaped identifiers;
- namespace selectors;
- pseudo-elements;
- functional pseudo-classes such as `:is()`, `:where()`, `:not()`, `:has()`, `:nth-child()`;
- any selector syntax outside the bounded grammar.

Selector lists are split with quote/bracket/parenthesis awareness so commas inside a functional selector or attribute value are not mistaken for list separators.

If an unsupported selector arm matches the element and declares a supported property, that property is marked unresolved rather than guessed.

## Specificity and ordering

For supported author declarations the comparison tuple is:

```text
important
inline-style specificity bit
id count
class/attribute/pseudo-class count
type count
source order
```

All supported stylesheet rules are unlayered author rules. Inline styles participate with the inline specificity bit set.

`!important` outranks normal author declarations. Specificity and then source order break ties.

Cascade layers are excluded because important declarations reverse layer precedence and a partial layer model would be unsafe.

## Result schema

The existing per-element style trace gains:

```text
author_cascade {
  scope: "supported_author_subset"
  coverage_complete: bool
  unresolved_properties: [property...]
  winners: [
    {
      source_kind,
      stylesheet_path?,
      selector?,
      property,
      value,
      important,
      specificity: [inline, id, class, type],
      source_order
    }
  ]
}
```

When `coverage_complete == false`, `winners` must be empty.

A winner is omitted for a property tainted by a matching unsupported selector.

## Bounds

Existing declaration-trace bounds remain authoritative:

- at most 96 stylesheets;
- at most 512 visited rules;
- at most 12 retained ordinary declaration records;
- bounded selector/value/path lengths.

Cascade comparison may inspect all visited rules under the 512-rule budget even after the 12 declaration-retention budget is full. The retention cap must never change the winner result.

Additional bounds:

- selector-list arms <= 32 per rule;
- selector specificity counters <= 255 each;
- unresolved property set <= supported property count;
- winner count <= supported property count;
- nested conditional depth <= 8.

Exhausting or exceeding a proof bound makes cascade coverage incomplete or the affected property unresolved.

## Privacy

The cascade layer adds no source text and no arbitrary stylesheet dump.

It inherits all CSS trace privacy fences:

- CSS `url(...)` payload redaction;
- no remote stylesheet contents;
- no absolute filesystem path;
- bounded same-origin stylesheet URL-path hint only;
- no cookies, tokens or page secrets.

## Non-claims

This slice does not claim:

- browser-final computed-style provenance;
- user-agent or user-origin cascade reconstruction;
- cascade layers;
- container queries;
- `@scope`;
- imported stylesheet authority;
- shadow-DOM/adoptedStyleSheets coverage;
- pseudo-element cascade;
- functional-selector specificity;
- CSS animations/transitions provenance;
- inheritance provenance when no direct declaration wins;
- shorthand expansion;
- custom-property dependency tracing;
- CSS source line/column;
- Sass/Less source-map recovery;
- root-cause proof.

## Verification

Focused tests must prove:

1. inactive media declarations are absent;
2. active media declarations are eligible;
3. inactive/active supports handling;
4. unsupported active grouping rules make coverage incomplete;
5. inaccessible/untrusted stylesheet coverage makes winners empty;
6. inline normal vs stylesheet normal ordering;
7. stylesheet important beats inline normal;
8. inline important beats stylesheet important;
9. ID/class/type specificity ordering;
10. later source order wins an equal-specificity tie;
11. a matching unsupported functional selector taints only its supported properties;
12. declaration retention capped at 12 does not truncate winner computation;
13. Rust projection rejects malformed specificity/source-order/property evidence;
14. fresh HTTP endpoint returns bounded author-cascade proof;
15. existing CSS trace, React/Svelte/Vue ownership and full workspace regressions remain green.

## Completion definition

Complete when LocalView can return a fresh per-element CSS trace whose author-cascade winner claims are correct inside this explicit supported domain, inactive conditional rules are excluded, unsupported cascade mechanisms fail closed, and exact-head focused plus full repository regression gates are green.
