<div align="center">

# LocalView

### Runtime visual local-first para desarrolladores, agentes de programación y aplicaciones localhost.

**Observa la aplicación en ejecución. Entiende lo que realmente ocurrió. Actúa con autoridad limitada. Verifica el resultado con evidencia.**

[English](README.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Español](README.es.md)

![Version](https://img.shields.io/badge/version-0.2.0--rc.1-4f46e5)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-64748b)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

---

## ¿Qué es LocalView?

LocalView es un **runtime visual AI-native para localhost**. Está diseñado para desarrolladores y agentes de programación que necesitan comprender cómo se comporta realmente una aplicación en ejecución, no solo leer el código fuente y asumir su estado.

LocalView descubre aplicaciones locales, mantiene la identidad de proyecto/sesión, observa estructura semántica y renderizado real, recoge evidencia de console/network/performance/accessibility, correlaciona runtime con ownership de código y expone un modelo compacto mediante Desktop, CLI y MCP.

No pretende ser otro navegador generalista.

> Su objetivo es convertir una aplicación localhost en ejecución en un entorno observable, verificable y seguro para humanos y agentes.

Preguntas que LocalView intenta responder:

- ¿Qué se está renderizando realmente ahora?
- ¿Qué stable ref y qué región de código corresponden a este elemento?
- ¿El cambio visual esperado ocurrió de verdad después de un fix?
- ¿Aparecieron nuevas regresiones de console, network, layout o accessibility?
- ¿Puede un agente hacer click/type/scroll sin recibir autoridad ilimitada sobre la página?
- ¿El resultado se verifica con una nueva observación después de la acción?
- ¿El sistema falla de forma cerrada cuando la evidencia es stale, incompleta o ambigua?

---

## Por qué LocalView es diferente

| Principio | Enfoque |
| --- | --- |
| **Runtime truth first** | Observa la aplicación viva en vez de asumir que el código representa el UI actual. |
| **Local-first** | El control plane se limita a loopback y al flujo de desarrollo localhost. |
| **Evidence-first** | Semantic snapshots, visual evidence nativa, eventos, contracts, receipts, hashes y provenance son datos de primera clase. |
| **Autoridad limitada** | Las acciones pasan por plan → confirm → exact dispatch → fresh verification. |
| **Fail closed** | Stale lineage, unknown contracts, impactos inesperados y evidencia incompleta no se convierten en éxito. |
| **Prueba nativa multiplataforma** | Windows/WebView2, macOS/WKWebView y Linux/WebKitGTK tienen rutas de rendered-pixel evidence. |
| **Resource-aware** | Chromium completo es Tier 3, no el motor por defecto. |
| **Human + Agent** | Desktop, CLI y MCP comparten el mismo modelo runtime/evidence. |

---

## El ciclo de LocalView

```text
localhost application
        │
        ▼
 discovery + session
        │
        ▼
semantic / visual / runtime observation
        │
        ▼
 evidence + source correlation
        │
   human / agent
        │
        ▼
 bounded action plan
        │
 explicit confirmation
        │
        ▼
 exact dispatch
        │
        ▼
 fresh post-action observation
        │
        ▼
 contracts + mutation challenges + receipts
        │
        ▼
 verified / rejected / inconclusive
```

LocalView separa **“ejecuté una acción”** de **“demostré el estado resultante”**.

---

## Alcance probado de V1

La frontera exacta de producción está en [docs/PRODUCTION_CLOSURE_MATRIX.md](docs/PRODUCTION_CLOSURE_MATRIX.md).

### Runtime y discovery

- localhost discovery y bounded HTTP probing;
- clasificación frontend/API y framework/HMR evidence;
- durable project/session identity;
- reconnect/disconnect grace/cleanup;
- normalized observation bus;
- resource-governed engine escalation.

### Semantic, layout y source

- stable semantic refs;
- semantic + geometry snapshots/diffs;
- overflow/overlap/alignment primitives;
- responsive/adaptive viewport analysis;
- source-map y project-owned source correlation;
- foundations de ownership para React/Vue/Svelte;
- CSS declaration/source/cascade authority;
- point-select y affected-region verification.

### Visual evidence

- captura nativa renderizada en Windows/macOS/Linux;
- pixel diff y changed-region localization;
- private-region redaction antes de persistencia;
- progressive visual targeting;
- guarded full-page stitching;
- límites de geometry/memory/deadline;
- content-addressed artifacts/evidence.

### Diagnóstico runtime

- console grouping/dedup;
- análisis de red failed/slow/duplicate/large/CORS;
- exact-session localhost network-fault authority;
- performance-lite, long-task, layout-instability y HMR health;
- accessibility checks;
- correlación action → request → UI-response.

---

## Acciones consequential controladas

Managed WebView soporta:

- `click`
- `focus`
- `type_text`
- `key`
- `scroll`

Cadena de producción:

```text
fresh semantic cut
→ plan
→ one-shot confirmation
→ payload/lineage verification
→ exact executor permit
→ durable dispatch journal
→ fresh post-dispatch observation
→ postcondition receipt
```

El plaintext sensible permanece process-local y la capa durable almacena commitments HMAC. Un reinicio no restaura confirmation, payload ni dispatch authority.

CLI/MCP solo exponen **plan → confirm → status**.

---

## Trusted Fix y Wave 9

Un fix revisado por el usuario pasa por:

1. exact candidate/revision identity;
2. bounded affected-state prediction;
3. disposable source-only preflight;
4. prueba de repository-side-effect containment;
5. Apply;
6. fresh semantic/visual observation;
7. production hard contracts;
8. safe synthetic mutation challenges;
9. actual-vs-predicted impact;
10. bounded verification receipt.

Un receipt puede ser **Verified** para el target exacto de la canonical route actual si todas las obligaciones bounded están limpias.

Eso no se promociona a una prueba de toda la aplicación.

El verdict whole-impact autónomo sigue fail-closed hasta disponer de un completeness-certified dependency denominator y un universo completo de revalidación.

---

## Desktop / CLI / MCP

### Desktop

- Tauri 2.11
- React 19
- Vite 8
- Windows: WebView2
- macOS: WKWebView
- Linux: WebKitGTK / WRY

### CLI

```bash
cargo run -p localview -- sessions
cargo run -p localview -- snapshot <session-id>
cargo run -p localview -- performance-lite <session-id>
```

### MCP

`integrations/mcp` proporciona un bridge stdio MCP-compatible JSON-RPC con el mismo modelo bounded de evidence/authority que el Desktop.

---

## Desarrollo

Requisitos:

- Rust stable compatible con `rust-version = 1.85`
- Node.js 24+
- dependencias de plataforma de Tauri 2

```bash
cargo test --workspace --exclude localview-desktop
cargo run -p localview-daemon
```

En otra terminal:

```bash
cargo run -p localview -- sessions
```

Desktop:

```bash
cd apps/desktop
npm ci
npm run tauri dev
```

Release candidate local:

```bash
cd apps/desktop
npm ci
npm run tauri build
```

Consulta [docs/PRODUCTION_RELEASE.md](docs/PRODUCTION_RELEASE.md) antes de distribuir.

---

## Modelo de seguridad

LocalView prioriza **bounded authority** frente a automatización de navegador ilimitada.

Principales fronteras:

- authenticated loopback control;
- sin generic shell permission para dashboard WebViews;
- preview/workspace capability separation;
- provider/target incarnation fencing;
- one-shot confirmation;
- process-local sensitive payload authority;
- durable dispatch/postcondition receipts;
- stale/restart/replay protection;
- private visual redaction;
- bounded artifact retention;
- unknown/incomplete verification debe fallar cerrado;
- exact-head release evidence.

Más detalles: [docs/SECURITY.md](docs/SECURITY.md).

---

## Estado de producción y release

**Bounded V1 software-production está completo en `main`.**

El cierre incluye cross-platform CI, release-candidate bundles, clean-machine install/first launch, rendered-pixel validation, rollback-state policy, SBOM/provenance, security/contract gates y adversarial production-truth checks.

Próximo hito:

**v0.2.0-rc.1 — unsigned pre-release candidate**

El release público final firmado todavía necesita:

- credenciales de Windows code signing;
- macOS Developer ID + notarization;
- production updater-signing authority;
- W10 physical mixed-DPI evidence si se anuncia soporte de esa topología.

El updater V1 solo comprueba un canal HTTPS fijado; no descarga/instala automáticamente sin signature-verification authority.

---

## Lo que LocalView no afirma

V1 no afirma:

- whole-app Autonomous Verified con dependency universe incompleto;
- arbitrary internet interception / TLS MITM;
- browser/OS automation ilimitada;
- native child-WebView como workspace por defecto sin evidencia completa de composition/focus/z-order/minimize-restore/DPI;
- signed public installers sin credenciales;
- W10 mixed-DPI closure sin evidencia real multi-monitor.

Unknown permanece unknown.

---

## Documentación

- [Architecture](docs/ARCHITECTURE.md)
- [Implementation status](docs/IMPLEMENTATION_STATUS.md)
- [Production closure matrix](docs/PRODUCTION_CLOSURE_MATRIX.md)
- [Production release](docs/PRODUCTION_RELEASE.md)
- [Security](docs/SECURITY.md)
- [Spec coverage](docs/SPEC_COVERAGE.md)
- [Roadmap](docs/ROADMAP.md)

Los planes históricos, evidence y diseños superseded se archivan por separado para que el repositorio de producto conserve documentación operativa actual y contratos normativos activos.

---

## Filosofía

> **Una herramienta de AI coding debe distinguir entre “ejecuté una acción” y “demostré que la aplicación ahora satisface la condición esperada”.**

Esta idea impulsa evidence, fresh observation, exact lineage, bounded authority y explicit unknown states en LocalView.

---

## Licencia

MIT OR Apache-2.0.

<div align="center">

**LocalView — observa la aplicación en ejecución, no solo el árbol de código.**

</div>
