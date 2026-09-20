# Wave 5 CSS Source Coordinate Authority — Engineering Design

## Status

Canonical bounded source-correlation lane for CSS declaration evidence.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@154f810d04628f6687d228d5e005962559abade3`

Branch: `feat/wave5-css-source-coordinates`

## Goal

Upgrade existing fresh CSS declaration/cascade evidence from a runtime stylesheet hint to project-owned source identity and an exact declaration coordinate only when the backend can prove it.

This lane does **not** rebuild CSS cascade authority and does not convert an author winner into browser-final computed provenance.

The intended chain is:

```text
fresh style trace
  -> bounded declaration evidence
  -> exact-session project root
  -> canonical project-contained stylesheet
  -> bounded CSS parse
  -> unique declaration match
  -> project-relative file + exact declaration property position
  -> optional project-owned Source Map v3 remap
```

## Authority model

Every projected declaration and author-cascade winner carries a separate `source_authority` object.

Levels:

- `unresolved`: no safe runtime/file identity is available.
- `stylesheet_hint`: same-origin bounded stylesheet path is runtime evidence only; no filesystem ownership is claimed.
- `project_file_verified`: backend canonicalized the target from the exact session project root, proved containment, proved it is a regular file, and emits only a project-relative identity.
- `exact_declaration_position`: a unique bounded declaration match was proven. `file`, `line`, and `column` are present.
- `runtime_inline`: orthogonal inline authority for `style=""` and inline stylesheet evidence. It intentionally has no filesystem coordinate.

`source_authority` is independent from `author_cascade`. A declaration may have an exact source position without being a proven author winner, and a proven author winner may lack an exact source position.

## Coordinate contract

Direct project CSS coordinates use:

- line: 1-based;
- column: 0-based Unicode scalar position in the source line;
- `mapping = "direct_css"`;
- `coordinate_space = "line_1_based_column_unicode_scalar_0_based"`.

Project Source Map coordinates use the existing bounded Source Map v3 consumer:

- original line: 1-based;
- original column: 0-based Source Map v3 coordinate;
- `mapping = "project_source_map"`;
- `coordinate_space = "source_map_v3_original_line_1_based_column_0_based"`.

The generated column supplied to the Source Map resolver is counted in UTF-16 code units. Direct UTF-8/Unicode source reporting remains independent of Source Map remapping.

## Project ownership

For a `same_origin_stylesheet` hint the backend:

1. obtains `git_root`, otherwise `cwd`, from the exact session;
2. canonicalizes the project root and requires a directory;
3. accepts only bounded relative paths;
4. rejects absolute paths, parent components and percent-encoded paths;
5. canonicalizes the candidate;
6. requires the canonical candidate to remain under the canonical project root;
7. requires a regular file;
8. reports only a normalized project-relative path.

A symlink that resolves outside the project therefore never becomes project-file authority.

The filesystem absolute path is never serialized.

## Direct CSS parser

Exact direct coordinates are attempted only for a verified file whose extension is `.css`.

This avoids treating an SCSS/Sass/Less/PostCSS source file as browser-generated CSS merely because its runtime URL resembles a project path.

Hard bounds:

- file bytes: 512 KiB;
- visited rules: 512;
- retained/inspected declarations: 4096;
- recursive grouping depth: 12;
- component nesting depth: 32;
- selector bytes: 256;
- property bytes: 64;
- normalized source value bytes: 1024.

The parser is a bounded state machine, not a whole-file regex. It handles:

- comments;
- quoted strings and escapes;
- semicolons inside strings/functions;
- brackets/parentheses;
- `!important`;
- CRLF and LF line accounting;
- UTF-8 source text;
- nested `@media` and `@supports`;
- known non-selector at-rule blocks that can be skipped safely.

Unsupported cascade/grouping constructs such as `@layer`, `@container`, `@scope`, CSS nesting, malformed strings/comments/brackets, or exhausted bounds fail closed for exact source correlation.

The parser does not re-evaluate media/supports activity. Runtime activity/disabled filtering remains the responsibility of the already-landed fresh declaration trace. If explicit `active: false` or `disabled: true` markers appear in backend evidence, projection rejects the snapshot.

## Declaration matching

The backend matches only existing evidence:

- selector;
- property;
- value;
- `important`.

It does not infer a declaration from computed style.

A declaration coordinate is emitted only when exactly one parsed candidate matches. Duplicate selectors, repeated same-property/value declarations, or any other multi-match condition stop at `project_file_verified`.

Source order remains cascade evidence and is not fabricated from the source parser.

## Source Maps

After a unique direct CSS declaration is located, LocalView may ask the existing exact-session project-owned Source Map runtime to remap that generated line/column.

This reuses the already-landed authority:

- sibling project-owned `.map`;
- map-size cap before parse;
- bounded Source Map v3 consumer;
- no remote map loading;
- no `sourcesContent` retention or response;
- canonical original-source containment;
- no absolute-source leakage.

A successful safe mapping upgrades the returned exact coordinate to the original project-owned source.

If the map is absent, invalid, unsupported, remote, oversized, or resolves outside the project, the map is ignored and the proven direct generated-CSS coordinate remains authoritative.

No filename-pattern heuristic is used to claim SCSS/Sass/Less/PostCSS ownership.

## Inline cases

### Element `style=""`

The declaration may retain runtime ownership, but source authority is `runtime_inline`. No filesystem line/column is invented.

### `<style>`

Without an independent project-file binding, inline stylesheet evidence is also `runtime_inline`.

This lane does not infer a source file from component ownership, DOM position, framework filenames, or selector text.

## Privacy and response contract

Allowed response data is bounded provenance only:

- authority level;
- verified project-relative file when proven;
- exact line/column when proven;
- bounded mapping/coordinate-space labels.

Never returned:

- absolute paths;
- source file contents;
- raw stylesheet text;
- Source Map `sourcesContent`;
- arbitrary remote URLs;
- arbitrary selectors beyond the already-landed trace bounds.

## Adversarial verification

Focused tests cover:

- simple unique CSS exact match;
- repeated declarations of one property;
- duplicate selectors;
- same property/value duplicates;
- comments;
- quoted semicolons;
- data URL strings;
- nested media/supports;
- explicit inactive/disabled evidence rejection;
- traversal and encoded traversal;
- remote source-kind rejection;
- symlink escape;
- oversized CSS;
- ambiguous match downgrade;
- inline element authority;
- inline stylesheet authority;
- minified one-line CSS;
- CRLF line counting;
- UTF-8 column counting;
- safe project-owned Source Map upgrade;
- Source Map escape rejection with direct-CSS fallback;
- no absolute path or `sourcesContent` leakage.

## Non-claims

This lane does not claim:

- browser-final computed provenance;
- a new cascade engine;
- cascade-layer ordering;
- selector matching against the live DOM;
- preprocessor source ownership without a project-owned Source Map;
- inline `<style>` filesystem ownership without an independent binding;
- source coordinates for ambiguous declarations;
- remote stylesheet or remote Source Map authority.

## Completion definition

Complete when an authenticated fresh style trace can prove:

```text
fresh declaration evidence
  -> exact-session project authority
  -> bounded file/map processing
  -> unique declaration
  -> project-relative exact source coordinate
```

while every ambiguous, escaped, unsupported, oversized, inline-without-file, or remote case fails closed and cascade-winner authority remains a separate claim.
