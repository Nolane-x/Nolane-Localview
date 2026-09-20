# Wave 4 — Adaptive Responsive Intelligence Design

Date: 2026-09-20  
Branch: \`feat/wave4-responsive-adaptive\`  
Owner: Worker AI-2 / responsive adaptive execution only

## 1. Scope

This slice connects the already-landed responsive primitives to the live LocalView-owned preview without reopening browser emulation, layout-engine ownership, source tracing, point-select, or content/locale stress.

It closes two previously explicit Wave-4 gaps:

1. bounded adaptive/binary responsive execution;
2. deeper deterministic responsive issue intelligence over live evidence.

The existing canonical four-preset sweep remains the trusted visual authority. This design layers adaptive execution into that same transaction rather than adding a second screenshot or browser path.

## 2. Reused authority

The implementation reuses the existing responsive transaction and does not replace any of these authorities:

- exact session-owned preview-window identity;
- one per-session capture gate;
- LocalView-owned resize;
- bounded resize convergence;
- the existing capture-settle endpoint;
- freeze / native capture / exact restore / private-pixel redaction for canonical visual frames;
- restoration of the original preview size before persistence;
- the existing responsive contact-sheet artifact and Visual evidence endpoint;
- fresh authenticated semantic snapshots, which already travel through the native semantic/layout evidence path;
- retained-resource admission for the contact-sheet artifact.

No Playwright/Chromium/device-emulation path is added. No permanent browser process exists.

## 3. Transaction

The live transaction is:

\`\`\`text
exact preview ownership
-> per-session capture gate
-> read original route + physical viewport + scale
-> remove canonical preview minimum size
-> canonical preset loop
     -> resize
     -> convergence
     -> settle
     -> freeze
     -> native viewport capture
     -> exact visual restore
     -> redaction
     -> fresh semantic/layout snapshot
     -> seed responsive detector
-> bounded adaptive planning
-> only missing adaptive widths
     -> exact preview revalidation
     -> resize
     -> convergence
     -> route revalidation
     -> settle
     -> exact preview revalidation
     -> fresh semantic/layout snapshot
     -> deterministic detector
-> optional bounded binary refinement
-> observed transition bracket / no-transition / inconclusive
-> bounded issue fusion
-> exact original preview restore
-> restored settle
-> restored route validation
-> canonical contact-sheet build
-> retained-resource admission
-> one contact-sheet artifact
-> one existing responsive Visual evidence record
-> receipt including bounded adaptive issue evidence
\`\`\`

Any work failure still reaches the existing outer preview restoration path. Persistence remains after successful restoration only.

## 4. Probe policy

Backend-owned constants:

- responsive width domain: 320–1440 CSS px;
- adaptive hard cap: **12 unique widths total**;
- initial adaptive target: at most 6 widths;
- observed-transition tolerance: 16 CSS px;
- canonical preset widths seed the adaptive cache;
- the original live viewport width is clamped into the backend-owned domain and may become an anchor;
- duplicate widths are cache hits and never trigger another resize;
- no arbitrary caller-authored width is accepted.

The initial planner keeps domain endpoints and deterministically subdivides gaps. Binary refinement is attempted only when the already-observed detector sequence has exactly one concrete PASS/FAIL transition.

The existing \`discover_breakpoint\` primitive is used only to choose additional live probes. Its scalar return value is deliberately not exposed as breakpoint truth.

## 5. Breakpoint truth model

A valid result is named an **observed responsive transition**, never a CSS source breakpoint.

A resolved result contains:

- detector identity;
- lower sampled width;
- upper sampled width;
- state on each side;
- bounded tolerance;
- claim string \`observed_responsive_transition\`.

The runtime does not claim the exact media-query declaration, source file, CSS token, or an exact breakpoint width.

Resolution requires:

- same exact session;
- same canonical route;
- same detector;
- concrete PASS/FAIL evidence;
- exactly one monotonic state transition;
- a final bracket no wider than tolerance.

The result is \`inconclusive\` when:

- semantic projection is truncated;
- the same width has contradictory state evidence;
- detector state is non-monotonic;
- a transition cannot be narrowed within tolerance;
- probe evidence itself is inconclusive.

No transition is a separate truthful result.

## 6. Deterministic width-local detector

Binary search requires an order-independent condition. Therefore \`responsive_geometry_v1\` PASS/FAIL is based only on single-width deterministic geometry facts:

- horizontal viewport overflow by a bounded observed node;
- text/control region outside the viewport;
- geometric collision between independent interactive controls.

Cross-width observations do **not** alter this width-local PASS/FAIL state. This prevents binary results from depending on probe order.

Clipping without computed overflow authority, disappearance across a nearby width, and other semantically ambiguous observations can still become issues, but they do not manufacture a width-local failure.

## 7. Bounded semantic projection

Each adaptive width obtains a fresh authenticated \`PageSnapshot\` after resize convergence and settle.

The responsive projection keeps only:

- session identity;
- canonical route;
- viewport;
- snapshot version;
- stable element ref;
- parent stable ref when present;
- finite positive rectangle;
- interactive boolean;
- one derived text/control boolean.

Names, DOM text, source content, form values, cookies, storage, selectors, CSS source declarations, and raw pixels are not retained in adaptive issue packets.

At most 256 geometry nodes are projected per adaptive observation. If that bound truncates the snapshot, the detector state is \`inconclusive\`, never PASS.

## 8. Responsive issue model

Issues are deterministic/bounded evidence objects with:

- session;
- route;
- viewport;
- stable refs where available;
- detector;
- evidence strings containing snapshot version plus bounded numeric facts;
- confidence in milli-units;
- deterministic / suspected / inconclusive class;
- before/after widths where relevant.

Supported kinds:

| Kind | Class / truth boundary |
|---|---|
| \`horizontal_overflow\` | Deterministic observed geometry outside viewport; not a CSS-source claim |
| \`clipping\` | Suspected when child geometry exceeds parent geometry; computed overflow style is not asserted |
| \`unexpected_disappearance\` | Suspected when a stable interactive/text-control ref disappears at a nearby width; intentional responsive hiding is possible |
| \`control_collision\` | Deterministic geometric overlap above the fixed ratio for independent interactive controls |
| \`dramatic_layout_jump\` | Deterministic geometric delta across nearby sampled widths; not an aesthetic judgment |
| \`text_or_control_outside_viewport\` | Deterministic bounded geometry fact |
| \`breakpoint_local_regression\` | Deterministic sampled state pattern around a middle width |
| \`nearby_width_instability\` | Inconclusive/non-monotonic sampled detector behavior |

There is no aesthetic score, “bad UI” score, or subjective design rating.

Issue deduplication is deterministic and capped at 64 issues per responsive transaction.

## 9. Visual and privacy policy

Adaptive widths intentionally do **not** create per-width screenshots. That is a resource and privacy property, not a missing capability.

Canonical widths continue to use:

\`\`\`text
settle -> freeze -> native capture -> exact restore -> private redaction
\`\`\`

Only already-redacted canonical pixels reach the in-memory contact-sheet builder. Adaptive widths use bounded fresh semantic/layout evidence and create zero extra PNG artifacts.

Therefore adaptive mode cannot become a brute-force screenshot farm. Artifact count remains the existing one contact sheet for a successful sweep.

## 10. Drift and failure policy

Every non-cached adaptive probe revalidates exact preview ownership before resize and after settle.

Fail-closed classes include:

- \`responsive_resize_failed\`;
- \`responsive_resize_timeout\`;
- \`responsive_settle_failed\`;
- \`responsive_session_drift\`;
- \`responsive_route_drift\`;
- \`responsive_evidence_capture_failed\`;
- \`responsive_invalid_observation\`;
- \`responsive_probe_cap_exceeded\`;
- \`responsive_transaction_timeout\`.

Canonical capture retains its existing native-capture, freeze, restore, redaction, viewport, memory, and contact-sheet failures.

A failed adaptive probe cannot authorize persistence. The outer transaction still attempts exact preview restoration.

## 11. Resource bounds

Adaptive resource limits are explicit:

- maximum 12 unique responsive probe widths;
- maximum 256 projected geometry nodes per width;
- maximum 64 derived issues;
- same 30-second responsive transaction deadline;
- same 5-second cleanup reserve;
- same 2-second resize convergence bound;
- zero additional adaptive image artifacts;
- zero Chromium spawns;
- one existing contact-sheet artifact only after restoration.

The existing retained-resource ledger remains the persistence admission boundary. If it rejects the final artifact projection, the transaction fails closed.

This slice does not invent an independent runtime-governor authority outside the responsive ownership boundary.

## 12. Restoration semantics

The adaptive execution lives inside the same \`work\` future as the canonical sweep. The existing mandatory cleanup is outside that future:

\`\`\`text
work (canonical + adaptive + binary)
-> restore_responsive_preview
-> decide whether work may continue
-> only then build/persist/register
\`\`\`

Thus resize, settle, route drift, session drift, evidence acquisition, detector, cap, and timeout failures all flow through preview restoration.

The current architecture does not expose a separate public responsive cancellation primitive. Explicit transaction timeout/failure paths are covered; this slice does not claim force-safe async cleanup after an external runtime abort that drops the entire command future.

## 13. Evidence persistence boundary

Fresh semantic snapshots used by adaptive probes already use the authenticated exact-session snapshot action/result path and its existing Semantic/Layout evidence authority.

The adaptive receipt carries derived issue packets and snapshot versions so each issue remains traceable to a live observation. This slice does not add a second daemon evidence endpoint or a new persistence schema because that would exceed Worker AI-2 ownership.

The durable visual record remains the existing \`responsive_contact_sheet\` evidence. The receipt must not be described as a CSS source proof or as a durable new Issue-store record.

## 14. Adversarial proof matrix

Pure responsive policy tests cover:

- monotonic transition;
- no transition;
- non-monotonic detector;
- contradictory same-width evidence;
- probe hard cap;
- duplicate-width elimination;
- invalid width arithmetic/range;
- issue detection and classification;
- issue dedup;
- no false exact-breakpoint claim.

Desktop contracts cover:

- exact preview/session ownership;
- adaptive execution under the existing session capture gate;
- resize + convergence + settle + fresh snapshot;
- no per-adaptive-width native screenshot;
- no Chromium path;
- route drift;
- session drift;
- resize failure;
- settle failure;
- evidence capture failure;
- canonical native capture failure;
- restoration after adaptive success/failure/timeout via the outer transaction;
- restore-before-persistence;
- truncated semantic evidence cannot become PASS.

## 15. Non-goals / truth boundaries

This slice does not claim:

- CSS media-query source breakpoint discovery;
- arbitrary device emulation;
- user-agent/DPR spoofing;
- browser compatibility testing;
- continuous resize scanning;
- hundreds of widths;
- content/locale stress;
- layout-engine ownership;
- computed grid/flex diagnosis;
- CSS declaration/source-map correlation;
- point-select;
- source-open/Fix/Verify;
- aesthetic quality scoring;
- guaranteed semantic intent for hidden/disappearing elements;
- a permanent Chromium process.

## 16. Completion chain

The implementation is complete for this ownership slice when the exact code path proves:

\`\`\`text
canonical preview
-> canonical trusted evidence
-> bounded adaptive probes
-> deterministic live detector
-> monotonic transition bracket when supported
-> bounded binary resolution
-> responsive issue evidence
-> exact preview restore
-> existing contact-sheet persistence
\`\`\`

The governing truth rule is:

> **LocalView may report what changed across bounded observed widths; it must not invent why CSS changed or pretend an observed transition is an exact source breakpoint.**
