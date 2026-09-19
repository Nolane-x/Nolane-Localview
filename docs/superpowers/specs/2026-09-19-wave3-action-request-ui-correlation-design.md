# Wave 3 Action → Request → UI Response Correlation — Canonical Design

## Status

Implementation-authorized foundation for the Wave 3 runtime-telemetry roadmap item:

> action → request → UI response correlation.

This slice deliberately separates **observed causality authority** from **temporal association**. A nearby event is not automatically a caused event.

## 1. Goal

LocalView needs to answer a bounded question:

> After one exact LocalView action, which observed network request(s) and which subsequent UI response signals are plausibly associated with that action?

The first implementation step is a deterministic, bounded correlation policy that can consume exact action boundaries plus already-redacted observer signals.

## 2. Existing authority

The repository already has:

- exact session IDs;
- canonical action IDs;
- action result receipts;
- bounded observer event sequence/history;
- network metadata without response bodies;
- DOM/layout/route observer signals;
- retained Interaction, Network, Semantic and Layout evidence;
- V4.3 consequential authority for UI-changing actions.

The correlation layer must reuse those authorities. It must not create a second action execution path.

## 3. Trust rule

Correlation has two possible evidence classes.

### 3.1 Exact context binding

Future live integration may prove that an observer event carries an exact daemon-owned action context.

Only that future class may claim an exact action binding.

### 3.2 Temporal window

The initial foundation correlates events only when they fall inside a bounded action window and, when known, the exact route matches.

This is **association evidence**, not proof of causation.

The model therefore records:

`basis = temporal_window`

and caps confidence at a conservative value.

## 4. Inputs

### ActionCorrelationWindow

- non-empty action ID;
- daemon-owned action start boundary;
- daemon-owned completion boundary;
- optional exact loopback route.

Completion must not precede start.

### RuntimeSignal

Only metadata already permitted by LocalView privacy rules:

- opaque signal ID;
- kind;
- bounded monotonic/normalized observation time;
- optional route;
- optional stable element reference.

No request body, response body, cookie, authorization header, form value or raw secret is introduced.

### Policy

- bounded tail after action completion;
- bounded total signals inspected;
- bounded UI responses retained per request.

## 5. Initial signal classes

Network request evidence:

- `network`.

UI-response evidence:

- `dom_mutation`;
- `layout`;
- `route`.

Console/performance events may remain in the candidate window but do not become a UI-response link in this first slice.

## 6. Route authority

When the action window has a route, a candidate signal must carry the same route.

Unknown-route or different-route signals are excluded.

When no action route is available, the trace remains lower-confidence temporal evidence.

## 7. Ordering

Eligible signals are sorted by:

1. observed time;
2. stable signal ID.

For each network signal, only later UI-response signals are linked.

A UI signal before the request is not rewritten as a response to that request.

## 8. Boundedness

Default foundation policy:

- tail: 1.5 seconds;
- max candidate signals: 256;
- max UI responses per request: 16.

Hard policy ceilings:

- accepted tail: 10 seconds;
- candidate signals: 4,096;
- UI responses per request: 64.

A zero-capacity or oversized policy fails closed. The hard ceilings exist even though the defaults are much smaller, so an internal misconfiguration cannot silently turn the pure policy into an unbounded scan or output fan-out.

If candidate count exceeds the cap, the trace is marked `truncated`.

## 9. Confidence

Temporal correlation never receives deterministic confidence.

Initial values:

- exact route present and matched: at most 0.65;
- route unavailable: at most 0.55.

Later exact-context evidence must use a distinct basis rather than silently raising temporal confidence.

## 10. Failure behavior

The pure policy returns explicit failure for:

- empty action ID;
- completion before start;
- zero signal capacity;
- zero response capacity;
- tail above the hard maximum.

Arithmetic uses saturating bounds.

## 11. Live integration sequence

The later live closure must follow this authority order:

```text
canonical action admitted
-> daemon records exact action execution boundary
-> observer/network evidence continues through existing drain
-> action completes
-> bounded correlation cut is selected
-> correlation policy runs
-> trace is retained as derived evidence
-> UI/agent surfaces consume the trace
```

No caller may submit:

- action timestamps;
- trusted request IDs;
- trusted UI-response IDs;
- confidence;
- correlation basis;
- a precomputed trace.

## 12. Consequential-action compatibility

UI-changing actions already require canonical V4.3 consequential authority where applicable.

Wave 3 correlation must attach to the canonical action ID/receipt produced by that authority. It must not re-enable the legacy direct click/type route.

## 13. Persistence

The foundation type is serializable, but this slice does not yet claim live persistence.

When live persistence lands:

- parent evidence IDs must be exact;
- derived evidence must identify the source action ID;
- correlation confidence must remain distinct from observed evidence confidence;
- a trace must never upgrade temporal evidence into deterministic proof.

## 14. Human-facing behavior

A future Network/Diagnostics surface may display:

- “Associated with action” for temporal evidence;
- “Bound to action” only for future exact-context evidence.

The UI must not use “caused by” for a `temporal_window` trace.

## 15. Non-goals

This foundation does not yet claim:

- end-to-end live causal telemetry;
- request-body inspection;
- response-body inspection;
- distributed tracing;
- server-side span propagation;
- arbitrary browser instrumentation;
- background-request ownership;
- exact causation from timestamps alone;
- causal scoring by an LLM;
- action replay;
- network mocking/delay.

## 16. Foundation completion gates

The pure foundation is complete when executable tests prove:

- requests before the action window are excluded;
- requests after the bounded tail are excluded;
- UI responses must occur after a request;
- exact route mismatch is excluded;
- ordering is deterministic;
- signal count is bounded;
- response count is bounded;
- truncation is explicit;
- invalid windows fail closed;
- oversized policies fail closed;
- temporal confidence never exceeds the documented cap;
- basis is explicitly `temporal_window`.

## 17. Live closure gates

The roadmap item remains Partial until a later exact-head implementation proves:

- daemon-owned action start/completion boundaries;
- exact session binding;
- canonical consequential action compatibility;
- observer cut selection without caller authority;
- derived evidence parent IDs;
- authenticated read API;
- frontend Network/Diagnostics presentation;
- stale session isolation;
- runtime browser evidence;
- full repository CI and platform regressions.

## 18. Governing rule

> **LocalView may correlate nearby evidence, but it may call evidence causal only when the authority chain proves the binding.**
