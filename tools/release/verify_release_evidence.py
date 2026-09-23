#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def artifact_inventory(bundle_dir: pathlib.Path) -> list[dict]:
    return [
        {
            "path": path.relative_to(bundle_dir).as_posix(),
            "size_bytes": path.stat().st_size,
            "sha256": sha256_file(path),
        }
        for path in sorted(
            (candidate for candidate in bundle_dir.rglob("*") if candidate.is_file()),
            key=lambda candidate: candidate.relative_to(bundle_dir).as_posix(),
        )
    ]


def verify(
    repo_root: pathlib.Path,
    bundle_dir: pathlib.Path,
    evidence_dir: pathlib.Path,
    candidate_sha: str,
    platform: str,
    version: str,
) -> None:
    manifest_path = evidence_dir / "artifact-manifest.json"
    sbom_path = evidence_dir / "sbom.spdx.json"
    provenance_path = evidence_dir / "provenance.json"
    for path in [manifest_path, sbom_path, provenance_path]:
        if not path.is_file():
            raise SystemExit(f"missing release evidence file: {path}")

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))

    expected_identity = (candidate_sha, platform, version)
    if (
        manifest.get("candidate_sha"),
        manifest.get("platform"),
        manifest.get("version"),
    ) != expected_identity:
        raise SystemExit("artifact manifest identity mismatch")
    if manifest.get("schema_version") != 1:
        raise SystemExit("unsupported artifact manifest schema")
    if manifest.get("signed") is not False or manifest.get("public_release") is not False:
        raise SystemExit("unsigned release candidate must not claim signing/publication")

    actual_artifacts = artifact_inventory(bundle_dir)
    if manifest.get("artifacts") != actual_artifacts:
        raise SystemExit("artifact manifest does not exactly match bundle bytes")

    if (
        provenance.get("candidate_sha"),
        provenance.get("platform"),
        provenance.get("version"),
    ) != expected_identity:
        raise SystemExit("provenance identity mismatch")
    if provenance.get("release_class") != "unsigned-release-candidate":
        raise SystemExit("release class is not the bounded unsigned candidate class")
    claims = provenance.get("claims", {})
    if claims.get("dependency_resolution_locked") is not True:
        raise SystemExit("provenance does not prove locked dependency resolution")
    if claims.get("artifact_digests_bound") is not True:
        raise SystemExit("provenance does not bind artifact digests")
    if claims.get("code_signing_verified") is not False:
        raise SystemExit("unsigned candidate cannot claim Windows code-signing verification")
    if claims.get("macos_notarization_verified") is not False:
        raise SystemExit("unsigned candidate cannot claim macOS notarization")

    cargo_lock = repo_root / "Cargo.lock"
    npm_lock = repo_root / "apps" / "desktop" / "package-lock.json"
    if provenance.get("lockfiles") != {
        "Cargo.lock": sha256_file(cargo_lock),
        "apps/desktop/package-lock.json": sha256_file(npm_lock),
    }:
        raise SystemExit("provenance lockfile digests do not match committed bytes")
    if provenance.get("artifact_manifest_sha256") != sha256_file(manifest_path):
        raise SystemExit("provenance artifact-manifest digest mismatch")
    if provenance.get("sbom_sha256") != sha256_file(sbom_path):
        raise SystemExit("provenance SBOM digest mismatch")

    if sbom.get("spdxVersion") != "SPDX-2.3":
        raise SystemExit("SBOM is not SPDX 2.3")
    if sbom.get("name") != f"LocalView-{version}-{platform}":
        raise SystemExit("SBOM identity mismatch")
    packages = sbom.get("packages")
    if not isinstance(packages, list) or not packages:
        raise SystemExit("SBOM contains no dependency packages")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--bundle-dir", required=True)
    parser.add_argument("--evidence-dir", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    verify(
        pathlib.Path(args.repo_root).resolve(),
        pathlib.Path(args.bundle_dir).resolve(),
        pathlib.Path(args.evidence_dir).resolve(),
        args.candidate_sha.strip().lower(),
        args.platform.strip(),
        args.version.strip(),
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
