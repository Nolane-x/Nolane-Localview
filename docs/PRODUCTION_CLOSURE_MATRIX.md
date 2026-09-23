# LocalView Production Closure Matrix

This file is the release-truth boundary for LocalView. A capability may remain broader in `SPEC_COVERAGE.md` without blocking V1 production when the V1 claim is explicitly bounded here.

Status vocabulary:

- **Closed** — implementation and required software evidence are on `main`.
- **In progress** — active production blocker with software work remaining.
- **Externally blocked** — code can continue, but final evidence requires credentials or hardware not currently available.
- **Post-V1 breadth** — useful expansion that is not part of the bounded V1 production claim.

| Area | V1 production claim | Status | Required evidence / remaining work |
| --- | --- | --- | --- |
| Desktop daemon + sidecar packaging | Self-contained LocalView desktop bundles daemon and starts/owns it safely | Closed | R1; Windows/macOS/Linux release-candidate bundle gates |
| Managed WebView authority | Exact primary managed surface/provider/target authority; stale incarnation fails closed | Closed | R2–R5 |
| Consequential click/focus | Fresh plan → explicit one-shot confirmation → exact dispatch → fresh durable postcondition proof | Closed | R5–R7 |
| Consequential type/key/scroll | Same chain plus private process-local payload + durable HMAC commitment and pre-executor verification | Closed | R8 |
| Agent-facing consequential interface | CLI/MCP expose only `plan -> confirm -> status`; legacy one-step mutation shortcuts stay closed | Closed | R9 exact-head CI |
| Trusted Fix preflight | Exact revision/candidate identity, disposable source-only shadow, cleanup proof, bounded affected-state prediction | Closed for bounded V1 preflight | No autonomous Verified claim from preflight alone |
| Wave 9 post-Apply orchestration | Preflight authority survives restart and fresh Verify evidence creates an autonomous observation receipt | In progress | R10 exact-head CI; live contract catalog + safe mutation challenge binding; tighter revalidation accounting |
| Autonomous Verified verdict | Only allowed with complete hard-contract evidence, mutation challenge closure, denominator/revalidation authority, resource/cleanup proof and proven external side-effect containment | In progress | Must remain Inconclusive until all proof obligations exist |
| Trusted Verify restart recovery | Pending baseline/context survives restart without restoring write authority | Closed | R2 durable recovery + R10 Wave 9 context extension |
| Native viewport capture | Real WebView2/WKWebView/WebKitGTK rendered pixels through production capture path | Closed | Hosted GUI gates on all 3 platforms |
| Native workspace default promotion | Native child WebView may become default only after composition/focus/crash/DPI policy closure | In progress | Software lifecycle/focus/crash closure; mixed-DPI evidence if claim includes that topology |
| W10 mixed-DPI physical proof | Exact same-HWND cross-monitor geometry/DPI proof on real Windows multi-monitor hardware | Externally blocked | PR #116; self-hosted Windows with at least 2 displays at distinct effective DPI |
| Headless/CI/reporting/attestation | Bounded headless execution and deterministic report/attestation path | Closed | Wave 8 |
| Clean-machine install/launch | Unsigned candidate can be installed/launched on clean supported systems before signing credentials exist | In progress | Fresh VM/machine install and first-launch smoke for supported OS targets |
| Upgrade/rollback | Upgrade from previous supported build and rollback preserves/recovers expected state | In progress | Cross-version fixture and rollback evidence |
| Updater software/channel | Trusted manifest/channel and fail-closed updater behavior exist independent of production signing secret | In progress | Implement/test channel logic; production key remains credential-dependent |
| Provenance / SBOM | Release artifacts carry reproducible provenance/SBOM and digest linkage | In progress | Generate, validate and attach to release-candidate evidence |
| Windows code signing | Public Windows artifacts are signed and signature verified | Externally blocked | Signing identity/certificate not currently available |
| macOS Developer ID + notarization | Public macOS artifacts are Developer ID signed and notarized | Externally blocked | Developer ID/notary credentials not currently available |
| Documentation truth | README/security/status/coverage match current product behavior; historical research lives in Localview-document | In progress | Continue reference-safe archival and final truth audit |
| Broader profiling/framework/network/timeline breadth | Additional CPU/heap profiling, deeper every-framework ownership, broader interception, richer timeline UI | Post-V1 breadth | Must not be advertised as supported V1 behavior until separately proven |

## Software-production completion rule

LocalView may be called **software-production complete** when every row marked **In progress** that belongs to the bounded V1 claim is Closed, with only explicitly recorded **Externally blocked** signing/hardware evidence remaining.

It may be called a **public production release** only after the relevant externally blocked signing/notarization requirements are also satisfied for the distributed platform artifacts.

No `Partial` item in a broader research roadmap automatically blocks V1. Conversely, anything advertised as a V1 supported behavior must either be Closed here or have its claim narrowed before release.
