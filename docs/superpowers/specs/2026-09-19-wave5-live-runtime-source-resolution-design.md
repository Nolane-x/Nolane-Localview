# Wave 5 Live Runtime Source Resolution — Engineering Design

## Status

Canonical implementation specification for the first live Source Map consumer wiring slice.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Stacked base: `feat/wave5-source-map-consumer@e8862fd3c68876383104ea9f3e7796d64de59707`

Main lineage: `main@a73d4b71fb07ba8a7bf25e82027cf4bf493c71c8`

Branch: `feat/wave5-live-runtime-source-resolution`

## Goal

Resolve generated positions from already-retained exact-session runtime error observations back to original source locations through bounded Source Map v3 discovery and the `localview-source-map` consumer.

This slice must not accept caller-supplied source URLs, source-map URLs, map JSON, generated source text or source positions. All resolution inputs come from retained LocalView runtime observations for the exact authenticated session.

## Existing authority

Already landed or supplied by the stacked parent:

- exact session `Endpoint` and project identity;
- bounded retained `ObserverEventKind::RuntimeError` history;
- runtime `error` events with `source`, `line` and `column` metadata;
- instrumentation `safeUrl` secret redaction and bounded URL strings;
- authenticated exact-session control plane;
- bounded ordinary Source Map v3 parser and generated-position resolver;
- hard Source Map JSON/mappings/source/name/segment/coordinate limits;
- query/fragment stripping before original source references are retained.

## Public surface

Add authenticated read-only endpoint:

`GET /v1/sessions/{id}/source/runtime-errors`

The caller provides only the session ID in the path.

No request body or query parameters are accepted as source-resolution authority.

The response contains at most 8 bounded resolution records derived from the newest retained runtime errors.

## Candidate runtime errors

Read at most the existing bounded 2,048-event session observer window and select only `ObserverEventKind::RuntimeError` events that contain:

- a non-empty `source` URL;
- positive `line`;
- positive `column`.

Unusable/malformed events become bounded unresolved records or are omitted according to deterministic policy; they never create a guessed source location.

Browser runtime error line numbers are passed to the Source Map API as 1-based generated lines. Runtime error columns are converted to zero-based generated columns by subtracting one; a zero input column is invalid.

## Exact-origin source authority

The generated source URL must:

- parse as HTTP(S);
- contain no username/password;
- use the exact session endpoint scheme;
- use the exact session endpoint host (case-insensitive for domain text);
- use the exact session endpoint port;
- remain loopback;
- contain no LocalView `[REDACTED]` marker in a query value;
- have a script/module-like path extension from a fixed allowlist:
  `.js`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.jsx`.

A source URL outside that authority is never fetched.

No localhost alias widening is performed: a session owned as `127.0.0.1` does not authorize `localhost`, another 127/8 address or `::1`.

## HTTP client policy

Runtime source/map discovery uses a dedicated reqwest client with:

- no proxy;
- redirects disabled;
- no cookie store;
- no Authorization or application headers;
- GET only;
- 1.5-second request timeout;
- exact-origin validation before every request.

The client does not inherit browser credentials.

## Bounded generated-source fetch

Generated source response requirements:

- 2xx status only;
- optional `Content-Length` over 2 MiB rejects before body read;
- chunked/body streaming is accumulated only while the total remains <= 2 MiB;
- exceeding the cap aborts immediately;
- no generated source body is persisted or inserted as evidence.

If a valid Source Map HTTP header exists, it has precedence over body `sourceMappingURL` discovery and the generated body need not be retained after the header is obtained.

Support both `SourceMap` and legacy `X-SourceMap`, preferring `SourceMap`.

## sourceMappingURL discovery

When no Source Map header exists, inspect only the last 64 KiB of the bounded generated source text.

Recognize line-comment annotations:

- `//# sourceMappingURL=...`
- `//@ sourceMappingURL=...`

The annotation must be a standalone trimmed line comment rather than an arbitrary string occurrence.

External references are bounded to 2,048 UTF-8 bytes.

## External map authority

An external Source Map reference is resolved relative to the generated source URL.

Before fetch it must satisfy the same exact-session origin policy as the generated source URL, except the path extension need not be script-like.

Additional rules:

- username/password forbidden;
- fragment removed before request;
- LocalView `[REDACTED]` ambiguity rejected;
- redirects disabled;
- map body capped at 2 MiB with chunked streaming;
- map response need only be a successful bounded body; MIME type is not trusted as map validity.

The Source Map v3 parser remains the semantic validity authority.

## Inline map authority

Support `data:application/json...;base64,...` sourceMappingURL forms.

Requirements:

- base64 marker required;
- decoded bytes capped at 2 MiB;
- UTF-8 required;
- decoded JSON is passed directly to the bounded Source Map v3 parser;
- percent-encoded/non-base64 data URLs are not supported in this slice;
- decoded map text is never persisted.

## Resolution response

Each successful resolution returns only bounded metadata:

- observer sequence;
- generated source URL sanitized for retention;
- generated line;
- generated zero-based column;
- original source reference;
- original 1-based line;
- original zero-based column;
- optional bounded name;
- discovery method: `source_map_header`, `x_source_map_header`, `external_annotation` or `inline_annotation`.

The response never returns:

- generated JS/TS body;
- Source Map JSON;
- `sourcesContent`;
- raw HTTP headers;
- cookies;
- auth values;
- runtime error message;
- arbitrary application payload.

## Failure semantics

Resolution is observational and best-effort per retained runtime error.

A failure for one candidate must not prevent bounded results for other candidates.

Use a fixed bounded reason enum for unresolved candidates, such as:

- `invalid_runtime_position`;
- `source_outside_session_origin`;
- `source_fetch_failed`;
- `source_too_large`;
- `map_reference_missing`;
- `map_reference_invalid`;
- `map_outside_session_origin`;
- `map_fetch_failed`;
- `map_too_large`;
- `map_invalid`;
- `position_unmapped`.

Do not place network/library error strings in the public response.

## Resource bounds

- retained observer scan: <= 2,048 events;
- runtime error candidates attempted: <= 8;
- generated source: <= 2 MiB each;
- source tail scanned for annotation: <= 64 KiB;
- external map reference: <= 2,048 bytes;
- map body / decoded inline map: <= 2 MiB;
- request timeout: 1.5 seconds each;
- no cache/history introduced in this slice;
- no concurrent fan-out: resolve candidates sequentially so one request cannot create unbounded socket concurrency.

## Verification gates

### Gate 1 — pure URL/sourceMappingURL policy

Prove:

- exact session origin acceptance;
- alias/cross-port/cross-scheme/cross-host rejection;
- credentials rejected;
- redacted-query ambiguity rejected;
- script extension allowlist;
- SourceMap header precedence;
- external annotation resolution;
- standalone-comment requirement.

### Gate 2 — bounded HTTP fixture

Use a real local HTTP fixture to prove:

- successful generated-source + external map resolution;
- chunked source/map bodies respect hard caps;
- oversized body rejection;
- redirect rejection;
- map cannot escape exact session origin;
- no browser cookie/auth header is sent.

### Gate 3 — inline map

Prove bounded base64 inline map decoding and successful resolution; malformed/non-base64/oversized data maps fail closed.

### Gate 4 — exact-session control authority

Prove:

- auth required;
- known session required;
- caller has no URL/map/body authority;
- only retained RuntimeError events are candidates;
- at most 8 attempts/results;
- unrelated session events cannot influence output;
- runtime error messages/raw payload markers do not escape response.

### Gate 5 — regression

Minimum focused gate:

```text
cargo test -p localview-source-map
cargo test -p localview-control --test runtime_source_resolution
cargo check -p localview-control --all-targets
```

Full repository CI is required on the eventual PR exact head.

## Explicit non-claims

This slice does not claim:

- DOM element/component ownership;
- React/Vue/Svelte fiber/component attribution;
- CSS declaration/specificity mapping;
- arbitrary remote source-map fetching;
- browser credential reuse;
- source-map persistence/cache;
- percent-encoded inline data maps;
- indexed/sectioned maps;
- source-map chaining;
- source-file mutation;
- complete root-cause proof.

## Completion definition

This slice is complete when an authenticated LocalView session can take its own retained browser runtime-error generated position, discover a source map only through bounded exact-origin authority, and return a bounded original source location without caller-controlled fetch targets, browser credentials, raw generated code, map JSON or source contents escaping the transaction.
