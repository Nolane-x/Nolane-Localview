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
| Wave 9 bounded post-Apply verification | Preflight survives restart; fresh Verify evidence evaluates live hard contracts + safe mutation challenges and can verify only the exact selected target on the current canonical route | Closed | R10 durable orchestration + R12 containment + R13 production contract/mutation catalogs + R17 scope-explicit bounded receipt; exact-head campaign required before merge |
| Whole-impact Autonomous Verified | A whole-app/whole-impact Verified verdict requires a completeness-certified dependency denominator and complete revalidation universe | Post-V1 breadth | Not advertised as a V1 supported behavior. The existing whole-impact verdict remains fail-closed Inconclusive until those stronger proof obligations exist |
| Trusted Verify restart recovery | Pending baseline/context survives restart without restoring write authority | Closed | R2 durable recovery + R10 Wave 9 context extension |
| Native viewport capture | Real WebView2/WKWebView/WebKitGTK rendered pixels through production capture path | Closed | Hosted GUI gates on all 3 platforms |
| V1 workspace surface | The production default remains the proven dashboard/iframe workspace plus standalone managed preview surfaces; native child-WebView is not required for the V1 claim | Closed | Current production default + hosted rendered/native capture/preview evidence |
| Native child-WebView default promotion | Optional native child-WebView may become the default only after composition/focus/z-order/minimize-restore/DPI proof | Post-V1 breadth | Feature-gated/opt-in only; do not advertise as V1 default until separately proven |
| W10 mixed-DPI physical proof | Exact same-HWND cross-monitor geometry/DPI proof on real Windows multi-monitor hardware | Externally blocked | PR #116; required only for claims that include that physical mixed-DPI topology |
| Headless/CI/reporting/attestation | Bounded headless execution and deterministic report/attestation path | Closed | Wave 8 |
| Clean-machine install/launch | Unsigned candidate can be installed/launched on clean supported systems before signing credentials exist | Closed | R14 fresh hosted-runner install + first-launch health proof for Linux, macOS and Windows; signed public artifacts must repeat smoke after credentials exist |
| Upgrade/rollback | Initial supported release has no fictional predecessor; rollback-readable state is required now, and every later release must prove installer upgrade + rollback from the declared previous supported version | Closed for initial release | R10 rollback-readable Trusted Verify state + R15 executable release policy; from release 2 onward this row reopens unless previous-version upgrade/rollback evidence is present |
| Update check/channel | User-triggered check uses a compile-time pinned HTTPS manifest URL, rejects redirects/cross-origin artifacts/oversized or malformed manifests, and never grants install authority | Closed | Exact-head UI/security/runtime tests; build remains explicitly unconfigured when no channel URL is compiled |
| Signed updater install/apply | Automatic download/install is permitted only after production update-signature verification authority exists | Externally blocked | Production update signing key/public verification authority are not currently available; V1 remains manual-update-only |
| Provenance / SBOM | Release artifacts carry reproducible provenance/SBOM and digest linkage | Closed | R11 exact-head artifact manifest, SPDX SBOM, provenance linkage and independent tamper verifier |
| Windows code signing | Public Windows artifacts are signed and signature verified | Externally blocked | Signing identity/certificate not currently available |
| macOS Developer ID + notarization | Public macOS artifacts are Developer ID signed and notarized | Externally blocked | Developer ID/notary credentials not currently available |
| Documentation truth | Release/status matrix states bounded V1 behavior, unsupported/post-V1 breadth and external blockers without converting research breadth into production claims | Closed | Final exact-head docs/contracts must land with R16; historical research remains reference-safe in Localview-document |
| Broader profiling/framework/network/timeline breadth | Additional CPU/heap profiling, deeper every-framework ownership, broader interception, richer timeline UI | Post-V1 breadth | Must not be advertised as supported V1 behavior until separately proven |

## Software-production completion rule

LocalView may be called **software-production complete** when every row marked **In progress** that belongs to the bounded V1 claim is Closed, with only explicitly recorded **Externally blocked** signing/hardware evidence remaining.

It may be called a **public production release** only after the relevant externally blocked signing/notarization requirements are also satisfied for the distributed platform artifacts.

No `Partial` item in a broader research roadmap automatically blocks V1. Conversely, anything advertised as a V1 supported behavior must either be Closed here or have its claim narrowed before release.
