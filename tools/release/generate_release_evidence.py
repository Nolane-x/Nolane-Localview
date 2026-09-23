#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
from datetime import datetime, timezone

SCHEMA_VERSION = 1


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def relative_files(root: pathlib.Path) -> list[pathlib.Path]:
    return sorted(
        (path for path in root.rglob("*") if path.is_file()),
        key=lambda path: path.relative_to(root).as_posix(),
    )


def cargo_packages(lock_path: pathlib.Path) -> list[dict]:
    # Cargo.lock package identity fields are simple quoted scalars. Parse only
    # those fields so the release tool works on Python 3.10 without adding a
    # runtime dependency solely for TOML parsing.
    packages = []
    current = None

    def flush() -> None:
        nonlocal current
        if current and current.get("name") and current.get("version"):
            item = {
                "name": current["name"],
                "version": current["version"],
                "ecosystem": "cargo",
            }
            if current.get("source"):
                item["source"] = current["source"]
            if current.get("checksum"):
                item["checksum"] = current["checksum"]
            packages.append(item)
        current = None

    for raw_line in lock_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line == "[[package]]":
            flush()
            current = {}
            continue
        if current is None or "=" not in line:
            continue
        key, raw_value = (part.strip() for part in line.split("=", 1))
        if key not in {"name", "version", "source", "checksum"}:
            continue
        try:
            value = json.loads(raw_value)
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid Cargo.lock scalar for {key}: {error}") from error
        if not isinstance(value, str):
            raise SystemExit(f"Cargo.lock {key} must be a string")
        current[key] = value
    flush()
    return sorted(packages, key=lambda item: (item["name"], item["version"], item.get("source", "")))


def npm_packages(lock_path: pathlib.Path) -> list[dict]:
    data = json.loads(lock_path.read_text(encoding="utf-8"))
    packages = []
    for location, package in data.get("packages", {}).items():
        if not location or not package.get("name") or not package.get("version"):
            continue
        item = {
            "name": package["name"],
            "version": package["version"],
            "ecosystem": "npm",
            "location": location,
        }
        if package.get("resolved"):
            item["resolved"] = package["resolved"]
        if package.get("integrity"):
            item["integrity"] = package["integrity"]
        if package.get("license"):
            item["license"] = package["license"]
        packages.append(item)
    return sorted(packages, key=lambda item: (item["name"], item["version"], item["location"]))


def spdx_package(item: dict, index: int) -> dict:
    package = {
        "SPDXID": f"SPDXRef-Package-{index}",
        "name": item["name"],
        "versionInfo": item["version"],
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": item.get("license", "NOASSERTION"),
        "copyrightText": "NOASSERTION",
        "externalRefs": [
            {
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": (
                    f"pkg:cargo/{item['name']}@{item['version']}"
                    if item["ecosystem"] == "cargo"
                    else f"pkg:npm/{item['name']}@{item['version']}"
                ),
            }
        ],
        "comment": json.dumps(
            {key: value for key, value in item.items() if key not in {"name", "version", "license"}},
            sort_keys=True,
            separators=(",", ":"),
        ),
    }
    return package


def build_evidence(
    repo_root: pathlib.Path,
    bundle_dir: pathlib.Path,
    output_dir: pathlib.Path,
    candidate_sha: str,
    platform: str,
    version: str,
) -> None:
    cargo_lock = repo_root / "Cargo.lock"
    npm_lock = repo_root / "apps" / "desktop" / "package-lock.json"
    if not cargo_lock.is_file() or not npm_lock.is_file():
        raise SystemExit("release evidence requires committed Cargo.lock and apps/desktop/package-lock.json")
    if not bundle_dir.is_dir():
        raise SystemExit(f"bundle directory does not exist: {bundle_dir}")

    artifacts = [
        {
            "path": path.relative_to(bundle_dir).as_posix(),
            "size_bytes": path.stat().st_size,
            "sha256": sha256_file(path),
        }
        for path in relative_files(bundle_dir)
    ]
    if not artifacts:
        raise SystemExit("bundle directory contains no files")

    cargo = cargo_packages(cargo_lock)
    npm = npm_packages(npm_lock)
    dependencies = cargo + npm
    generated = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")

    output_dir.mkdir(parents=True, exist_ok=True)
    artifact_manifest = {
        "schema_version": SCHEMA_VERSION,
        "candidate_sha": candidate_sha,
        "platform": platform,
        "version": version,
        "signed": False,
        "public_release": False,
        "artifacts": artifacts,
    }
    provenance = {
        "schema_version": SCHEMA_VERSION,
        "candidate_sha": candidate_sha,
        "platform": platform,
        "version": version,
        "generated_at": generated,
        "builder": "github-actions",
        "release_class": "unsigned-release-candidate",
        "lockfiles": {
            "Cargo.lock": sha256_file(cargo_lock),
            "apps/desktop/package-lock.json": sha256_file(npm_lock),
        },
        "artifact_manifest_sha256": "",
        "sbom_sha256": "",
        "claims": {
            "dependency_resolution_locked": True,
            "artifact_digests_bound": True,
            "code_signing_verified": False,
            "macos_notarization_verified": False,
        },
    }

    manifest_path = output_dir / "artifact-manifest.json"
    manifest_path.write_text(json.dumps(artifact_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    namespace_digest = hashlib.sha256(
        f"{candidate_sha}\0{platform}\0{version}".encode("utf-8")
    ).hexdigest()
    spdx_packages = [spdx_package(item, index + 1) for index, item in enumerate(dependencies)]
    sbom = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"LocalView-{version}-{platform}",
        "documentNamespace": f"https://github.com/Nolane-x/Nolane-Localview/sbom/{namespace_digest}",
        "creationInfo": {
            "created": generated,
            "creators": ["Tool: LocalView release evidence v1"],
        },
        "packages": spdx_packages,
        "relationships": [
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": package["SPDXID"],
            }
            for package in spdx_packages
        ],
        "comment": (
            "Bounded dependency SBOM generated from committed Cargo.lock and "
            "apps/desktop/package-lock.json. Rust license metadata is not present "
            "in Cargo.lock and remains NOASSERTION."
        ),
    }
    sbom_path = output_dir / "sbom.spdx.json"
    sbom_path.write_text(json.dumps(sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    provenance["artifact_manifest_sha256"] = sha256_file(manifest_path)
    provenance["sbom_sha256"] = sha256_file(sbom_path)
    provenance_path = output_dir / "provenance.json"
    provenance_path.write_text(json.dumps(provenance, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--bundle-dir", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()

    candidate_sha = args.candidate_sha.strip().lower()
    if len(candidate_sha) != 40 or any(ch not in "0123456789abcdef" for ch in candidate_sha):
        raise SystemExit("candidate SHA must be an exact 40-character lowercase Git SHA")
    if not args.platform.strip() or not args.version.strip():
        raise SystemExit("platform and version must be non-empty")

    build_evidence(
        pathlib.Path(args.repo_root).resolve(),
        pathlib.Path(args.bundle_dir).resolve(),
        pathlib.Path(args.output_dir).resolve(),
        candidate_sha,
        args.platform.strip(),
        args.version.strip(),
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
