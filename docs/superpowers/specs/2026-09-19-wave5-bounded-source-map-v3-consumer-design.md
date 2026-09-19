# Wave 5 Bounded Source Map V3 Consumer — Engineering Design

## Status

Canonical implementation specification for the first real Source Map v3 consumer slice in LocalView Source Intelligence.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Base: `main@a73d4b71fb07ba8a7bf25e82027cf4bf493c71c8`

Branch: `feat/wave5-source-map-consumer`

## Goal

Replace the current source-map crate's regex-only stack/source-hint primitive with a real, bounded, deterministic Source Map v3 decoder that can translate a generated line/column into one original source location without reading the filesystem, fetching remote maps, trusting caller-owned paths, or retaining arbitrary source content.

This slice is intentionally a **pure source-intelligence authority**. Wiring the consumer to live runtime nodes and project-owned map discovery is the next slice.

## Existing foundation

Already landed:

- live explicit `data-source` / `data-component-source` propagation into semantic nodes;
- fresh semantic snapshot projection of bounded `SourceLocation`;
- progressive component targeting that consumes only corroborated explicit component ownership;
- `localview-source-map` stack parsing and hint ranking primitives;
- `localview-source-graph` source-region and dependency graph primitives.

Missing:

- actual Source Map v3 `mappings` decoding;
- deterministic generated-position lookup;
- bounded source/name/string handling;
- malformed-map failure semantics;
- path/URL normalization rules for map sources.

## Supported input

A Source Map v3 JSON document with:

- `version = 3`;
- `sources: string[]`;
- optional `sourceRoot`;
- `mappings: string`;
- optional `names`;
- optional `file`.

This first slice does not require or expose `sourcesContent`.

Indexed/sectioned maps are rejected in this slice.

## Hard bounds

The decoder must enforce before or during parse:

- JSON document: max 2 MiB;
- `sources`: max 4,096 entries;
- `names`: max 8,192 entries;
- one source/name string: max 1,024 UTF-8 bytes;
- `mappings`: max 1 MiB;
- decoded mapping segments: max 250,000;
- generated line: max 1,000,000;
- generated/original column: max 10,000,000;
- original line: max 10,000,000.

All integer accumulation uses checked arithmetic. Overflow is an error, never wraparound.

## VLQ semantics

Implement standard Base64 VLQ decoding.

For every generated line:

- generated column resets to zero;
- source/original line/original column/name indices remain delta-carried according to Source Map v3 semantics.

A segment may contain:

- generated-column delta only: no original source binding;
- 4 fields: generated column, source index, original line, original column;
- 5 fields: same plus name index.

Any other field count fails the map.

Negative or out-of-range resolved indices fail the map.

## Lookup semantics

Public pure API:

`SourceMap::parse(json: &str) -> Result<SourceMap, SourceMapError>`

`SourceMap::resolve(generated_line: u32, generated_column: u32) -> Option<ResolvedSourceLocation>`

Coordinates exposed by the API are 1-based lines and 0-based columns.

Resolution is deterministic:

1. locate the exact generated line;
2. choose the last segment whose generated column is <= requested column;
3. if that segment is unmapped, return `None` rather than carrying an earlier mapping across an explicitly unmapped region;
4. otherwise return original source + 1-based original line + 0-based column + optional bounded name.

No nearest-line fallback is allowed. A line with no mapped source segment returns `None`.

## Source normalization

This pure consumer does **not** assert filesystem ownership.

It only canonicalizes the textual source reference enough for deterministic comparison:

- strip query and fragment metadata from `sourceRoot` and source references before retention;
- combine relative `sourceRoot` + relative source using slash joining;
- normalize backslashes to `/`;
- collapse `.` path components;
- preserve `..` components instead of silently escaping them;
- preserve absolute URL/path identity after query/fragment stripping;
- reject a normalized combined source reference that exceeds the same 1,024-byte hard string bound;
- never fetch URL content;
- never open files.

Filesystem containment beneath the LocalView project root belongs to the later live wiring slice.

## Privacy and security

The parsed runtime representation stores:

- normalized source reference;
- original line/column;
- optional symbol/name;
- generated line/column mapping table.

It does not retain:

- `sourcesContent`;
- arbitrary source code;
- source file bytes;
- network responses;
- query/fragment metadata or tokens embedded in source-map source URLs.

If `sourcesContent` exists in the JSON, it is ignored and not copied into the parsed structure.

## Verification gates

### Gate 1 — VLQ correctness

Tests cover:

- zero;
- positive and negative values;
- continuation digits;
- invalid base64 characters;
- truncated continuation sequences;
- checked overflow.

### Gate 2 — Source Map v3 decoding

Use deterministic known maps to prove:

- multiple generated lines;
- generated-column deltas;
- source/original coordinate deltas;
- optional name index;
- unmapped segments;
- sourceRoot joining;
- 1-based line conversion.

### Gate 3 — lookup semantics

Prove:

- last mapped segment <= requested column wins;
- no previous-line fallback;
- unmapped-only line returns `None`;
- out-of-range generated line returns `None`;
- duplicate/ordered segments remain deterministic.

### Gate 4 — fail-closed malformed input

Reject:

- version other than 3;
- sections/indexed maps;
- oversized JSON/mappings/source arrays;
- invalid segment field counts;
- negative resolved indices;
- out-of-range source/name index;
- integer overflow;
- malformed VLQ.

### Gate 5 — regression

Minimum:

```text
cargo test -p localview-source-map
cargo check -p localview-source-map --all-targets
```

Repository/full CI is required once the implementation PR is opened.

## Explicit non-claims

This slice does not claim:

- automatic `sourceMappingURL` discovery;
- filesystem map loading;
- remote map download;
- project-root containment;
- React/Vue/Svelte component ownership;
- CSS declaration mapping;
- live runtime-node correlation;
- source-code parsing;
- source-file mutation;
- indexed/sectioned source maps;
- source-map chaining.

## Completion definition

This slice is complete when LocalView can parse a bounded ordinary Source Map v3 document and deterministically translate one generated line/column into a bounded original source location using standards-compatible VLQ semantics, while malformed or oversized maps fail closed and source contents are never retained.
