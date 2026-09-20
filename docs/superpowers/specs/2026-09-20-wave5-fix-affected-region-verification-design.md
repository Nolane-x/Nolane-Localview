# Wave 5 Trusted Fix Affected-Region Verification — Engineering Design

## Status

Canonical design for closing the live source-edit verification loop by connecting the existing Trusted Fix/Verify path to the existing bounded changed-region planner and visual evidence authority.

Date: 2026-09-20

Repository: `Nolane-x/Nolane-Localview`

Base: `main@5f98f0286409e889ac73e1c8fa3be2b98501c9d6`

Branch: `feat/wave5-fix-affected-region-verification`

## Goal

Close this bounded live loop:

```text
selected element
  -> trusted source mapping
  -> human-approved source edit
  -> existing HMR/settle authority
  -> exact fresh semantic snapshot
  -> one redacted native viewport acquisition
  -> bounded changed-region planning
  -> affected-region evidence
  -> deterministic Verify receipt
```

The slice must reuse existing authorities instead of introducing a second HMR detector, screenshot engine, visual-diff algorithm, source mapper or persistence path.

## Existing authorities reused

- Trusted Fix exact source preimage/postimage transaction.
- Trusted Verify one-shot verification record and route/source revalidation.
- daemon-owned capture settle gate, including the existing optional 300 ms HMR quiet authority.
- exact fresh semantic snapshot after settle.
- native managed-surface capture, freeze/restore and private-pixel redaction.
- `localview-visual::plan_changed_css_regions`.
- visual artifact/evidence registration.
- bounded Contract visual-diff evidence.

## Current debt

Trusted Verify currently captures one post-Apply redacted viewport and persists the entire viewport before it knows whether the visual change is local.

That proves visual change, but it does not connect verification to the already-landed changed-region planner and it makes the common local-edit path persist more pixels than necessary.

## Required behavior

### 1. One post-Apply native acquisition

After existing Verify settle and fresh semantic/source checks:

- acquire exactly one redacted current viewport;
- do not invoke `capture_changed_regions` as a second native capture;
- compare that current frame against the in-memory pre-Apply Verify baseline.

### 2. One shared visual threshold

Trusted Verify continues to use its existing visual threshold of 12.

Changed-region planning for this slice must use:

```text
ChangedRegionPolicy {
  threshold: VERIFY_PIXEL_THRESHOLD,
  ..ChangedRegionPolicy::default()
}
```

so the affected-region plan cannot claim a change using a looser threshold than the deterministic Verify comparison.

### 3. Bounded affected-region plan

For compatible before/after frames:

- `Unchanged` -> no new Visual artifact; emit only bounded visual-diff Contract evidence.
- `Regions` -> persist only the planner-selected redacted crops; at most the existing planner maximum.
- `Viewport` -> persist one redacted viewport artifact as explicit broad-change fallback.

The native frame remains in memory only long enough to calculate deterministic Verify facts and persist the selected evidence.

### 4. Receipt authority

Trusted Verify receipt gains:

```text
visualChangeMode?: unchanged | regions | viewport
affectedRegions: Rect[]
affectedVisualEvidenceIds: string[]
```

Rules:

- semantic-only verification: `visualChangeMode = null`, empty arrays;
- incompatible/missing visual evidence: no visual change mode and no affected regions;
- `unchanged`: zero regions, zero Visual evidence ids;
- `regions`: one bounded region per selected CSS rectangle and matching Visual evidence ids;
- `viewport`: one viewport rectangle and exactly one Visual evidence id.

`visualDiffEvidenceId` remains the Contract evidence binding the selected visual parents and changed ratio.

### 5. Deterministic status remains conservative

This slice does not change the existing status ordering:

- new regression signals dominate;
- semantic change or target-local visual change -> `change_observed`;
- incompatible visual evidence -> `inconclusive` when visual scope was promised;
- visual change outside the selected target without semantic change -> `inconclusive`;
- otherwise -> `no_observable_change`.

Affected-region geometry is additional evidence. It must not turn a broad unrelated visual change into a selected-target success.

## Privacy and storage

- crop only after existing private-pixel redaction;
- never persist the unredacted frame;
- never expose private selectors;
- no filesystem paths in visual evidence;
- no source contents in the visual receipt;
- no extra browser process;
- no second screenshot acquisition;
- unchanged verification creates no new image artifact.

## Fail-closed cases

Affected-region authority is omitted when:

- before/after viewport differs;
- native pixel dimensions differ;
- PNG decode fails;
- changed-region planner rejects geometry;
- route changes;
- source mapping changes;
- exact postimage is no longer present;
- current managed surface is unavailable.

Failure to persist selected affected-region evidence must fail the visual verification transaction rather than silently claiming a region without durable evidence.

## Non-claims

This slice does not claim:

- the source edit caused every changed pixel;
- root-cause proof;
- HMR is guaranteed to occur;
- no unrelated animation can exist outside the existing settle/freeze policy;
- CSS source-line ownership for a changed pixel;
- browser final cascade provenance beyond the separately landed bounded CSS authority;
- cross-route verification;
- autonomous source editing without the existing human Apply gate.

## Verification

Focused contracts must prove:

1. Verify still waits on the existing settle authority before the fresh snapshot.
2. Exactly one current native capture is used after Apply.
3. A local visual change produces bounded region evidence, not a viewport artifact.
4. A broad change falls back to one viewport artifact.
5. An unchanged frame creates no Visual artifact and still creates Contract diff evidence.
6. Threshold 12 is shared between Verify pixel comparison and affected-region planning.
7. Region crops are made only from the already-redacted current frame.
8. Region evidence ids become parents of the visual-diff evidence.
9. Incompatible viewport/dimensions produce no affected-region claim.
10. Existing deterministic Verify status behavior remains unchanged.
11. Existing Trusted Fix, visual capture, changed-region scheduler and full desktop CI remain green.

## Completion definition

Complete when a human-approved Trusted Fix can be followed by one bounded Verify transaction that waits for the existing HMR-aware settle gate, revalidates exact source/route/target state, captures one redacted native viewport, persists only the bounded visual area needed to prove the observed change, and returns a deterministic receipt with durable affected-region evidence.
