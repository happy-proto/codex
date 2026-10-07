import hashlib
import json
import os
import pty
import signal
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import release


class VersionTests(unittest.TestCase):
    def test_timed_cargo_preserves_arguments_and_failure_status(self):
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\nprint(json.dumps(sys.argv[1:]))\nsys.exit(7)\n"
            )
            cargo.chmod(0o755)
            result = subprocess.run(
                [
                    str(release.ROOT / "scripts/fork/cargo-timed.sh"),
                    "build",
                    "--release",
                ],
                env={**os.environ, "PATH": f"{directory}:{os.environ['PATH']}"},
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(
                json.loads(result.stdout), ["build", "--release", "--timings"]
            )
            self.assertEqual(result.returncode, 7)

    def test_initialization_keeps_input_open_until_the_response(self):
        with tempfile.TemporaryDirectory() as directory:
            cli = Path(directory) / "fake-app-server"
            cli.write_text(
                """#!/usr/bin/env python3
import json, select, sys
request = json.loads(sys.stdin.readline())
if select.select([sys.stdin], [], [], 0.05)[0] and sys.stdin.read() == "":
    sys.exit(0)
print(json.dumps({"id": request["id"], "result": {}}), flush=True)
sys.stdin.read()
"""
            )
            cli.chmod(0o755)
            release.initialize_app_server(cli, os.environ.copy())

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


class PublicationTests(unittest.TestCase):
    def test_new_draft_without_tag_can_publish_verified_package(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "fork-dist"
            dist.mkdir()
            archive = b"verified package bytes"
            metadata = {
                "tag": "fork-v0.162.0-alpha.14.fork",
                "version": "0.162.0-alpha.14.fork",
                "source_commit": "a" * 40,
                "upstream_tag": "rust-v0.162.0-alpha.14",
                "upstream_commit": "b" * 40,
                "asset": "package.tar.gz",
                "sha256": hashlib.sha256(archive).hexdigest(),
            }
            (dist / "fork-release.json").write_text(json.dumps(metadata))
            (dist / metadata["asset"]).write_bytes(archive)
            tags = {}
            calls = []

            def api(path, *args):
                if path.endswith("heads/fork"):
                    return {"object": {"sha": metadata["source_commit"]}}
                if "POST" in args and path.endswith("git/refs"):
                    tags[metadata["tag"]] = metadata["source_commit"]
                    return {}
                if "PATCH" in args:
                    if metadata["tag"] not in tags:
                        raise ValueError("GitHub cannot update a missing draft tag")
                    tags[metadata["tag"]] = metadata["source_commit"]
                    return {}
                raise AssertionError((path, args))

            def run(*args, **kwargs):
                calls.append(args)
                if args[:3] == ("gh", "release", "download"):
                    destination = Path(args[args.index("--dir") + 1])
                    (destination / metadata["asset"]).write_bytes(archive)

            def query(args, **kwargs):
                if args[:3] == ["gh", "release", "view"]:
                    return subprocess.CompletedProcess(args, 0)
                return subprocess.CompletedProcess(
                    args, 1, stderr="gh: Not Found (HTTP 404)"
                )

            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(release, "api", side_effect=api),
                mock.patch.object(release, "run", side_effect=run),
                mock.patch.object(release.subprocess, "run", side_effect=query),
            ):
                release.publish()
            self.assertEqual(tags[metadata["tag"]], metadata["source_commit"])
            self.assertEqual(calls[-1][:3], ("gh", "release", "edit"))
            self.assertIn("--draft=false", calls[-1])

    def test_superseded_build_does_not_modify_releases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fork-dist").mkdir()
            (root / "fork-dist/fork-release.json").write_text(
                json.dumps({"source_commit": "a" * 40})
            )
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(
                    release, "api", return_value={"object": {"sha": "b" * 40}}
                ),
                mock.patch.object(release, "run") as write,
                mock.patch.object(release.subprocess, "run") as query,
            ):
                release.publish()
            write.assert_not_called()
            query.assert_not_called()


@unittest.skipUnless(sys.platform == "darwin", "安装契约由 macOS 产物构建 job 验证")
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
        self.tools = tools
        self.executable(
            tools / "gh",
            """#!/bin/sh
[ "$*" = 'auth token --hostname github.com' ] || exit 2
[ -n "${FAKE_GH_TOKEN:-}" ] || exit 1
printf '%s\\n' "$FAKE_GH_TOKEN"
""",
        )
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
headers = []
for i, arg in enumerate(args):
    if arg in ("-H", "--header"):
        value = args[i + 1]
        headers.extend(pathlib.Path(value[1:]).read_text().splitlines() if value.startswith("@") else [value])
with (pathlib.Path(os.environ["FAKE_NETWORK"]) / "requests.jsonl").open("a") as log:
    import json
    log.write(json.dumps({"url": url, "headers": headers, "args": args}) + "\\n")
if url.endswith(".tar.gz") and os.environ.get("FAIL_PACKAGE_DOWNLOAD") == "1":
    print("curl: (22) The requested URL returned error: 503", file=sys.stderr)
    sys.exit(22)
if "/releases?" in url and os.environ.get("REQUIRE_GH_AUTH") == "1":
    if headers != ["Authorization: Bearer " + os.environ["FAKE_GH_TOKEN"]]:
        print("curl: (22) The requested URL returned error: 403", file=sys.stderr)
        sys.exit(22)
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
            check=False,
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

    def test_noninteractive_install_has_compact_output_and_silent_downloads(self):
        self.prepare_build("a")
        result = self.install()
        self.assertIn(f"✓ Installed {self.version} (aaaaaaaa)", result.stdout)
        self.assertIn("Restart Codex to use the new version.", result.stdout)
        self.assertNotIn("a" * 40, result.stdout)
        self.assertNotIn("Background upgrades", result.stdout)
        self.assertEqual(result.stderr, "")
        requests = [
            json.loads(line)
            for line in (self.network / "requests.jsonl").read_text().splitlines()
        ]
        for request in requests:
            self.assertTrue(
                "-s" in request["args"] or "-fsS" in request["args"], request
            )

    def test_current_build_skips_download_and_restart_message(self):
        self.prepare_build("a")
        self.install()
        old = self.selected()
        (self.network / "requests.jsonl").unlink()
        result = self.install()
        self.assertIn(f"Already up to date: {self.version} (aaaaaaaa)", result.stdout)
        self.assertNotIn("Restart", result.stdout)
        self.assertNotIn("Installed", result.stdout)
        self.assertEqual(self.selected(), old)
        requests = (self.network / "requests.jsonl").read_text().splitlines()
        self.assertFalse(
            any(json.loads(line)["url"].endswith(".tar.gz") for line in requests)
        )

    def test_interactive_download_only_shows_progress_for_package(self):
        self.prepare_build("a")
        master, slave = pty.openpty()
        try:
            result = subprocess.run(
                ["sh", str(Path(__file__).with_name("install.sh"))],
                env=self.env,
                stdout=subprocess.PIPE,
                stderr=slave,
                text=True,
                timeout=15,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout)
        finally:
            os.close(slave)
            os.close(master)
        requests = [
            json.loads(line)
            for line in (self.network / "requests.jsonl").read_text().splitlines()
        ]
        for request in requests:
            self.assertEqual(
                "--progress-bar" in request["args"],
                request["url"].endswith(".tar.gz"),
            )

    def test_download_failure_preserves_diagnostics_and_selected_package(self):
        self.prepare_build("a")
        self.install()
        old = self.selected()
        self.prepare_build("b")
        self.env["FAIL_PACKAGE_DOWNLOAD"] = "1"
        result = self.install(succeeds=False)
        self.assertIn("curl: (22)", result.stderr)
        self.assertIn("Package download failed; current CLI unchanged.", result.stderr)
        self.assertNotIn("✓ Installed", result.stdout)
        self.assertEqual(self.selected(), old)

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

    def test_update_invalidates_the_previous_build_notice(self):
        self.prepare_build("a")
        self.install()
        cache = self.home / "fork-version.json"
        cache.write_text(json.dumps({"latest_commit": "a" * 40}))
        self.prepare_build("b")
        self.install()
        self.assertFalse(
            cache.exists(), "old commit would look like a different update"
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

    def test_logged_in_gh_authenticates_api_without_leaking_to_downloads(self):
        self.prepare_build("a")
        self.env["FAKE_GH_TOKEN"] = "test-only-gh-token"
        self.env["REQUIRE_GH_AUTH"] = "1"
        result = self.install()
        requests = [
            json.loads(line)
            for line in (self.network / "requests.jsonl").read_text().splitlines()
        ]
        self.assertEqual(
            requests[0]["headers"], ["Authorization: Bearer test-only-gh-token"]
        )
        self.assertTrue(all(not request["headers"] for request in requests[1:]))
        self.assertNotIn(self.env["FAKE_GH_TOKEN"], result.stdout + result.stderr)

    def test_installer_without_gh_can_use_public_api(self):
        self.prepare_build("a")
        (self.tools / "gh").unlink()
        self.env["PATH"] = str(self.tools) + ":/usr/bin:/bin:/usr/sbin:/sbin"
        self.install()
        requests = (self.network / "requests.jsonl").read_text().splitlines()
        self.assertTrue(all(not json.loads(line)["headers"] for line in requests))

    def test_logged_out_gh_can_use_public_api(self):
        self.prepare_build("a")
        self.install()
        requests = (self.network / "requests.jsonl").read_text().splitlines()
        self.assertTrue(all(not json.loads(line)["headers"] for line in requests))

    def test_official_installer_lock_prevents_fork_selection(self):
        self.prepare_build("a")
        root = self.home / "packages/standalone"
        root.mkdir(parents=True)
        owner = subprocess.Popen(
            [
                "/usr/bin/lockf",
                "-t",
                "0",
                str(root / "install.lock"),
                "python3",
                "-c",
                "import time; print('locked', flush=True); time.sleep(30)",
            ],
            stdout=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        try:
            self.assertEqual(owner.stdout.readline().strip(), "locked")
            self.install(succeeds=False)
            self.assertFalse((root / "current").exists())
        finally:
            os.killpg(owner.pid, signal.SIGTERM)
            owner.wait()
            owner.stdout.close()


if __name__ == "__main__":
    unittest.main()
