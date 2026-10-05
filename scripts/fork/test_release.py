import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock

import release


class VersionTests(unittest.TestCase):
    def test_package_builder_receives_repository_root_without_compilation(self):
        with mock.patch.dict(os.environ):
            os.environ.pop("CODEX_REPO_ROOT", None)
            result = release.run(
                "python3",
                "scripts/build_codex_package.py",
                "--help",
                capture_output=True,
                text=True,
            )
        self.assertIn("--package-version", result.stdout)

    def test_alpha_version_has_only_fork_marker(self):
        self.assertEqual(
            release.fork_version("rust-v0.162.0-alpha.14"), "0.162.0-alpha.14.fork"
        )

    def test_stable_and_untrusted_tags_are_rejected(self):
        for tag in [
            "rust-v0.162.0",
            "rust-v0.162.0-beta.1",
            "../alpha.1",
            "rust-v0.162.0-alpha.1.fork",
        ]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.fork_version(tag)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "codex-home"
        self.bin = self.root / "visible-bin"
        self.network = self.root / "network"
        self.network.mkdir()
        tools = self.root / "tools"
        tools.mkdir()
        self.executable(
            tools / "uname",
            '#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n',
        )
        self.executable(
            tools / "curl",
            """#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith("https://"))
filename = "releases.json" if "/releases?" in url else url.rsplit("/", 1)[1]
shutil.copyfile(pathlib.Path(os.environ["FAKE_NETWORK"]) / filename, args[args.index("-o") + 1])
""",
        )
        self.env = {
            **os.environ,
            "PATH": str(tools) + os.pathsep + os.environ["PATH"],
            "CODEX_HOME": str(self.home),
            "CODEX_INSTALL_DIR": str(self.bin),
            "FAKE_NETWORK": str(self.network),
        }
        self.env.pop("CODEX_RELEASE", None)
        self.version = "0.162.0-alpha.14.fork"
        self.tag = f"fork-v{self.version}"
        (self.network / "releases.json").write_text(
            json.dumps(
                [
                    {"tag_name": "fork-v9.0.0-alpha.1.fork", "draft": True},
                    {"tag_name": self.tag, "draft": False},
                ]
            )
        )

    def executable(self, path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(0o755)

    def prepare_build(self, commit):
        package = self.root / f"package-{commit}"
        package.mkdir()
        (package / "codex-package.json").write_text(
            json.dumps({"version": self.version, "target": release.TARGET})
        )
        self.executable(
            package / "bin/codex", f"#!/bin/sh\necho 'codex-cli {self.version}'\n"
        )
        for binary in [
            "bin/codex-code-mode-host",
            "codex-path/rg",
            "codex-resources/zsh/bin/zsh",
        ]:
            self.executable(package / binary, "#!/bin/sh\nexit 0\n")
        # Changing source without changing the software version changes the actual package.
        (package / "source.txt").write_text(commit)
        archive = self.root / "archive.tar.gz"
        with tarfile.open(archive, "w:gz") as handle:
            handle.add(package, arcname=".")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        asset = f"codex-package-{release.TARGET}-{digest}.tar.gz"
        (self.network / asset).write_bytes(archive.read_bytes())
        manifest = {
            "version": self.version,
            "source_commit": commit * 40,
            "sha256": digest,
            "asset": asset,
        }
        (self.network / "fork-release.json").write_text(json.dumps(manifest))
        return manifest

    def install(self, *args, succeeds=True):
        result = subprocess.run(
            ["sh", str(Path(__file__).with_name("install.sh")), *args],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=15,
        )
        if succeeds:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def selected(self):
        return (self.home / "packages/standalone/current").resolve()

    def test_same_version_update_selects_new_build_and_preserves_old_package(self):
        first = self.prepare_build("a")
        self.install()
        old = self.selected()
        second = self.prepare_build("b")
        self.install()
        self.assertNotEqual(self.selected(), old)
        self.assertTrue(old.is_dir())
        self.assertEqual(
            json.loads((self.selected() / "fork-release.json").read_text()), second
        )
        self.assertNotEqual(first["sha256"], second["sha256"])
        self.assertFalse(
            (self.home / "packages/standalone/auto-update-version").exists()
        )
        self.install("--rollback")
        self.assertEqual(self.selected(), old)

    def test_checksum_failure_leaves_current_unchanged(self):
        self.prepare_build("a")
        self.install()
        old = self.selected()
        manifest = self.prepare_build("b")
        (self.network / manifest["asset"]).write_bytes(b"corrupted download")
        self.install(succeeds=False)
        self.assertEqual(self.selected(), old)
        self.assertTrue((self.bin / "codex").exists())

    def test_same_build_update_is_idempotent(self):
        self.prepare_build("a")
        self.install()
        old = self.selected()
        self.install()
        self.assertEqual(self.selected(), old)
        self.assertFalse(
            (self.home / "packages/standalone/fork-previous").exists(),
            str((self.home / "packages/standalone/fork-previous").resolve())
            + " vs "
            + str(old),
        )

    def test_manifest_version_mismatch_does_not_install(self):
        manifest = self.prepare_build("a")
        manifest["version"] = "0.162.0-alpha.13.fork"
        (self.network / "fork-release.json").write_text(json.dumps(manifest))
        self.install(succeeds=False)
        self.assertFalse((self.home / "packages/standalone/current").exists())

    def test_explicit_version_uses_fork_release(self):
        self.prepare_build("a")
        self.install("--release", self.version)
        self.assertTrue((self.bin / "codex").exists())


if __name__ == "__main__":
    unittest.main()
