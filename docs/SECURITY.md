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


## Credential persistence hardening

`control.token` is a local credential, not ordinary cache data. The daemon now refuses token symlink/reparse leaves, uses create-new semantics for first creation, reuses a non-empty existing credential instead of rotating it implicitly, and never truncates a pre-existing path to create the token.

On Unix, the LocalView state directory is tightened to owner-only mode and the token is tightened to owner read/write mode independently of the process umask. Reads revalidate the opened file identity before trusting its contents.

On Windows, LocalView does **not** claim that Unix permission bits protect the credential. The token remains under the per-user LocalAppData tree and inherits that Windows ACL model; LocalView additionally opens credential paths with reparse-point-aware flags and rejects reparse entries. A future dedicated Windows DACL primitive would be a separate security change and must preserve same-user daemon/desktop/CLI/MCP access.

## Bundled dashboard CSP

Production builds use a non-null CSP. Script execution remains `'self'` only; `'unsafe-eval'` is not granted. Inline styles remain allowed because the current dashboard uses runtime React positioning styles. Iframe loading is limited to self plus HTTP(S) loopback sources for `localhost`, `127.0.0.1` and `::1`.

The development CSP remains disabled explicitly so Vite/HMR is not accidentally constrained by the production policy. Tauri capability isolation remains the authorization boundary: CSP is defense in depth and does not replace command capabilities.

## Open UI-owned sandbox divergence

The React fallback in `apps/desktop/src/app/WorkspaceSurface.tsx` currently renders the localhost preview iframe without a `sandbox` attribute, while the native-workspace design document describes a sandboxed iframe fallback. This security-adjacent divergence is intentionally **not** fixed in the runtime/persistence security branch because that file belongs to the UI audit lane.

The UI owner should add a least-privilege iframe sandbox after rendered validation. Do not grant top-navigation, popup escape or download authority without evidence that the product requires it; keep `referrerPolicy="no-referrer"`. Any `allow-scripts`, `allow-forms` or `allow-same-origin` token should be justified by localhost application compatibility evidence.
