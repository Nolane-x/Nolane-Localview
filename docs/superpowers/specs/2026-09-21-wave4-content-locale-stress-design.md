# Wave 4 Content / Locale Stress Authority

## Goal

Close the remaining Wave 4 content-stress / locale-expansion debt without pretending that LocalView owns translation quality.

The authority is deliberately synthetic:

`managed LocalView WebView -> bounded temporary pseudo-content mutation -> settled fresh semantic evidence -> deterministic geometry issues -> exact restoration -> fresh restoration proof`.

## Profiles

LocalView executes exactly four bounded synthetic profiles:

- `expanded_130`: Latin-like pseudo content at roughly 130% source character count.
- `expanded_180`: stronger Latin-like expansion at roughly 180%.
- `dense_cjk`: dense no-space CJK-like glyph pressure.
- `rtl_pseudo`: RTL pseudo-locale pressure with bounded expansion and temporary `dir=rtl`.

These are layout stressors, not translations and not language-quality evaluations.

## Privacy

The runtime may retain original text only inside the managed page for the lifetime of one restoration transaction.

Original/stressed text is never placed in:

- Tauri completion transport;
- Rust receipt;
- evidence payloads;
- logs;
- artifacts.

Input, textarea, select/option, code/pre/kbd/samp, contenteditable, aria-hidden and LocalView/private/sensitive subtrees are excluded.

## Bounds

- at most 160 text nodes per profile;
- source text longer than 512 Unicode scalar values is skipped;
- generated stress text is capped at 768 scalar values;
- exactly four profiles;
- at most 64 reported issues.

The command never crawls arbitrary routes and never creates permanent Chromium ownership.

## Restoration authority

For every mutated text node LocalView keeps, in page memory only:

- node identity;
- original value;
- exact synthetic value written by LocalView.

Restore is allowed only when the node still contains LocalView's synthetic value, or the app already restored the exact original value.

If the app writes a third value during the stress lease, LocalView does not overwrite it. The transaction returns `restore_conflict` and fails closed.

The same compare-before-restore rule applies to temporary document `lang` and `dir`.

After a successful restore LocalView waits through the existing stable-capture settle authority, acquires a new fresh semantic snapshot and checks:

- exact route;
- exact viewport;
- stable semantic node set;
- tag/name/interactive identity;
- geometry within a one-pixel restoration tolerance.

No stress receipt is successful before this restoration proof.

## Deterministic issue subset

The initial authority reports only geometry conditions supportable by fresh semantic evidence:

### `content_viewport_overflow`

A stable element was inside the viewport in baseline evidence and extends outside the same viewport under a synthetic profile.

### `content_sibling_collision`

Two stable siblings had at most a 5% overlap ratio at baseline and at least 25% overlap under stress.

### `content_interactive_disappeared`

A baseline interactive stable ref is absent under stress while the overall semantic state remains sufficiently correlated.

If fewer than 80% of baseline refs survive, the profile is rejected as semantic-state drift rather than interpreted as a layout defect.

## Integration

Desktop exposes:

`capture_content_locale_stress(session_id)`

The command:

1. requires an exact LocalView-managed loopback surface;
2. waits for the existing settle gate;
3. acquires one fresh baseline snapshot;
4. applies each canonical profile through the injected LocalView runtime;
5. accepts only exact session / route / bridge-generation completion;
6. settles and acquires fresh stressed evidence;
7. evaluates bounded deterministic issues;
8. restores the page;
9. settles again;
10. proves restoration with another fresh snapshot;
11. returns only bounded synthetic profile/issue metadata.

## Non-claims

This slice does not claim:

- real translation correctness;
- locale-specific typography correctness;
- linguistic naturalness;
- browser-final text clipping provenance;
- arbitrary framework state safety when the application itself re-renders during the lease;
- content stress across cross-origin frame documents.

Those cases fail closed or remain future work.

## Closure

This closes the Wave 4 roadmap item "content stress matrix and locale expansion" at the runtime/evidence level, subject to exact-head CI and real Chromium proof.
