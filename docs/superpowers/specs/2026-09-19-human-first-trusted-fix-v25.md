# Human-First Trusted Fix V2.5 — Engineering Specification

Status: canonical implementation specification for the first write-capable Human-First LocalView action.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Branch: `feat/human-first-trusted-fix-v25`

Base: `main@d6176ffcdbe64262cffb9b11e94b89c051b44981`

Predecessors:
- Human-First UI/UX V2
- Trusted Capture V2.1
- Trusted Measure V2.2
- Trusted Open Source V2.3
- Trusted Ask AI V2.4

---

## 0. Purpose

This document is the durable source of truth for Trusted Human-First Fix V2.5.

V2.5 is the first Human-First wave allowed to mutate source code.

That makes it qualitatively different from Capture, Measure, Open Source and Ask AI.

The target experience is simple:

> Select an element, ask LocalView to propose a fix, review the exact diff, then explicitly apply or discard it.

The implementation underneath that experience must remain strict, bounded, reviewable, rollback-capable and fail-closed.

A future AI must be able to resume this wave from this file without relying on chat history.

---

## 1. Product goal

A developer should be able to:

1. select a stable LocalView element;
2. choose Fix;
3. see a clear disclosure that a bounded source excerpt will be sent to the connected AI provider;
4. explicitly request a proposal;
5. receive one bounded source edit against the trusted mapped source file;
6. inspect a human-readable diff before any write occurs;
7. Apply or Discard;
8. have Apply revalidate the current LocalView source authority and exact file preimage;
9. write through a backend-owned transaction;
10. receive success or a humanized failure;
11. continue using LocalView if proposal generation or application fails.

V2.5 does not authorize autonomous repository editing.

---

## 2. Primary safety model: proposal is not permission to write

Generating a fix proposal and applying a fix are separate operations.

The provider may never write files.

The frontend may never author source paths or replacement text.

No code is changed until the user performs a distinct Apply action on a currently valid backend-owned proposal.

This separation is non-negotiable.

---

## 3. Two-phase authority

Phase A — Prepare:

```
human selection + human instruction
            |
            v
backend re-resolves current trusted source
            |
            v
backend reads one bounded source file/excerpt
            |
            v
provider proposes one bounded edit
            |
            v
backend validates proposal and stores it
            |
            v
frontend receives diff preview + opaque proposal id
```

Phase B — Apply:

```
opaque proposal id
       |
       v
backend re-resolves session/reference/source
       |
       v
route + path + file preimage + proposal TTL revalidated
       |
       v
backend transactional write
       |
       v
post-write exact verification
       |
       v
success OR rollback/fail closed
```

---

## 4. Frontend authority boundary

React may author only:

For proposal generation:

```
sessionId
reference
instruction
```

For apply/discard:

```
proposalId
```

React must not author:

- source file path;
- project root;
- absolute path;
- source line/column authority;
- edit start/end lines;
- replacement text;
- unified diff;
- source preimage;
- expected hash;
- editor command;
- shell command;
- write mode;
- backup path;
- temporary path;
- provider endpoint;
- provider headers;
- provider model;
- provider secret.

The backend owns all mutation authority.

---

## 5. Provider authority boundary

The AI provider may propose text, but it has no direct mutation capability.

Provider output is untrusted data.

The provider must not receive or control:

- filesystem handles;
- arbitrary file path authority;
- shell tools;
- git commands;
- Tauri commands;
- page actions;
- LocalView control token;
- proposal storage;
- transaction commit/rollback.

Backend validates provider output before even showing it as an applicable proposal.

---

## 6. V2.5 scope: one file, one contiguous edit

The first write-capable wave intentionally stays narrow.

One proposal may modify:

- exactly one trusted source file;
- exactly one contiguous line range;
- inside one bounded source excerpt;
- with one bounded replacement string.

No multi-file patches in V2.5.

No create file.

No delete file.

No rename file.

No chmod.

No package installation.

No lockfile editing unless that lockfile is itself the selected mapped source file, which normal instrumentation should not produce.

No repository-wide refactor.

---

## 7. Why one contiguous edit

A single contiguous edit gives LocalView strong invariants:

- the file target is backend-owned;
- proposal validation is simple and deterministic;
- diff generation is bounded;
- stale-line detection is exact;
- rollback is straightforward;
- visual review is understandable;
- provider output cannot smuggle secondary file paths.

Future waves may extend scope only after this authority model is proven.

---

## 8. Fix remains Human-First

The user sees:

- selected target;
- provider status;
- a concise instruction;
- privacy disclosure;
- proposal summary;
- changed file;
- exact diff;
- Apply;
- Discard;
- status.

The user should not need to understand:

- snapshot version;
- canonical path;
- preimage bytes;
- transaction backup file;
- provider response schema;
- proposal generation token;
- route fence;
- stale preimage detection.

Those mechanics remain backend-owned.

---

## 9. Fix entry points

Potential entry points:

- Inspector Fix button;
- AI panel Fix selection;
- Command Palette `ai.fixSelection`.

All entry points must route to the same canonical Human-First Fix flow.

No entry point may apply immediately.

At most they open the Fix review surface or begin proposal generation after the required disclosure/confirmation interaction.

---

## 10. Explicit source-sharing disclosure

V2.4 does not send source contents by default.

V2.5 necessarily needs source text for useful code edits.

This is a privacy boundary change.

Before proposal generation, the UI must state that:

- a bounded excerpt of the trusted selected source file will be sent to the connected AI provider;
- only one file/excerpt is included;
- repository-wide source is not sent;
- no write occurs until the user reviews and applies the proposal.

The first Fix click should not silently upload source if the disclosure has not been presented.

---

## 11. Proposal generation request

Suggested frontend request:

```ts
interface HumanFixProposalRequest {
  sessionId: string;
  reference: string;
  instruction: string;
}
```

No path.

No source.

No replacement.

No line range.

No provider configuration.

---

## 12. Instruction bounds

The user instruction is human intent.

Initial limits:

- trim surrounding whitespace;
- minimum 1 Unicode scalar;
- maximum 8 KiB UTF-8;
- reject NUL;
- preserve ordinary newlines;
- shell/path-like text remains plain instruction text.

Instruction text never becomes filesystem or shell authority.

---

## 13. Trusted source resolution

Proposal generation must reuse or share the Trusted Open Source V2.3 authority model:

1. validate stable LocalView reference;
2. resolve requested session from backend control authority;
3. read managed-surface canonical route;
4. obtain fresh semantic snapshot;
5. canonicalize snapshot route;
6. require route equality;
7. resolve exactly one matching reference;
8. require trusted source mapping;
9. choose backend-owned project root from session identity;
10. validate project-relative source path;
11. canonicalize project root;
12. canonicalize target file;
13. require target inside project root;
14. require regular file;
15. require mapped source line exists.

V2.5 must not recreate weaker source resolution.

---

## 14. Shared trusted source authority

Prefer refactoring V2.3 source target resolution into a reusable internal module if practical.

Do not introduce a second, subtly different path-validation implementation for Fix.

The following concepts should become shared:

- stable reference validation;
- snapshot source resolution;
- project-relative path validation;
- canonical project root;
- canonical file containment;
- source line/column bounds;
- regular-file requirement;
- project-relative display path.

V2.3 Open Source behavior must remain unchanged.

---

## 15. Symlink policy for writes

Reading/opening a source symlink is less dangerous than mutating through one.

V2.5 write targets must be stricter.

Reject if:

- the source path itself is a symlink;
- any project-relative path component is a symlink;
- canonicalized target escapes the canonical project root.

This avoids writing through mutable symlink chains.

Future support for safe symlinks requires a separate design.

---

## 16. File type policy

V2.5 supports regular text source files only.

Reject:

- directories;
- devices;
- FIFOs;
- sockets;
- symlinks;
- binary files;
- invalid UTF-8.

Do not guess an encoding and rewrite it.

---

## 17. Editable file size bound

Initial maximum source file size for V2.5:

`2 MiB`

Reason:

- proposal preparation may hold exact preimage bytes;
- apply needs exact stale comparison;
- rollback may retain original bytes;
- diff construction must stay bounded.

Files larger than the bound fail closed with a humanized unsupported message.

---

## 18. Exact preimage

At proposal generation time, backend reads the exact full target file bytes after trusted source resolution.

Those bytes are the proposal preimage.

The proposal stores the exact preimage in backend memory or an equivalent exact bounded representation sufficient for byte-for-byte stale comparison and rollback.

Do not trust modification time alone.

Do not trust file length alone.

---

## 19. Proposal TTL

A proposal is short-lived.

Initial TTL:

`5 minutes`

After expiry:

- Apply fails;
- proposal becomes unusable;
- user must generate a new proposal from fresh authority.

Discard may remove it earlier.

---

## 20. One-shot apply

A proposal may be applied at most once.

After:

- successful apply;
- explicit discard;
- failed stale validation;
- expiry;

the proposal cannot later be applied.

Use an opaque backend-generated proposal id.

---

## 21. Proposal storage

Proposal state is backend-owned.

Suggested Tauri managed state:

```rust
struct FixProposalStore {
    proposals: Mutex<...>
}
```

Bound the store.

Suggested limits:

- max active proposals: 8;
- max proposals per session: 2;
- expired proposals reaped on access;
- total retained preimage/replacement memory bounded.

No proposal contents in localStorage.

---

## 22. Proposal identity

Proposal id must be opaque and unguessable enough for in-process capability lookup.

A UUID v4 is acceptable.

Proposal id is not sufficient authority by itself; Apply revalidates the proposal bindings and current source authority.

---

## 23. Proposal binding

Store at minimum:

- proposal id;
- session id;
- stable reference;
- canonical route at generation;
- snapshot version;
- canonical project root;
- canonical source file;
- project-relative display file;
- mapped source line;
- original full file bytes;
- excerpt start/end lines;
- validated edit range;
- replacement;
- generated postimage bytes;
- provider label;
- provider summary;
- created timestamp;
- expiry timestamp;
- state: pending/applied/discarded/invalidated.

Do not send hidden canonical paths back to React.

---

## 24. Source excerpt policy

Provider does not receive the whole source file by default.

Build one bounded excerpt around the mapped source line.

Suggested initial policy:

- up to 80 lines before;
- selected line;
- up to 80 lines after;
- max 24 KiB UTF-8 source text;
- never cross file boundary.

If line-based window exceeds byte budget, shrink deterministically around the selected line.

---

## 25. Excerpt line numbers

Provider-visible excerpt must include absolute file line numbers.

Example conceptual shape:

```
38 | export function Toolbar() {
39 |   ...
40 |   return (
41 |     <button ...
42 |       Deploy
43 |     </button>
44 |   )
45 | }
```

This lets the provider propose a line range without receiving file path authority.

---

## 26. Provider-visible source path

Provider may receive the backend-generated project-relative source locator.

Example:

`src/components/DeployButton.tsx`

Never send the absolute project root.

Never send the user's home directory.

---

## 27. Provider-visible context

Fix may reuse the privacy-minimized V2.4 trusted semantic context plus the bounded source excerpt.

Provider-visible fields may include:

- selection semantic summary;
- project label;
- route path with query/fragment removed;
- bounded issue summaries;
- project-relative source locator;
- source excerpt;
- human instruction.

No repository scan.

No screenshots by default.

No network bodies.

No credentials.

---

## 28. Fix provider schema

V2.5 requires a structured provider response.

Conceptual response:

```json
{
  "summary": "Adjust the disabled-state styling for the Deploy button.",
  "edit": {
    "startLine": 40,
    "endLine": 46,
    "replacement": "..."
  }
}
```

Provider does not return file path.

Provider does not return shell commands.

Provider does not return a git patch containing arbitrary paths.

---

## 29. Edit range semantics

`startLine` and `endLine` are 1-based inclusive line numbers in the trusted target file.

Rules:

- both positive;
- start <= end;
- both inside provider-visible excerpt;
- edit must intersect or remain within a bounded distance from the selected mapped source line;
- initial maximum changed span: 120 original lines.

Insertion-only edits may later use an explicit schema.

V2.5 may support replacement of at least one existing line only for simpler correctness.

---

## 30. Replacement bounds

Initial limits:

- max replacement UTF-8 bytes: 32 KiB;
- max replacement lines: 240;
- reject NUL;
- output file remains <= 2 MiB;
- replacement must be valid UTF-8.

No binary writes.

---

## 31. No provider path authority

If provider output contains a file/path field:

- ignore only if schema parser rejects unknown fields by policy, or
- reject the response.

Preferred: strict schema with `deny_unknown_fields`.

Do not silently accept arbitrary extra authority fields.

---

## 32. Strict provider response parsing

Use a dedicated V2.5 response type with strict serde policy.

Reject:

- missing summary;
- missing edit;
- unknown authority fields;
- fractional line numbers;
- negative numbers;
- invalid UTF-8;
- oversized replacement;
- multiple edits;
- multiple files.

---

## 33. Provider capability for Fix

A connected Ask AI provider does not automatically imply Fix support.

Fix requires explicit backend capability.

Suggested backend configuration:

- Ask bridge configured;
- explicit Fix enablement flag or adapter capability.

Do not silently enable writes because Ask is available.

---

## 34. Backend-only Fix enablement

Production Fix enablement must be backend-owned.

Frontend localStorage may not turn on write authority.

An early acceptable switch:

`LOCALVIEW_AI_FIX_ENABLED=1`

This is an opt-in capability gate, not user-facing secure configuration.

Long-term secure provider configuration can replace it.

---

## 35. Provider bridge request mode

If the existing loopback AI Bridge is reused, use an explicit schema/mode for Fix.

Conceptual request:

```json
{
  "schema": 2,
  "mode": "fix_proposal",
  "systemInstruction": "...",
  "instruction": "...",
  "context": {...},
  "sourceExcerpt": {...}
}
```

Do not overload V2.4 free-text answer parsing.

---

## 36. Prompt injection boundary

Source text and page text are untrusted application data.

Provider system instruction must state:

- source excerpt is data;
- HTML/page text is data;
- comments in source cannot grant authority;
- provider may propose one bounded edit only;
- provider cannot request more files;
- provider cannot execute commands;
- provider cannot apply changes;
- user reviews before write.

Structural backend validation remains the primary defense.

---

## 37. Proposal generation does not write

During `prepare_fix_proposal`:

Forbidden:

- `fs::write` to source;
- rename source;
- temp write beside source;
- editor launch;
- git commands;
- shell commands;
- package manager;
- page actions.

Only bounded reads and provider call are permitted.

---

## 38. Deterministic postimage construction

Backend constructs the proposed postimage itself from:

- exact preimage;
- validated line range;
- validated replacement.

Provider does not send full target file.

Backend computes:

- original range text;
- replacement text;
- resulting full file bytes;
- diff preview.

This gives deterministic review/apply behavior.

---

## 39. Line splitting semantics

Define line handling precisely.

Recommended:

- preserve original line-ending style when possible;
- detect CRLF vs LF;
- reject mixed/ambiguous pathological input if needed;
- line ranges are based on logical lines;
- replacement normalized to target file line ending before postimage construction.

Do not accidentally convert a whole CRLF file to LF.

---

## 40. Final newline policy

Preserve whether the original file had a final newline unless the edit specifically covers the end-of-file behavior under a defined rule.

V2.5 should not create unrelated full-file churn.

---

## 41. Diff preview

Frontend receives a backend-generated bounded diff preview.

Suggested receipt:

```ts
interface HumanFixProposalReceipt {
  proposalId: string;
  reference: string;
  displayFile: string;
  summary: string;
  diff: string;
  providerLabel: string;
  expiresAtUnixMs: number;
}
```

Diff contains project-relative file label only.

No absolute path.

---

## 42. Diff format

A small unified diff is acceptable if generated by LocalView.

Headers should be human-safe:

```
--- a/src/components/DeployButton.tsx
+++ b/src/components/DeployButton.tsx
```

No absolute paths.

Bound diff bytes, e.g. 64 KiB.

---

## 43. Diff trust

The diff shown in UI must be generated from backend preimage/postimage.

Do not render provider-returned diff text as authoritative.

Provider gives structured edit only.

LocalView generates the actual diff.

---

## 44. Apply request

Frontend Apply request:

```ts
interface HumanApplyFixRequest {
  proposalId: string;
}
```

No replacement.

No path.

No expected bytes.

No force flag.

No “ignore stale” flag.

---

## 45. Apply revalidation

Before any write, backend must:

1. look up pending unexpired proposal;
2. mark it applying under store lock or acquire proposal-specific gate;
3. resolve current session;
4. resolve managed canonical route;
5. obtain a fresh semantic snapshot;
6. require current reference still exists exactly once;
7. require source mapping still exists;
8. derive current project root;
9. re-run strict write-target path validation;
10. require same canonical target file as proposal;
11. require route still matches proposal route or defined route policy;
12. read exact current file bytes;
13. require exact byte equality with stored preimage;
14. only then begin write transaction.

Any mismatch invalidates proposal.

---

## 46. No force apply

V2.5 has no force option.

If source changed since proposal:

- Apply fails;
- proposal becomes stale;
- user must regenerate.

Do not attempt fuzzy patching.

Do not auto-rebase.

Do not search for similar text elsewhere.

---

## 47. TOCTOU hardening

Path/file state can change between proposal and apply.

Apply must revalidate:

- canonical root;
- path components;
- no symlink;
- regular file;
- exact preimage.

Do not rely on stored canonical path alone.

---

## 48. Per-file apply gate

Concurrent writes to the same file must be serialized.

Suggested key:

canonical file path inside backend state.

Do not allow two Fix proposals to apply concurrently to one file.

---

## 49. Apply transaction

V2.5 requires a backend-owned same-directory write transaction.

Conceptual sequence:

1. create unique temp file in target directory with `create_new`;
2. write full postimage;
3. flush;
4. `sync_all` temp;
5. copy/preserve original file permissions;
6. re-read target and reconfirm preimage if necessary immediately before swap;
7. create/rename bounded backup state;
8. replace target with temp using platform-safe strategy;
9. read target bytes;
10. require exact postimage;
11. on failure, restore original from backup;
12. sync as practical;
13. delete backup/temp on success;
14. mark proposal applied.

Implementation may use a platform-specific replace helper, but shell is forbidden.

---

## 50. Temporary and backup filenames

Generated backend-only names.

Example:

`.localview-fix-<uuid>.tmp`
`.localview-fix-<uuid>.bak`

Requirements:

- same directory as target;
- create_new where applicable;
- never caller-controlled;
- never provider-controlled;
- cleaned after transaction;
- ignored from provider context.

---

## 51. Permissions

Preserve target file permissions.

V2.5 must not make an executable file non-executable or vice versa as a side effect.

If permission preservation fails before commit, fail closed.

---

## 52. Metadata expectations

V2.5 guarantees source contents, not all filesystem metadata.

Document platform limitations if timestamps/inode change.

Do not claim atomicity stronger than actual implementation.

---

## 53. Rollback

If replacement/post-write verification fails after original was moved/replaced:

- restore original bytes/path;
- return failure;
- mark proposal invalidated;
- do not leave temp/backup debris where avoidable.

Rollback path must be unit tested with injected filesystem operations where practical.

---

## 54. No git requirement

Fix must work in a project without git.

Git root may be used as project identity/root when already available, but:

- no `git apply`;
- no `git checkout`;
- no automatic commit;
- no stash;
- no reset.

Rollback is LocalView transaction-owned.

---

## 55. No shell

Explicitly forbidden in Fix core:

- `sh -c`;
- `cmd /C`;
- PowerShell command strings;
- shell redirect;
- patch executable;
- sed/perl replacements;
- editor CLI as writer.

Use Rust filesystem APIs only.

---

## 56. Apply receipt

Suggested receipt:

```ts
interface HumanApplyFixReceipt {
  proposalId: string;
  reference: string;
  displayFile: string;
  applied: true;
  changedStartLine: number;
  changedEndLine: number;
  appliedAtUnixMs: number;
}
```

No absolute path.

---

## 57. Discard

Frontend may discard by proposal id.

Discard:

- removes proposal;
- releases preimage/postimage memory;
- never writes;
- idempotent human behavior is acceptable.

---

## 58. Proposal expiry UI

Show expiry in human terms only if useful.

If Apply discovers expiry:

- show “Proposal expired. Generate a new fix.”

Do not expose backend timestamps as primary UX.

---

## 59. Fix state model

Conceptual state:

```ts
type HumanFixState =
  | { status: 'idle' }
  | { status: 'disclosure'; reference: string }
  | { status: 'proposing'; reference: string; instruction: string }
  | { status: 'proposal'; proposalId: string; ... }
  | { status: 'applying'; proposalId: string }
  | { status: 'success'; displayFile: string }
  | {
      status: 'failure';
      reason:
        | 'provider_unavailable'
        | 'source_unavailable'
        | 'invalid_instruction'
        | 'proposal_invalid'
        | 'proposal_expired'
        | 'source_changed'
        | 'apply_failed'
        | 'failed'
    };
```

---

## 60. Stale selection isolation

Proposal completion is bound to:

- session id;
- reference;
- generation.

If selection changes while proposal generation is in flight:

- old completion must not attach to new selection;
- it may be discarded from active UI;
- backend proposal should be explicitly discarded when practical.

---

## 61. Session switch isolation

If session changes:

- active proposal-generation UI resets;
- current proposal review is invalidated in active surface;
- Apply still revalidates backend binding and should fail if stale.

No cross-session proposal reuse.

---

## 62. Route change isolation

Proposal stores canonical route.

If route changes before Apply:

Initial V2.5 policy: proposal is stale and Apply fails.

This is strict but understandable.

Later versions may relax if source identity remains provably stable.

---

## 63. Source mapping change

If fresh snapshot maps the same reference to a different file or line before Apply:

Fail stale.

Do not apply old proposal to newly mapped source.

---

## 64. File content change

If exact current bytes differ from proposal preimage:

Fail stale.

Even unrelated external edits invalidate V2.5 proposal.

This avoids accidental overwrite.

---

## 65. Duplicate proposal generation

While a proposal request is active for current selection:

- Generate button disabled;
- `aria-busy=true`;
- repeated clicks suppressed.

---

## 66. Duplicate apply

While Apply is in progress:

- Apply disabled;
- Discard disabled or carefully serialized;
- repeated clicks do not create duplicate writes.

Backend one-shot proposal state is the final protection.

---

## 67. Failure isolation

Fix failure must not break:

- Inspect;
- Open Source;
- Measure;
- Capture;
- Ask AI;
- Responsive;
- Console;
- Network;
- Settings;
- Advanced.

Mutation capability is auxiliary.

---

## 68. Human review requirement

V2.5 must display the diff before Apply.

Do not provide a hidden setting to auto-apply AI fixes.

Do not auto-apply “small” fixes.

Do not auto-apply because confidence is high.

Human review remains required.

---

## 69. Apply button semantics

Apply is a consequential action.

UI requirements:

- clear label;
- disabled while stale/busy;
- visually distinct but not alarming;
- no accidental Enter-key apply from textarea;
- keyboard reachable;
- confirmation may be the review screen itself if interaction is explicit.

Do not hide Apply behind ambiguous icon-only UI.

---

## 70. Discard semantics

Discard must be easy and non-destructive.

Closing a proposal panel may discard or preserve pending proposal depending on UX, but behavior must be deterministic.

For V2.5, explicit Discard is preferred and closing the panel may leave proposal pending until TTL unless memory policy says otherwise.

---

## 71. Source excerpt display

The user need not see the entire excerpt sent to provider, but should be able to understand which file is involved.

The diff is the primary review artifact.

Optionally show a privacy disclosure such as:

“Fix uses a bounded excerpt of this source file.”

---

## 72. AI panel

V2.5 may extend the existing AI panel with a Fix tab/state.

Minimum:

- provider/Fix capability;
- selected target;
- instruction;
- disclosure;
- Generate proposal;
- summary;
- diff;
- Apply;
- Discard;
- status.

---

## 73. Inspector Fix

Inspector Fix button should open/focus the Fix review flow for current stable selection.

It must not immediately mutate.

If Fix provider capability unavailable:

- disabled or opens a calm unavailable explanation.

---

## 74. Command Palette

`ai.fixSelection` routes through the same canonical Fix review flow.

No direct apply.

No separate mutation command path.

---

## 75. Verify change remains separate

V2.5 applies source changes but does not claim visual correctness automatically.

The existing `ai.verifyChange` may remain disabled unless a separate trusted verification design is implemented.

Post-write byte verification is not the same as verifying the UI fix.

Do not conflate them.

---

## 76. HMR/runtime behavior

After Apply, LocalView may observe HMR naturally.

V2.5 may show “Applied” immediately after exact file verification.

It must not claim the target app successfully updated unless there is specific runtime evidence.

---

## 77. Optional lightweight runtime observation

If implemented, a post-apply informational state may wait for:

- HMR start/settle;
- fresh snapshot availability.

But absence must not trigger destructive rollback because a valid source edit may temporarily break runtime behavior.

Filesystem rollback is for transaction failure, not for application semantic failure.

---

## 78. Provider error sanitization

Frontend must never receive:

- provider secret;
- Authorization header;
- raw provider request;
- absolute path;
- full source preimage;
- hidden backup/temp path;
- stack trace.

Return bounded reason codes.

---

## 79. File write error sanitization

Human-facing errors:

- source changed;
- proposal expired;
- could not apply;
- source unavailable.

Do not expose OS path strings by default.

Detailed diagnostics may be available in Advanced with absolute-path care.

---

## 80. Logging

Do not info-log:

- full source excerpt;
- full replacement;
- full preimage/postimage;
- provider token.

Safe diagnostic fields:

- proposal id;
- session id;
- project-relative display file;
- range;
- byte counts;
- reason codes;
- timings;
- proposal state transition.

---

## 81. Sensitive content

Source code itself may contain secrets.

V2.5 source excerpt builder should add basic minimization where feasible, but source semantics are difficult to perfectly redact without corrupting code.

Therefore the disclosure is mandatory.

At minimum:

- never include neighboring files;
- never include environment files by path unless they are the actual mapped selected source and allowed by source policy;
- consider rejecting obviously sensitive filenames.

---

## 82. Sensitive filename denylist

Initial proposal generation should reject mapped files with sensitive basename patterns such as:

- `.env`;
- `.env.*`;
- credential files;
- private key extensions;
- known secret-store files.

Exact denylist must be conservative and tested.

A UI element should normally never map to such a file; denial is defense in depth.

---

## 83. Source extension allowlist

For first production write wave, an allowlist is safer than arbitrary text files.

Suggested initial web-source extensions:

- ts
- tsx
- js
- jsx
- css
- scss
- html
- htm
- vue
- svelte
- json

Consider whether JSON should be included initially.

Do not include executable scripts/configs by default unless explicitly justified.

---

## 84. V2.5 recommended initial allowlist

To minimize risk, start with:

- `.ts`
- `.tsx`
- `.js`
- `.jsx`
- `.css`
- `.scss`
- `.html`
- `.vue`
- `.svelte`

No shell.

No PowerShell.

No Python initially.

No CI YAML initially.

No package manager manifests initially.

Expansion can follow after evidence.

---

## 85. Provider summary

Provider may return a concise summary.

Bound:

- max 1 KiB;
- plain text;
- advisory.

Summary is not authority.

---

## 86. Diff bound

Backend-generated diff:

- max 64 KiB UTF-8;
- reject proposal if diff exceeds budget rather than silently truncate the review artifact.

Human must be able to review the entire authorized edit.

---

## 87. Proposal context version

Introduce explicit Fix context/proposal schema version.

Suggested:

`FIX_CONTEXT_VERSION = 1`

`FIX_PROPOSAL_SCHEMA = 1`

Version changes when privacy or edit semantics change materially.

---

## 88. Tauri commands

Prefer exact commands:

```
ai_fix_capability
prepare_fix_proposal
apply_fix_proposal
discard_fix_proposal
```

No:

- `write_file`;
- `apply_patch`;
- `save_text`;
- `run_command`;
- `edit_path`;
- `replace_file`.

The webview gets proposal workflow capability, not filesystem primitives.

---

## 89. Tauri permission surface

Add only exact Fix commands.

Do not expose generic filesystem APIs.

No shell plugin.

No arbitrary HTTP endpoint from React.

---

## 90. Backend module separation

V2.5 should live primarily in a dedicated module.

Suggested:

`apps/desktop/src-tauri/src/trusted_fix.rs`

Responsibilities:

- instruction validation;
- file policy;
- source excerpt;
- provider Fix request/response;
- edit validation;
- postimage construction;
- diff generation;
- proposal storage types;
- transaction logic;
- apply stale checks.

Shared V2.3 source authority may move to `trusted_source.rs` if refactoring is safe.

---

## 91. Refactoring rule

Do not destabilize V2.3 merely to make code prettier.

If source-authority extraction is performed:

- preserve exact existing V2.3 tests;
- add shared-unit tests;
- ensure Open Source V2.3 exact behavior remains.

A bounded duplication may temporarily be safer than a broad refactor, but security-critical validation should converge to one implementation before V2.5 merge if possible.

---

## 92. Provider adapter

Extend V2.4 bridge abstraction, not a vendor SDK.

No OpenAI-specific UI contract.

No Anthropic-specific UI contract.

No model names in React authority.

---

## 93. Fix capability query

Suggested frontend API:

```ts
aiFixCapability(): Promise<AiFixCapability>
```

Capability may include:

- available;
- provider label;
- reason.

No secrets.

---

## 94. Proposal API

Suggested:

```ts
prepareFixProposal({
  sessionId,
  reference,
  instruction,
}): Promise<HumanFixProposalReceipt>
```

Intent-only.

---

## 95. Apply API

Suggested:

```ts
applyFixProposal({
  proposalId,
}): Promise<HumanApplyFixReceipt>
```

Opaque id only.

---

## 96. Discard API

Suggested:

```ts
discardFixProposal({ proposalId }): Promise<void>
```

No write.

---

## 97. No full proposal trust in frontend

React may display:

- diff;
- file label;
- summary.

But when Apply is clicked, it sends only proposal id.

Do not echo replacement back to backend.

Do not rebuild patch client-side.

---

## 98. Dedicated RED contract

Add:

`apps/desktop/src-tauri/tests/human_first_trusted_fix_v25_contract.rs`

Initial RED assertions:

- canonical V2.5 spec exists;
- request is `sessionId + reference + instruction`;
- apply request is proposal id only;
- no frontend path/replacement authority;
- dedicated trusted Fix module exists;
- proposal store exists;
- source excerpt bounded;
- source extension policy exists;
- sensitive path denial exists;
- exact preimage stored;
- proposal TTL exists;
- one-file/one-edit schema exists;
- strict provider schema exists;
- provider cannot return path authority;
- diff generated backend-side;
- Apply re-resolves fresh source authority;
- route/source/preimage stale checks exist;
- no force option;
- transactional write helper exists;
- rollback path exists;
- shell forbidden;
- Fix state/generation fencing exists;
- Inspector/AI panel/command palette share same flow;
- Apply is separate user action;
- Verify remains separate;
- localization exists;
- runtime audit markers exist.

RED first.

---

## 99. Dedicated workflow

Add:

`.github/workflows/human-first-trusted-fix-v25.yml`

Run on Fix spec/code/UI/test changes.

Minimum steps:

1. V2.5 contract;
2. trusted Fix unit tests;
3. V2.4 Ask AI regression;
4. V2.3 Open Source regression;
5. V2.2 Measure regression;
6. V2.1 Capture regression;
7. frontend build.

---

## 100. Transaction unit tests

Use temporary directories and injectable operations where possible.

Test:

- normal apply;
- exact preimage mismatch;
- route/source stale upstream helpers;
- temp create failure;
- temp write failure;
- sync failure abstraction if injectable;
- backup/replace failure;
- post-write mismatch;
- rollback success;
- cleanup;
- permission preservation;
- no symlink writes;
- same-file concurrent apply serialization.

---

## 101. Provider proposal tests

Test:

- valid single edit;
- unknown field;
- provider path field;
- multiple edits;
- start before excerpt;
- end after excerpt;
- start > end;
- changed span too large;
- replacement too large;
- output file too large;
- NUL;
- empty summary;
- valid Unicode.

---

## 102. Source excerpt tests

Test:

- selected line center;
- beginning of file;
- end of file;
- 24 KiB shrink;
- line numbers preserved;
- CRLF;
- LF;
- final newline;
- invalid UTF-8;
- file >2 MiB;
- sensitive filename;
- unsupported extension.

---

## 103. Proposal store tests

Test:

- insert;
- max capacity;
- per-session capacity;
- expiry;
- discard;
- one-shot apply state;
- invalidation;
- concurrent lookup/apply gate;
- memory release after terminal state.

---

## 104. Stale authority tests

Before apply simulate:

- selection gone;
- duplicate reference;
- source mapping gone;
- source file changed;
- route changed;
- source maps to another file;
- mapped line changed;
- project root changed;
- symlink inserted after proposal;
- target replaced with directory;
- target replaced with unsupported file.

All must fail closed.

---

## 105. Render/runtime audit

Extend Human-First render audit beyond V2.4 state 83.

Minimum new states:

84. Fix provider unavailable.
85. Fix disclosure.
86. Fix ready with stable selection.
87. No selection.
88. No session.
89. Empty instruction.
90. Oversized instruction.
91. Proposal generating.
92. Duplicate proposal suppressed.
93. Proposal success.
94. Diff visible.
95. Proposal provider failure.
96. Source unavailable.
97. Sensitive source refused.
98. Unsupported extension refused.
99. Stale selection before proposal completion.
100. Stale session before proposal completion.
101. Apply ready.
102. Apply in progress.
103. Duplicate Apply suppressed.
104. Apply success.
105. Apply source-changed failure.
106. Apply route-changed failure.
107. Apply expired failure.
108. Apply transaction failure.
109. Apply raw OS/provider error hidden.
110. Discard.
111. Discard performs no write.
112. Vietnamese disclosure.
113. Vietnamese proposal success.
114. Vietnamese apply success.
115. Narrow viewport diff review.
116. Fix failure does not break Ask/Measure/Capture/Open Source.
117. Command Palette no-selection disabled.
118. Command Palette Fix unavailable disabled.
119. Command Palette routes to same review flow.
120. Apply request contains proposal id only.
121. Prepare request contains intent only.
122. No caller replacement/path authority.
123. No hidden auto-apply.
124. Verify remains separate/unavailable unless independently implemented.

The audit must execute behavioral assertions, not screenshot-only checks.

---

## 106. Filesystem integration evidence

Ordinary browser render harness cannot prove filesystem transaction correctness.

Dedicated Rust integration tests are required.

Do not use render screenshots as evidence of actual disk write correctness.

---

## 107. Real tempdir tests

Use real filesystem temp directories in Rust tests to prove:

- bytes before;
- proposal postimage;
- apply;
- bytes after;
- stale refusal;
- rollback.

Avoid mocking everything.

---

## 108. Cross-platform closure

Because file replacement semantics differ by OS, full CI Linux/Windows/macOS is mandatory.

A Linux-only transaction implementation is not sufficient.

Windows is especially important because rename/replace semantics differ.

---

## 109. Windows strategy

Do not assume POSIX rename replacement works on Windows.

Implementation must use a tested Windows-compatible strategy.

If a fully robust cross-platform replace cannot be implemented in V2.5, fail closed on unsupported platform rather than pretend.

The goal is cross-platform support, but correctness outranks feature parity.

---

## 110. macOS/Linux strategy

Same-directory temp + controlled replace is expected, but test actual behavior.

Do not rely on shell utilities.

---

## 111. Native GUI smoke

Fix changes must not break:

- WebKitGTK;
- WKWebView;
- WebView2.

Keep existing native smoke green.

---

## 112. Windows provider regressions

Keep:

- Windows UIA Observe GREEN;
- Windows Real Provider Seeds GREEN;
- applicable V4.3 contracts GREEN.

Fix must not weaken observation/provider authority.

---

## 113. No production fake provider

Same rule as V2.4.

Fake Fix provider exists only in tests/render audit.

If Fix bridge capability is not configured:

- UI says unavailable;
- no fake proposal.

---

## 114. No paid CI requirement

Deterministic fake provider tests are sufficient for required CI.

Do not require external paid model calls or secrets for merge.

---

## 115. Localization

Add all new Fix primary-flow strings to all supported locales.

Concepts:

- Fix this;
- Fix unavailable;
- Generate fix;
- Source excerpt disclosure;
- Instruction;
- Generating proposal;
- Review proposed change;
- Apply;
- Discard;
- Applied;
- Proposal expired;
- Source changed;
- Proposal no longer valid;
- Could not generate;
- Could not apply;
- Sensitive source unsupported;
- Unsupported source type;
- AI proposal is advisory;
- No code changes until Apply.

No mixed-language primary flow.

---

## 116. Accessibility

Diff review:

- keyboard scrollable;
- readable line wrapping/overflow behavior;
- labels not color-only;
- added/removed semantics available without relying solely on red/green.

Apply/Discard:

- accessible names;
- disabled states;
- busy state;
- clear focus.

---

## 117. Narrow viewport

At narrow width:

- diff remains horizontally usable;
- buttons remain reachable;
- no whole-page horizontal overflow;
- file label truncates safely;
- Apply/Discard remain visible or accessible through scrolling.

---

## 118. Reduced motion

Fix flow must not depend on animation.

---

## 119. Styling

Keep Human-First muted LocalView visual language.

Do not turn Fix into a flashy autonomous-agent experience.

Consequential action can use restrained emphasis.

---

## 120. Security checklist before merge

Verify:

- no generic write command;
- no shell;
- no frontend path;
- no frontend replacement;
- no provider path authority;
- one file only;
- one edit only;
- bounded excerpt;
- bounded edit;
- strict extension policy;
- sensitive file denial;
- no symlink write;
- exact preimage;
- proposal TTL;
- one-shot apply;
- fresh source re-resolution;
- route fence;
- exact file stale check;
- transactional write;
- rollback;
- backend-generated diff;
- explicit user Apply;
- no auto-apply;
- no Verify overclaim.

---

## 121. Privacy checklist before merge

Answer concretely:

- Which source text leaves the machine?
- Exactly one bounded excerpt.
- Is the whole repository sent? No.
- Is absolute path sent? No.
- Are unrelated files sent? No.
- Does proposal generation write? No.
- Does provider write? No.
- Does LocalView write without Apply? No.
- Is provider secret in frontend? No.

---

## 122. RED → GREEN implementation order

Required order:

1. canonical V2.5 spec;
2. Draft PR;
3. RED V2.5 contract;
4. dedicated V2.5 gate;
5. source authority sharing/extraction as needed;
6. trusted Fix core types;
7. instruction/file/excerpt policy;
8. strict provider proposal schema;
9. postimage + diff generation;
10. proposal store;
11. filesystem transaction + rollback tests;
12. fresh apply revalidation;
13. Tauri capability/proposal/apply/discard commands;
14. frontend intent-only API;
15. shell state/generation fencing;
16. Fix panel/review;
17. Inspector wiring;
18. Command Palette wiring;
19. localization;
20. render/runtime audit;
21. V2.4/V2.3/V2.2/V2.1 regressions;
22. full cross-platform CI;
23. Windows UIA/provider closure;
24. exact-head artifact/evidence closure;
25. merge.

---

## 123. Merge gate

Do not mark the PR ready until one immutable exact head has:

- V2.5 contract GREEN;
- trusted Fix core/unit/integration tests GREEN;
- V2.4 Ask AI regression GREEN;
- V2.3 Open Source regression GREEN;
- V2.2 Measure regression GREEN;
- V2.1 Capture regression GREEN;
- frontend build GREEN;
- V2.5 render audit GREEN;
- full CI GREEN on Linux/Windows/macOS;
- WebKitGTK/WKWebView/WebView2 smoke GREEN;
- Windows UIA Observe GREEN;
- Windows Real Provider Seeds GREEN.

If head changes, regenerate evidence.

---

## 124. PR closure evidence

Record:

- exact head SHA;
- render screenshot count;
- executable check count;
- render artifact digest;
- dedicated gate run;
- full CI 7/7;
- Windows gate results;
- filesystem integration test evidence.

Do not call complete before these exist.

---

## 125. Rollback of the feature wave

The V2.5 feature should be revertable without breaking V2.4 Ask AI.

If V2.5 is reverted:

- Fix returns to unavailable;
- Ask AI remains functional;
- Open Source/Measure/Capture remain functional.

Keep write authority isolated enough to support this.

---

## 126. Future V2.6 boundary

Likely next wave:

Trusted Verify Change.

That wave can design:

- post-fix HMR settling;
- fresh semantic comparison;
- targeted capture;
- visual/semantic assertions;
- provider-assisted verification;
- rollback suggestion.

Do not smuggle V2.6 into V2.5.

---

## 127. Continuation protocol for future AI

A future AI resuming V2.5 must:

1. read this spec;
2. fetch current PR/head;
3. inspect all Fix commits;
4. inspect dedicated gate/render runs;
5. preserve two-phase Proposal → Apply;
6. preserve intent-only frontend requests;
7. preserve one-file/one-edit scope;
8. preserve explicit source-sharing disclosure;
9. preserve backend-only write authority;
10. preserve exact preimage stale checks;
11. preserve transactional rollback;
12. keep Verify separate;
13. never use GREEN from an older head;
14. never weaken a failing contract merely to make CI green;
15. update this spec if architecture materially changes.

If another process advances the branch, re-read before mutation.

---

## 128. Definition of done

Trusted Fix V2.5 is complete only when a developer can select a LocalView element, explicitly request a bounded AI fix proposal, review the exact backend-generated diff, and apply it through a one-shot backend transaction while LocalView proves that:

- source identity came from fresh trusted LocalView authority;
- provider received only one bounded source excerpt;
- provider had no filesystem authority;
- React never authored path or replacement;
- no write occurred before explicit Apply;
- apply revalidated route/source/file preimage;
- stale proposals failed closed;
- write was transactionally verified;
- failure could roll back original bytes;
- Fix failure did not damage other LocalView capabilities;
- all required exact-head cross-platform evidence was GREEN.
