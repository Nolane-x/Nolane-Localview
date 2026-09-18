# Trusted Human-First Open Source V2.3

Status: canonical design specification  
Repository: `Nolane-x/Nolane-Localview`  
Wave: Human-First V2.3  
Base: `main@99fef881628dfea1e8df548c9c68d9d22f0964cd`  
Canonical path: `docs/superpowers/specs/2026-09-18-human-first-trusted-open-source-v23.md`

---

## 0. Durable recovery contract

This document is the durable source of truth for the Trusted Human-First Open Source V2.3 wave.

Future AI sessions must recover from:

1. the current repository branch;
2. this canonical specification;
3. executable tests and workflow evidence;
4. the exact final implementation head.

Chat history is not implementation truth.

The branch may advance after this file is committed. This specification defines intended product behavior, authority boundaries, failure semantics, evidence requirements and merge conditions. Executable tests plus the exact branch head define implementation truth.

---

## 1. Objective

Make the Human-First Inspector **Open source** control a real capability without allowing React, observer display text, AI output or caller-supplied strings to invent filesystem authority.

The user experience should be simple:

- select an element;
- press **Open source**;
- LocalView resolves the selected element to trusted source provenance;
- LocalView validates the source target against the exact active project;
- LocalView opens that source target through a bounded desktop-owned launch path;
- failure remains human-readable and does not expose raw local paths or internal exceptions unnecessarily.

The backend remains strict even though the UI remains calm.

---

## 2. Why this wave exists

Human-First V2 deliberately left Open source fail-closed.

The current Inspector may observe a display string such as:

`src/components/DeployButton.tsx:42`

inside focus event payload.

That string is useful diagnostic context, but the Human-First UI must not promote it directly into filesystem authority.

A browser-side string can be:

- stale;
- malformed;
- unrelated to the current route;
- relative to an unknown root;
- absolute and outside the current project;
- crafted by application code;
- copied from a different element;
- copied from a different session;
- syntactically valid but backed by no current file;
- a path traversal attempt;
- a symlink escape;
- a source-map hint that no longer matches the active revision.

Therefore the current disabled button is correct.

V2.3 enables the action only after LocalView owns the complete authority path.

---

## 3. Existing trusted primitives

The repository already contains important building blocks.

### 3.1 Stable element references

LocalView instrumentation assigns bounded stable references such as:

`@e1a2b3`

V2.2 already hardened the public reference validator.

V2.3 should reuse the same stable-reference grammar rather than create a second incompatible identity system.

### 3.2 Fresh semantic snapshot

Control owns authenticated fresh semantic snapshot acquisition for the exact session.

The fresh snapshot path:

- queues a fresh snapshot action;
- waits for the exact action result;
- projects a bounded `PageSnapshot`;
- rejects malformed payloads;
- bounds route, viewport, tree depth, node count, strings and source fields;
- produces `SemanticNode.source: Option<SourceLocation>`.

V2.3 must use a fresh snapshot or another equivalently strong current authority source.

Stale observer history alone is insufficient.

### 3.3 SourceLocation

Protocol already defines:

- file;
- line;
- optional column;
- optional component identity.

Fresh snapshot projection currently admits explicit:

- `data-source`;
- `data-component-source`.

This wave may use those projected source locations.

It must not silently elevate weaker unbound hints into trusted source authority.

### 3.4 Source map and source graph primitives

The repository contains source-map and source-graph primitives.

They are useful future inputs, but their existence does not prove that a particular selected element currently belongs to a particular file.

V2.3 first closes the exact selected-element path.

A later wave may add multi-source ranking or dependency navigation.

### 3.5 Project identity

Session identity contains process-derived project information including:

- `git_root` when available;
- otherwise `cwd`.

The project root is backend/session authority.

React must not submit a project root.

---

## 4. Product invariant

> The Human-First UI may ask LocalView to open the trusted source for the selected element. It may not decide what file, line, column, root or launcher is trusted.

This invariant applies to every API, test, adapter and follow-up commit.

---

## 5. Non-negotiable request boundary

The frontend request shape is conceptually:

```text
session_id
element_reference
```

React must not submit authoritative:

- file path;
- project root;
- line;
- column;
- component name;
- route;
- revision;
- editor executable;
- editor command line;
- URI scheme;
- working directory;
- shell fragment;
- source confidence;
- evidence id.

Those values must be derived by LocalView-owned layers.

---

## 6. Human interaction flow

The intended flow is:

```text
Human selects element
  -> LocalView focus/selection carries stable @e reference
  -> Inspector enables Open source only when:
       current session exists
       stable reference exists
       no Open source request is in flight

Human presses Open source
  -> React sends sessionId + reference only
  -> desktop resolves exact managed session/project authority
  -> acquire fresh semantic snapshot for exact session
  -> locate exact node by stable reference
  -> require trusted projected SourceLocation
  -> validate current route/session continuity
  -> derive backend-owned project root
  -> resolve SourceLocation.file beneath project root
  -> canonical filesystem containment checks
  -> validate file + line + optional column
  -> bounded desktop launcher opens target
  -> return sanitized SourceOpenReceipt
  -> UI shows success/failure without raw internal error leakage
```

No step may substitute guessed source data.

---

## 7. Stable reference authority

V2.3 should reuse the same bounded stable-reference grammar already established for Measure:

- required prefix `@e`;
- non-empty hexadecimal suffix;
- bounded byte length;
- no whitespace;
- no path characters;
- no arbitrary selector input.

Malformed references fail before any filesystem operation.

A syntactically valid reference is not proof that the element currently exists.

Fresh snapshot resolution remains mandatory.

---

## 8. Freshness contract

Open source must resolve against current state.

The action must fail closed when:

- the reference is absent from the fresh snapshot;
- the source location is absent;
- the route changes during the operation in a way that invalidates lineage;
- the selected reference changes before the UI applies a returned state;
- the session disappears;
- the project root changes or becomes unavailable;
- the source target no longer resolves safely.

The UI must not fall back to an earlier focus payload source string.

---

## 9. Exact node resolution

V2.3 requires a bounded tree search over the fresh `PageSnapshot`.

The resolver must:

- compare the exact stable reference;
- return at most one selected node;
- reject ambiguity if duplicate references somehow appear;
- reject absent references;
- consume only the bounded fresh snapshot already projected by control.

The resolver must not search by:

- element name;
- role;
- displayed text;
- DOM tag alone;
- CSS selector guessed from UI;
- nearest matching filename.

---

## 10. Source provenance admitted in V2.3

The trusted first slice may admit `SemanticNode.source` projected from the fresh snapshot.

The source must originate from the bounded LocalView source projection.

The implementation must retain enough provenance internally to know that the location came through the fresh snapshot authority path.

The UI does not need to display origin strings by default.

### 10.1 Explicit data-source

`data-source` may provide file + line + optional column.

It does not automatically prove component ownership.

For Open source, file location is sufficient if all filesystem/project validations pass.

### 10.2 Explicit data-component-source

`data-component-source` provides explicit component source identity tied to file + line.

It may be treated as stronger semantic ownership evidence.

### 10.3 Stack-only hints

Stack-derived source hints are not part of the trusted selected-element path in the initial V2.3 slice unless an explicit later implementation proves exact binding.

Do not silently mix them into the first implementation.

### 10.4 Observer display source

The existing human-facing `focused.payload.source` string is diagnostic only.

It must not be used as the backend target.

---

## 11. Source target receipt

Desktop should operate on an internal structure conceptually equivalent to:

```rust
struct TrustedSourceTarget {
    session_id: SessionId,
    reference: String,
    project_root: PathBuf,
    canonical_file: PathBuf,
    project_relative_file: String,
    line: u32,
    column: Option<u32>,
    snapshot_version: u64,
    canonical_route: String,
}
```

The public UI receipt must be narrower.

Conceptually:

```text
status
display_file
line
column?
launcher
```

Do not return unrestricted absolute filesystem paths to the default Inspector unless there is a specific product need.

Advanced/debug surfaces may expose more under an explicit separate contract.

---

## 12. Project-root authority

Project root must come from the exact session:

1. `git_root` when present;
2. otherwise `cwd`;
3. otherwise fail closed.

React must not choose the root.

The root string must be bounded.

The desktop implementation must convert the root into a filesystem path using platform-aware rules.

---

## 13. Relative source path policy

Preferred source locations are project-relative paths such as:

`src/components/Button.tsx`

Rules:

- empty path rejected;
- NUL rejected;
- path length bounded;
- unresolved parent traversal rejected;
- drive/device prefixes rejected when a relative path is expected;
- URL-looking strings rejected;
- non-file schemes rejected;
- source path must not be interpreted by a shell.

---

## 14. Absolute source path policy

If LocalView later admits absolute source locations, they must still be proven inside the exact trusted project root.

The first V2.3 implementation may choose to reject absolute source paths entirely if that yields a smaller, stronger boundary.

Do not accept an absolute path merely because it exists.

---

## 15. Filesystem canonicalization

Opening a local file is a filesystem authority action.

V2.3 must defend against lexical and filesystem escapes.

Required checks:

1. derive trusted project root;
2. ensure the project root exists when the action executes;
3. canonicalize the project root for the live filesystem operation;
4. join the bounded relative source path;
5. canonicalize the resolved target;
6. require canonical target to remain inside canonical project root;
7. reject symlink escape;
8. reject directory targets;
9. reject non-file targets;
10. reject inaccessible targets;
11. perform no shell expansion.

The durable session identity subsystem intentionally does not require filesystem canonicalization for identity recovery.

That rule does not apply to this live file-opening action.

V2.3 should canonicalize for the live authority decision.

---

## 16. Platform path semantics

Tests must cover platform-sensitive path behavior.

### Windows

Account for:

- drive prefixes;
- UNC/device paths;
- case-insensitive containment semantics where appropriate;
- alternate separators;
- reserved device names;
- path normalization without invoking a shell.

### macOS/Linux

Account for:

- absolute root;
- symlink escape;
- case-sensitive paths unless filesystem semantics say otherwise;
- relative traversal;
- inaccessible files.

Cross-platform tests may use pure path validators where physical filesystem behavior cannot be reproduced in every CI job.

---

## 17. File-type policy

The initial implementation should open regular project files only.

Reject:

- directories;
- FIFOs;
- sockets;
- device nodes;
- special files.

Opening a symlink is acceptable only when the canonical target remains inside the canonical project root.

---

## 18. File-size policy

Open source launches an editor and does not need to read the whole file.

Therefore file size need not be used as a content budget.

However, if V2.3 reads source text to validate line/column or create a preview, reads must be bounded.

The initial implementation should avoid reading the whole file unless required.

---

## 19. Line validation

Source line must be:

- >= 1;
- within `u32`;
- bounded by a defensive maximum.

If the launcher cannot route to a line, the target may still open at file level if product semantics explicitly mark that fallback.

Do not silently change a trusted invalid line to line 1.

If line validation requires reading the file, use a bounded streaming line count rather than unbounded whole-file loading.

---

## 20. Column validation

Column is optional.

When present:

- >= 1 if LocalView source semantics use one-based columns;
- bounded;
- never used as a shell fragment;
- must not cause the action to fail if the selected safe launcher supports file+line but not column, unless exact-column behavior is part of that launcher contract.

The receipt should record whether the launch honored:

- file only;
- file + line;
- file + line + column.

---

## 21. Route continuity

Source opening is tied to the currently managed target.

Before fresh resolution, capture the canonical managed route.

After fresh snapshot resolution and before launching, verify route continuity.

If the managed target navigated to a different canonical route during the operation, fail closed.

This prevents source provenance from one page from being applied after navigation.

---

## 22. Session continuity

The exact session must remain active and owned throughout the operation.

Do not reuse another session merely because it points to the same port or project name.

If durable session identity reconnect logic changes the active session state, the source action must still bind to the exact request lineage.

---

## 23. Revision semantics

V2.3 first slice does not require git revision locking if the existing fresh snapshot and filesystem target are current enough for interactive use.

However:

- the action must not claim revision-exact provenance unless it proves it;
- future evidence may add current git revision;
- any future revision gate must be backend-owned.

The UI must not submit a revision string as authority.

---

## 24. Desktop command

Conceptual Tauri command:

```rust
async fn open_source_for_selection(
    app: AppHandle,
    session_id: SessionId,
    reference: String,
) -> Result<HumanSourceOpenReceipt, String>
```

No file/path/line/column argument is allowed.

The command owns:

- reference validation;
- session lookup;
- route continuity;
- fresh snapshot acquisition;
- exact node resolution;
- source location validation;
- project root resolution;
- filesystem containment;
- launcher selection;
- bounded result projection.

---

## 25. Frontend API

Conceptual API:

```ts
openSourceForSelection: (sessionId: string, reference: string) =>
  invoke('open_source_for_selection', { sessionId, reference })
```

The API object must not contain:

- `file`;
- `path`;
- `line`;
- `column`;
- `root`;
- `route`;
- `editor`;
- `command`.

Executable tests should assert this boundary.

---

## 26. Launcher authority

Source resolution and source launching are separate responsibilities.

The launcher must receive only a backend-resolved trusted source target.

It must never receive arbitrary user-authored shell text.

### 26.1 No shell concatenation

Forbidden patterns include:

- building a command string;
- `sh -c`;
- `cmd /C` with interpolated source strings;
- PowerShell `-Command` with interpolated source strings;
- passing a caller-authored URI.

### 26.2 Fixed executable + argv

If an editor adapter uses a process executable, construct it as:

- fixed/discovered trusted executable;
- discrete argv entries;
- no shell interpretation.

### 26.3 System opener

If a system-default opener is used, invoke a platform API/library that accepts a path directly.

The opener must not reinterpret the path as a URL scheme chosen by the frontend.

### 26.4 Launcher failure

If no safe launcher is available:

- fail closed;
- keep Inspector alive;
- show a humanized “Couldn’t open source” state;
- preserve the resolved source target internally only as needed;
- do not expose raw process errors in the default UI.

---

## 27. Editor routing strategy

V2.3 may initially support one safe generic launcher.

Future adapters may support IDE-aware line/column routing.

Potential adapters may include:

- VS Code;
- Cursor;
- Zed;
- JetBrains products;
- system default file association.

No adapter is trusted merely by name.

Each adapter must define:

- executable discovery;
- argv format;
- path handling;
- line/column capability;
- platform support;
- failure behavior;
- executable provenance.

---

## 28. No arbitrary protocol URI authority

The frontend must not be allowed to submit:

- `vscode://...`;
- `cursor://...`;
- `file://...`;
- custom schemes.

If a backend adapter internally constructs a protocol URI, it must do so solely from the trusted canonical source target and an allowlisted adapter implementation.

---

## 29. Human-First UI states

Open source should have explicit states:

- unavailable: no active session;
- unavailable: no stable selection;
- unavailable: no trusted source mapping;
- ready;
- opening;
- success;
- failure;
- stale selection;
- launcher unavailable.

The default Inspector must not display raw authority vocabulary.

Human copy should remain concise.

---

## 30. Enablement rule

The UI must not enable Open source merely because `focused.payload.source` is present.

Safe options:

### Option A — optimistic request with fail-closed backend

Enable when:

- current session exists;
- stable `@e` selection exists;
- no request in flight.

Backend then resolves source and may return “source unavailable.”

### Option B — explicit source capability probe

Backend exposes a bounded resolver/probe proving source availability before enablement.

For the first slice, Option A is acceptable if failure UX is clear and no guessed source is used.

Do not add a second untrusted frontend source parser just to decide enablement.

---

## 31. Stale-selection UI race

If element A is selected and Open source is requested, then the user selects B before A completes:

- A may still safely launch if the backend request already resolved A under the exact trusted request lineage;
- UI success/failure state for A must not be shown as if it belongs to B;
- state should carry the request reference;
- the shell should only project result state onto the current selection when references match.

The action is external and may already have occurred; stale UI protection still matters.

---

## 32. Concurrency

At minimum:

- one Open source request per Inspector action state;
- duplicate clicks while opening disabled;
- repeated opens after completion allowed;
- request state reference-bound.

Do not create an unbounded launcher queue.

---

## 33. Timeout behavior

Source resolution must be bounded.

Fresh snapshot already has a bounded timeout.

Launcher startup should also use a bounded desktop policy where the API permits it.

Do not wait indefinitely for an editor process to exit.

A successful launch means the launch request was accepted/started, not that the editor later displayed the file perfectly.

---

## 34. Failure isolation

Open source failure must not collapse:

- Inspector;
- Measure;
- Capture;
- preview;
- observer;
- session discovery.

Errors must be isolated to the source action state.

---

## 35. Humanized error boundary

Default Inspector must not expose raw errors such as:

- canonicalization OS errors;
- absolute project paths;
- spawn command lines;
- executable discovery paths;
- `element reference not found`;
- filesystem permission internals;
- raw Tauri errors;
- internal control URLs.

Map internal failures to bounded categories.

Example categories:

- source unavailable;
- source outside project;
- source missing;
- source changed;
- launcher unavailable;
- open failed;
- runtime unavailable.

Advanced diagnostics may expose more only under an explicit separate privacy contract.

---

## 36. Privacy

Local filesystem paths are sensitive machine metadata.

Default Human-First UI should prefer project-relative display paths.

Avoid showing:

- home directory;
- username embedded in path;
- full repository absolute root;
- editor executable absolute path.

Evidence storage should also prefer sanitized relative paths unless absolute path retention is explicitly required and justified.

---

## 37. Evidence semantics

A successful Open source action is a user-intent/desktop-action observation, not proof that source code is correct.

If evidence is retained, store only bounded fields such as:

- session lineage identifier;
- stable reference;
- project-relative file;
- line;
- optional column;
- launcher kind;
- accepted/success status;
- sanitized failure category;
- timestamp.

Do not store:

- shell command text;
- environment variables;
- absolute editor executable path;
- arbitrary source file contents;
- route secrets;
- raw OS errors.

---

## 38. Source evidence versus UI receipt

The default UI receipt should be minimal.

Possible success copy:

`Opened Button.tsx:42`

The evidence record can retain more trusted provenance.

Do not make the UI an evidence dump.

---

## 39. Source-map and source-graph relationship

V2.3 does not delete or replace source-map/source-graph primitives.

The initial trusted selected-element path is:

`fresh snapshot -> exact node -> SourceLocation`

Future waves may:

- rank multiple hints;
- show source alternatives;
- navigate component ownership;
- show dependency graph;
- trace style origin;
- connect request errors to frontend/backend source.

Those capabilities require their own authority design.

---

## 40. No source guessing

Explicitly forbidden fallbacks:

- append `.tsx` to component display name;
- search project for the closest filename;
- infer source from DOM class name;
- infer source from React component text in production DOM;
- trust a filename mentioned by AI output;
- trust a path from console text without provenance;
- use the old focus payload string as filesystem target;
- scan the whole repository and choose the first match.

If trusted source is unavailable, the button fails closed.

---

## 41. No repository-wide scan in the primary action

Open source should be low latency and bounded.

The primary action must not recursively scan the project.

Trusted source metadata should identify the target directly.

Repository search belongs to a different explicit feature.

---

## 42. File existence race

The file may disappear after source resolution but before launch.

Required behavior:

- validate as close to launch as practical;
- launcher failure remains bounded;
- no fallback to another similarly named file;
- no directory-wide search.

---

## 43. Symlink race

Canonicalization introduces TOCTOU considerations.

The first implementation should minimize the gap between containment validation and launch.

Where a platform/library allows opening by already-resolved canonical path, use it.

Do not retain an untrusted pre-canonical path after validation.

This action is not a privileged sandbox escape boundary, but containment must still be explicit and tested.

---

## 44. Source line race

The file may change between snapshot production and editor opening.

V2.3 should not claim immutable revision accuracy.

If the requested line now exceeds the current file length:

- either open file without line under an explicit degraded receipt;
- or fail closed.

Choose one behavior and test it.

Do not silently clamp to an unrelated line while still claiming exact source provenance.

---

## 45. Accessibility

The Open source button must:

- remain a semantic button;
- expose an accessible name;
- expose busy/disabled state;
- retain keyboard activation;
- not move focus unpredictably on completion;
- announce success/failure through appropriate live status without excessive repetition.

---

## 46. Localization

All new Human-First strings must exist across the complete locale dictionaries.

Expected message concepts:

- source opening;
- source opened;
- source unavailable;
- source mapping unavailable;
- source open failed;
- launcher unavailable;
- select an element first.

Do not reintroduce mixed-language primary surfaces.

---

## 47. Reduced motion

Source action state must not depend on animation.

Any progress visual follows existing reduced-motion preferences.

---

## 48. Command palette

The existing canonical command `source.open` must route through the same trusted action path.

Do not implement a second direct path that bypasses Inspector authority checks.

Command palette behavior with no selection must fail closed/humanize appropriately.

---

## 49. API and command singularity

There should be one canonical source-open capability.

Inspector and Command Palette must share it.

Avoid:

- one Tauri command for Inspector;
- another unvalidated utility for Command Palette;
- direct browser-side filesystem APIs.

---

## 50. Desktop permission surface

Register only the exact new Tauri command needed.

Do not broaden filesystem permissions globally.

Do not expose arbitrary open-file APIs to the webview.

The webview receives a capability for “open trusted source for this LocalView selection,” not “open any local path.”

---

## 51. No generic filesystem API

V2.3 must not create public frontend methods such as:

- `openPath(path)`;
- `readFile(path)`;
- `openEditor(path, line)`;
- `shell(command)`.

Those would destroy the authority boundary.

---

## 52. RED -> GREEN execution

Implementation must proceed deliberately:

1. canonical spec;
2. RED contract;
3. dedicated workflow;
4. backend source resolver;
5. filesystem containment;
6. launcher adapter;
7. frontend geometry/path-free API;
8. Human-First lifecycle;
9. runtime/render evidence;
10. cross-platform regression;
11. exact-head closure;
12. merge.

Do not make the button clickable first and retrofit trust later.

---

## 53. Dedicated RED contract

Add a dedicated contract such as:

`apps/desktop/src-tauri/tests/human_first_trusted_open_source_v23_contract.rs`

The RED contract should require:

- command exists;
- frontend API carries session + reference only;
- no frontend path/line/root authority;
- fresh snapshot authority used;
- project root derived backend-side;
- canonical containment validator exists;
- symlink/outside-root tests exist;
- launcher path receives trusted target only;
- Inspector Open source is no longer generic unavailable action;
- stale selection state is reference-bound;
- localization keys complete;
- render audit states exist.

---

## 54. Dedicated workflow

Add a branch-scoped workflow:

`.github/workflows/human-first-trusted-open-source-v23.yml`

It should run on changes to:

- V2.3 spec;
- V2.3 contract;
- relevant desktop backend;
- relevant control/fresh snapshot code if touched;
- frontend API/shell/Inspector;
- i18n;
- launcher module;
- V2.3 render harness;
- dependency manifests if changed.

The workflow must execute:

- focused Rust tests;
- desktop contract;
- frontend build;
- launcher/path validators.

---

## 55. Render/runtime audit

Extend or derive the existing Human-First render harness.

Minimum executable states:

1. source-ready selected element;
2. source opening;
3. successful open;
4. source unavailable;
5. no selection;
6. no session;
7. malformed reference rejected;
8. forced launcher failure;
9. raw error not leaked;
10. stale selection changed before completion;
11. source path outside project rejected;
12. source path traversal rejected;
13. symlink escape rejected where CI filesystem supports it;
14. Vietnamese success/failure copy;
15. narrow viewport with source status;
16. Measure/Capture remain usable after source failure.

The audit must assert behavior, not only capture screenshots.

---

## 56. Path validator tests

Pure/unit tests should cover:

- simple relative file;
- nested file;
- empty path;
- `..` traversal;
- repeated separators;
- absolute Unix path;
- Windows drive path;
- UNC/device path;
- URL-like path;
- NUL;
- oversized path;
- canonical target inside root;
- canonical target outside root;
- symlink inside root;
- symlink escape;
- directory target;
- missing file.

---

## 57. Reference resolver tests

Test:

- exact reference found once;
- reference absent;
- duplicate reference ambiguity;
- source missing;
- valid source;
- invalid source projection cannot become target.

---

## 58. Route tests

Test:

- canonical route stable;
- route changes during resolution;
- non-loopback route rejected by underlying managed-surface authority where applicable;
- session disappears;
- another session with same project does not satisfy request.

---

## 59. Launcher tests

Use a fake launcher abstraction in unit tests.

Verify:

- trusted target forwarded exactly;
- no raw frontend path exists;
- fixed argv model;
- no shell concatenation;
- launcher unavailable;
- launcher rejected;
- accepted launch;
- line/column capability projection;
- error sanitized.

Do not require a real IDE in ordinary unit CI.

---

## 60. Real integration evidence

Where practical, add a platform smoke that launches a harmless test adapter or records an accepted open request without relying on a developer-installed editor.

Do not make CI depend on VS Code/Cursor/Zed being installed.

---

## 61. Inspector state shape

Conceptual frontend state:

```ts
type HumanSourceOpenState =
  | { status: 'idle' }
  | { status: 'opening'; reference: string }
  | { status: 'success'; reference: string; displayFile: string; line: number; column?: number }
  | { status: 'failure'; reference: string; reason: SourceOpenFailureReason };
```

Never store an arbitrary caller-authored absolute file path in this state.

---

## 62. Shell ownership

LocalViewShell should own source action lifecycle, matching Capture and Measure architecture.

Responsibilities:

- current session;
- current stable selected reference;
- request start;
- duplicate suppression;
- API call;
- reference-bound completion;
- humanized failure;
- shared command routing.

Inspector remains presentational.

---

## 63. Inspector ownership

Inspector should:

- render button;
- render state;
- render project-relative success receipt;
- emit stable reference action request.

Inspector should not:

- parse source strings;
- canonicalize paths;
- choose project root;
- choose editor executable;
- construct file URI;
- read filesystem.

---

## 64. Existing focus payload cleanup

The current Inspector derives:

`focused.payload.source`

for disabled reason copy.

After trusted V2.3 wiring, primary Open source behavior must not depend on that field.

It may remain diagnostic in Advanced if useful.

Tests should explicitly prevent accidental reintroduction of:

`onOpenSource(focused.payload.source)`

or equivalent caller-authored target paths.

---

## 65. Advanced diagnostics

Advanced may show source mapping diagnostics, but must not become an alternate unvalidated source-open path.

If Advanced exposes an “open” action later, it must call the same trusted source command.

---

## 66. Security review questions

Before merge, explicitly verify:

- Can a malicious inspected app cause LocalView to open a file outside project root?
- Can a malicious source attribute inject shell syntax?
- Can a malicious app force a custom URL scheme?
- Can symlink traversal escape the root?
- Can a stale route open source for the previous page?
- Can a stale selection result overwrite the new selection state?
- Can a frontend caller submit an arbitrary file path through Tauri?
- Can absolute machine paths leak into default UI/evidence?
- Can source failure break Measure/Capture?

A “yes” to any of the first seven blocks merge.

---

## 67. Performance

The primary action should remain bounded:

- one fresh snapshot;
- one bounded tree lookup;
- one project-root resolution;
- one canonicalization path;
- one launcher request.

No recursive repository scan.

No source graph traversal in the first slice.

---

## 68. Resource ownership

Opening a source file should not acquire long-lived visual/capture resource leases.

Reuse existing session/control authority only as needed.

Do not hold the managed surface lock while an external editor remains open.

---

## 69. Telemetry/logging

If logs are added:

Allowed examples:

- source open success category;
- launcher kind;
- sanitized project-relative path;
- bounded timing.

Avoid:

- absolute home path;
- editor environment;
- entire command line;
- file contents;
- route secrets.

---

## 70. Backward compatibility

V2.3 must not regress:

- Human-First V2 foundation;
- trusted Capture V2.1;
- trusted Measure V2.2;
- session discovery;
- observer evidence;
- native capture;
- guarded full-page stitching;
- managed surface authority;
- V4.3 consequential action authority;
- Windows/macOS/Linux provider gates.

---

## 71. Failure fallback

If trusted source resolution is unavailable:

- keep Open source disabled or return explicit unavailable state;
- do not guess;
- keep other actions operational.

Fail-closed is a valid product outcome.

---

## 72. Non-goals

V2.3 does not need to implement:

- source code editing;
- AI code rewriting;
- repository-wide symbol search;
- source graph UI;
- stack trace ranking UI;
- CSS winning-rule navigation;
- git revision checkout;
- remote repository source opening;
- SSH/WSL/container path remapping;
- source-map download from production sites;
- generic file explorer;
- arbitrary shell execution.

Each requires independent authority design.

---

## 73. Future waves

Potential later waves:

- V2.4 trusted Responsive using managed-surface bounds authority;
- V2.5 provider-neutral Ask AI;
- V2.6 Fix/Verify with explicit write/verification authority;
- source alternatives and source graph navigation;
- CSS cause -> exact stylesheet location;
- WSL/container/devcontainer path translation;
- editor preference management;
- artifact/evidence browser;
- cross-provider source reconciliation.

Do not bundle them into V2.3.

---

## 74. Files expected to be central

Likely files:

- `docs/superpowers/specs/2026-09-18-human-first-trusted-open-source-v23.md`
- `apps/desktop/src-tauri/src/lib.rs`
- new desktop source resolver/launcher module if separation improves testability
- `apps/desktop/src-tauri/tests/human_first_trusted_open_source_v23_contract.rs`
- `apps/desktop/src/api.ts`
- `apps/desktop/src/app/LocalViewShell.tsx`
- `apps/desktop/src/features/FloatingTools.tsx`
- `apps/desktop/src/i18n.ts`
- `tools/human-first-ui-v2/capture.mjs`
- V2.3 workflow files
- manifests only if a launcher dependency is added

Future AI must inspect the current branch rather than assuming this list is exhaustive.

---

## 75. Implementation order

Recommended order:

### Phase A — RED authority contract
Lock request shape, trusted resolver requirement and no caller-authored path.

### Phase B — source resolver
Implement exact fresh-snapshot reference -> SourceLocation resolution.

### Phase C — filesystem boundary
Implement project-root derivation, canonical containment and file validation.

### Phase D — launcher abstraction
Implement bounded safe source launcher with fake-test adapter.

### Phase E — desktop command
Bind session/reference to resolver + launcher.

### Phase F — frontend API
Expose session + reference only.

### Phase G — Human-First lifecycle
Make Inspector Open source a real action with bounded state.

### Phase H — runtime evidence
Prove success/failure/stale selection/path attack states.

### Phase I — cross-platform regression
Run full CI/provider/native gates.

### Phase J — exact-head merge closure
Update PR body with exact head/evidence, mark ready and merge only if all required checks correspond to the exact final head.

---

## 76. Merge readiness

V2.3 may merge only when one immutable exact final head satisfies:

1. canonical V2.3 spec committed;
2. dedicated RED/GREEN contract passes;
3. fresh selected-element source resolver tests pass;
4. project-root/path containment tests pass;
5. symlink/outside-root tests pass where applicable;
6. safe launcher adapter tests pass;
7. frontend request contains session + reference only;
8. Inspector Open source uses the trusted action path;
9. raw source/OS errors do not leak into default Inspector;
10. stale-selection state cannot overwrite current selection state;
11. Measure V2.2 regressions remain green;
12. Capture V2.1 regressions remain green;
13. frontend build passes;
14. V2.3 runtime/render audit passes;
15. full repository CI passes;
16. applicable Windows/macOS/Linux provider/native gates remain green;
17. PR body records exact final head and exact evidence;
18. no code change occurs after the evidence cited for merge.

Do not merge using green evidence from an earlier head.

---

## 77. Final invariant

The Human-First UI may request:

> Open the source that LocalView can currently prove belongs to this selected element.

The Human-First UI may not request:

> Open this arbitrary path I supplied.

LocalView owns source provenance, project containment and launch authority.

That distinction must remain true in every implementation and follow-up commit.
