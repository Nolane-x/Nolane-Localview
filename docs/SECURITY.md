# LocalView Security Model

## Trust boundary

The primary boundary is the developer machine. LocalView observes local development applications, but those applications may load remote resources or contain untrusted content. Therefore localhost does not imply trusted JavaScript.

## Control plane

The control HTTP server binds only to loopback. Sensitive endpoints require a random bearer token stored in the user's local application-data directory. The health endpoint is intentionally non-sensitive.

CLI and MCP clients treat `control.token` as authority for one canonical local control-plane origin, not as a credential that may follow an arbitrary URL. Their configurable control origin is restricted to the daemon's exact shipping origin `http://127.0.0.1:45454`; alternate hosts (including `localhost`/`::1`), ports, userinfo, base paths, query strings and fragments are rejected, and authenticated requests do not follow redirects.

A future Unix-domain-socket / named-pipe transport can replace TCP without changing those authority semantics.

## Agent-facing consequential actions

The legacy CLI/MCP DOM mutation commands (`click`, `type`, `key`, `scroll`, `focus`) are not advertised. The public `/actions` route remains observe-only for `Snapshot`/`Measure`; it is not reopened for mutations.

The shipping canonical V4.3 consequential authority is Windows UIA-specific and binds fresh provider element identity, confirmation, risk and postcondition authority. A DOM stable reference cannot be truthfully converted into that provider authority, so CLI/MCP do not fabricate such a mapping. Generic agent-driven DOM mutation remains unsupported until a canonical consequential transport with equivalent authority exists.

## Navigation

Managed native preview/workspace WebViews accept only the URL forms currently granted by the shipping Tauri preview capability: HTTP on `localhost` or exact `127.0.0.1`. HTTPS and IPv6 loopback are rejected before a managed bridge surface is created because the current remote capability does not grant those URL forms end to end.

A created managed surface is additionally pinned to its initial exact origin (scheme + host + effective port). Same-origin route/path/query/fragment navigation remains allowed, while navigation to another loopback port or host alias is rejected. This prevents a valid bridge attestation from being carried onto an unrelated localhost application.

External resource requests made by the local app are a separate policy surface and may be observed, blocked or mocked later. Navigation policy and resource policy must remain separate.

## Tauri capability isolation

Only the main dashboard receives `core:default`. Remote preview/workspace WebViews receive only the narrow `previewbridge` command permission.

Localhost JavaScript is still untrusted. A managed surface receives a Rust-owned random bridge attestation scoped to its exact session, surface identity and incarnation. The initialization bridge captures the invoke primitive and attestation before the application bridge loop is installed; every `previewbridge` command revalidates the live surface identity plus attestation in Rust. Recreating a surface rotates the attestation, and closing the exact surface revokes it. The daemon `control.token` is never placed in the page realm.

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

## Fallback iframe boundary

The React fallback now renders the localhost application with `sandbox="allow-scripts allow-same-origin"` and `referrerPolicy="no-referrer"`. It does not grant top-navigation, popup escape or download authority.

That sandbox is a compatibility boundary for the embedded application, not evidence that the application is trusted. In particular, `allow-scripts allow-same-origin` permits ordinary application JavaScript on its own origin; it does not grant the native `previewbridge` attestation or dashboard capabilities.

## Discovery and provider redirect containment

Discovery probes start from loopback listeners and revalidate every redirect hop. An effective discovery request must remain HTTP(S) on loopback; redirects to external, private-LAN or non-HTTP(S) targets fail closed, and redirect depth is bounded. Discovery does not attach LocalView credentials.

Trusted Ask AI/Fix provider endpoints remain loopback-only. The shared provider transport does not follow redirects, so bounded AI context, source excerpts and provider bearer credentials are sent only to the configured local provider endpoint.


## Process-launch trust

Security-sensitive helper execution in this audit lane does not resolve through an attacker-controlled `PATH`. Listener discovery selects fixed absolute OS utility paths and strips common loader-injection variables before execution. Trusted Open Source launching uses absolute platform launcher paths (`/usr/bin/open`, `/usr/bin/xdg-open`, or `%SystemRoot%\\System32\\rundll32.exe`) and passes the already-canonicalized project file as one argv item rather than through a shell.

This is a scoped claim, not a repository-wide claim that every subprocess has been hardened. In particular, components outside this audit ownership that still invoke tools by name require their own process-trust review before being treated as hostile-`PATH` safe.
