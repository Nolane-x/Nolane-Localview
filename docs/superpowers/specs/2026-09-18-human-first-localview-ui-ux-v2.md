# Nolane LocalView — Human-First UI/UX V2 Canonical Specification

## Status

**Canonical design and execution contract. Foundation and bounded Human-First V2 closures are merged; continuation is evidence-gated.**

This document is the durable source of truth for the Human-First LocalView UI/UX V2 wave. It is intentionally stored in the repository so a later AI can recover the project without depending on chat history.

Repository: `Nolane-x/Nolane-Localview`

Historical foundation:
- `#142 — feat: human-first LocalView UI/UX V2 foundation` — merged;
- `#143 — Trusted Capture V2.1` — merged;
- `#144 — Trusted Measure V2.2` — merged;
- `#145 — Trusted Open Source V2.3` — merged;
- `#146 — Trusted Ask AI V2.4` — merged;
- `#147 — Trusted Fix V2.5` — merged;
- `#148 — Trusted Verify Change V2.6` — merged;
- `#149 — Human-First Chrome Position Persistence closure` — merged.

The branch/base/head recorded at spec creation are historical provenance, not the continuation frontier. Future work must first read the current `main` head and any current Human-First continuation PR, then use exact-head executable evidence as implementation truth while this file defines the intended product and acceptance boundaries.

---

## 1. Why this wave exists

LocalView already has a strong machine-facing substrate: project/session discovery, observer evidence, native webview control, source/runtime context, visual capture, platform-specific verification, and guarded full-page stitching.

The previous desktop UI exposed too much of that machinery directly to humans. Runtime identity, semantic snapshots, observer status, evidence cards, project paths and diagnostic vocabulary competed with the actual task: inspect a local app, understand what is on screen, capture evidence, open source, test responsive behavior and make changes.

Human-First V2 changes the product hierarchy without weakening the machine substrate.

The core rule is:

> **The machine may remain complex; the human surface must feel calm, obvious and task-oriented.**

Diagnostics are not deleted. They move behind an explicit Advanced surface.

---

## 2. Product outcome

A person opening LocalView should be able to understand the primary workspace without knowing LocalView's internal architecture.

The default experience must answer, in order:

1. What app/target am I looking at?
2. What can I do to it?
3. What is currently selected?
4. Can I open its source?
5. Can I measure or capture it?
6. Can I test responsive behavior?
7. Can I use AI on the current selection?
8. Where are settings?
9. Where can I find deeper diagnostics if I need them?

Machine-oriented concepts must not dominate the default inspector or top-level chrome.

---

## 3. Non-negotiable design principles

### 3.1 Human-first, not capability-reduced

Do not remove evidence, observer or runtime capabilities merely to simplify the UI.

Instead:

- move machine diagnostics to `Advanced`;
- use progressive disclosure;
- preserve the underlying command/action paths;
- keep source, inspect, responsive, console, network, AI and capture functionality available.

### 3.2 Quiet visual language

The default UI must avoid the generic “AI dashboard” appearance.

Required visual direction:

- graphite / neutral dark surfaces;
- muted moss accent;
- low-noise borders;
- restrained elevation;
- minimal glow;
- no gratuitous neon blue/purple gradients;
- no decorative AI sparkle language as the main product identity;
- status should be legible without turning the shell into a monitoring console.

Current canonical accent family begins with:
`--lv-accent: #9aa982`

Future visual tuning may change exact values only if it preserves the quiet, non-AI-dashboard direction and passes visual evidence review.

### 3.3 LocalView remains a tool, not a dashboard

The viewport and inspected application are the primary object.

Chrome should occupy only the space necessary for action and orientation.

### 3.4 Progressive disclosure

Default surfaces: human tasks.

Advanced surface: implementation/runtime diagnostics.

A normal workflow must not require reading:

- semantic snapshot internals;
- raw project identity;
- observer event counts;
- evidence provenance;
- transport/backend names;
- internal pipeline labels.

Those remain accessible in Advanced.

### 3.5 No fake controls

Every visible primary control must either:

- perform its real action;
- route to an existing real surface;
- or be explicitly disabled with a reason when the capability is unavailable.

No decorative buttons that imply functionality not wired to product behavior.

---

## 4. Information architecture

The Human-First V2 workspace consists of these conceptual layers.

### 4.1 Managed preview

The inspected local application remains visually dominant.

### 4.2 Target bar

Purpose:

- identify the current local target;
- expose the minimum target/session state needed by a human;
- provide target-level actions;
- remain hideable.

Requirements:

- visibility controlled by `preferences.showTargetBar`;
- keyboard command exists to toggle it;
- hidden state persists;
- hiding the bar must not terminate or alter the session;
- machine status strings such as “observer idle” or “Native observer attached” must not appear as primary chrome copy.

### 4.3 Tool rail

Primary top-level tools:

- Inspect
- Responsive
- Console
- Network
- AI
- More / Advanced
- Settings

The rail must be hideable independently from the target bar.

### 4.4 Floating/bottom tool surfaces

Inspector, Responsive, AI, Settings and Advanced are task panels.

Console and Network may use bottom-sheet behavior where appropriate because their data naturally benefits from width.

### 4.5 Command palette

The command palette must operate on canonical command identifiers rather than duplicate one-off UI behavior.

The command registry must remain the central vocabulary for human-facing workspace actions.

---

## 5. Canonical command model

Current command IDs are defined in:
`apps/desktop/src/commands.ts`

The V2 contract includes at minimum:

- `workspace.chrome.toggle`
- `workspace.targetBar.toggle`
- `workspace.toolRail.toggle`
- `workspace.resetLayout`
- `session.switch`
- `session.pauseDiscovery`
- `preview.openNative`
- `inspect.activate`
- `source.open`
- `responsive.open`
- `console.open`
- `network.open`
- `ai.askSelection`
- `ai.fixSelection`
- `advanced.open`
- `settings.open`
- `language.change`

Future UI actions should prefer extending this registry rather than inventing hidden parallel action semantics.

---

## 6. Inspector contract

The default Inspector is for the human's current task, not for debugging LocalView itself.

### 6.1 Default Inspector content

When a target/selection is available, the panel should prioritize:

- current target or selected element;
- source availability;
- Open source;
- Measure;
- Capture;
- Ask AI;
- Fix.

When no selection exists, the UI should say so in human language.

When no local target exists, the empty state should tell the user to run/open a development server or target, not explain daemon internals.

### 6.2 Forbidden default Inspector content

The default Inspector must not foreground:

- Semantic Snapshot;
- Project identity;
- X-Ray pipeline;
- raw `EvidenceCard`;
- observer internals.

These belong in Advanced.

### 6.3 Capability unavailable behavior

Unavailable actions must be disabled with useful affordance/reason.

Example: Open source may be disabled when the observer has not provided a source location.

The UI must not fabricate a source path.

---

## 7. Advanced diagnostics contract

Advanced is the explicit boundary between human workflow and machine/runtime diagnostics.

It may contain:

- project identity;
- semantic/evidence data;
- observer state;
- diagnostic event information;
- evidence cards;
- runtime/backend detail needed for debugging LocalView.

Moving diagnostics to Advanced is not permission to reduce evidence quality.

Advanced remains first-class and testable; it is simply not the default conceptual model shown to every user.

---

## 8. Settings contract

Settings must be a real tool surface, not a placeholder.

At minimum it owns:

- interface language;
- target bar visibility;
- tool rail visibility;
- workspace reset.

The preference model is defined in:
`apps/desktop/src/preferences.ts`

Current V2 preference schema includes:

```text
version
locale
showTargetBar
showToolRail
rememberChromePositions
targetBarPosition
toolRailPosition
annotationPersistence
notifications
autoOpen
reducedMotion
density
accent
```

Settings UI may expose fields incrementally, but persisted values must remain schema-safe and future-compatible.

---

## 9. Preference persistence and safety

Canonical storage key:
`localview.preferences.v2`

Persistence is best-effort and must never make LocalView unusable.

Rules:

1. malformed JSON falls back safely;
2. unsupported locales normalize safely;
3. non-finite chrome coordinates are rejected;
4. missing fields receive explicit defaults;
5. enum-like fields are allow-listed;
6. unknown accent values cannot silently produce arbitrary visual modes in V2;
7. Reset Workspace resets workspace layout/chrome, not unrelated user choices unless explicitly specified.

Current default intent:

- target bar visible;
- tool rail visible;
- remember positions enabled;
- annotations session-scoped;
- notifications important-only;
- auto-open first session;
- system motion preference;
- comfortable density;
- muted moss accent.

---

## 10. Chrome position persistence

V2 includes a preference foundation for remembering target-bar and tool-rail positions.

If drag/reposition behavior is implemented or extended, it must obey all of the following:

- coordinates must be finite;
- restored chrome must be clamped to the current usable window;
- a monitor/resolution change must not strand controls off-screen;
- resetting workspace clears saved chrome positions;
- persistence must not affect the inspected page layout;
- drag handles must be keyboard/accessibility compatible or the reposition feature must remain non-essential.

Do not blindly trust stale persisted coordinates.

---

## 11. Localization contract

Localization foundation lives in:
`apps/desktop/src/i18n.ts`

Default locale:
`en`

Current supported locale set:

- `en`
- `vi`
- `zh-CN`
- `zh-TW`
- `ja`
- `ko`
- `es`
- `fr`
- `de`
- `pt-BR`
- `id`
- `th`

### 11.1 Fallback rules

English is the canonical fallback.

Missing translation keys must never blank the interface.

### 11.2 Locale normalization

Browser/system variants must normalize into supported LocalView locales.

Examples:

- Chinese Traditional variants -> `zh-TW`;
- other Chinese variants -> `zh-CN`;
- Portuguese variants -> `pt-BR` in V2;
- unsupported locales -> `en`.

### 11.3 Document language

Changing the locale must update:
`document.documentElement.lang`

V2 currently assumes left-to-right layout because no RTL locale is in the supported set.

If RTL locales are added later, direction must become locale-aware rather than hard-coded.

### 11.4 No split-language primary flow

Primary chrome introduced or touched by V2 should be routed through the localization vocabulary.

Do not leave a panel half-localized when the untranslated string is part of its main task flow.

Diagnostic/Advanced copy can migrate in a later explicit wave, but user-facing Settings/Inspector/target/tool controls should converge on translation keys in this wave.

---

## 12. Empty, loading, paused and unavailable states

Every main surface needs calm state handling.

Required categories:

- no target;
- target ready;
- discovery paused;
- source unavailable;
- AI provider unavailable;
- no current selection;
- transient action in progress;
- recoverable error.

Rules:

- do not display internal exception dumps in primary UI;
- provide a direct next action where one exists;
- do not present unavailable AI as an application failure;
- maintain the inspected page even when auxiliary features fail.

---

## 13. AI surface contract

AI is one tool among several; it must not become the visual identity of LocalView.

The AI panel may provide actions such as:

- Ask selection;
- Explain issue;
- Fix selection;
- Verify change.

When no AI provider is connected, show a clear unavailable state.

Do not pretend a provider is connected.

Human-first V2 does not authorize hard-wiring LocalView to a single model/provider.

---

## 14. Source interaction

“Open source” is a primary human action.

Source identity must come from trusted runtime/observer evidence already produced by LocalView.

The UI must never invent source locations based solely on displayed DOM text or guessed project structure.

When source is unavailable, disable the action and explain that source mapping is unavailable.

Future implementation may add:

- source file;
- line/column;
- IDE/editor routing;
- preview of source context.

Those additions must preserve trusted provenance.

---

## 15. Measure and capture actions

Measure and Capture are primary inspector actions because they map visual inspection to concrete evidence.

Measure must use LocalView's existing geometry authority rather than reimplementing approximate DOM-only geometry in the frontend.

Capture must route into the existing native visual-capture contracts.

Human-First V2 must not weaken:

- private masking;
- native capture provenance;
- viewport authority;
- full-page fail-closed rules;
- evidence registration.

The UI can simplify vocabulary while the backend remains strict.

---

## 16. Full-page stitching relationship

Guarded Full-Page Stitching is already an independent completed design lineage.

Canonical files:

- `docs/superpowers/specs/2026-09-17-guarded-full-page-stitching-design.md`
- `docs/superpowers/plans/2026-09-17-guarded-full-page-stitching.md`

Human-First V2 may expose a simpler capture action, but it must not bypass or rewrite those invariants.

Do not replace native capture with Playwright/Chromium merely to simplify UI behavior.

---

## 17. Accessibility contract

Human-first requires actual accessibility, not only visual simplicity.

At minimum:

- interactive elements use semantic controls;
- icon-only controls have accessible names;
- focus is visible;
- panels can be closed without a pointer;
- hidden chrome remains recoverable via keyboard/command palette;
- disabled controls communicate why where practical;
- text/background contrast remains usable;
- reduced-motion preference is respected by non-essential animation;
- keyboard shortcuts must not trap focus or block common text editing behavior.

Any visual refinement that harms accessibility is not an acceptable V2 improvement.

---

## 18. Keyboard and recoverability

Users must not be able to permanently hide essential chrome with no recovery path.

At minimum:

- target bar has a keyboard/command toggle;
- tool rail has a keyboard/command toggle or is recoverable through the canonical workspace toggle/palette;
- command palette remains a recovery surface;
- Reset Workspace restores the default visible workspace.

Current contract includes `Ctrl+Shift+T` for target-bar toggling.

Future shortcut changes must update tests and user-facing discoverability together.

---

## 19. Visual system

### 19.1 Tone

Target feel:

- calm;
- precise;
- tool-like;
- native-adjacent;
- low distraction;
- visually subordinate to the inspected app.

### 19.2 Color

V2 intentionally moves away from bright AI-blue as the product accent.

Current direction:

- graphite dark neutrals;
- muted moss accent;
- restrained semantic status colors.

### 19.3 Typography

Prioritize readability and density control over stylistic display typography.

Avoid excessive uppercase microcopy and dashboard-like metric presentation in primary flows.

### 19.4 Borders and elevation

Use borders/elevation to establish functional layers:

- workspace;
- chrome;
- active tool;
- floating panel;
- transient overlay.

Do not layer multiple unnecessary glass effects.

### 19.5 Motion

Motion should explain state transitions, not decorate.

Respect system/reduced-motion preference.

---

## 20. Human-facing copy rules

Preferred:

- “No app detected”
- “Run your development server to begin”
- “No element selected”
- “Source unavailable”
- “Open source”
- “Capture”
- “Settings”
- “Advanced”

Avoid in default flow:

- “semantic snapshot”
- “observer pipeline”
- “native attachment”
- “evidence provenance”
- “project identity authority”
- backend names.

Technical truth remains available in Advanced.

---

## 21. Testing strategy

This wave follows RED -> GREEN.

Existing dedicated executable contract:
`apps/desktop/src-tauri/tests/human_first_ui_v2_contract.rs`

Existing branch-specific workflow:
`.github/workflows/human-first-ui-v2-tdd.yml`

The dedicated contract currently protects at least:

1. persisted settings/localization foundation;
2. default Inspector hiding machine diagnostics;
3. hideable human-facing target bar;
4. Settings and Advanced as real tool surfaces;
5. muted moss replacing the previous AI-blue accent.

These are minimum gates, not the full definition of done.

---

## 22. Required additional executable closure

Before the V2 wave is ready to merge, add/retain executable evidence for the following areas where not already covered:

### 22.1 Localization integrity

Test:

- every supported locale is registered;
- every canonical primary-flow key has an English fallback;
- locale normalization is deterministic;
- stored invalid locale cannot corrupt startup.

Prefer programmatic key-set verification over string-presence-only checks where feasible.

### 22.2 Preference corruption safety

Test malformed storage, invalid enums, non-finite positions and old/partial preference payloads.

### 22.3 Chrome recoverability

Test that hidden target/tool chrome can be restored through commands/reset.

### 22.4 Accessibility structure

At minimum statically or component-level verify labels for icon-only controls and semantic button usage on critical controls.

Where the existing native/browser harness allows it, add runtime keyboard focus verification.

### 22.5 No accidental diagnostic regression

Default Inspector must remain free of machine diagnostic blocks even as Advanced evolves.

### 22.6 Build/type closure

Frontend TypeScript/build must pass at exact final head.

Rust workspace and Tauri integration gates required by the repository must pass at exact final head.

### 22.7 Native smoke

Existing native smoke gates must remain green. A UI-only wave is not permission to break WebView2/WebKitGTK/WKWebView startup or managed-surface behavior.

---

## 23. Visual evidence contract

Human-First V2 is a UI/UX change, so text tests alone are insufficient.

Before merge, obtain exact-head visual evidence for representative states.

Minimum evidence set:

1. default workspace with active target;
2. Inspector with selection;
3. Inspector with no selection;
4. Settings open;
5. Advanced open;
6. target bar hidden;
7. tool rail hidden/recovered;
8. at least one non-English locale;
9. no-target empty state;
10. AI unavailable state.

Evidence should verify:

- inspected app remains visually primary;
- chrome hierarchy is calm;
- no clipped panels;
- no off-screen primary controls;
- accent usage is restrained;
- diagnostics are not leaking into default Inspector;
- translated strings do not obviously overflow core controls.

Where LocalView's own capture/evidence infrastructure can generate this evidence, prefer it.

---

## 24. Responsive desktop-shell behavior

The shell itself must survive practical desktop window resizing.

At narrower widths:

- target information may truncate safely;
- panel width must remain usable;
- primary controls must not become unreachable;
- long project/app names must not force layout overflow;
- bottom sheets must not cover all recovery controls;
- tool rail may compact, but essential actions remain discoverable.

Do not optimize only for one development screenshot size.

---

## 25. Error isolation

A failure in one auxiliary tool must not unnecessarily collapse the full workspace.

Examples:

- AI provider failure should not break Inspect;
- source lookup failure should not break Capture;
- localStorage failure should not break startup;
- translation fallback failure for one locale should use English/key fallback rather than blank UI.

---

## 26. Architectural boundaries

Human-First V2 is primarily a frontend/shell reorganization.

Do not use it as permission to:

- rewrite daemon authority;
- weaken native observer evidence;
- replace platform capture adapters;
- remove diagnostics needed for testing;
- move trusted runtime truth into caller-supplied frontend state;
- duplicate backend state machines in React.

React owns presentation and human interaction orchestration.

Existing Rust/native layers retain authority for system/runtime truth.

---

## 27. Files currently central to this wave

Primary files at current implementation lineage:

- `apps/desktop/src/app/LocalViewShell.tsx`
- `apps/desktop/src/features/FloatingTools.tsx`
- `apps/desktop/src/styles.css`
- `apps/desktop/src/components/icons.tsx`
- `apps/desktop/src/i18n.ts`
- `apps/desktop/src/preferences.ts`
- `apps/desktop/src/commands.ts`
- `apps/desktop/src-tauri/tests/human_first_ui_v2_contract.rs`
- `.github/workflows/human-first-ui-v2-tdd.yml`

Future AI must inspect the current branch before assuming this list is exhaustive.

---

## 28. Implementation lineage already present

The branch was built deliberately from RED toward GREEN.

Observed commits before this canonical spec:

- `ef9d4f1640f657f90dca279d2ac833dc8f067ad9` — RED human-first UI V2 contract
- `98e83c992629be607260e87f6fc1e7f456014140` — branch CI for RED contract
- `6a2d0aea73bb9984a72c1c053ebbe0451575bad1` — executable RED gate correction
- `5f2d0c2771dd4cf257b8bb63937dfb0c016ac823` — contract source-path correction
- `adc7086cff5184ca3a3e25c4852cc52b5da708e5` — corrected RED rerun
- `ae9d863b6eb68b2d92afd467a9e2dd3d3e21b7e1` — localization foundation
- `8bc5990a90c01329b41ab52c7ba94400c2ed662a` — persisted workspace preferences
- `9ffdf13ad65b05890f9075ab662068a8d88deaaf` — canonical human command registry
- `eac3161f04f2cb8de6afbd4bd2e8321a7432a194` — workspace control icons
- `18e9153f23eba5ca1f224467990fdfbafb0f126d` — human-first panels and Advanced diagnostics
- `cc754e20f5df3b1802750e98f02af11db8673bc5` — hideable chrome + persisted settings
- `e1074c59dab178aad8a377ab70830f7732b444db` — quiet graphite + muted moss visual system

This list is provenance, not a substitute for checking the latest head.

---

## 29. Known implementation risks to audit

Future work should explicitly audit these rather than assuming V2 is done because the first contract passes.

### 29.1 String-presence tests can overclaim behavior

The current Rust contract intentionally provides a cheap cross-language gate, but source-string assertions cannot prove all runtime behavior.

Add stronger runtime/component evidence where practical.

### 29.2 Partial localization

A locale table existing does not guarantee every primary surface uses it.

Audit human-facing strings in touched V2 surfaces.

### 29.3 Preference schema ahead of UI

Some preferences may exist in the schema before they are exposed or fully implemented.

Do not imply settings behavior exists simply because the field is persisted.

### 29.4 Command IDs ahead of wiring

A command registry entry is not proof the command is reachable and performs the real action.

Verify command routing.

### 29.5 Disabled primary actions

Open source/Measure/Capture/Ask AI/Fix must be audited for real wiring and clear disabled states.

### 29.6 Chrome position persistence

Persisted position fields require clamping and actual drag/restore behavior before they can be called complete.

### 29.7 Visual polish can hide functional regressions

Never trade observer/capture/session functionality for prettier chrome.

---

## 30. Merge readiness

The historical foundation PR #142 and bounded continuations #143–#149 are merged. These rules now apply to every future Human-First continuation PR rather than to one permanent branch.

Minimum merge conditions for a continuation that changes Human-First behavior or its executable contract:

1. canonical spec remains compatible with the proposed behavior;
2. the master Human-First V2 source contract is GREEN;
3. frontend build/typecheck is GREEN;
4. applicable Rust/Tauri workspace checks are GREEN;
5. native GUI smoke gates are GREEN where required by repository policy;
6. no regression in session/observer/capture behavior;
7. runtime/browser evidence is collected when the change affects visible or interactive behavior;
8. accessibility/recovery behavior remains closed;
9. PR body reflects exact final scope and evidence;
10. all required checks correspond to the exact final head, not an earlier green commit.

Do not cite earlier green commits as proof after code changes.

---

## 31. Continuation protocol for future AI

When resuming this project:

1. read this file;
2. fetch the current `main` head;
3. inspect the latest relevant Human-First continuation PR, if one exists;
4. record the exact current branch/PR head before making a mutation;
5. inspect commits and changed files after the latest merged Human-First closure;
6. inspect check runs for the exact current head;
7. do not assume an in-progress/cancelled older run represents the current head;
8. run/fix the master Human-First V2 source contract and applicable runtime audit;
9. audit the Known Implementation Risks section against current code rather than historical assumptions;
10. choose the smallest bounded missing contract;
11. add RED executable test/evidence when introducing a new behavioral claim;
12. implement GREEN;
13. run broader regression gates;
14. commit with narrow provenance;
15. update this spec when its durable product/continuation contract becomes stale, not for routine implementation progress;
16. keep the continuation PR Draft until exact-head closure is real.

If another AI or process advances `main` or the continuation branch while working, re-read the relevant head before making a mutation. Never overwrite newer implementation with stale file contents.

---

## 32. Completion definition

Human-First UI/UX V2 is complete when LocalView can retain its deep machine verification capabilities while presenting a default shell that a developer can use without understanding those internals.

The desired result is not “less powerful LocalView.”

It is:

> **the same or stronger LocalView substrate, exposed through a calmer and more human product surface.**

That distinction must remain true in every follow-up commit.
