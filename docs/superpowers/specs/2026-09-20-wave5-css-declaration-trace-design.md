# Wave 5 CSS Declaration Trace — Engineering Design

## Status

Canonical first slice for bounded CSS declaration evidence.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@0fd77c0a31226597500761e8d556b61156f77e4d`

Branch: `feat/wave5-css-declaration-trace`

Pull request: #177

## Goal

Let an agent ask why a bounded semantic element has its current layout/visual CSS values without turning every semantic snapshot into a stylesheet dump and without claiming a full browser cascade debugger.

The first slice connects:

```text
fresh element ref
  -> computed style packet
  -> bounded matching CSS declarations
  -> same-origin project-relative stylesheet identity when available
```

This evidence remains independent of `SourceLocation` and `ComponentOwnership`.

## Runtime collection

Instrumentation already computes a bounded style packet for selected semantic nodes. For the same selected nodes, add `styleTrace`.

Hard bounds:

- at most 96 stylesheets considered;
- at most 512 CSS rules visited;
- at most 12 retained declarations per element;
- selector <= 256 UTF-8 bytes;
- CSS value <= 256 UTF-8 bytes;
- stylesheet file <= 260 UTF-8 bytes;
- fixed CSS property allowlist only.

Inline element declarations are considered first.

Stylesheet rules are read only through CSSOM. A stylesheet whose `cssRules` throws (for example cross-origin authority) is skipped rather than bypassed.

Only selectors that successfully satisfy `element.matches(selector)` contribute declaration evidence.

Nested rule groups are traversed under the same global rule budget.

## Privacy and path authority

CSS declaration values replace every `url(...)` payload with `url(<redacted>)` before retention.

A stylesheet file identity is retained only when:

- `sheet.href` parses as a URL;
- origin equals the managed page origin;
- path is not `/@fs/`;
- path has no encoded-percent form;
- normalized identity is bounded project-relative syntax.

Inline stylesheet rules remain identifiable as `inline_stylesheet` but do not invent a source file.

No stylesheet text, source contents, remote URLs, cookies, tokens, absolute filesystem paths or arbitrary CSS custom properties are retained.

## Control surface

Add:

```text
GET /v1/sessions/{id}/style-trace/{reference}
```

The endpoint:

1. requires the normal local bearer token;
2. validates the element reference;
3. acquires a fresh Snapshot action result;
4. searches the bounded semantic tree for exactly one matching ref;
5. fails closed on duplicate refs or malformed/unbounded tree data;
6. validates computed properties and declaration evidence again in Rust;
7. returns only the selected element's trace.

This preserves progressive disclosure: CSS trace is requested for one element rather than serialized into the normal `PageSnapshot` protocol.

## Evidence schema

```text
CssStyleTrace {
  reference,
  computed: { property -> value },
  declarations: [
    {
      source_kind,
      file?,
      selector?,
      property,
      value,
      important
    }
  ]
}
```

Allowed `source_kind` values:

- `inline_element`
- `same_origin_stylesheet`
- `inline_stylesheet`

## Truth boundary

A returned declaration means:

> this bounded declaration belongs to an inline style or a CSS rule whose selector matches the element.

It does **not** yet prove that the declaration is the final cascade winner.

This first slice does not claim:

- full cascade ordering;
- specificity winner reconstruction;
- inheritance provenance;
- pseudo-element ownership;
- active media/container-query proof;
- CSS Layers ordering;
- declaration source line/column;
- Sass/Less source-map recovery;
- cross-origin stylesheet inspection;
- root-cause proof.

Those require a later cascade-authority slice rather than optimistic inference.

## Verification

Focused gates cover:

- bounded instrumentation constants and CSSOM path;
- separation from component ownership;
- URL-value redaction;
- same-origin and `/@fs/` path fencing;
- Rust projection of valid same-origin declaration evidence;
- traversal/unsafe file rejection;
- duplicate element-ref fail-closed behavior;
- control/instrumentation compile;
- full workspace check.

## Completion definition

This slice is complete when an authenticated client can request one fresh element ref and receive bounded computed CSS plus matching declaration evidence, while malformed, ambiguous, cross-authority and path-unsafe inputs fail closed and the existing semantic/component protocol remains unchanged.
