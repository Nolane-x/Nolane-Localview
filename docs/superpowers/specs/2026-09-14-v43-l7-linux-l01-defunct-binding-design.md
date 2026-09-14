# V4.3 Linux L01 — AT-SPI DEFUNCT Binding Invalidation Design

Date: 2026-09-14
Status: design approved in chat; implementation not yet authorized from this written spec
Base authority: `main@fb8c0b21c4a56369cd22312253c708ea5630c7dd`
Scope: V43-L01 only

## 1. Problem

V4.3 Linux failure taxonomy defines:

> V43-L01 — AT-SPI DEFUNCT still actionable → invalid binding

AT-SPI defines the `DEFUNCT` state as meaning that an accessible object no longer has a valid backing widget. A LocalView element binding that observes this state must therefore stop being usable as action authority. Keeping the same D-Bus destination/object path, role, name, or prior capability after DEFUNCT is not sufficient to keep the binding valid.

L01 closes only this lifetime-invalidity boundary. It does not solve STALE freshness, visibility/occlusion, toolkit event completeness, XDG Desktop Portal lifecycle, PipeWire identity, keyboard permission, or clipboard authority; those remain L02–L10.

## 2. Existing repository context

The repository currently has:

- provider-neutral contracts in `crates/native-provider`;
- a Windows UIA provider/runtime stack;
- a macOS AX provider stack;
- no shipping Linux AT-SPI provider crate registered in the workspace.

`crates/native-provider` already carries provider-neutral snapshot and identity concepts and must remain free of Linux-specific D-Bus/AT-SPI transport ownership. L01 therefore introduces a Linux sibling provider instead of putting AT-SPI dependencies into the provider-neutral crate.

## 3. Chosen architecture

Create a production crate:

`crates/linux-atspi-provider`

Package name:

`localview-linux-atspi-provider`

Its responsibility is the Linux AT-SPI provider boundary. It owns live AT-SPI transport/proxy access, Linux provider-local binding state, and the conversion of live platform facts into LocalView provider semantics.

The crate may depend on `localview-native-provider` and `localview-protocol`; the inverse dependency is forbidden. `localview-native-provider` must not gain `atspi`, `zbus`, portal, or PipeWire dependencies as part of L01.

The first implementation targets exact `atspi = 0.30.0`, the current release verified during design. The implementation plan must include an MSRV/dependency-resolution preflight against the repository's Rust 1.85 floor before production code is written. If exact 0.30.0 cannot satisfy that preflight, the design spec must be amended and re-reviewed before any alternate AT-SPI version is used. Floating/wildcard dependency selection is forbidden.

## 4. Binding model

### 4.1 Binding identity

A Linux AT-SPI binding represents one provider-observed accessible instance inside one provider/target lifetime. It must contain enough provider-local identity to distinguish the bound object without claiming that D-Bus object path alone is globally durable identity.

The minimum conceptual fields are:

- provider incarnation reference;
- target/application incarnation reference;
- AT-SPI bus destination or equivalent application endpoint identity;
- AT-SPI object path;
- acquisition cut/revision;
- binding lifecycle state.

The exact public field layout may remain private if constructor/accessor APIs preserve these invariants.

### 4.2 Lifecycle state

The binding lifecycle must distinguish at least:

- `Live` — last authoritative L01 liveness check did not observe DEFUNCT;
- `InvalidDefunct` — a fresh state observation contained AT-SPI `DEFUNCT`;
- `Unavailable`/unknown observation outcome — the provider could not obtain a trustworthy state observation.

`Unavailable` is deliberately not `InvalidDefunct`. A transport failure, vanished bus peer, timeout, malformed reply, or generic D-Bus error may require fail-closed behavior, but L01 must not misreport those conditions as an explicit DEFUNCT fact.

### 4.3 Terminal invalidation

Once a particular binding revision enters `InvalidDefunct`, that binding is terminally invalid. It cannot transition back to `Live` merely because:

- the same object path appears again;
- the bus destination is reused;
- the same role/name/capabilities reappear;
- a later state call on a newly created object is non-defunct.

A usable object after DEFUNCT requires reacquisition into a new binding revision/incarnation. This prevents object-path reuse from resurrecting stale authority and prepares the identity boundary required by L02 without implementing L02 freshness semantics.

## 5. Authority rule

No action-authorizing API may derive or return actionable capability from an L01 binding until it has performed or consumed a fresh provider-owned liveness validation for that binding revision.

The critical rule is:

`DEFUNCT observed -> binding invalid -> zero action authority from that binding`

The provider must reject authorization before dispatch. L01 does not need to implement a complete Linux action executor; it needs an explicit production authority boundary that future Linux actions must pass through.

The authorization result must be typed. A boolean such as `is_actionable: bool` is insufficient as the sole authority representation because it erases whether denial came from DEFUNCT, unavailable state observation, lineage mismatch, or another future policy reason.

A validation/authorization API should therefore return an opaque or provider-owned permit on success and a typed failure on denial. Callers must not be able to construct the permit directly.

## 6. Freshness boundary for L01

L01 requires a fresh state read before action authorization, but it does not define the general proof freshness model that belongs to L02.

For L01, "fresh" means the provider asks the live AT-SPI endpoint for the current state set as part of the authorization operation or consumes a provider-owned state observation whose validity is restricted to the same authorization transaction. A previously cached non-DEFUNCT state must not be sufficient to authorize an action after a later DEFUNCT state has been observed.

L02 will extend this with explicit STALE/freshness state binding. L01 must not preemptively conflate `STALE` with `DEFUNCT`.

## 7. State interpretation

The provider must use AT-SPI state semantics directly:

- `State::Defunct` / equivalent state-set membership is the only positive L01 signal for `InvalidDefunct`;
- absence of DEFUNCT in a successfully obtained current state set permits the L01 liveness gate to remain live, subject to all other LocalView authorities;
- `State::Stale` is not handled by L01 and must not be silently treated as equivalent to DEFUNCT;
- `VISIBLE` is irrelevant to L01 actionability and must not be treated as proof of liveness or unobscured geometry.

The implementation must use the library's typed state representation (`StateSet`/`State`) rather than parsing human-readable D-Bus error strings or state names.

## 8. Production API shape

The exact Rust names may be refined during implementation, but the design requires these roles:

1. `AtspiElementBinding` — provider-owned binding revision/lifecycle.
2. `AtspiBindingInvalidation` or equivalent typed reason including `Defunct`.
3. A provider operation that obtains the live state set from the bound accessible endpoint.
4. An authorization operation that validates lineage + current L01 liveness and yields an opaque one-shot/bounded action-eligibility permit or a typed denial.
5. A reconciliation/reacquisition operation that can create a new binding after terminal invalidation without mutating the old binding back to live.

The permit does not itself dispatch input or invoke an AT-SPI action in L01. It proves only that the binding passed the L01 liveness gate at the authorization cut. Future action work must combine it with capability, principal, freshness, policy, resource and postcondition authorities.

## 9. Error semantics

At minimum, typed failures must distinguish:

- binding is explicitly DEFUNCT;
- provider/target lineage mismatch;
- state observation unavailable/failed;
- binding already terminally invalidated.

A generic state-read failure must fail closed for action authorization, but it must preserve uncertainty. The provider must not claim `Defunct` unless the AT-SPI state set actually contained DEFUNCT.

No L01 path may convert an unknown transport error into action permission through fallback.

## 10. Real-provider validation

L01 is not closed by unit tests alone. The validation lab must exercise the production Linux provider against a real AT-SPI/D-Bus accessibility endpoint on Ubuntu CI.

### 10.1 Seed requirements

Create a deterministic accessibility seed under the validation lab/provider-seed area. The seed must expose a real accessible object that is initially non-defunct and supports a recognizable actionable surface or action interface.

The harness must connect through the same production AT-SPI provider code used by LocalView, not a lab-only provider implementation.

The seed/test control channel may be test-only, but it may only control ground truth/lifecycle transitions. It must not feed the provider the answer for whether the accessible is DEFUNCT.

### 10.2 Required oracle sequence

The real-provider test must prove, in order:

1. accessibility bus/seed is live;
2. provider acquires a binding to the exact seed accessible;
3. a live state read does not contain DEFUNCT;
4. L01 authorization succeeds for that live binding;
5. seed destroys or invalidates the backing accessible such that the real AT-SPI endpoint reports DEFUNCT for the old accessible reference;
6. provider observes DEFUNCT from AT-SPI itself;
7. the old binding becomes terminally invalid;
8. action authorization from the old binding is denied with typed DEFUNCT/invalid-binding semantics;
9. no action dispatch occurs after that denial;
10. if the same object path or semantic attributes are reused by a replacement object, the old binding still cannot revive; only reacquisition may create a new binding.

If a candidate toolkit removes the object immediately from D-Bus instead of exposing an observable DEFUNCT state for the old reference, that toolkit is insufficient for the L01 oracle. The implementation must choose another real seed/toolkit or lifecycle construction that produces the platform DEFUNCT fact. A merely unavailable/removed object may be tested as a fail-closed secondary case, but it cannot substitute for the required DEFUNCT proof. **L01 must not merge without an actual platform-observed DEFUNCT oracle.** Production code must never simulate or inject DEFUNCT to satisfy this gate.

## 11. Evidence artifact

The Linux L01 real-provider workflow should publish a compact machine-readable record bound to the exact candidate SHA. It should include non-secret facts such as:

- schema version;
- case id `L01`;
- candidate SHA;
- provider family;
- binding revision/opaque non-sensitive identity digest if needed;
- initial defunct state observed: false;
- final defunct state observed: true;
- old binding authorization after DEFUNCT: denied;
- denial class: invalid/defunct binding;
- dispatch count after DEFUNCT: zero;
- `old_binding_revival_succeeded: false`;
- reacquisition outcome if the replacement-object subcase is exercised.

Do not persist raw environment-specific D-Bus data unless needed for reproducibility. Evidence must not turn bus/object identifiers into a new cross-session durable authority.

## 12. TDD sequence

Implementation must follow this order:

1. RED contract: old binding remains actionable after a typed DEFUNCT observation because the Linux authority API does not yet exist.
2. Minimal GREEN: production binding lifecycle + typed invalidation + opaque authorization gate.
3. RED terminality/reuse contract: object path reuse attempts to revive the invalidated binding.
4. GREEN: reacquisition creates a new binding; old revision remains terminal.
5. Real-provider RED/bring-up: production provider cannot yet satisfy the Ubuntu AT-SPI oracle.
6. Real-provider GREEN: real AT-SPI state proves live -> DEFUNCT -> denied, with zero dispatch.
7. Exact-head retained regression sweep.

Tests must fail for the intended semantic reason before GREEN code is written. Workflow/YAML failures do not count as RED evidence.

## 13. CI and regression gates

Before merge, one immutable exact head must pass:

- Linux L01 contract workflow;
- Linux L01 real-provider workflow on Ubuntu;
- full repository CI including Clippy with warnings denied;
- retained Windows real-provider/UIA gates;
- retained macOS M01–M10 contract/real-provider gates;
- any existing cross-platform native-provider contracts affected by the new crate.

The implementation must not weaken, skip, mark-continue-on-error, or conditionally bypass existing gates to accommodate Linux.

## 14. Expected repository scope

Expected implementation scope is limited to:

- workspace registration for the Linux provider crate;
- `crates/linux-atspi-provider/**`;
- Linux L01 contract test/workflow;
- Linux real-provider seed/harness/workflow;
- minimal shared protocol/native-provider additions only if an existing provider-neutral type is genuinely missing and both Windows/macOS semantics remain unchanged;
- implementation plan/design references.

Changes to portal/PipeWire, keyboard/pointer dispatch, clipboard, macOS AX behavior, Windows UIA behavior, perception budget, Chromium, or unrelated UI code are out of scope.

## 15. Non-goals

L01 explicitly does not:

- close L02 `STALE` freshness semantics;
- infer visibility or unobscured status;
- implement event reliability/reconciliation beyond what is needed to invalidate this binding;
- implement XDG portal lifecycle;
- establish PipeWire node durability;
- grant keyboard, pointer, or clipboard authority;
- implement a general Linux action executor;
- claim object path as durable identity;
- parse human error text to infer provider state;
- create a mock-only provider and call it production proof.

## 16. Security/correctness invariants

L01 is complete only if all of these hold:

- A fresh explicit DEFUNCT observation can never produce action authority from the affected binding.
- The affected binding cannot revive.
- Reuse of bus/object path does not revive the old binding.
- Unknown state-read failure fails closed without being mislabeled DEFUNCT.
- A prior cached non-DEFUNCT observation cannot override a later DEFUNCT observation.
- Lab evidence is generated through the shipping provider path.
- Existing Windows/macOS provider semantics do not change.
- L01 introduces no silent fallback to visual/input authority.

## 17. Definition of Done

V43-L01 is closed only when:

1. production Linux AT-SPI provider boundary exists in the workspace;
2. binding lifecycle contains a terminal DEFUNCT invalidation state;
3. action eligibility is provider-owned and fail-closed;
4. explicit DEFUNCT is distinguished from unavailable/unknown transport state;
5. old binding cannot revive after path/semantic reuse;
6. a real Ubuntu AT-SPI oracle demonstrates non-defunct -> DEFUNCT -> authorization denied with zero post-defunct dispatch;
7. evidence artifact is exact-SHA bound;
8. retained Windows/macOS/full CI gates are green on the same immutable head;
9. PR audit shows only expected scope and no unresolved review/comment drift;
10. merge uses an expected-head guard and post-merge `main` is verified.

Only after these conditions hold may work proceed to V43-L02.

## 18. External platform facts verified for this design

Verified on 2026-09-14 against current upstream documentation:

- AT-SPI `DEFUNCT` means the accessible no longer has a valid backing widget.
- AT-SPI `STALE` is a different state meaning returned information may no longer be synchronized with application state.
- AT-SPI `VISIBLE` does not guarantee an object is unobscured.
- Rust `atspi` 0.30.0 exposes typed `StateSet` and `State::Defunct` semantics and is an asynchronous zbus-based AT-SPI implementation.

References:

- https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/enum.StateType.html
- https://docs.rs/atspi/0.30.0/atspi/
- https://docs.rs/atspi/0.30.0/atspi/struct.StateSet.html
- https://docs.rs/atspi/0.30.0/atspi/enum.State.html
