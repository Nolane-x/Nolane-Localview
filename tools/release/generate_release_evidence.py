#!/usr/bin/env python3
"""Generate deterministic unsigned release-candidate evidence for LocalView.

This tool deliberately does not claim signing or SLSA provenance. It binds one
exact candidate SHA to the files emitted by Tauri, plus a dependency inventory
from Cargo.lock and the desktop package-lock.json.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib
import urllib.parse
import uuid

SCHEMA_VERSION = 1
CANDIDATE_RE = re.compile(r"^[0-9a-fA-F]{40,64}$")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def safe_relative(path: Path, root: Path) -> str:
    relative = path.relative_to(root).as_posix()
    if not relative or relative.startswith("/") or ".." in Path(relative).parts:
        raise ValueError(f"unsafe release path: {relative!r}")
    return relative


def bundle_entries(root: Path) -> list[dict[str, object]]:
    if not root.is_dir():
        raise ValueError(f"bundle root does not exist: {root}")
    entries: list[dict[str, object]] = []
    for path in sorted(root.rglob("*"), key=lambda item: item.as_posix()):
        relative = safe_relative(path, root)
        if path.is_symlink():
            target = os.readlink(path)
            entries.append(
                {
                    "path": relative,
                    "kind": "symlink",
                    "target": target,
                    "target_sha256": sha256_bytes(target.encode("utf-8")),
                }
            )
        elif path.is_file():
            entries.append(
                {
                    "path": relative,
                    "kind": "file",
                    "size": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
            )
    if not entries:
        raise ValueError("release bundle contains no files")
    return entries


def cargo_components(lock_path: Path) -> list[dict[str, object]]:
    raw = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    components: list[dict[str, object]] = []
    for package in raw.get("package", []):
        name = str(package.get("name", "")).strip()
        version = str(package.get("version", "")).strip()
        if not name or not version:
            raise ValueError("Cargo.lock contains a package without name/version")
        component: dict[str, object] = {
            "type": "library",
            "bom-ref": f"pkg:cargo/{urllib.parse.quote(name, safe='')}@{urllib.parse.quote(version, safe='')}",
            "name": name,
            "version": version,
            "properties": [{"name": "localview:ecosystem", "value": "cargo"}],
        }
        checksum = package.get("checksum")
        if checksum:
            component["hashes"] = [{"alg": "SHA-256", "content": str(checksum)}]
        source = package.get("source")
        if source:
            component["properties"].append({"name": "localview:cargo-source", "value": str(source)})
        components.append(component)
    return components


def npm_components(lock_path: Path) -> list[dict[str, object]]:
    raw = json.loads(lock_path.read_text(encoding="utf-8"))
    packages = raw.get("packages")
    if not isinstance(packages, dict):
        raise ValueError("package-lock.json does not contain a packages map")
    components: list[dict[str, object]] = []
    for package_path, package in sorted(packages.items()):
        if package_path == "" or not isinstance(package, dict):
            continue
        version = str(package.get("version", "")).strip()
        if not version:
            continue
        name = str(package.get("name", "")).strip()
        if not name:
            marker = "node_modules/"
            name = package_path.rsplit(marker, 1)[-1] if marker in package_path else package_path
        component: dict[str, object] = {
            "type": "library",
            "bom-ref": f"pkg:npm/{urllib.parse.quote(name, safe='@/')}@{urllib.parse.quote(version, safe='')}",
            "name": name,
            "version": version,
            "properties": [
                {"name": "localview:ecosystem", "value": "npm"},
                {"name": "localview:package-lock-path", "value": package_path},
            ],
        }
        integrity = package.get("integrity")
        if isinstance(integrity, str) and integrity:
            component["properties"].append({"name": "localview:npm-integrity", "value": integrity})
        components.append(component)
    return components


def dedupe_components(components: list[dict[str, object]]) -> list[dict[str, object]]:
    deduped: dict[str, dict[str, object]] = {}
    for component in components:
        ref = str(component["bom-ref"])
        existing = deduped.get(ref)
        if existing is not None and existing != component:
            raise ValueError(f"conflicting SBOM component identity: {ref}")
        deduped[ref] = component
    return [deduped[key] for key in sorted(deduped)]


def write_json(path: Path, value: object) -> str:
    data = canonical_json_bytes(value)
    path.write_bytes(data)
    return sha256_bytes(data)


def verify_bundle(root: Path, manifest: dict[str, object]) -> None:
    expected = manifest.get("files")
    if not isinstance(expected, list) or not expected:
        raise ValueError("release manifest has no file inventory")
    current = bundle_entries(root)
    if current != expected:
        raise ValueError("release bundle changed after manifest generation")


def generate(args: argparse.Namespace) -> None:
    candidate_sha = args.candidate_sha.lower()
    if not CANDIDATE_RE.fullmatch(candidate_sha):
        raise ValueError("candidate SHA must be 40-64 hexadecimal characters")

    bundle_root = args.bundle_root.resolve()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    tauri = json.loads(args.tauri_config.read_text(encoding="utf-8"))
    product_name = str(tauri.get("productName", "")).strip()
    version = str(tauri.get("version", "")).strip()
    identifier = str(tauri.get("identifier", "")).strip()
    if not product_name or not version or not identifier:
        raise ValueError("Tauri config is missing productName/version/identifier")

    files = bundle_entries(bundle_root)
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "candidate_sha": candidate_sha,
        "platform": args.platform,
        "product": {
            "name": product_name,
            "version": version,
            "identifier": identifier,
        },
        "unsigned_release_candidate": True,
        "files": files,
    }
    manifest_path = output_dir / "release-manifest.json"
    manifest_digest = write_json(manifest_path, manifest)

    components = dedupe_components(
        cargo_components(args.cargo_lock) + npm_components(args.package_lock)
    )
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, f'localview:{candidate_sha}:{args.platform}')}",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": product_name,
                "version": version,
                "bom-ref": f"pkg:generic/{urllib.parse.quote(product_name, safe='')}@{urllib.parse.quote(version, safe='')}",
            },
            "properties": [
                {"name": "localview:candidate-sha", "value": candidate_sha},
                {"name": "localview:platform", "value": args.platform},
                {"name": "localview:unsigned-release-candidate", "value": "true"},
            ],
        },
        "components": components,
    }
    sbom_path = output_dir / "sbom.cyclonedx.json"
    sbom_digest = write_json(sbom_path, sbom)

    provenance = {
        "schema_version": SCHEMA_VERSION,
        "kind": "localview-unsigned-release-candidate-provenance",
        "candidate_sha": candidate_sha,
        "platform": args.platform,
        "product": {
            "name": product_name,
            "version": version,
            "identifier": identifier,
        },
        "release_manifest_sha256": manifest_digest,
        "sbom_cyclonedx_sha256": sbom_digest,
        "signing": {
            "windows": "not_present_external_blocker",
            "macos": "not_present_external_blocker",
        },
        "claims": {
            "slsa_provenance": False,
            "public_signed_release": False,
            "bundle_bytes_bound": True,
            "dependency_inventory_bound": True,
        },
    }
    provenance_path = output_dir / "provenance.json"
    write_json(provenance_path, provenance)

    verify_bundle(bundle_root, manifest)
    for path in (manifest_path, sbom_path, provenance_path):
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"release evidence output missing: {path}")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser()
    value.add_argument("--bundle-root", type=Path, required=True)
    value.add_argument("--candidate-sha", required=True)
    value.add_argument("--platform", choices=("linux", "macos", "windows"), required=True)
    value.add_argument("--tauri-config", type=Path, required=True)
    value.add_argument("--cargo-lock", type=Path, required=True)
    value.add_argument("--package-lock", type=Path, required=True)
    value.add_argument("--output-dir", type=Path, required=True)
    return value


def main() -> int:
    try:
        generate(parser().parse_args())
    except Exception as error:
        print(f"release evidence generation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
