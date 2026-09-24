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

def main() -> None:
    text = MATRIX.read_text(encoding="utf-8")
    rows = parse_rows(text)

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
