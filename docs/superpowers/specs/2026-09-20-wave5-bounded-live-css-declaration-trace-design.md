# Wave 5 Bounded Live CSS Declaration Trace — Engineering Design

## Status

Canonical design for exact-element live CSS declaration/specificity evidence.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@93b86ee77df90cf4ba3c1360cdbf25e1fe03fc08`

Branch: `feat/wave5-css-inspect-trace`

## Goal

Expose bounded, privacy-safe matched author CSS declaration evidence for one exact LocalView stable element reference without inflating ordinary semantic snapshots and without pretending LocalView has a complete CSS Cascade Level 6 winner model.

## Authority

Add a dedicated authenticated exact-session endpoint:

`GET /v1/sessions/{id}/css/inspect/{reference}`

The caller provides only the exact session and bounded stable `@e...` reference.

The control plane:
1. authenticates the caller;
2. verifies the exact session;
3. validates the stable reference;
4. enqueues a dedicated read-only `CssInspect` bridge action;
5. waits only for the matching action result;
6. parses the result into a fixed bounded schema;
7. reserializes that schema, dropping arbitrary page payload.

The generic public `/actions` endpoint MUST continue to reject `CssInspect`; only the dedicated CSS endpoint may enqueue it.

## Page-side evidence

`window.__LOCALVIEW__.inspectCss(reference)` resolves the exact existing stable reference and returns:

- fixed-property computed values using the existing style-property allowlist;
- inline declarations for allowlisted properties;
- matched declarations from readable author `CSSStyleRule` objects;
- the exact matched selector branch, bounded and redacted;
- `!important` flag;
- conservative selector specificity when it can be proven by the bounded parser;
- stylesheet/rule ordinal only, not stylesheet URL/path;
- scan counts, inaccessible stylesheet count and explicit truncation.

## Bounds

- max stylesheets: 32;
- max CSS rules scanned: 2,048;
- max matched selector branches: 128;
- max retained declarations: 256;
- selector: 512 UTF-8 bytes;
- property: only the existing fixed `STYLE_PROPERTIES` allowlist;
- value: 256 UTF-8 bytes after existing secret redaction;
- nested grouping depth: 8.

Cross-origin/inaccessible `cssRules` are skipped and counted; they do not fail the inspection.

## Selector specificity

The first slice reports exact specificity only for the bounded supported selector grammar.

Supported:
- type/universal selectors;
- ID, class and attribute selectors;
- ordinary pseudo-classes;
- pseudo-elements;
- `:where()` as zero specificity;
- `:is()`, `:not()`, `:has()` using the maximum supported argument specificity.

If parsing is ambiguous/unsupported, `specificity = null`.

LocalView does NOT infer a cascade winner from specificity alone.

## Conditions and cascade truth boundary

Matched rules inside inactive media conditions are excluded where `matchMedia(conditionText)` is available.

The packet reports declarations and the browser-computed final property value separately.

This slice does NOT claim:
- complete cascade ordering across layers, scopes, animations/transitions, shadow trees or UA/user origins;
- exact source line/file for a CSS rule;
- cross-origin stylesheet introspection;
- CSS source-map resolution;
- that the highest specificity retained declaration is the actual winner.

## Privacy

Never retain:
- stylesheet href/path;
- full cssText;
- arbitrary custom properties;
- stylesheet source content;
- cross-origin CSS rules;
- DOM text;
- cookies/storage;
- raw page payload beyond the parsed schema.

Selectors and values pass through existing redaction and byte caps.

## Verification

Required:
- instrumentation source contract;
- real Chromium fixture proving matched declarations, `!important`, computed value, specificity and inline evidence;
- cross-origin CSSOM remains inaccessible and does not leak href/content;
- unsupported selector specificity degrades to null without dropping valid declaration evidence;
- dedicated control endpoint auth/session/reference fencing;
- exact action-result correlation;
- raw extra page fields/secret markers are absent from control response;
- generic public action endpoint rejects `CssInspect`;
- workspace/full repository regression green.

## Completion definition

Complete when one exact stable element can be inspected through the authenticated control plane and yields bounded matched declaration evidence plus browser-computed values, with no raw CSS source/href leakage and no overclaim of complete cascade winner authority.
