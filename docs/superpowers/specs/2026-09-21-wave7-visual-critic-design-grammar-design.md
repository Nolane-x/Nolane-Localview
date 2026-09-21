# Wave 7 Visual Critic + Design Grammar — Design

Date: 2026-09-21  
Owner: Worker AI-2 / Wave 7  
Branch: `feat/wave7-visual-critic-design-grammar`

## Scope

This slice connects LocalView's existing live semantic/layout evidence to a bounded project design grammar and an explainable visual critic. It does not add accessibility or interaction instrumentation, headless/report persistence, autonomous mutation, trusted-fix behavior, or a new CSS/source resolver.

The runtime chain is deliberately narrow:

```text
latest retained SemanticSnapshot
  -> bounded stable-ref + geometry + fixed computed-style projection
  -> observed metric samples
  -> inferred project families
  -> measured density/balance/hierarchy features
  -> explicitly classified critic findings
  -> serializable design baseline + pure diff
```

Wave 5 source evidence may be carried as an optional hint. Wave 7 never upgrades a hint to an exact CSS declaration coordinate by itself.

## Evidence authority and truth boundary

Wave 7 consumes only evidence already produced by LocalView. The live projection accepts stable refs, finite rectangles, the existing fixed computed-style packet, interactive state, snapshot identity, route/viewport metadata, and bounded source hints. It does not retain arbitrary DOM text, arbitrary attributes, form values, props/state/context/hooks, network bodies, or source contents.

An observed CSS/geometry value is an **observed value**. A repeated cluster derived from several observed values is an **inferred family / observed scale / project pattern**. It is not an official design token. There is no Wave 7 field that promotes an inferred family to an official token.

If an input is not present in the current style packet, the metric is `unavailable`; it is never synthesized. Border radius therefore remains unavailable unless actual radius evidence appears.

Font weight is accepted only when the packet contains a numeric weight. Keywords such as `normal` and `bold` are not converted to guessed numbers. Line height requires an observed pixel value; unresolved values remain unavailable.

A semantic `sourceHint` is carried as `observed_runtime_hint`. Only independently proven Wave 5 CSS authority may be represented as `exact_declaration_position`; this live projection never manufactures that upgrade.

## Bounded project grammar

Each observed metric sample carries a numeric value, stable `ElementRef`, snapshot/route/viewport provenance when known, bounded evidence keys, and explicit `observed` status.

Repeated families contain center/min/max, sample count, support ratio, bounded refs, bounded provenance, confidence, and explicit `inferred` status. Confidence describes evidence support only and cannot change a critic evidence class.

The live grammar extracts spacing, type size, numeric font weight, pixel line height, interactive control height, gap, padding, and radius only when radius is actually observed.

Per-metric extraction is capped at 2,048 samples; family refs are capped at 24. Live semantic projection is capped at 512 retained nodes and tree depth 16.

## Responsive evidence

Wave 7 does not own responsive execution. It accepts independently produced grammar snapshots for viewport widths and reports each metric as stable, varies, or unavailable. It does not infer a breakpoint and does not re-run Wave 4 responsive execution.

## Density, balance, and hierarchy

Density exposes measured occupied-area proxy, whitespace proxy, observed node/control counts, typography-bearing count, and controls per viewport area. A statement that the UI is **too dense** is always heuristic.

Balance uses clipped region geometry as a visual-mass approximation across viewport halves. The numeric approximation is exposed, but `visual_mass_imbalance_candidate` remains heuristic and is never called a deterministic bug.

Hierarchy derives bounded salience from observed font size, numeric weight, geometry area, optional contrast evidence, viewport position, and known interactive state. It does not infer business importance. A weak-hierarchy finding is heuristic even at high confidence.

## Explicit critic evidence classes

Every critic finding stores an explicit class; class is never inferred from confidence:

- `deterministic`: the statement itself is directly supported, such as an observed value measurably differing from a sufficiently repeated observed family. This does not claim aesthetic wrongness.
- `heuristic`: design interpretation such as excessive density, visual-mass imbalance, weak hierarchy, or similar bounded design heuristics.
- `subjective`: an aesthetic preference such as “feels visually heavy”.

A subjective finding cannot fail CI and cannot trigger an automatic fix. In Wave 7, every critic finding returns `can_trigger_automatic_fix = false`; autonomous mutation is Wave 9 ownership.

Classification and severity are separate. A deterministic finding can only be CI-failable if its severity is explicitly `error`.

## Structured finding contract

Each finding contains stable ID/code, explicit class, severity, confidence, affected stable refs, evidence summary, exact bounded measurements, optional expected/deviation values, optional related inferred family, optional source hints with authority, and a reason explaining the classification.

This preserves the explanation chain:

```text
what LocalView observed
  -> what LocalView measured
  -> why this evidence class is allowed
```

Wave 7 intentionally has no single “design score”.

## Overlay boundary

`apps/desktop/src/features/visual-critic/overlay.ts` is self-contained and is not wired into `LocalViewShell.tsx` by this worker.

It mounts only into a LocalView chrome host supplied by an integrator, never queries or mutates the inspected app DOM, receives stable-ref rectangles externally, uses fixed-position paint-only highlights, exposes class/confidence/deviation/source hint and selection, hides the entire root while evidence capture is active, and removes itself on destroy.

The executable overlay contract rejects target-DOM coupling such as `querySelector`, iframe document access, or `innerHTML`.

## Design regression baseline

Wave 7 owns only a serializable `DesignGrammarBaseline` and pure diff. It does not create persistent storage, content-addressed retention, a CLI reporter, or a headless runner.

The baseline contains schema version, grammar families and evidence metadata, density/balance distribution summary where available, hierarchy summary where available, and deterministic/heuristic fact codes.

The diff reports added family, removed family, scale drift, distribution change, hierarchy regression, confidence change, and `inconclusive` when evidence is missing. Missing evidence is not converted to zero. Subjective judgment is not baseline truth.

Persistence and retention remain Wave 8 ownership.

## Privacy and bounds

Wave 7 retains only bounded numeric/style/geometry evidence, stable refs, bounded snapshot/route provenance, and bounded source hints. Integration tests assert that arbitrary DOM text, attributes, and values are absent from serialized Wave 7 output.

Invalid refs, invalid geometry, invalid viewport, unsafe source paths, unsupported style values, and absent evidence fail closed or become unavailable.

## Verification

The dedicated workflow verifies the Wave 7 ownership boundary, Rust formatting, compile, clippy, focused unit/adversarial tests, real semantic/layout integration, overlay isolation, and desktop TypeScript build.

The required proof set covers repeated/noisy spacing, insufficient sample handling, typography families, control heights, unsupported radius, deterministic drift, heuristic density, hierarchy, balance, subjective classification, class independent from confidence, optional source hints, stable refs, responsive variation, baseline no-change and real drift, missing-evidence inconclusive behavior, privacy, bounded caps, and integration from the retained live semantic/layout evidence path.
