# Wave 3 Live Network Fault Authority — Canonical Design

## Status

Implementation-authorized design for the Wave 3 runtime-telemetry roadmap item:

> network failure/delay/mock layer wired to live sessions.

This design is intentionally narrower than a general-purpose proxy. It creates a bounded, session-owned test fault layer for LocalView-managed loopback pages and reuses the existing live instrumentation, mutation, session, observer and evidence authorities.

## 1. Goal

LocalView must be able to run an explicit developer-authorized experiment against one exact live session:

- fail a matching request;
- delay a matching request;
- return a bounded empty mock response;
- observe the resulting network/UI evidence;
- remove the experiment deterministically.

The layer exists to answer questions such as:

- does the UI expose a useful failure state when one API request fails?
- does a loading state remain usable under a bounded delay?
- does a branch correctly handle a known HTTP status?

It is not an arbitrary browser interception system.

## 2. Existing authority to reuse

The repository already has:

- exact session identity and reconnect lifecycle;
- authenticated loopback control plane;
- LocalView-owned preview/workspace surfaces;
- fetch/XHR instrumentation with metadata-only network observation;
- true aggregate in-flight request accounting;
- bounded observer history;
- action → request → UI temporal correlation policy/live closure;
- `MutationOperator::ForceHttpStatus` and `MutationOperator::ForceTimeout` primitives;
- deterministic evidence retention;
- generic public BridgeAction isolation from private capture actions.

The new layer must reuse these authorities. It must not create a second session model, a permanent browser proxy, or an alternate evidence channel.

## 3. Governing trust rule

A network fault is executable only when all of the following are true:

1. the caller is authenticated;
2. the exact session is live;
3. the target is a LocalView-managed page;
4. the rule targets only an HTTP(S) loopback request;
5. the rule shape is canonical and bounded;
6. the lease is finite;
7. installation reaches the exact session-owned managed surface;
8. the runtime acknowledges the exact lease token.

A caller never supplies JavaScript, browser interception callbacks, response bodies, headers, cookies, authorization data, or arbitrary error text.

## 4. Canonical rule model

The pure policy crate exposes canonical types conceptually equivalent to:

```rust
pub struct NetworkFaultPlan {
    pub rules: Vec<NetworkFaultRule>,
    pub lease_ms: u64,
}

pub struct NetworkFaultRule {
    pub id: String,
    pub transport: FaultTransport,
    pub method: FaultMethod,
    pub path: String,
    pub effect: NetworkFaultEffect,
    pub max_hits: u16,
}

pub enum FaultTransport {
    Fetch,
    Xhr,
    Both,
}

pub enum FaultMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

pub enum NetworkFaultEffect {
    Fail,
    Delay { milliseconds: u64 },
    MockStatus { status: u16 },
}
```

These are data authorities, not script-generation authorities.

## 5. Target matching

Rules match only canonical request metadata.

### 5.1 Path

A rule path:

- starts with `/`;
- is at most 256 UTF-8 bytes;
- contains no scheme or host;
- contains no fragment;
- contains no username/password authority;
- contains no control characters;
- does not include a query string.

Runtime URL normalization strips query and fragment before matching.

This prevents test rules from retaining query secrets and prevents a rule from becoming an arbitrary external URL selector.

### 5.2 Origin

The runtime verifies the live request URL itself.

A fault may execute only when the final resolved URL is:

- `http:` or `https:`;
- loopback by canonical hostname/IP rules;
- inside the currently managed LocalView page context.

Non-loopback requests always bypass the fault layer unchanged.

A caller cannot override this check.

### 5.3 Method and transport

Method and transport are explicit canonical enums.

There is no regex matcher, substring matcher, arbitrary JavaScript predicate or caller-authored function.

## 6. Effects

### 6.1 Fail

`Fail` produces a deterministic network-style failure.

For fetch:

- reject with a fixed LocalView network error class/message;
- do not call the underlying fetch.

For XHR:

- complete through a bounded synthetic error path;
- produce the expected error/loadend behavior;
- never fabricate a successful status.

No caller-authored error string is accepted.

### 6.2 Delay

`Delay { milliseconds }` waits before invoking the real request.

Policy:

- minimum: 1 ms;
- maximum: 5,000 ms;
- delay contributes to in-flight accounting;
- lease expiry does not cancel a request that already consumed a delay rule;
- no busy-looping.

After the delay, the original fetch/XHR path executes normally.

### 6.3 MockStatus

`MockStatus { status }` returns a synthetic empty response with a canonical status.

Allowed status range:

- 200–599;
- status 204/205/304 use an empty body as required;
- all mocks use an empty body;
- no caller headers;
- no cookies;
- no redirect URL;
- no response body fixture in this slice.

The runtime emits explicit metadata marking the observation as LocalView fault-injected.

This first live layer tests status-handling behavior, not API payload fixtures.

## 7. Boundedness

Hard ceilings:

- at most 16 rules per plan;
- at most 256 bytes per path;
- lease: 100 ms–30,000 ms;
- delay: 1–5,000 ms;
- `max_hits`: 1–64;
- at most one active lease per session;
- at most one effect per rule.

Rules with duplicate `(transport, method, path)` selectors are rejected.

Rules are evaluated in stable input order after validation. Because duplicate selectors are rejected, first-match ambiguity cannot occur.

A rule stops matching after its hit budget is exhausted.

## 8. Lease authority

Every installation receives a daemon-minted opaque lease token and expiry.

The lease is bound to:

- exact session ID;
- exact plan fingerprint;
- exact managed surface incarnation;
- expiry.

The page runtime receives only the validated canonical plan plus opaque token.

Replacement requires exact-session authority and atomically supersedes the prior lease.

Clear requires the exact active token unless session cleanup is occurring.

Session removal, managed-surface loss, navigation outside the LocalView-owned loopback context, or lease expiry clears the active plan.

## 9. Runtime implementation boundary

The live fault executor lives inside LocalView instrumentation because that is already the single bounded fetch/XHR observation boundary.

It must not:

- install a service worker;
- use a permanent proxy;
- use Chromium DevTools interception;
- add platform-specific WebView interception APIs;
- modify application source files;
- persist page request bodies.

The instrumentation runtime keeps the active plan in closure-private state.

Conceptual private methods:

```text
installNetworkFaultPlan(token, canonicalPlan)
clearNetworkFaultPlan(token)
networkFaultState()
```

The generic application page cannot obtain authority merely by submitting a public BridgeAction. Dedicated daemon/desktop control owns installation.

## 10. Bridge isolation

Network fault control is a private page-control operation, not a public user action.

The generic public action endpoint must reject:

- install network fault plan;
- clear network fault plan;
- inspect private lease token.

The live bridge stores only sanitized acknowledgement data:

- installed/cleared;
- plan fingerprint;
- bounded rule count;
- expiry metadata;
- aggregate hit counts.

It never stores:

- request bodies;
- response bodies;
- request headers;
- response headers;
- cookies;
- auth tokens;
- arbitrary page payload.

## 11. Control-plane API

The authenticated control plane exposes explicit session-scoped operations conceptually equivalent to:

```text
POST   /v1/sessions/{session_id}/network-faults
GET    /v1/sessions/{session_id}/network-faults
DELETE /v1/sessions/{session_id}/network-faults/{lease_id}
```

POST accepts only canonical rule data and lease duration.

The daemon:

1. validates auth/session;
2. canonicalizes/validates the plan;
3. resolves exact current managed-surface authority;
4. mints lease identity/token;
5. queues one private install operation;
6. waits for exact acknowledgement;
7. records bounded lease metadata;
8. returns a sanitized receipt.

DELETE:

1. validates auth/session/lease identity;
2. queues exact clear;
3. waits for acknowledgement;
4. clears daemon state;
5. returns a sanitized receipt.

GET exposes no token and no page secrets.

## 12. Desktop authority

The desktop worker may execute network fault private controls only against the exact LocalView-owned surface registered to the requested session.

It must reject:

- missing surface;
- stale surface incarnation;
- owner mismatch;
- non-loopback current route;
- unexpected action kind;
- acknowledgement token mismatch.

No workspace/preview fallback across sessions is allowed.

## 13. Fetch execution semantics

For every fetch:

1. resolve the request URL using browser-native URL resolution;
2. normalize method;
3. test current lease validity;
4. verify loopback;
5. normalize path;
6. select a canonical matching rule;
7. consume one hit atomically in JS runtime order;
8. apply fail/delay/mock or pass through;
9. emit one ordinary `network` observation plus fault metadata.

A pass-through request retains existing behavior.

Fault metadata is bounded to:

- rule ID;
- effect class;
- injected boolean;
- configured delay/status when applicable.

## 14. XHR execution semantics

XHR preserves the existing `open` + `send` accounting guarantees.

The implementation must prove:

- a second rejected `send()` cannot release the first request;
- delay starts exactly one in-flight request;
- fail/mock produce exactly one completion path;
- loadend/error/readystatechange behavior is bounded and deterministic;
- synthetic completion cannot later also complete through the native request;
- hit counters cannot be consumed twice by one send.

The synthetic XHR surface must implement only the browser-visible fields/events required by this bounded contract. It must not pretend to be a full browser network stack.

## 15. Observation and evidence

Every injected request still produces network metadata through the normal observer drain.

Injected observation fields are explicit so downstream analysis can distinguish:

- naturally failed request;
- LocalView-injected failure;
- naturally slow request;
- LocalView-injected delay;
- synthetic status response.

Fault metadata must not make a deterministic claim about resulting UI behavior.

Action/request/UI correlation remains a separate authority.

## 16. Capture-settle interaction

Injected delays participate in the existing aggregate in-flight count.

Therefore stable capture must naturally wait while a delayed real request remains pending.

Synthetic fail/mock completion decrements the in-flight count exactly once.

No special bypass is added to capture settling.

## 17. Mutation integration

The live layer reuses mutation semantics rather than replacing them.

Mapping:

- `ForceTimeout` → bounded fail or delay/timeout experiment according to the explicit live test plan;
- `ForceHttpStatus` → `MockStatus`.

The pure mutation crate does not gain live session authority.

A future mutation challenge coordinator may request these canonical live plans, but the network authority still validates session, target and lease independently.

## 18. Failure classes

Stable bounded error classes include:

- `network_fault_session_unavailable`;
- `network_fault_surface_unavailable`;
- `network_fault_surface_owner_mismatch`;
- `network_fault_route_not_loopback`;
- `network_fault_invalid_plan`;
- `network_fault_invalid_path`;
- `network_fault_invalid_effect`;
- `network_fault_rule_limit`;
- `network_fault_lease_limit`;
- `network_fault_install_timeout`;
- `network_fault_ack_mismatch`;
- `network_fault_stale_lease`;
- `network_fault_clear_failed`.

Raw Tauri/WebView/page exceptions are not primary UI copy.

## 19. Privacy

The feature must not introduce:

- response-body capture;
- request-body capture;
- header capture;
- cookie/localStorage/sessionStorage capture;
- query-value persistence;
- arbitrary URL persistence beyond existing redacted network metadata;
- source-code mutation;
- external proxy credentials.

Paths in retained plan metadata are bounded and query-free.

## 20. Human-facing surface

The initial UI may live under Diagnostics/Advanced.

Required states:

- no session;
- preview unavailable;
- no active fault plan;
- plan ready;
- installing;
- active with expiry and bounded hit counters;
- clearing;
- deterministic failure with Retry.

Human copy must state that the experiment affects only the current LocalView session.

No UI may imply that LocalView intercepted the whole operating system or arbitrary internet traffic.

## 21. Non-goals

This slice does not claim:

- arbitrary internet interception;
- browser-extension request interception;
- OS proxying;
- TLS MITM;
- response-body fixtures;
- arbitrary headers;
- cookie injection;
- bandwidth throttling;
- packet loss simulation;
- WebSocket interception;
- EventSource interception;
- service-worker manipulation;
- offline mode;
- DNS mutation;
- native browser DevTools emulation;
- persistent faults across LocalView restart;
- autonomous mutation campaigns.

## 22. TDD closure sequence

### Gate 1 — Pure policy

RED tests prove:

- canonical method/transport/effect enums;
- empty rules rejected;
- >16 rules rejected;
- invalid/oversized path rejected;
- query-bearing path rejected;
- duplicate selectors rejected;
- invalid status rejected;
- delay above 5 seconds rejected;
- invalid lease rejected;
- zero/excessive hit budget rejected;
- deterministic plan fingerprint;
- query values never enter canonical plan output.

### Gate 2 — Instrumentation contract

RED tests prove:

- private lease state exists;
- loopback check is runtime-owned;
- fail/delay/mock paths exist;
- fetch accounting completes exactly once;
- XHR accounting completes exactly once;
- fault metadata is emitted;
- bodies/headers/cookies are never read for matching;
- expired leases bypass;
- hit budgets exhaust deterministically.

### Gate 3 — Private bridge authority

RED tests prove:

- install/clear are not representable as canonical public action envelopes;
- exact session ownership;
- token/plan fingerprint acknowledgement;
- bounded sanitized result retention;
- session cleanup clears private lease authority.

### Gate 4 — Control-plane live session

Integration tests prove:

- auth required;
- exact session required;
- invalid rules fail before queueing;
- exact managed surface required;
- stale owner/incarnation rejected;
- install waits exact acknowledgement;
- GET never exposes token;
- clear is exact-lease;
- session cleanup clears state.

### Gate 5 — Browser runtime

A deterministic loopback fixture proves in a real managed browser surface:

- fetch fail;
- fetch delay;
- fetch empty-status mock;
- XHR fail;
- XHR delay;
- XHR empty-status mock;
- unrelated request pass-through;
- non-loopback request pass-through;
- hit exhaustion;
- lease expiry;
- clear restores normal behavior;
- observer metadata marks injection;
- in-flight count returns to zero.

### Gate 6 — Product regression / exact-head closure

Minimum:

```text
cargo fmt --check
cargo test -p localview-network
cargo test -p localview-instrumentation
cargo test -p localview-live-bridge
cargo test -p localview-control --test network_fault_live
cargo check --workspace --all-targets
npm run build
full repository CI
Windows UIA Observe
Windows Real Provider Seeds
Human-First browser audit
```

## 23. Documentation truth after closure

Only after one immutable exact implementation head is green:

- move Wave 3 network failure/delay/mock from Remaining to landed live path;
- update Network analysis coverage to describe live bounded fault injection;
- keep payload fixtures, arbitrary internet interception and broader network simulation explicitly unclaimed.

## 24. Completion definition

This slice is complete when a developer can install a bounded fault plan on one exact LocalView-managed live session, observe real fetch/XHR fail/delay/empty-status behavior, and prove the plan is loopback-only, privacy-safe, hit-bounded, lease-bounded, session-isolated and completely removable.

The governing rule is:

> **LocalView may perturb only the exact managed loopback session it owns, through canonical bounded rules; it never becomes a general browser or network proxy.**
