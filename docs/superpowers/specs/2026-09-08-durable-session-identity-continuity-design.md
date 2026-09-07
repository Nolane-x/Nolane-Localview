# Durable Session Identity Continuity Design

## Status

Approved architectural direction for Phase D1 after Phase C hidden-surface authority closure.

Base: `main` at `105f9d50a367a05fa6e0012398ebcd33fc321c5d`.

Branch: `feat/durable-session-identity-continuity`.

This document is the design gate for D1 only. D2 surface-owner crash/restart reconciliation depends on D1 but is intentionally not implemented by this slice.

## Goal

Make a logical LocalView localhost session keep the same `SessionId` across daemon restart when LocalView can identify the same target unambiguously, without persisting or resurrecting transient runtime authority.

The durable unit is therefore **identity continuity**, not a durable `Session` object.

After D1, restarting the daemon should not force a still-valid managed preview/bridge to become permanently bound to a dead UUID merely because `SessionManager` restarted. At the same time, a reused UUID must never imply that old action authorization, provider attachment, evidence freshness, governor leases, preview visibility, observer queues, or verification state survived the restart.

## Product contract

LocalView V4.3 requires crash/restart paths to reconcile ownership instead of silently leaking or fabricating authority. Resource reservation lineage is session/target-bound and process crash requires parent-side cleanup. Cleanup-to-baseline and leak detection remain explicit product requirements. The spec also establishes a general restart rule in platform reconciliation: old runtime/provider identities and action authorization do not survive merely because earlier permission existed.

D1 applies the same principle to LocalView's own session identity:

- logical identity may be restored;
- volatile authority must be reacquired;
- ambiguous identity must fail closed rather than alias two targets;
- restart continuity must not depend on a process-random hash implementation;
- no timeout may be treated as proof that a target or owner is the same target.

## Existing problem

`SessionManager::new` starts with an empty in-memory map. During discovery, a new session uses `Uuid::new_v4()` whenever no current in-memory session matches the discovered target.

That is correct inside one daemon lifetime but not across daemon restart. The daemon loses the old `SessionId`, even when the same project and dev server are still running.

Current in-process reconciliation already prefers:

1. `ProjectIdentity.key + ServerKind`, then
2. exact endpoint.

`ProjectIdentity.key` is currently derived with `std::collections::hash_map::DefaultHasher` from project root/CWD plus command text. That key is a useful process-local grouping hint, but D1 must not promote it into a durable storage contract because the hashing algorithm is not an explicit persistent schema and the command string may contain volatile arguments.

A second consequence is architectural: desktop preview/workspace bridge code can retain a `SessionId` across a daemon restart. If the daemon silently allocates a different UUID, the bridge and the new daemon disagree about the identity of the same target. D2 cannot safely reconcile live surfaces until this identity split is removed.

There is also an in-process edge case D1 must close. If a dev server changes both command text and port inside one daemon lifetime, the legacy project-key match and endpoint fallback may both miss the old session. A durable resolver must not then discover that the same UUID is still active and create a second volatile session. Canonical lineage therefore participates in current-process reconciliation as well as restart recovery.

## Non-goal: durable runtime state

D1 does **not** persist a serialized `Session` and does not restore these fields or authorities across restart:

- `SessionStatus`;
- `preview_visible`;
- `first_seen`, `last_seen`, or `disconnected_at` as cross-process history;
- ObservationBus queues;
- semantic snapshots or freshness generations;
- LiveBridge action state;
- consequential confirmation/dispatch authority;
- provider/UIA attachments;
- RuntimeResourceGovernor reservations or live leases;
- evidence freshness or verification verdicts;
- Chromium process authority;
- surface owner authority;
- portal/capture session authority.

Those subsystems must reacquire their own current authority after restart.

A restored `SessionId` is an identity reference only.

## Approaches considered

### A. Deterministic `SessionId` from project fields

Derive the UUID directly from canonical project fields, for example UUIDv5.

Advantages:

- no durable file;
- restart naturally recomputes the same UUID.

Rejected because:

- any future lineage-schema improvement changes the UUID;
- project moves/renames cannot retain identity without compatibility aliases;
- ambiguous same-root targets are difficult to fence cleanly;
- fallback endpoints make UUID stability depend on port;
- identity derivation and public UUID format become permanently coupled.

### B. Persist the full `SessionManager`

Serialize sessions to disk and reload them on daemon boot.

Advantages:

- superficially simple continuity story.

Rejected because it dangerously conflates identity with runtime truth. Reloading old status, visibility, attachment, or freshness fields would invite stale authority after restart. It also creates migration burden far beyond D1.

### C. Versioned durable lineage-to-UUID registry — selected

Persist a small mapping from a versioned stable lineage key to `SessionId`.

Advantages:

- UUID is decoupled from lineage encoding;
- lineage schema can evolve without changing public IDs;
- only identity survives restart;
- ambiguity can be fenced before reuse;
- D2 can later rely on an old desktop bridge and a restarted daemon converging on the same UUID.

D1 selects approach C.

## Architecture

D1 adds three bounded concepts:

1. `SessionLineage` — typed, versioned logical target identity used for current-process and durable matching;
2. `SessionIdentityRegistry` — durable lineage-to-UUID mapping with strict validation and atomic replacement;
3. `SessionIdentityResolver` — the only component allowed to choose whether a newly discovered target reuses a durable UUID or receives a new UUID.

The registry belongs to session identity, not to resource governance. It does not contain budgets, leases, permissions, evidence, or owner liveness.

### Suggested ownership boundary

`crates/sessions` should own resolution semantics because that crate already owns session creation/reconciliation.

A small durable-store implementation may live inside `crates/sessions` or a focused module used only by it. The daemon owns the state-directory path and constructs the resolver/store during startup.

`crates/core::project_identity` may continue producing existing `ProjectIdentity` for compatibility. D1 should not rewrite unrelated project-grouping behavior unless required to expose the canonical fields needed by lineage construction.

## SessionLineage V1

D1 must introduce an explicit schema rather than hashing `ProjectIdentity.key`.

Conceptual shape:

```text
SessionLineageV1
  discriminator = "localview-session-lineage-v1"
  anchor
  server_kind
```

`anchor` has two forms.

### Project anchor — preferred

When discovery has a usable project root/CWD:

```text
ProjectAnchor
  normalized_project_path
```

The durable lineage is:

```text
(project anchor, ServerKind)
```

Port is intentionally excluded. Scheme is intentionally excluded. Framework/title/PID are intentionally excluded.

This preserves identity when the same project restarts on another localhost port and avoids coupling the durable key to volatile process IDs or probe metadata.

The same project root may legitimately expose more than one `ServerKind`; `ServerKind` therefore remains part of the lineage.

### Endpoint anchor — conservative fallback

When no trustworthy project path exists:

```text
EndpointAnchor
  scheme
  host
  port
```

Endpoint fallback is exact. D1 does not guess that port 5173 and 5174 are the same target when there is no project anchor.

This sacrifices liveness for correctness: a target with no project identity that changes port receives a new durable lineage.

## Project-path normalization

The durable schema must define path normalization explicitly. It must not rely on process-random hashing and must not require the path to still exist during registry reload.

V1 normalization requirements:

- use the discovered `git_root` when available, otherwise CWD;
- normalize path separators into a stable internal representation;
- remove redundant trailing separators except filesystem root;
- normalize `.` path components lexically;
- reject unresolved `..` traversal in the durable key rather than inventing a filesystem canonical result;
- on Windows, normalize drive-letter casing and use case-insensitive comparison semantics for the durable key;
- on Unix-like systems, preserve path case;
- do not call filesystem `canonicalize()` as a requirement for restore, because the target may be temporarily unavailable when state is loaded.

The implementation plan must encode this normalization as a separately tested pure function.

## Why command text is not in V1 lineage

The current process-local project key includes command text. D1 deliberately does not persist the full command as part of the durable lineage because dev-server commands commonly contain volatile port, mode, shell-wrapper, or temporary arguments.

Removing command text creates a possible ambiguity when two simultaneous servers of the same `ServerKind` run from one project root. D1 handles that with an explicit ambiguity fence instead of silently adding unstable command text back into the durable identity.

## Batch lineage analysis

Lineage is computed for the complete discovery batch before session-map mutation.

For each scan:

1. construct a canonical lineage for every discovered server;
2. group discovered servers by lineage;
3. mark lineage groups of size one as unambiguous;
4. mark groups of size greater than one as ambiguous;
5. reconcile active in-memory sessions using only identity evidence that is safe for that group;
6. consult the durable registry only for targets still requiring a new in-memory session.

This ordering prevents the first item in an ambiguous batch from consuming a durable UUID before LocalView notices the second item.

## In-memory reconciliation

Canonical lineage is not disk-only metadata. It is also the preferred continuity key for an existing in-memory session when the current discovery lineage is unambiguous.

For an unambiguous project lineage, reconciliation should prefer:

1. exact matching active/in-memory canonical lineage;
2. compatibility matching needed for existing process-local behavior, such as current `ProjectIdentity.key + ServerKind`;
3. exact endpoint fallback where appropriate;
4. durable identity resolution only if no existing in-memory session matches.

This closes the command+port-change edge case: the current session remains the current session instead of creating a second object that collides with its persisted UUID.

For an ambiguous lineage group, canonical-lineage matching must not merge multiple active targets. Existing exact endpoint/process-local matches may keep already-distinct sessions stable, but unmatched ambiguous targets remain distinct and are not assigned one shared durable identity.

D1 must not turn every 750 ms discovery scan into disk I/O. Healthy scans that continue existing in-memory sessions do not rewrite or reload the durable registry.

## Ambiguity fence

A durable lineage may be reused only when that lineage resolves to exactly one discovered logical target in the current reconciliation batch.

If two or more simultaneously discovered servers produce the same project lineage:

- do not assign one persisted UUID to either arbitrarily;
- do not merge them into one `Session`;
- preserve already-distinct active sessions only through exact safe matches;
- allocate distinct volatile/new session IDs for unmatched active targets;
- do not overwrite the existing durable mapping while ambiguity exists;
- emit a diagnostic that durable identity reuse was blocked by ambiguity.

When ambiguity disappears in a later daemon lifetime or scan, normal exact reuse may resume.

This is a safety-before-liveness rule.

## Durable registry schema

The on-disk state is intentionally small and versioned.

Conceptual document:

```text
SessionIdentityRegistryFile
  schema_version = 1
  records[]

SessionIdentityRecord
  lineage
  session_id
```

The durable file must not contain serialized runtime `Session` state.

The implementation may store the canonical lineage fields directly or a stable cryptographic digest plus sufficient schema metadata. If a digest is used, its algorithm and canonical byte encoding become part of the explicit V1 schema; `DefaultHasher` is forbidden for persistent identity.

For debuggability and migration safety, the recommended D1 representation is the typed lineage fields directly in a versioned local-only JSON document. Project paths are already LocalView-local metadata; D1 does not upload or expose this registry over the control API.

## Storage location

The daemon already owns a LocalView state directory. D1 stores the identity registry under that existing state root, for example:

```text
<state_dir>/session-identities-v1.json
```

The exact filename is implementation detail, but there must be one canonical path per LocalView daemon profile/state root.

No second database or background storage service is introduced.

## Atomic persistence

A newly allocated durable identity must not be published as durable before the mapping has been safely committed.

Required write protocol:

1. serialize the complete next registry state;
2. write a temp file in the same directory;
3. flush/sync the temp file;
4. atomically replace/rename the registry file;
5. sync the parent directory where supported;
6. only then treat the new mapping as durably committed.

If the replacement fails, the previous valid registry remains authoritative.

No in-place partial JSON writes.

## Startup loading

Daemon startup loads and validates the registry before discovery begins.

Validation includes:

- known schema version;
- structurally valid lineage records;
- valid non-nil UUIDs;
- no duplicate lineage mapped to different UUIDs;
- no same UUID mapped to different lineages unless a future explicitly-versioned alias mechanism defines that behavior;
- bounded file size and bounded record count;
- canonical normalized lineage form.

Unknown schema versions or corruption must not be silently rewritten as an empty valid registry.

Preferred failure behavior:

- log/diagnose identity registry unavailable or invalid;
- preserve the invalid file for inspection;
- enter explicit **volatile identity mode** for targets that cannot be safely resolved durably;
- do not claim restart continuity for those sessions;
- do not resurrect authority from the invalid file.

D1 must not make the entire LocalView daemon unusable solely because identity persistence is unavailable.

## Healthy creation flow

For an unambiguous discovered target with a healthy registry and no matching current in-memory session:

### Existing durable mapping

1. construct canonical lineage;
2. find the persisted `SessionId`;
3. verify the UUID is not already bound to a different active target in the current daemon;
4. create the fresh in-memory `Session` using that UUID;
5. publish normal discovery events with the reused UUID.

No old runtime state is loaded.

### New durable mapping

1. construct canonical lineage;
2. allocate a fresh `Uuid::new_v4()`;
3. create the next registry state;
4. atomically persist it;
5. after durable commit succeeds, insert the fresh in-memory `Session` with that UUID;
6. publish discovery events.

This ordering closes the crash window where a UUID could be exposed to desktop/bridge consumers but never become recoverable after an immediate daemon crash.

## Volatile identity mode

If persistence is unavailable, invalid, over capacity, ambiguity blocks durable reuse, or a new mapping cannot be committed, LocalView may still create an in-memory session so localhost discovery remains usable.

That session is **volatile**:

- it receives a fresh UUID when no existing safe in-memory identity can be continued;
- no restart-continuity guarantee is made;
- D1 should expose an internal diagnostic/health signal that continuity is degraded;
- D2 must not later pretend that this UUID is safely recoverable across daemon restart.

D1 does not need to add a new public API field to every `Session` unless implementation proves it necessary. A focused internal resolver result or health diagnostic is sufficient.

## Session removal semantics

Removing an in-memory session after disconnect grace does **not** delete its durable lineage mapping.

Reason: a stopped dev server may legitimately return later and should retain the same logical LocalView identity.

D1 therefore separates:

```text
active session lifetime != durable identity lifetime
```

No age-based TTL is used as identity truth in D1.

## Bounded storage behavior

The registry must have explicit limits to prevent unbounded or malicious state growth.

The implementation plan must choose and test conservative constants for:

- maximum registry bytes read;
- maximum number of records;
- maximum normalized path/host lengths.

When capacity is reached, D1 must not silently evict an existing mapping merely to make room, because silent eviction breaks continuity unpredictably. New unmatched targets become volatile and a diagnostic is emitted until a future explicit compaction/management policy exists.

## Upgrade semantics

Repositories before D1 did not persist the old UUID mapping, so D1 cannot reconstruct a pre-upgrade daemon's random UUID after that daemon has already exited.

Therefore:

- the first healthy D1 discovery establishes the durable mapping;
- continuity guarantees begin after that mapping has been committed;
- no migration code invents an old UUID that was never persisted.

If D1 is installed while the old daemon is still running, cross-process handoff of that old UUID is outside this slice unless an existing supported handoff primitive already exposes it safely.

## Authority separation after restart

Reusing `SessionId` must not make stale authority current.

After daemon restart, even when the UUID is restored:

- provider attachments start detached;
- action confirmations/dispatch permits are absent;
- semantic/visual freshness starts unreconciled;
- evidence stores start according to their own durability policy, not because UUID matched;
- runtime resource leases are not recreated by D1;
- preview visibility is not inferred from old session metadata;
- platform portal/capture stream identities must be reacquired according to platform rules;
- live desktop surface ownership waits for D2 owner reconciliation.

This is a hard correctness boundary.

## D2 dependency contract

D2 may rely on this D1 invariant:

> If a desktop process retains a surface/bridge for logical target L and the restarted daemon rediscovers the same unambiguous durable lineage L with a healthy identity registry, the daemon reuses the same `SessionId` that was committed before restart.

D2 may **not** assume:

- the session UUID proves the desktop owner is alive;
- a surface lease survived;
- visibility survived;
- an action/provider authority survived;
- a volatile D1 session can be safely re-adopted after restart.

D2 still needs its own owner-instance identity, boot epoch, recovery debt, and strong owner-liveness proof.

## Failure semantics

### Registry missing

Normal first-run state. Create an empty healthy registry and establish mappings on discovery.

### Registry corrupt or unknown version

Do not parse partially and do not overwrite it as empty. Enter degraded volatile identity mode and report diagnostics.

### New mapping persistence failure

Do not claim the new UUID is durable. Keep the previous registry authoritative. The discovered target may run with a volatile UUID.

### Existing mapping points to UUID already active for another lineage

Treat as registry invariant failure/ambiguity. Do not alias the sessions.

### Simultaneous duplicate project lineage

Fence durable reuse for that ambiguous lineage and keep active targets distinct.

### Project path changes

V1 treats the moved project as a new lineage. Automatic rename/move aliasing is out of scope.

### Projectless target changes port

Endpoint fallback changes lineage; it receives a new UUID. No heuristic port migration.

### Daemon crash during atomic replacement

On restart, either the old complete registry or the new complete registry is accepted. A partial temp file is not authoritative.

## Data-flow summary

```text
DiscoveryEngine
  -> discovered target batch
  -> canonical SessionLineage V1 for every target
  -> batch ambiguity analysis
  -> SessionManager current-process reconciliation
      -> safe in-memory lineage/project/endpoint match: continue current SessionId
      -> no safe current match:
           SessionIdentityResolver
             -> unambiguous durable mapping exists: reuse UUID
             -> unambiguous new lineage + persist succeeds: commit UUID, then publish
             -> ambiguity/persistence unavailable/failure: volatile UUID + diagnostic
  -> fresh or continued in-memory Session
  -> ObservationEvent using resolved SessionId
```

No provider/resource/evidence state is loaded by this path.

## Expected implementation seams

D1 is expected to touch a narrow set of areas:

- `crates/sessions/src/lib.rs` — creation/reconciliation integration;
- a focused sessions identity/lineage module — canonical lineage, registry, resolver;
- `crates/core` only if a reusable path/project normalization primitive is needed;
- `apps/daemon/src/main.rs` — state-directory store construction and startup diagnostics;
- workspace dependencies only if an explicit stable serialization/hash primitive is required;
- session/daemon contract tests and CI gate;
- implementation-status documentation.

D1 should not require changes to resource-governor semantics, Perception Budget, surface registry lifecycle, or Windows provider authority.

## TDD / failure oracles

Implementation must proceed RED before GREEN. Required contracts include at least:

1. same project anchor + same `ServerKind` across two fresh `SessionManager`/resolver lifetimes reuses the exact UUID;
2. same project anchor changing localhost port reuses the UUID;
3. same project anchor changing endpoint scheme still reuses the UUID;
4. same unambiguous project lineage changing both command text and port inside one daemon lifetime continues the same in-memory UUID;
5. different normalized project roots never share a UUID;
6. same project root with different `ServerKind` values receives distinct durable lineages;
7. projectless exact endpoint survives restart with the same UUID;
8. projectless target changing port receives a new UUID;
9. two simultaneous targets producing the same project lineage are fenced from durable aliasing and remain distinct;
10. already-distinct ambiguous targets may continue only through exact safe current-process matches;
11. healthy in-process reconciliation does not perform repeated registry writes;
12. in-memory session removal does not delete its durable identity mapping;
13. a new UUID is not published as durable before the atomic registry commit succeeds;
14. simulated persistence failure produces a volatile session and leaves the previous valid registry unchanged;
15. corrupt registry does not get silently replaced with empty state;
16. unknown schema version fails closed into degraded identity mode;
17. duplicate lineage-to-different-UUID records are rejected;
18. same UUID bound to unrelated lineages is rejected;
19. bounded file/record limits are enforced without silent eviction;
20. path normalization is deterministic across restart and tested for platform-specific rules;
21. `ProjectIdentity.key` compatibility is not accidentally promoted as the durable identity source;
22. reused UUID does not restore `preview_visible`, provider attachments, action authority, resource leases, or stale freshness state;
23. first D1 run establishes continuity only after successful mapping persistence;
24. a named CI gate runs durable session identity continuity contracts.

## Scope boundaries

In scope:

- versioned stable session lineage;
- current-process canonical-lineage reconciliation;
- durable lineage-to-UUID registry;
- atomic registry persistence;
- startup load/validation;
- ambiguity fencing;
- volatile degraded mode;
- `SessionManager` creation integration;
- daemon construction/diagnostics;
- exact restart-continuity tests and CI gate.

Out of scope:

- D2 surface owner recovery;
- desktop owner-instance liveness;
- boot epoch/recovery-debt journal;
- persistence of full `Session` state;
- persistence/restoration of action authorization;
- persistence/restoration of provider attachments;
- persistence/restoration of governor reservations/live leases;
- persistence of preview visibility;
- automatic project-move aliases;
- heuristic endpoint port migration without a project anchor;
- Perception Budget changes;
- analysis concurrency;
- Chromium child-process-tree accounting.

## Acceptance criteria

D1 is complete only when:

- a healthy persisted lineage mapping causes the same unambiguous localhost target to receive the exact same `SessionId` after daemon restart;
- canonical unambiguous lineage also prevents current-process duplicate identity when command and port change together;
- port changes do not break identity for project-anchored targets;
- projectless fallback remains exact and conservative;
- ambiguous same-lineage targets never get silently merged;
- the persistent key does not use `DefaultHasher` or another unspecified process-local hash contract;
- new durable UUIDs are committed before they are published as durable identities;
- corrupt/unknown/over-capacity state cannot fabricate identity and is not silently destroyed;
- persistence failure degrades explicitly to volatile identity rather than crashing the whole product or claiming continuity;
- session removal does not erase durable identity;
- restored UUID does not restore stale runtime authority;
- healthy in-process reconnect behavior remains correct and disk-cold;
- D1 introduces no second governor and no Perception Budget change;
- exact-head CI passes the named durable session identity gate plus full existing regression suites;
- post-merge verification is checked before D1 is declared closed.
