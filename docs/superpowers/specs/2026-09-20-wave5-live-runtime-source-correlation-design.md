# Wave 5 Live Runtime-Position Source Correlation — Engineering Design

## Status

Canonical implementation specification for the first live runtime-position → project source correlation slice.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@93ebf3cc0d93befef5837f224ba01bb5eaf3844c`

Branch: `feat/wave5-live-runtime-source-correlation`

## Goal

Connect one retained RuntimeError observation from an exact LocalView session to the landed project-owned Source Map runtime without allowing the caller to provide generated file, line, column, map path, map JSON, project root, or original source identity.

This slice turns already-observed runtime position evidence into one bounded project-relative source resolution.

It does not claim framework component ownership or root-cause proof.

## Existing authority

Already landed:

- managed-page runtime-error observation with bounded `source`, `line` and `column` metadata;
- exact-session bounded observer history;
- authenticated control plane;
- bounded Source Map v3 consumer;
- project-owned sibling-map discovery;
- backend-owned project-root selection;
- canonical generated/map/source containment;
- remote/protocol-relative source rejection;
- project-relative-only source-map response.

## Endpoint

Add authenticated:

`POST /v1/sessions/{id}/runtime-source/resolve`

Request:

```json
{
  "event_seq": 42
}
```

The caller cannot submit:

- generated file;
- generated line;
- generated column;
- runtime URL;
- map path;
- map contents;
- source path;
- project root.

Unknown request fields are rejected.

## Observer authority

The daemon reads only the exact session's retained observer window.

The selected event must:

1. exist at the requested `event_seq`;
2. be `ObserverEventKind::RuntimeError`;
3. contain a bounded source URL;
4. contain a positive browser line number;
5. contain a positive browser column number.

Console, network, HMR, layout, semantic, and unrelated event kinds cannot satisfy the request.

No message text, stack body, route, query, token, or arbitrary observer payload is copied into the response.

## Runtime URL authority

The runtime source must parse as an HTTP(S) URL and must remain on the exact development server authority:

- host must be loopback: `localhost`, IPv4 loopback, or IPv6 loopback;
- scheme must match the exact session endpoint;
- effective port must match the exact session endpoint port;
- URL query and fragment are ignored for filesystem identity;
- percent-encoded path components are rejected in this first slice to avoid ambiguous decoded traversal/identity;
- the URL path is converted only to a project-relative generated-file candidate.

Remote CDN/runtime sources, mismatched ports and other schemes fail closed.

## Coordinate normalization

Browser `ErrorEvent.lineno` and `ErrorEvent.colno` are treated as 1-based.

Source Map v3 generated coordinates are:

- line: 1-based;
- column: 0-based.

Therefore:

`generated_column = browser_column - 1`

Zero/invalid/overflow coordinates fail before Source Map lookup.

## Reused project authority

The derived generated file/position is passed to the existing project-owned Source Map runtime.

This slice must not duplicate:

- project-root selection;
- path canonicalization;
- sibling-map discovery;
- map size checks;
- Source Map parsing;
- original-source containment.

One authority remains responsible for each invariant.

## Response

Successful response contains only:

- `event_seq`;
- the existing bounded project-owned Source Map resolution.

Example:

```json
{
  "event_seq": 42,
  "resolution": {
    "generated_file": "dist/app.js",
    "map_file": "dist/app.js.map",
    "generated_line": 12,
    "generated_column": 47,
    "source": {
      "file": "src/App.tsx",
      "line": 7,
      "column": 3,
      "name": "render"
    }
  }
}
```

No runtime message, route, raw URL query, stack, absolute filesystem path, `sourcesContent`, or source-code content is returned.

## Error semantics

Bounded runtime-correlation errors:

- `unauthorized`
- `session_not_found`
- `runtime_event_not_found`
- `runtime_event_kind_unsupported`
- `runtime_source_missing`
- `runtime_source_unsupported`
- `runtime_source_authority_mismatch`
- `runtime_position_invalid`

Once live runtime authority is established, existing bounded project-source-map error codes remain authoritative for project/map/source failures.

## Verification gates

### Gate 1 — live happy path

A real temporary project plus a retained RuntimeError observation must resolve through:

runtime event → same-server generated path/position → sibling Source Map → project-relative original source.

### Gate 2 — caller non-authority

Prove:

- request accepts only `event_seq`;
- unknown fields are rejected;
- caller cannot choose generated file/line/column.

### Gate 3 — session/event fencing

Prove:

- missing auth is rejected;
- unknown session is rejected;
- missing event is rejected;
- same sequence in another session cannot satisfy the target session;
- non-RuntimeError event is rejected.

### Gate 4 — runtime source fencing

Prove rejection of:

- remote host;
- session-port mismatch;
- scheme mismatch;
- percent-encoded path;
- missing/zero line;
- missing/zero column.

### Gate 5 — privacy

Inject marker strings into runtime message, route, stack and query.

Successful response must contain none of them.

### Gate 6 — repository regression

Minimum exact-head closure:

```text
cargo fmt --check
cargo test -p localview-control --test runtime_source_correlation
cargo test -p localview-control --test project_source_map_runtime
cargo test -p localview-source-map
cargo clippy -p localview-control --all-targets --no-deps -- -D warnings
cargo check -p localview-control --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- unhandled-rejection source discovery when no exact generated position exists;
- stack-frame fan-out or multi-frame ranking;
- remote/CDN source maps;
- `sourceMappingURL` discovery;
- inline/data URL maps;
- indexed maps;
- map chaining;
- React/Vue/Svelte component ownership;
- CSS source maps;
- source editing;
- source content return;
- root-cause proof;
- arbitrary browser DevTools stack inspection.

## Completion definition

The slice is complete when an authenticated caller can name only one retained RuntimeError sequence and LocalView independently derives and validates its same-server generated position, reuses the project-owned Source Map authority, and returns one bounded project-relative source resolution without leaking runtime payload text or accepting caller-authored source coordinates.
