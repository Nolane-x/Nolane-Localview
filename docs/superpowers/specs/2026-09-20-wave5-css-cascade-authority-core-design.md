# Wave 5 CSS Cascade Authority Core — Engineering Design

## Status

Canonical fail-closed core for resolving a winner **inside one bounded author-origin CSS domain**.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@0fd77c0a31226597500761e8d556b61156f77e4d`

Branch: `feat/wave5-css-cascade-authority-core`

## Motivation

The existing `localview-inspector::winning_css_cause` is a useful heuristic seed, but it is not browser cascade authority. It currently compares only:

```text
important -> CssOrigin enum -> specificity
```

and therefore cannot by itself prove the declaration a browser applies.

The CSS cascade additionally depends on relevance, origin/importance, encapsulation context, style attribute precedence, layers, specificity, scope proximity and order of appearance. Dynamic transition/animation origins also sit outside ordinary author declarations.

Reference:
- CSS Cascading and Inheritance Level 6, §2.1 Cascade Sorting Order:
  https://www.w3.org/TR/css-cascade-6/#cascading-origins
- Level 6 is an exploratory Working Draft and explicitly directs implementers to Level 5 for stable implementation reference:
  https://www.w3.org/TR/css-cascade-6/

## Goal

Add an additive resolver that can truthfully answer:

> Which declaration wins **within the supplied author-origin, unlayered, unscoped, single-encapsulation-context candidate set**?

It must refuse to answer when those domain facts are not proven.

It does not replace the existing heuristic API in this slice.

## Candidate model

```text
AuthorCascadeCandidate {
  property,
  value,
  selector?,
  origin: Stylesheet | Inline,
  specificity: (id, class, type),
  important,
  source_order,
  source?,
  active
}
```

This representation intentionally cannot express:
- user-agent origin;
- user origin;
- animation origin;
- transition origin.

Therefore the resolver result is an **author-origin winner**, not the final browser-computed winner.

## Required domain proof

```text
AuthorCascadeDomainProof {
  relevance_proven,
  single_encapsulation_context,
  unlayered_only,
  unscoped_only
}
```

All fields must be true.

If any field is false, resolution returns `UnsupportedDomain`.

### Why each proof is required

- `relevance_proven`: inactive media/supports/container conditions must not compete.
- `single_encapsulation_context`: Shadow DOM/context precedence differs for normal and important declarations.
- `unlayered_only`: cascade layers outrank specificity and reverse order for important declarations.
- `unscoped_only`: scope proximity participates after specificity.

## Ordering inside the admitted domain

For active candidates matching one property:

1. `!important` beats normal.
2. Within equal importance, inline style beats stylesheet rule.
3. Within equal origin kind, higher specificity wins.
4. If specificity is equal, later `source_order` wins.

This ordering is valid only because the domain proof has already excluded layers, scopes and cross-context comparison.

## Explicit non-claims

This core does not claim:

- final browser-computed winner;
- user-agent or user stylesheet precedence;
- animation/transition precedence;
- cascade-layer ordering;
- `@scope` proximity;
- Shadow DOM cross-context resolution;
- selector specificity parsing;
- selector matching;
- inheritance/value computation;
- `revert` / `revert-layer` semantic execution;
- source-map correlation;
- CSS source line/column;
- root-cause proof.

Those require additional evidence-producing slices.

## Integration path

The bounded CSS declaration trace slice (#177) is expected to supply matched author declarations.

A later adapter must separately prove:
- selector branch + specificity;
- source order;
- relevance;
- no layer/scope/context ambiguity.

Only then may it call `resolve_unlayered_author_winner`.

A final browser winner requires a separate reconciliation step against dynamic/user/user-agent authority or equivalent browser evidence.

## Verification

Tests must prove:

- important stylesheet declaration beats normal inline author declaration;
- normal inline declaration beats arbitrarily specific normal stylesheet rule;
- source order is considered only after equal importance/origin/specificity;
- inactive candidates never win;
- missing relevance proof fails closed;
- multi-context uncertainty fails closed;
- layered uncertainty fails closed;
- scoped uncertainty fails closed.

## Completion definition

Complete when the inspector crate exposes an additive, deterministic and fail-closed author-origin resolver whose admitted ordering matches the bounded CSS cascade domain and whose type/API cannot be mistaken for a full browser cascade winner.
