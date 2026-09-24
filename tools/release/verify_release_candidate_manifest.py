#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release-candidate.json"
EXPECTED_SCHEMA = "localview-release-candidate-v1"
TAG_RE = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-rc\.([1-9][0-9]*)$")


def fail(message: str) -> None:
    raise SystemExit(f"release candidate manifest invalid: {message}")


def strict_object(pairs):
    obj = {}
    for key, value in pairs:
        if key in obj:
            fail(f"duplicate JSON key {key!r}")
        obj[key] = value
    return obj


def load_json(path: pathlib.Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=strict_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = re.search(r"(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\Z)", text)
    if not section:
        fail("Cargo.toml is missing [workspace.package]")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', section.group(1))
    if not match:
        fail("workspace.package.version is missing")
    return match.group(1)


def main() -> int:
    data = load_json(MANIFEST)
    expected = {
        "schema",
        "tag",
        "version",
        "title",
        "prerelease",
        "signed",
        "public_production_release",
        "notes",
    }
    if set(data) != expected:
        fail(f"top-level keys differ from contract: {sorted(set(data) ^ expected)}")
    if data["schema"] != EXPECTED_SCHEMA:
        fail("unsupported schema")
    if type(data["prerelease"]) is not bool or data["prerelease"] is not True:
        fail("release candidate must be marked prerelease=true")
    if type(data["signed"]) is not bool or data["signed"] is not False:
        fail("current release candidate must remain explicitly unsigned")
    if type(data["public_production_release"]) is not bool or data["public_production_release"] is not False:
        fail("unsigned candidate must not claim public production release")

    tag = data["tag"]
    version = data["version"]
    match = TAG_RE.fullmatch(tag) if isinstance(tag, str) else None
    if not match:
        fail("tag must be strict vMAJOR.MINOR.PATCH-rc.N")
    tag_version = ".".join(match.group(i) for i in range(1, 4))
    if tag_version != version:
        fail("tag version and manifest version differ")

    if workspace_version() != version:
        fail("Cargo workspace version differs from release candidate version")
    tauri = load_json(ROOT / "apps/desktop/src-tauri/tauri.conf.json")
    if tauri.get("version") != version:
        fail("Tauri version differs from release candidate version")
    policy = load_json(ROOT / "release-policy.json")
    if policy.get("current_version") != version:
        fail("release policy version differs from release candidate version")
    if policy.get("initial_supported_release") is not True:
        fail("v0.2.0-rc.1 expects the initial-supported-release policy to remain true")

    notes = data["notes"]
    if not isinstance(notes, str) or notes != "docs/releases/v0.2.0-rc.1.md":
        fail("release notes path is not the approved v0.2.0-rc.1 document")
    notes_path = (ROOT / notes).resolve()
    if notes_path.parent != (ROOT / "docs/releases").resolve() or not notes_path.is_file():
        fail("release notes file is missing or outside docs/releases")
    notes_text = notes_path.read_text(encoding="utf-8")
    notes_lower = notes_text.lower()
    for marker in [
        "unsigned pre-release candidate",
        "windows code signing",
        "macos developer id",
        "production updater-signing authority",
        "bounded v1",
    ]:
        if marker not in notes_lower:
            fail(f"release notes missing required truth marker: {marker}")

    print(json.dumps({
        "schema": EXPECTED_SCHEMA,
        "tag": tag,
        "version": version,
        "prerelease": True,
        "signed": False,
        "result": "pass",
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
