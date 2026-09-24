#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
MATRIX = ROOT / "docs" / "PRODUCTION_CLOSURE_MATRIX.md"

CLOSED_STATUSES = {
    "Closed",
    "Closed for bounded V1 preflight",
    "Closed for initial release",
}
REQUIRED = {
    "Wave 9 bounded post-Apply verification": "closed",
    "Whole-impact Autonomous Verified": "post-v1",
    "V1 workspace surface": "closed",
    "Native child-WebView default promotion": "post-v1",
    "W10 mixed-DPI physical proof": "external",
    "Clean-machine install/launch": "closed",
    "Upgrade/rollback": "closed",
    "Update check/channel": "closed",
    "Signed updater install/apply": "external",
    "Provenance / SBOM": "closed",
    "Windows code signing": "external",
    "macOS Developer ID + notarization": "external",
    "Documentation truth": "closed",
}

REQUIRED_REPO_EVIDENCE = {
    ".github/workflows/release-install-smoke.yml": [
        "Clean-machine install and first launch",
        "wait_for_first_launch_health.py",
    ],
    "tools/release/wait_for_first_launch_health.py": [
        "status",
        "ready",
    ],
    ".github/workflows/release-upgrade-policy.yml": [
        "Release upgrade and rollback policy",
        "verify_release_policy.py",
    ],
    "tools/release/verify_release_policy.py": [
        "initial_supported_release",
        "installer_upgrade_required",
        "installer_rollback_required",
    ],
    "release-policy.json": [
        '"schema": "localview-release-policy-v1"',
        '"state_rollback_compatibility_required": true',
    ],
    ".github/workflows/release-candidate.yml": [
        "artifact-manifest.json",
        "sbom.spdx.json",
        "provenance.json",
        "verify_release_evidence.py",
    ],
    "tools/release/verify_release_evidence.py": [
        'SPDX-2.3',
        "artifact-manifest digest mismatch",
        "provenance SBOM digest mismatch",
    ],
    "apps/desktop/src-tauri/src/update_channel.rs": [
        'option_env!("LOCALVIEW_UPDATE_MANIFEST_URL")',
        "redirect(Policy::none())",
        "install_authorized: false",
        "same_origin(manifest_url, &artifact_url)",
    ],
    "apps/desktop/src-tauri/tests/update_channel_contract.rs": [
        "check_only_and_fail_closed_without_signature_authority",
        "install_authorized: false",
    ],
    "crates/verification/src/autonomous.rs": [
        "BoundedVerificationScope",
        "CurrentTargetCurrentRoute",
        "AUTONOMOUS_VERIFICATION_RECEIPT_SCHEMA_VERSION",
    ],
    "apps/desktop/src-tauri/src/trusted_verify.rs": [
        "AUTONOMOUS_VERIFICATION_RECEIPT_SCHEMA_VERSION",
        "AutonomousVerificationVerdict::Verified",
    ],
    "apps/desktop/src-tauri/tests/wave9_autonomous_verification_contract.rs": [
        "bounded_verification",
        "must never authorize the global Wave 9 handoff",
    ],
    "README.md": [
        "bounded V1 software-production closure",
        "Whole-impact Autonomous Verified remains fail-closed",
        "Public signed distribution is still externally blocked",
    ],
    "docs/SPEC_COVERAGE.md": [
        "scope-explicit bounded `current_target_current_route` receipt",
        "bounded verification never authorizes the whole-impact handoff",
    ],
    "docs/SECURITY.md": [
        "payload-bearing actions bind private process-local payloads to durable HMAC commitments",
        "journal-minted one-shot executor permit",
        "two-phase `plan -> confirm -> status` surface",
    ],
    "docs/ROADMAP.md": [
        "bounded V1 target/current-route verification path is software-closed",
        "whole-impact Autonomous Verified with a completeness-certified dependency denominator",
    ],
}


def fail(message: str) -> None:
    raise SystemExit(f"production closure matrix invalid: {message}")

def classify(status: str) -> str:
    if status in CLOSED_STATUSES:
        return "closed"
    if status == "Externally blocked":
        return "external"
    if status == "Post-V1 breadth":
        return "post-v1"
    if status == "In progress":
        return "in-progress"
    return "unknown"

def parse_rows(text: str) -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    in_table = False
    for raw in text.splitlines():
        if raw.startswith("| Area | V1 production claim | Status |"):
            in_table = True
            continue
        if not in_table:
            continue
        if raw.startswith("| ---"):
            continue
        if not raw.startswith("| "):
            break
        cells = [cell.strip() for cell in raw.strip().strip("|").split("|")]
        if len(cells) != 4:
            fail(f"table row must contain exactly 4 cells: {raw}")
        area, claim, status, evidence = cells
        if not area or not claim or not status or not evidence:
            fail(f"table row contains an empty cell: {raw}")
        if area in rows:
            fail(f"duplicate production area: {area}")
        rows[area] = {"claim": claim, "status": status, "evidence": evidence}
    if not rows:
        fail("production table was not found")
    return rows

def verify_repository_evidence() -> None:
    for relative, markers in REQUIRED_REPO_EVIDENCE.items():
        path = ROOT / relative
        if not path.is_file():
            fail(f"required closure evidence file is missing: {relative}")
        content = path.read_text(encoding="utf-8")
        for marker in markers:
            if marker not in content:
                fail(f"required closure evidence marker missing from {relative}: {marker}")


def verify_no_stale_public_claims() -> None:
    forbidden = {
        "README.md": [
            "canonical consequential DOM interaction authority",
            "full Wave 9 contract/mutation/actual-impact/revalidation orchestration",
            "durable Fix→Verify recovery across desktop restart",
        ],
        "docs/IMPLEMENTATION_STATUS.md": [
            "Updater channel/signature authority remains the active release-software frontier.",
        ],
        "docs/SPEC_COVERAGE.md": [
            "production contract-catalog execution, mutation challenges, complete denominator/revalidation authority and external side-effect containment are not proven",
        ],
        "docs/SECURITY.md": [
            "This managed-WebView proof is not yet a durable V4.3 journal commit",
            "type`/`key`/`scroll` plus CLI/MCP consequential entry points remain unavailable",
        ],
        "docs/ROADMAP.md": [
            "production closure reopened",
            "not yet production-orchestrated end-to-end",
            "Wave 9 is **Partial at the live-product level**",
        ],
    }
    for relative, phrases in forbidden.items():
        content = (ROOT / relative).read_text(encoding="utf-8")
        for phrase in phrases:
            if phrase in content:
                fail(f"stale public production claim remains in {relative}: {phrase}")


def main() -> None:
    text = MATRIX.read_text(encoding="utf-8")
    rows = parse_rows(text)
    verify_repository_evidence()
    verify_no_stale_public_claims()

    unknown = {
        area: row["status"]
        for area, row in rows.items()
        if classify(row["status"]) == "unknown"
    }
    if unknown:
        fail(f"unknown status vocabulary: {unknown}")

    in_progress = [area for area, row in rows.items() if classify(row["status"]) == "in-progress"]
    if in_progress:
        fail(f"bounded V1 still contains In progress rows: {in_progress}")

    for area, expected in REQUIRED.items():
        row = rows.get(area)
        if row is None:
            fail(f"required production area is missing: {area}")
        actual = classify(row["status"])
        if actual != expected:
            fail(f"{area!r} must classify as {expected}, found {row['status']!r}")

    whole = rows["Whole-impact Autonomous Verified"]
    if "Not advertised as a V1 supported behavior" not in whole["evidence"]:
        fail("whole-impact Autonomous Verified must remain explicitly outside the V1 claim")

    native = rows["Native child-WebView default promotion"]
    if "Feature-gated/opt-in only" not in native["evidence"]:
        fail("native child-WebView must remain explicitly opt-in/post-V1")

    signed_updater = rows["Signed updater install/apply"]
    if "production update signing" not in signed_updater["evidence"].lower():
        fail("signed updater external blocker must name production update signing authority")

    if "## Software-production completion rule" not in text:
        fail("software-production completion rule is missing")
    if "public production release" not in text.lower():
        fail("public-release distinction is missing")

    summary = {
        "schema": "localview-software-production-closure-v1",
        "rows": len(rows),
        "closed": sum(classify(row["status"]) == "closed" for row in rows.values()),
        "external": sum(classify(row["status"]) == "external" for row in rows.values()),
        "post_v1": sum(classify(row["status"]) == "post-v1" for row in rows.values()),
        "in_progress": 0,
        "result": "pass",
    }
    print(json.dumps(summary, sort_keys=True))

if __name__ == "__main__":
    main()
