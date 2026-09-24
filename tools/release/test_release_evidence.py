import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest

MODULE_PATH = pathlib.Path(__file__).with_name("generate_release_evidence.py")
SPEC = importlib.util.spec_from_file_location("release_evidence", MODULE_PATH)
release_evidence = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(release_evidence)

VERIFY_PATH = pathlib.Path(__file__).with_name("verify_release_evidence.py")
VERIFY_SPEC = importlib.util.spec_from_file_location("verify_release_evidence", VERIFY_PATH)
verify_release_evidence = importlib.util.module_from_spec(VERIFY_SPEC)
assert VERIFY_SPEC.loader is not None
VERIFY_SPEC.loader.exec_module(verify_release_evidence)


class ReleaseEvidenceTests(unittest.TestCase):
    def test_publish_workflow_uses_portable_macos_tar_transport(self):
        repo_root = pathlib.Path(__file__).resolve().parents[2]
        workflow = (repo_root / ".github/workflows/publish-release-candidate.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn(
            "COPYFILE_DISABLE=1 tar -czf target/release/release-payload/bundle.tar.gz",
            workflow,
        )
        self.assertIn(
            "tar -xzf \"$root/bundle.tar.gz\" -C \"$root/verified\"",
            workflow,
        )

    def test_evidence_binds_artifacts_locks_and_stays_unsigned(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "apps/desktop").mkdir(parents=True)
            (root / "Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.1"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                'checksum = "abc"\n',
                encoding="utf-8",
            )
            (root / "apps/desktop/package-lock.json").write_text(
                json.dumps({
                    "lockfileVersion": 3,
                    "packages": {
                        "": {"name": "@nolane/localview-desktop", "version": "0.2.0"},
                        "node_modules/react": {
                            "name": "react",
                            "version": "19.2.8",
                            "resolved": "https://registry.npmjs.org/react/-/react-19.2.8.tgz",
                            "integrity": "sha512-test"
                        }
                    }
                }),
                encoding="utf-8",
            )

            subprocess.run(["git", "init"], cwd=root, check=True, capture_output=True)
            subprocess.run(
                ["git", "config", "user.email", "release-test@local.invalid"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "LocalView Release Test"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "add", "Cargo.lock", "apps/desktop/package-lock.json"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "commit", "-m", "fixture: commit dependency locks"],
                cwd=root,
                check=True,
                capture_output=True,
            )
            candidate_sha = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            committed_cargo = subprocess.run(
                ["git", "show", f"{candidate_sha}:Cargo.lock"],
                cwd=root,
                check=True,
                capture_output=True,
            ).stdout
            committed_npm = subprocess.run(
                ["git", "show", f"{candidate_sha}:apps/desktop/package-lock.json"],
                cwd=root,
                check=True,
                capture_output=True,
            ).stdout

            # Simulate CRLF checkout normalization after the committed objects
            # already exist. Evidence must remain bound to Git blob bytes.
            (root / "Cargo.lock").write_bytes(committed_cargo.replace(b"\n", b"\r\n"))
            (root / "apps/desktop/package-lock.json").write_bytes(
                committed_npm.replace(b"\n", b"\r\n")
            )

            bundle = root / "bundle"
            bundle.mkdir()
            (bundle / "LocalView.bin").write_bytes(b"candidate")
            output = root / "evidence"

            release_evidence.build_evidence(
                root,
                bundle,
                output,
                candidate_sha,
                "linux",
                "0.2.0",
            )

            manifest = json.loads((output / "artifact-manifest.json").read_text())
            provenance = json.loads((output / "provenance.json").read_text())
            sbom = json.loads((output / "sbom.spdx.json").read_text())

            self.assertFalse(manifest["signed"])
            self.assertFalse(manifest["public_release"])
            self.assertEqual(manifest["candidate_sha"], candidate_sha)
            self.assertEqual(manifest["artifacts"][0]["path"], "LocalView.bin")
            self.assertTrue(manifest["artifacts"][0]["sha256"].startswith("sha256:"))
            self.assertEqual(provenance["release_class"], "unsigned-release-candidate")
            self.assertEqual(
                provenance["lockfiles"]["Cargo.lock"],
                release_evidence.sha256_bytes(committed_cargo),
            )
            self.assertEqual(
                provenance["lockfiles"]["apps/desktop/package-lock.json"],
                release_evidence.sha256_bytes(committed_npm),
            )
            self.assertNotEqual(
                provenance["lockfiles"]["Cargo.lock"],
                release_evidence.sha256_file(root / "Cargo.lock"),
            )
            self.assertNotEqual(
                provenance["lockfiles"]["apps/desktop/package-lock.json"],
                release_evidence.sha256_file(root / "apps/desktop/package-lock.json"),
            )
            self.assertTrue(provenance["claims"]["dependency_resolution_locked"])
            self.assertFalse(provenance["claims"]["code_signing_verified"])
            self.assertEqual(sbom["spdxVersion"], "SPDX-2.3")
            names = {package["name"] for package in sbom["packages"]}
            self.assertIn("serde", names)
            self.assertIn("react", names)

            verify_release_evidence.verify(
                root,
                bundle,
                output,
                candidate_sha,
                "linux",
                "0.2.0",
            )

            manifest["signed"] = True
            (output / "artifact-manifest.json").write_text(
                json.dumps(manifest),
                encoding="utf-8",
            )
            with self.assertRaises(SystemExit):
                verify_release_evidence.verify(
                    root,
                    bundle,
                    output,
                    candidate_sha,
                    "linux",
                    "0.2.0",
                )


if __name__ == "__main__":
    unittest.main()
