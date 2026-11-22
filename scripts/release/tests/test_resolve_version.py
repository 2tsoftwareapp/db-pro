"""Regression tests for the release workflow's version/tag boundary."""

from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

REPO_ROOT = Path(__file__).resolve().parents[3]
RESOLVER = REPO_ROOT / "scripts" / "release" / "resolve-version.py"


class ResolveVersionTests(unittest.TestCase):
    def resolve(self, manifest: Path, tag: str | None = None) -> subprocess.CompletedProcess[str]:
        command = [sys.executable, str(RESOLVER), str(manifest)]
        if tag is not None:
            command.extend(["--tag", tag])
        return subprocess.run(command, capture_output=True, text=True, check=False)

    def manifest(self, directory: str, content: str = '[package]\nname = "db-pro-native"\nversion = "1.2.3"\n') -> Path:
        path = Path(directory) / "Cargo.toml"
        path.write_text(content, encoding="utf-8")
        return path

    def assert_accepted(self, manifest: Path, tag: str | None = None) -> None:
        result = self.resolve(manifest, tag)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "1.2.3\n")
        self.assertEqual(result.stderr, "")

    def assert_rejected(self, manifest: Path, tag: str | None = None) -> None:
        result = self.resolve(manifest, tag)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertEqual(result.stdout, "")
        self.assertTrue(result.stderr.startswith("::error::"), result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_branch_and_commit_builds_do_not_require_a_tag(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assert_accepted(self.manifest(directory))

    def test_stable_tag_still_matches_exactly(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = self.manifest(directory)
            self.assert_accepted(manifest, "v1.2.3")
            self.assert_accepted(manifest, "refs/tags/v1.2.3")

    def test_matching_release_candidates_are_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = self.manifest(directory)
            for tag in ("v1.2.3-rc.1", "v1.2.3-rc.2", "v1.2.3-rc.10", "refs/tags/v1.2.3-rc.1"):
                with self.subTest(tag=tag):
                    self.assert_accepted(manifest, tag)

    def test_wrong_versions_and_malformed_candidates_are_rejected(self) -> None:
        tags = (
            "v1.2.4", "v1.2.4-rc.1", "v1.3.3-rc.1", "v2.2.3-rc.1",
            "v1.2.3-rc.0", "v1.2.3-rc.01", "v1.2.3-rc.", "v1.2.3-rc.1x",
            "v1.2.3-rc.1.extra", "v1.2.3-beta.1", "v1.2.30", "v1.2.3\n",
            "refs/tags/v1.2.4-rc.1", "refs/heads/v1.2.3-rc.1", "v1.2.3-RC.1", "",
        )
        with tempfile.TemporaryDirectory() as directory:
            manifest = self.manifest(directory)
            for tag in tags:
                with self.subTest(tag=tag):
                    self.assert_rejected(manifest, tag)

    def test_existing_cargo_prerelease_and_build_metadata_tags_remain_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            for version in ("1.2.3-beta.1", "1.2.3+build.5", "1.2.3-beta.1+build.5"):
                with self.subTest(version=version):
                    manifest = self.manifest(directory, f'[package]\nversion = "{version}"\n')
                    result = self.resolve(manifest, f"refs/tags/v{version}")
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, version + "\n")
                    self.assert_rejected(manifest, f"v{version}-rc.1")
                    self.assert_rejected(manifest, "v1.2.4")

    def test_reads_package_version_not_dependency_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = self.manifest(
                directory,
                '[dependencies.other]\nversion = "9.9.9"\n\n[package]\nversion = "1.2.3"\n',
            )
            self.assert_accepted(manifest, "v1.2.3-rc.1")

    def test_invalid_or_missing_manifest_fails_without_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            for content in (
                "",
                "[package]\n",
                '[package]\nversion = 123\n',
                '[package]\nversion = "1.2"\n',
                "[package\n",
            ):
                with self.subTest(content=content):
                    self.assert_rejected(self.manifest(directory, content))
            self.assert_rejected(Path(directory) / "missing.toml")

    def test_shipped_manifest_is_valid(self) -> None:
        manifest = REPO_ROOT / "crates" / "native-app" / "Cargo.toml"
        with manifest.open("rb") as handle:
            expected = tomllib.load(handle)["package"]["version"]
        result = self.resolve(manifest)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected + "\n")


if __name__ == "__main__":
    unittest.main()
