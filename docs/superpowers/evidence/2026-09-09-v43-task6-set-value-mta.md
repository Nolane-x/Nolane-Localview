# V4.3 Task 6 — Dedicated MTA SetValue Dispatch Evidence

Date: 2026-09-09

Base: `main@ebb024cfaa6bdde0c647c2f875671d756c54dbfa`

## RED lineage

- Request/type RED was isolated on `256bc55f368c76a158fa22874b1543f923c09337`: Windows UIA Observe #663 failed because the dedicated SetValue request symbols did not yet exist, after test-fixture noise was removed.
- Worker/public API RED was isolated on `56e2cdcc09e1e672dbd596d0226d51e252144a23`: Windows UIA Observe #670 failed because `WindowsUiaWorker::dispatch_set_value` did not yet exist at the public worker boundary.

## GREEN implementation proof

Verified apply run: `V4.3 Task 6 verified apply` #3, run `34346779721`, source head `1ade652cf7adcecd536dbb6d1e3117a3015759c1`.

The run completed these gates successfully before committing production code:

1. exact audited MTA worker patch;
2. public subscription-worker forwarding seam;
3. `cargo fmt -p localview-windows-uia-provider`;
4. `cargo check -p localview-windows-uia-provider --all-targets`;
5. `set_value_dispatch_contract`;
6. real Win32 `set_value_dispatch_worker_smoke` with `LOCALVIEW_UIA_SMOKE=1`;
7. existing real Invoke regression;
8. existing real SelectionItem regression;
9. existing real Toggle regression;
10. existing real ExpandCollapse regression.

Only after all gates passed did the verifier create production commit:

`a28bf8f431c3baf25930fec97d46d031c160eb79` — `feat(windows-uia): execute one-shot SetValue on MTA worker`

The temporary apply workflow and patch script were removed in that production commit.

## Exact-head note

The immediate PR-triggered CI #1457 and Windows UIA Observe #673 for `a28bf8f431c3baf25930fec97d46d031c160eb79` ended as `action_required` with zero jobs because the triggering actor was `github-actions[bot]`; this is not recorded as GREEN and is not treated as a code-test result.

This owner-authored evidence commit exists to produce a fresh exact PR head whose normal CI and Windows UIA workflows can run without that bot-trigger gate.

## Task 6 closure rule

Task 6 is implementation-complete only after the fresh owner-authored exact head receives normal CI + Windows UIA success. PR #102 remains draft and Task 7 must not be claimed GREEN from Task 6 evidence.
