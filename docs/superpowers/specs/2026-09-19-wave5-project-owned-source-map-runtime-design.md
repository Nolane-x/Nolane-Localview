# Wave 5 Project-Owned Source Map Runtime Authority — Engineering Design

## Status

Canonical implementation specification for the first project-owned Source Map runtime slice.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Base: `main@01553781b78023ef8fe6cd4b6be36bb0886c5d8f`

Branch: `feat/wave5-project-source-map-runtime`

## Goal

Connect the landed bounded Source Map v3 consumer to an authenticated exact-session runtime path without allowing callers to upload arbitrary map JSON or read arbitrary filesystem paths.

The control plane may resolve one generated position only from:

1. a project-relative generated file supplied by the caller;
2. that file's sibling `.map` file;
3. the exact session's backend-owned project root;
4. a resolved original source that canonicalizes to a regular file inside that same project root.

This slice establishes project-owned loading and containment authority. It does not yet claim automatic runtime-node discovery or framework component ownership.

## Existing authority

Already landed:

- exact-session authenticated control plane;
- backend-owned session project identity with `git_root` / `cwd`;
- bounded Source Map v3 parser/resolver in `localview-source-map`;
- hard JSON/mappings/source/name/segment/coordinate bounds;
- checked Base64 VLQ arithmetic;
- exact generated-line resolution with no previous-line fallback;
- normalized `sourceRoot` references while preserving `..` for later containment;
- no `sourcesContent` retention.

## Endpoint

Add authenticated:

`POST /v1/sessions/{id}/source-map/resolve`

Request:

```json
{
  "generated_file": "dist/assets/app.js",
  "generated_line": 12,
  "generated_column": 48
}
```

The caller cannot provide:

- map JSON;
- a map path;
- a project root;
- an original source path;
- a sourceRoot override;
- a filesystem escape policy.

## Project-root authority

Project root is selected server-side:

1. `Session.project.git_root` when present;
2. otherwise `Session.project.cwd`;
3. otherwise fail `project_root_unavailable`.

The root is canonicalized before use.

The caller's generated file must be non-empty, <= 1,024 UTF-8 bytes, relative, and contain no `..`, root, prefix, or other non-normal path escape component.

The generated file is canonicalized and must:

- remain beneath the canonical project root;
- exist;
- be a regular file.

## Map discovery

The only map discovery in this slice is deterministic sibling discovery:

`<generated-file-name>.map`

Examples:

- `dist/app.js` -> `dist/app.js.map`;
- `build/chunk.mjs` -> `build/chunk.mjs.map`.

No `sourceMappingURL` comment parsing is performed.
No remote URL is fetched.
No data URL is decoded.
No alternate map search path is accepted from the caller.

The map file is canonicalized independently so symlink escape cannot bypass project containment.

The file must be a regular file and <= 2 MiB before reading.

## Original-source containment

The bounded Source Map consumer returns a normalized source reference.

That reference is treated as untrusted until project containment succeeds.

Allowed forms:

- project-relative/path-relative source references, resolved relative to the sibling map directory;
- absolute local filesystem paths that canonicalize inside the exact project root;
- `file://` URLs that convert to local filesystem paths and canonicalize inside the project root.

Rejected forms include:

- `http://`, `https://`, `data:`, `webpack:`, and other non-file schemes;
- protocol-relative `//host/path` references;
- missing files;
- directories/non-regular files;
- symlink targets outside the project root;
- relative references escaping the project root after canonicalization.

The response exposes only the project-relative normalized source path, never the absolute project root.

## Position bounds

Require:

- generated line: 1..=1,000,000;
- generated column: 0..=10,000,000.

Out-of-range requests fail before any map resolution.

## Response

Successful response:

```json
{
  "generated_file": "dist/assets/app.js",
  "map_file": "dist/assets/app.js.map",
  "generated_line": 12,
  "generated_column": 48,
  "source": {
    "file": "src/App.tsx",
    "line": 7,
    "column": 3,
    "name": "render"
  }
}
```

No map contents, absolute path, `sourcesContent`, query, fragment, or arbitrary source payload is returned.

## Error semantics

Errors are bounded codes without absolute filesystem paths:

- `unauthorized`
- `session_not_found`
- `project_root_unavailable`
- `invalid_generated_file`
- `generated_file_unavailable`
- `source_map_unavailable`
- `source_map_too_large`
- `invalid_source_map`
- `generated_position_out_of_range`
- `unmapped_generated_position`
- `source_reference_unsupported`
- `source_outside_project`
- `source_unavailable`

## Verification gates

### Gate 1 — happy path

Prove a real temporary project with:

- generated JS file;
- sibling Source Map v3 file;
- original source beneath the project;

resolves through the authenticated router and returns only project-relative source identity.

### Gate 2 — auth/session/root

Prove:

- missing bearer token -> 401;
- unknown session -> 404;
- no backend-owned root -> fail closed.

### Gate 3 — filesystem containment

Prove:

- caller `..` traversal rejected before filesystem access;
- generated-file symlink outside root rejected;
- map-file symlink outside root rejected;
- resolved source `..` escape rejected;
- resolved source symlink outside root rejected;
- remote/protocol-relative source references rejected.

### Gate 4 — bounded map authority

Prove:

- missing sibling map rejected;
- oversized map rejected before parse;
- invalid Source Map rejected;
- unmapped generated position rejected;
- generated position bounds enforced.

### Gate 5 — repository regression

Minimum exact-head closure:

```text
cargo test -p localview-source-map
cargo test -p localview-control --test project_source_map_runtime
cargo check -p localview-control --all-targets
cargo check --workspace --all-targets
full repository CI
```

## Explicit non-claims

This slice does not claim:

- automatic `sourceMappingURL` discovery;
- remote map loading;
- inline/data-URL source maps;
- indexed/sectioned maps;
- source-map chaining;
- automatic runtime-node generated-position extraction;
- React/Vue/Svelte ownership;
- CSS source maps;
- source editing;
- source-code content return;
- source-root trust without canonical filesystem containment.

## Completion definition

The slice is complete when an authenticated exact LocalView session can resolve one bounded generated position through a sibling project-owned Source Map v3 file into one regular original source file inside the same canonical project root, while traversal, symlink escape, remote references and arbitrary caller-supplied map authority fail closed.
