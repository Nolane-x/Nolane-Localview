# Wave 3 Live HMR Signal Authority — Engineering Design

## Status

Canonical implementation specification for the first framework-specific live HMR telemetry slice.

Date: 2026-09-19

Repository: `Nolane-x/Nolane-Localview`

Base: `main@dd062f3949954669a1b74395f34517f1bf9cf78d`

Branch: `feat/wave3-live-hmr-signals`

## Goal

Close the gap between LocalView's already-live HMR-aware settle policy and real page-side HMR signal production.

This slice must make a real LocalView-managed development WebView emit bounded `ObserverEventKind::Hmr` telemetry when a supported dev-server HMR transport reports a compile/update transition, without retaining HMR payload bodies, module/source paths, WebSocket tokens, arbitrary socket traffic, or application messages.

## Existing authority

Already landed:

- `ObserverEventKind::Hmr` in the live bridge;
- capture-settle extraction of the latest HMR event timestamp;
- a 300 ms HMR quiet gate when an HMR event exists;
- authenticated observer drain and exact-session ownership;
- bounded instrumentation ring-buffer retention;
- secret/query redaction for route metadata.

The missing link is page-side production plus desktop normalization of real HMR events.

## Selected transport

Use passive WebSocket observation inside the already-injected managed-page instrumentation.

The instrumentation replaces `window.WebSocket` with a transparent `Proxy` over the native constructor. Every constructed socket remains a native WebSocket instance; LocalView only attaches a passive native `message` listener when the constructor arguments contain a strong HMR transport marker.

No socket is opened by LocalView.

Observation is loopback-only. Before any framework/protocol classification, the socket URL must resolve to `localhost`, IPv4 `127/8`, or IPv6 loopback `::1`. A remote socket is ignored even if it advertises the `vite-hmr` subprotocol or a familiar HMR path.

No socket message is modified, blocked, delayed, replied to, or retained.

## Strong framework classification

### Vite

Classify as Vite only when the requested WebSocket protocol contains `vite-hmr`.

The current Vite client uses that protocol for its HMR connection. `vite-ping` is not an update channel and must not produce HMR settle events.

Recognized incoming Vite payload types:

- `update` -> phase `update`;
- `full-reload` -> phase `full_reload`;
- `prune` -> phase `prune`;
- `error` -> phase `error`.

For `update`, retain only a bounded numeric `update_count`; never retain update paths.

### Next.js

Classify only strong Next development HMR paths:

- `/_next/hmr`;
- `/_next/webpack-hmr`.

Recognize a bounded allowlist of lifecycle action/type names such as building/built/sync/reload/server-component-change/error. Unknown messages are ignored.

### webpack-dev-server

Classify only strong webpack-specific paths such as `/sockjs-node` or paths containing `webpack-hmr`.

Do not classify a generic `/ws` path as webpack because application sockets commonly use that path.

Recognize only bounded compiler lifecycle message types such as invalid/hash/ok/still-ok/warnings/errors/static-changed.

## Privacy contract

The HMR observer may inspect an incoming text message transiently only to classify a fixed event type.

It must never place any of the following in the LocalView ring buffer or observer payload:

- raw `event.data`;
- module paths;
- accepted paths;
- file paths;
- source code snippets;
- error stacks/messages from the HMR payload;
- WebSocket URL query strings;
- WebSocket tokens;
- arbitrary custom-event payloads.

The retained event schema is bounded to:

```text
type = "hmr"
framework = "vite" | "next" | "webpack"
phase = bounded fixed allowlist
updateCount = optional bounded integer
```

The ordinary LocalView route field remains query-redacted by the existing `safeUrl` policy.

## Resource bounds

- Only string WebSocket messages are considered.
- Ignore any text message larger than 256 KiB.
- JSON parse is best-effort and exception-safe.
- Vite `updates.length` is clamped to 256.
- Unknown framework messages are ignored.
- Non-loopback sockets are ignored before payload classification.
- No new history is introduced beyond the existing instrumentation ring buffer.

## Desktop bridge

The native bridge event-kind map must map raw instrumentation `hmr` to serialized observer kind `hmr`.

Without this mapping the page event would be silently dropped before reaching the daemon.

## Settle semantics

The existing daemon settle evaluator remains the sole HMR quiet-policy authority.

A page-side HMR event contributes its daemon-received/captured observer timestamp exactly like the existing synthetic HMR tests. This slice does not create a second timer or a page-owned settle verdict.

## Failure semantics

Instrumentation failure is observational only:

- WebSocket constructor behavior must remain available even if LocalView classification throws.
- HMR message parse failure is ignored.
- Unknown or ambiguous sockets are ignored.
- LocalView must never break application WebSocket behavior to gain HMR telemetry.

## Verification gates

### Gate 1 — instrumentation contract

Prove generated bootstrap contains:

- configurable HMR telemetry enabled by default;
- transparent native WebSocket proxy;
- Vite `vite-hmr` strong classification;
- strong Next/webpack path classification;
- bounded 256 KiB text-message limit;
- bounded `update_count`;
- `push('hmr', ...)`;
- no retained raw message body.

### Gate 2 — desktop normalization

Prove the native bridge maps raw `hmr` to `ObserverEventKind::Hmr` serialization.

### Gate 3 — existing settle authority

Preserve the control/capture HMR quiet tests. No alternate settle policy is added.

### Gate 4 — real browser proof

Use deterministic Chromium + a local WebSocket server to prove:

1. a `vite-hmr` socket carrying an `update` payload emits one bounded HMR event;
2. update paths and token query do not appear in drained event JSON;
3. a generic application WebSocket does not emit HMR telemetry;
4. non-loopback sockets are outside HMR observation authority;
5. malformed/oversized messages do not break the socket or emit retained payload;
6. the desktop-normalized event can be represented as observer kind `hmr`.

## Explicit non-claims

This slice does not claim:

- all custom Vite HMR transports;
- arbitrary custom `server.ws.path` inference when no strong protocol/path marker remains;
- generic application WebSocket inspection;
- WebSocket fault injection;
- response/body capture;
- framework component ownership;
- source-map correlation;
- performance budget packets.

## Completion definition

The slice is complete when a real managed-browser HMR update on a strongly classified supported transport creates privacy-safe bounded `ObserverEventKind::Hmr` telemetry that feeds the already-existing 300 ms HMR settle authority, while unrelated WebSockets remain unobserved and raw HMR payload content is never retained.
