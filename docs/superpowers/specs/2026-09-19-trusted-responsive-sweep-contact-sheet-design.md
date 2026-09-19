# Trusted Responsive Sweep / Contact Sheet — Canonical Design

## Status

Implementation-authorized design for the next bounded responsive-intelligence slice after:

- guarded full-page native stitching landed through PR #139;
- Human-First Chrome Position Persistence closed through PR #149;
- Human-First V2 contract/runtime continuity closed through PR #150;
- current base `main@81b6106c230b9a40c8b67575c8e681142af9e128`.

This is **not** Human-First V2.7. Human-First V2 remains the product-surface contract. This slice connects one currently missing runtime capability that the roadmap and coverage ledger still describe as Partial: live responsive viewport execution and a bounded contact sheet.

## 1. Goal

Make the existing Responsive tool real without inventing browser authority.

A user explicitly selects one or more canonical LocalView viewport presets. LocalView then:

1. operates on the exact LocalView-owned preview window for the selected session;
2. temporarily removes the preview's 640×480 minimum-size constraint;
3. resizes the preview window to each backend-owned canonical viewport;
4. waits for the exact session to settle;
5. obtains a fresh trusted visual-freeze receipt for that resized viewport;
6. captures the existing native WebView viewport path;
7. validates route / viewport / device-scale / native pixel reality;
8. applies current private-region redaction before any responsive image reaches retained storage;
9. restores the visual-freeze state for that viewport;
10. retains the redacted frame only in bounded process memory;
11. restores the preview window to its exact original size and canonical minimum-size constraint;
12. proves the restored page settles again;
13. only then builds and persists a bounded contact sheet and registers dedicated responsive Visual evidence.

No responsive artifact or evidence may exist before successful preview-size restoration.

## 2. Why the existing preview window is the authority

The repository already has one universally available LocalView-owned native surface:

`preview-<session>`

It is:

- created by LocalView;
- bound to one exact session in `DesktopSurfaceRegistry`;
- admitted through hidden-surface Runtime Resource Governor authority;
- navigation-limited to loopback HTTP(S);
- instrumented with the LocalView observer bridge;
- already the first trusted surface used by `capture_managed_surface`.

The native workspace child remains feature-gated and iframe remains the conservative default workspace. Responsive execution therefore must **not** depend on native-workspace becoming the default.

Creating a second responsive WebView for the same session is forbidden in this slice because two same-session instrumented surfaces could compete for capture/freeze action delivery. The existing preview is resized in place instead.

## 3. Why mobile presets are possible

The normal preview is created with a 640×480 minimum inner size for ordinary human use.

Tauri 2.11.5 exposes `WebviewWindow::set_min_size(None)` and `set_size(...)`. The responsive transaction may temporarily remove the minimum-size constraint, resize the preview to a canonical mobile viewport, then restore both:

- exact original inner size;
- canonical preview minimum-size constraint: 640×480.

The normal preview creation policy itself is unchanged.

## 4. Frontend authority

Frontend must never send arbitrary width/height authority.

The only caller-controlled responsive request is:

```ts
type ResponsivePresetId =
  | 'mobile_s'
  | 'mobile'
  | 'tablet'
  | 'desktop';

interface ResponsiveSweepRequest {
  sessionId: string;
  presets: ResponsivePresetId[];
}
```

Backend owns the dimensions:

| ID | CSS viewport |
| --- | ---: |
| `mobile_s` | 320×568 |
| `mobile` | 390×844 |
| `tablet` | 768×1024 |
| `desktop` | 1440×900 |

Rules:

- 1–4 presets;
- duplicate preset IDs are rejected, not silently multiplied;
- unknown IDs fail deserialization;
- caller cannot submit width, height, device scale factor, route, pixel dimensions, artifact IDs, mask geometry or contact-sheet placement;
- order is canonical backend order, independent of caller ordering.

Future adaptive/binary discovery may choose additional widths internally, but that is not frontend authority in this slice.

## 5. Preview-window transaction authority

Responsive sweep requires the exact preview window for the session.

Before mutation:

1. resolve `preview_surface_label(session_id)`;
2. require the platform window to exist;
3. require matching current `DesktopSurfaceRegistry` owner identity;
4. require the surface to remain Runtime Resource Governor-owned;
5. require current route to be loopback HTTP(S);
6. record canonical route;
7. record original physical inner size;
8. record current scale factor;
9. reject maximized or fullscreen preview state in the first slice;
10. acquire the existing per-session visual capture gate.

The sweep must not operate on:

- arbitrary OS windows;
- external browsers;
- iframe DOM sizing;
- caller-selected window labels;
- a workspace child merely because it exists;
- a stale preview incarnation.

## 6. Size mutation and convergence

For each backend-owned viewport:

1. call `set_min_size(None)` once before the first responsive resize;
2. call `set_size(LogicalSize::new(width, height))`;
3. wait for native inner-size convergence under a bounded timeout;
4. call existing capture-settle;
5. call existing fresh freeze/private-mask path;
6. verify freeze viewport CSS dimensions match the requested preset within a strict CSS-pixel tolerance;
7. derive `ViewportMeta` from trusted freeze + live window scale factor;
8. call the existing native `capture_managed_surface`;
9. require canonical route unchanged;
10. require frame viewport equals the trusted requested responsive viewport;
11. require native dimensions to agree with device scale factor within existing capture tolerance;
12. restore the exact freeze token;
13. redact private pixels using that exact viewport's fresh mask geometry;
14. decode the redacted PNG into bounded RGBA only when needed for contact-sheet composition.

A viewport whose page cannot settle or whose exact CSS viewport cannot be proven fails the whole sweep.

## 7. Mandatory cleanup

Cleanup is part of success semantics.

Regardless of work success/failure, LocalView must attempt:

1. restore original preview inner size;
2. restore canonical minimum inner size 640×480;
3. wait for native size convergence;
4. wait for capture-settle at the restored size;
5. re-read and canonicalize route;
6. require route unchanged.

If cleanup fails:

- no responsive artifact is persisted;
- no responsive evidence is registered;
- any in-memory frame/contact-sheet bytes are dropped;
- the command returns a bounded human-facing failure.

The transaction must reserve cleanup time rather than spending the entire deadline on capture work.

## 8. Boundedness

First-slice hard bounds:

- maximum presets: 4;
- dimensions: backend-owned table only;
- total transaction deadline: 30 seconds;
- cleanup reserve: 5 seconds;
- size convergence timeout per transition: 2 seconds;
- maximum decoded responsive-frame aggregate: 64 MiB;
- maximum contact-sheet RGBA allocation: 96 MiB;
- maximum contact-sheet encoded PNG: existing visual encoded-image bound;
- no retries that can cause unbounded resize loops.

The four canonical presets fit comfortably inside these limits at ordinary device-scale factors. If actual native scale makes projected RGBA exceed a bound, the sweep fails closed before allocation/persistence.

## 9. Pure responsive policy

`localview-responsive` becomes owner of:

- canonical `ResponsivePresetId` → `Viewport` mapping;
- canonicalization/dedup validation;
- bounded responsive sweep planning;
- contact-sheet geometry projection;
- deterministic row-copy placement.

Suggested API:

```rust
pub enum ResponsivePresetId {
    MobileS,
    Mobile,
    Tablet,
    Desktop,
}

pub struct ResponsiveSweepPlan {
    pub viewports: Vec<Viewport>,
}

pub struct ContactSheetPlacement {
    pub viewport: Viewport,
    pub x: u32,
    pub y: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

pub struct ContactSheetGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba_bytes: usize,
    pub placements: Vec<ContactSheetPlacement>,
}
```

The contact sheet uses a vertical, deterministic layout with bounded gutters. No font renderer or synthetic labels are trusted into pixel evidence; viewport identity lives in evidence metadata and UI copy.

## 10. Contact-sheet pixel policy

Input frames are already private-redacted native pixels.

Contact-sheet builder must:

- validate every `RgbaImage`;
- accept only one frame per planned viewport;
- reject count/order/viewports that differ from the plan;
- reject arithmetic overflow;
- reject projected RGBA over policy bound before allocation;
- use deterministic placement;
- copy exact RGBA rows;
- never rescale captured pixels;
- never crop frames merely to force layout;
- use an opaque neutral gutter/background;
- keep no raw/unredacted source.

This contact sheet is a presentation artifact over already-trusted redacted captures, not a new observation source.

## 11. Dedicated responsive evidence

Do not overload ordinary viewport Visual evidence with `target = "responsive"` while omitting sweep provenance.

Add:

```text
POST /v1/sessions/{id}/evidence/visual-responsive
```

Request is desktop-owned and contains only bounded derived metadata:

```rust
struct ResponsiveVisualEvidenceRequest {
    artifact_id: String,
    route: String,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    contact_sheet_pixel_width: u32,
    contact_sheet_pixel_height: u32,
    viewports: Vec<ResponsiveViewportEvidence>,
}

struct ResponsiveViewportEvidence {
    preset: ResponsivePresetId,
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
    pixel_width: u32,
    pixel_height: u32,
    sheet_x: u32,
    sheet_y: u32,
}
```

Daemon validates:

- session exists;
- exact schema / deny unknown fields;
- 1–4 unique canonical presets;
- preset dimensions match backend policy;
- finite positive scale;
- bounded native/contact-sheet dimensions;
- placements are non-overlapping and inside the contact sheet;
- canonical loopback route;
- approved native backend/provenance if backend is included;
- timestamp/revision bounds consistent with existing Visual evidence rules.

Stored `EvidenceKind::Visual` payload uses explicit target `responsive_contact_sheet`.

No freeze token, private selectors, DOM text, cookies, storage, source contents or raw masks are stored.

## 12. Persistence order

Required order:

```text
session capture gate
-> exact preview authority
-> record original size / route
-> remove preview min-size constraint
-> [
     resize canonical preset
     -> size convergence
     -> settle
     -> freeze + fresh private geometry
     -> native viewport capture
     -> validate
     -> restore freeze
     -> redact
     -> hold bounded redacted frame in memory
   ] * N
-> restore original preview size
-> restore 640×480 min-size constraint
-> restored-size convergence
-> restored settle
-> route revalidation
-> build contact sheet from redacted frames
-> encode
-> retained-resource admission
-> persist one contact-sheet artifact
-> register one responsive Visual evidence
```

No persistence before preview restoration.

## 13. Human-First Responsive UI

Responsive panel changes from placeholder to an explicit trusted action surface.

Required states:

- no session;
- preview unavailable: clear "Open preview first" action/guidance;
- ready with four canonical preset toggles;
- sweep in progress;
- success with captured viewport list and responsive evidence id;
- deterministic failure with Retry;
- duplicate trigger suppression;
- stale session isolation;
- route/preview loss isolation.

The UI sends only session ID + preset IDs.

The existing disabled placeholder text must be removed only after backend authority is executable.

## 14. Non-goals

This slice does **not** claim:

- arbitrary caller viewport dimensions;
- continuous drag-resize testing;
- adaptive/binary breakpoint execution yet;
- automatic issue detection across widths;
- planner-autonomous responsive sweeps;
- locale/content stress matrices;
- fixed browser-device emulation;
- mobile user-agent emulation;
- DPR spoofing;
- external browser automation;
- Chromium/Playwright fallback;
- making native workspace the default;
- cross-monitor DPI migration;
- W10 mixed-DPI physical closure;
- contact-sheet AI scoring.

The existing `adaptive_sweep` and `discover_breakpoint` algorithms remain primitives for a later internally-owned execution wave.

## 15. Failure codes

Desktop exposes bounded stable classes such as:

- `responsive_preview_unavailable`
- `responsive_preview_owner_mismatch`
- `responsive_preview_maximized`
- `responsive_preview_fullscreen`
- `responsive_invalid_presets`
- `responsive_resize_failed`
- `responsive_resize_timeout`
- `responsive_settle_failed`
- `responsive_freeze_failed`
- `responsive_viewport_mismatch`
- `responsive_route_drift`
- `responsive_native_capture_failed`
- `responsive_redaction_failed`
- `responsive_memory_budget_exceeded`
- `responsive_restore_failed`
- `responsive_contact_sheet_failed`
- `responsive_transaction_timeout`

Raw OS/Tauri/native-capture errors do not become primary UI copy.

## 16. TDD closure sequence

### Gate 1 — Pure preset/contact-sheet policy

RED tests:

- canonical preset mapping;
- caller order canonicalized;
- duplicates rejected;
- empty / >4 rejected;
- exact contact-sheet geometry;
- mixed pixel dimensions;
- deterministic gutter placement;
- invalid buffers reject;
- projected memory overflow rejects;
- placement never exceeds output;
- row-copy content is exact.

### Gate 2 — Dedicated evidence schema

RED tests:

- auth/session required;
- unknown fields rejected;
- 1–4 unique presets;
- dimensions must match preset;
- invalid scale/dimensions rejected;
- placement outside output rejected;
- overlap rejected;
- non-loopback route rejected;
- payload contains no caller authority/private content.

### Gate 3 — Preview resize authority

RED source/runtime contracts:

- exact preview label + exact registry owner required;
- no workspace/iframe fallback in this slice;
- original physical size recorded before mutation;
- maximized/fullscreen rejected;
- `set_min_size(None)` occurs before mobile resize;
- backend canonical `LogicalSize` only;
- no caller width/height;
- bounded convergence;
- canonical min-size restore;
- exact original-size restore on every exit.

### Gate 4 — Sweep capture transaction

RED contracts prove:

- one session capture gate spans all presets;
- settle/freeze/capture/restore/redact per viewport;
- fresh mask geometry per viewport;
- route unchanged;
- viewport exact;
- no artifact persistence inside viewport loop;
- cleanup precedes contact-sheet construction/persistence;
- failure at any viewport still runs preview-size cleanup;
- restore failure prevents persistence.

### Gate 5 — Human-First runtime surface

Browser audit proves:

- four presets are real and localized;
- no arbitrary dimension inputs;
- request contains only `sessionId` + `presets`;
- duplicate click suppressed;
- success/failure/retry;
- stale session isolation;
- preview-unavailable guidance;
- existing Inspector/AI/Settings flows remain intact.

### Gate 6 — Regression / exact-head closure

Minimum:

```text
cargo fmt --check
cargo test -p localview-responsive
cargo test -p localview-control --test responsive_visual_evidence
cargo test -p localview-desktop --test responsive_sweep_contract
cargo test -p localview-desktop --test human_first_ui_v2_contract
cargo check --workspace --all-targets
npm run build
full repository CI
Windows UIA Observe
Windows Real Provider Seeds
Human-First browser audit
```

## 17. Documentation truth after closure

Only after one exact implementation head is green may docs change:

- Responsive intelligence from algorithm-only Partial to live canonical preset sweep/contact-sheet execution;
- Active Perception remaining list removes basic responsive sweep/contact-sheet execution;
- Wave 4 still retains adaptive/binary execution, content stress and deeper responsive issue intelligence unless separately closed.

Do not claim arbitrary device emulation or responsive correctness scoring.

## 18. Completion definition

This slice is complete when a developer can explicitly run a real four-preset responsive sweep against the exact LocalView-owned preview, receive one privacy-safe native-pixel contact sheet/evidence record, and LocalView proves the original preview size/route is restored before anything is retained.

The governing rule is:

> **Responsive evidence comes from real bounded LocalView-owned viewport changes, never from caller-authored geometry or visual simulation.**
