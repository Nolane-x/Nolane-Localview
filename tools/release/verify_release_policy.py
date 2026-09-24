#!/usr/bin/env python3
import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
SUPPORTED_TAG = re.compile(r"^v((0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*))$")
EXPECTED_SCHEMA = "localview-release-policy-v1"
EXPECTED_CONTRACTS = {
    "trusted-verify-v1-primary-backward-readable",
    "wave9-companion-ignored-by-previous-reader",
    "wave9-companion-committed-before-primary",
}


def fail(message):
    raise SystemExit(f"release policy invalid: {message}")


def strict_object(pairs):
    obj = {}
    for key, value in pairs:
        if key in obj:
            fail(f"duplicate JSON key {key!r}")
        obj[key] = value
    return obj


def load_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=strict_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def version_tuple(value, field):
    if not isinstance(value, str) or not SEMVER.fullmatch(value):
        fail(f"{field} must be strict MAJOR.MINOR.PATCH")
    return tuple(int(part) for part in value.split("."))


def workspace_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = re.search(r"(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\Z)", text)
    if not section:
        fail("Cargo.toml is missing [workspace.package]")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', section.group(1))
    if not match:
        fail("workspace.package.version is missing")
    return match.group(1)


def git_supported_tags():
    try:
        output = subprocess.run(
            ["git", "tag", "--list"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        ).stdout
    except (OSError, subprocess.SubprocessError) as error:
        fail(f"cannot enumerate release tags: {error}")
    tags = []
    for raw in output.splitlines():
        tag = raw.strip()
        match = SUPPORTED_TAG.fullmatch(tag)
        if match:
            tags.append((version_tuple(match.group(1), "release tag"), tag))
    return sorted(tags)


def assert_state_compatibility_contracts(policy):
    required = policy.get("required_state_compatibility_contracts")
    if not isinstance(required, list) or set(required) != EXPECTED_CONTRACTS:
        fail("required_state_compatibility_contracts must equal the audited V1 contract set")
    if len(required) != len(set(required)):
        fail("required_state_compatibility_contracts contains duplicates")

    recovery = (ROOT / "apps/desktop/src-tauri/src/trusted_verify_recovery.rs").read_text(
        encoding="utf-8"
    )
    contract = (
        ROOT / "apps/desktop/src-tauri/tests/wave9_autonomous_verification_contract.rs"
    ).read_text(encoding="utf-8")

    required_source = [
        "const RECOVERY_SCHEMA_VERSION: u32 = 1;",
        "const WAVE9_RECOVERY_SCHEMA_VERSION: u32 = 1;",
        'const RECOVERY_DIR: &str = "trusted-verify-v1";',
        "struct PersistedWave9PreflightV1",
        'format!("{id}.wave9")',
        "-> PersistedVerificationRecordV1",
        "fs::rename(&wave9_temp, &wave9)",
        "fs::rename(&temp, &meta)",
    ]
    for marker in required_source:
        if marker not in recovery:
            fail(f"rollback source contract missing marker: {marker}")

    wave9_commit = recovery.find("fs::rename(&wave9_temp, &wave9)")
    primary_commit = recovery.find("fs::rename(&temp, &meta)")
    if wave9_commit < 0 or primary_commit < 0 or wave9_commit >= primary_commit:
        fail("Wave 9 companion must commit before rollback-readable primary metadata")

    if "trusted_verify_wave9_recovery_preserves_previous_reader_compatibility" not in contract:
        fail("rollback compatibility regression contract is missing")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--policy",
        default="release-policy.json",
        help="repository-relative release policy path",
    )
    args = parser.parse_args()

    policy_path = (ROOT / args.policy).resolve()
    if policy_path.parent != ROOT or policy_path.name != "release-policy.json":
        fail("policy path must be repository-root release-policy.json")
    policy = load_json(policy_path)

    expected_keys = {
        "schema",
        "current_version",
        "initial_supported_release",
        "previous_supported_version",
        "installer_upgrade_required",
        "installer_rollback_required",
        "state_rollback_compatibility_required",
        "required_state_compatibility_contracts",
    }
    if set(policy) != expected_keys:
        fail(f"top-level keys differ from contract: {sorted(set(policy) ^ expected_keys)}")
    if policy["schema"] != EXPECTED_SCHEMA:
        fail("unsupported release-policy schema")

    current = version_tuple(policy["current_version"], "current_version")
    tauri = load_json(ROOT / "apps/desktop/src-tauri/tauri.conf.json")
    if tauri.get("version") != policy["current_version"]:
        fail("tauri.conf.json version does not match release policy")
    if workspace_version() != policy["current_version"]:
        fail("workspace Cargo version does not match release policy")

    for field in (
        "initial_supported_release",
        "installer_upgrade_required",
        "installer_rollback_required",
        "state_rollback_compatibility_required",
    ):
        if type(policy[field]) is not bool:
            fail(f"{field} must be boolean")

    if not policy["state_rollback_compatibility_required"]:
        fail("state rollback compatibility cannot be disabled")
    assert_state_compatibility_contracts(policy)

    tags = git_supported_tags()
    initial = policy["initial_supported_release"]
    previous = policy["previous_supported_version"]

    if initial:
        if previous is not None:
            fail("initial supported release must not invent a previous supported version")
        if policy["installer_upgrade_required"] or policy["installer_rollback_required"]:
            fail("installer upgrade/rollback cannot be required without a previous supported release")
        if tags:
            fail(
                "initial_supported_release cannot remain true after a supported vMAJOR.MINOR.PATCH tag exists"
            )
    else:
        previous_tuple = version_tuple(previous, "previous_supported_version")
        if previous_tuple >= current:
            fail("previous_supported_version must be lower than current_version")
        if not policy["installer_upgrade_required"] or not policy["installer_rollback_required"]:
            fail("all non-initial releases require installer upgrade and rollback evidence")
        expected_tag = "v" + previous
        if expected_tag not in {tag for _, tag in tags}:
            fail(f"previous supported release tag {expected_tag} is absent")

    print(
        json.dumps(
            {
                "schema": EXPECTED_SCHEMA,
                "current_version": policy["current_version"],
                "initial_supported_release": initial,
                "supported_release_tags": [tag for _, tag in tags],
                "state_rollback_contracts": sorted(EXPECTED_CONTRACTS),
                "result": "pass",
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
