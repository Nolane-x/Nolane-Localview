import argparse
import json
from pathlib import Path
import tempfile
import unittest

from generate_release_evidence import generate


class ReleaseEvidenceTests(unittest.TestCase):
    def test_generation_binds_candidate_bundle_and_dependency_inventory_without_signing_claim(self):
        with tempfile.TemporaryDirectory(prefix="localview-release-evidence-") as raw:
            root = Path(raw)
            bundle = root / "bundle"
            bundle.mkdir()
            (bundle / "LocalView.bin").write_bytes(b"localview-release-candidate\n")

            tauri = root / "tauri.conf.json"
            tauri.write_text(
                json.dumps(
                    {
                        "productName": "LocalView",
                        "version": "0.2.0",
                        "identifier": "com.nolane.localview",
                    }
                ),
                encoding="utf-8",
            )
            cargo = root / "Cargo.lock"
            cargo.write_text(
                """version = 4

[[package]]
name = "fixture-crate"
version = "1.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
""",
                encoding="utf-8",
            )
            package = root / "package-lock.json"
            package.write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {
                            "": {"name": "localview-desktop", "version": "0.2.0"},
                            "node_modules/fixture-package": {
                                "name": "fixture-package",
                                "version": "4.5.6",
                                "integrity": "sha512-fixture",
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )
            output = root / "evidence"
            candidate = "a" * 40
            generate(
                argparse.Namespace(
                    bundle_root=bundle,
                    candidate_sha=candidate,
                    platform="linux",
                    tauri_config=tauri,
                    cargo_lock=cargo,
                    package_lock=package,
                    output_dir=output,
                )
            )

            manifest = json.loads((output / "release-manifest.json").read_text())
            sbom = json.loads((output / "sbom.cyclonedx.json").read_text())
            provenance = json.loads((output / "provenance.json").read_text())

            self.assertEqual(manifest["candidate_sha"], candidate)
            self.assertTrue(manifest["unsigned_release_candidate"])
            self.assertEqual(manifest["files"][0]["path"], "LocalView.bin")
            self.assertEqual(sbom["bomFormat"], "CycloneDX")
            refs = {component["bom-ref"] for component in sbom["components"]}
            self.assertIn("pkg:cargo/fixture-crate@1.2.3", refs)
            self.assertIn("pkg:npm/fixture-package@4.5.6", refs)
            self.assertFalse(provenance["claims"]["public_signed_release"])
            self.assertFalse(provenance["claims"]["slsa_provenance"])
            self.assertTrue(provenance["claims"]["bundle_bytes_bound"])
            self.assertEqual(provenance["signing"]["windows"], "not_present_external_blocker")
            self.assertEqual(provenance["signing"]["macos"], "not_present_external_blocker")


if __name__ == "__main__":
    unittest.main()
