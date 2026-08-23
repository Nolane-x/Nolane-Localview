# LocalView Security Model

## Trust boundary

The primary boundary is the developer machine. LocalView observes local development applications, but those applications may load remote resources or contain untrusted content. Therefore localhost does not imply trusted JavaScript.

## Control plane

The control HTTP server binds only to loopback. Sensitive endpoints require a random bearer token stored in the user's local application-data directory. The health endpoint is intentionally non-sensitive. A future Unix-domain-socket / named-pipe transport can replace TCP without changing client semantics.

## Navigation

Top-level preview creation accepts only `localhost`, `127.0.0.1` and `::1`. External resource requests made by the local app are a separate policy surface and may be observed, blocked or mocked later. Navigation policy and resource policy must remain separate.

## Tauri capability isolation

Only the main dashboard window receives `core:default`. Dynamically created localhost preview WebViews do not receive the dashboard capability definition and therefore should not be treated as trusted command callers.

## Secret handling

`localview-security` redacts common Authorization, Cookie, API key, token, password and secret shapes before agent-facing serialization. This is defense in depth, not a guarantee that every domain-specific secret can be recognized. Network bodies and storage values should be deny-by-default until an explicit policy grants them.

## Agent permissions

The protocol models Observe, Interact, Test and Advanced capability classes. Production interactions, JS evaluation, storage modification, network mocks and mutation injection must remain separately grantable.

## Side effects

A future action executor must attach side-effect class, allowed scope, external boundary and rollback strategy to high-impact actions. Unknown external side effects should be denied or require human approval.
