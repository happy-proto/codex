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
from datetime import UTC, datetime, timedelta
from pathlib import Path
from unittest import mock

import release


class VersionTests(unittest.TestCase):
    def test_xz_package_preserves_binary_contents_and_executable_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "package"
            (package / "bin").mkdir(parents=True)
            binary = package / "bin/codex"
            binary.write_bytes(b"binary contents\x00\xff")
            binary.chmod(0o755)
            archive = release.archive_package(package, root)
            with tarfile.open(archive, "r:xz") as handle:
                member = handle.getmember("./bin/codex")
                self.assertEqual(handle.extractfile(member).read(), binary.read_bytes())
                self.assertEqual(member.mode & 0o777, 0o755)

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
                sys.executable,
                "scripts/build_codex_package.py",
                "--help",
                capture_output=True,
                text=True,
            )
        self.assertIn("--package-version", result.stdout)

    def test_alpha_version_has_numbered_fork_marker(self):
        self.assertEqual(
            release.fork_version("rust-v0.162.0-alpha.14"), "0.162.0-alpha.14.fork.1"
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

    def test_revision_is_numeric_and_resets_for_each_alpha(self):
        existing = [
            {"tag_name": tag}
            for tag in [
                "fork-v0.162.0-alpha.14.fork.1",
                "fork-v0.162.0-alpha.14.fork.2",
                "fork-v0.162.0-alpha.14.fork.10",
                "fork-v0.162.0-alpha.14.2.fork.30",
                "fork-v0.162.0-alpha.15.fork.50",
                "fork-v0.162.0-alpha.14.fork.invalid",
            ]
        ]
        self.assertEqual(
            release.next_version("rust-v0.162.0-alpha.14", existing),
            "0.162.0-alpha.14.fork.11",
        )
        self.assertEqual(
            release.next_version("rust-v0.162.0-alpha.16", existing),
            "0.162.0-alpha.16.fork.1",
        )

    def test_pinned_version_must_match_alpha_and_positive_revision(self):
        for version in [
            "0.163.0-alpha.5.fork.0",
            "0.163.0-alpha.5.fork.01",
            "0.163.0-alpha.4.fork.1",
        ]:
            with (
                mock.patch.dict(os.environ, CODEX_FORK_VERSION=version),
                self.assertRaises(ValueError),
            ):
                release.selected_version()


class RetentionTests(unittest.TestCase):
    def test_keep_latest_three_and_all_versions_within_seven_days(self):
        now = datetime(2026, 10, 10, tzinfo=UTC)

        def item(number, age, draft=False):
            date = (now - timedelta(days=age)).isoformat()
            return {
                "tag_name": f"fork-v0.163.0-alpha.5.fork.{number}",
                "draft": draft,
                "published_at": None if draft else date,
                "created_at": date,
            }

        existing = [
            item(1, 12),
            item(3, 9),
            item(4, 8),
            item(5, 8),
            item(6, 7),
            item(7, 1, True),
        ]
        existing.append(
            {
                "tag_name": "rust-v0.163.0-alpha.1",
                "draft": False,
                "published_at": "2020-01-01T00:00:00Z",
            }
        )
        keep, expired = release.expired_releases(existing, now)
        self.assertEqual(keep, {existing[i]["tag_name"] for i in (2, 3, 4)})
        self.assertEqual(
            [r["tag_name"] for r in expired], [existing[i]["tag_name"] for i in (0, 1)]
        )

    def test_old_draft_without_a_tag_can_be_cleaned_up(self):
        existing = [
            {
                "tag_name": "fork-v0.163.0-alpha.4.fork.1",
                "draft": True,
                "published_at": None,
                "created_at": "2020-01-01T00:00:00Z",
            }
        ]
        with (
            mock.patch.object(release, "releases", return_value=existing),
            mock.patch.object(release, "api", return_value=[[]]),
            mock.patch.object(release, "run") as write,
        ):
            release.prune(apply=True)
        write.assert_called_once_with(
            "gh",
            "release",
            "delete",
            existing[0]["tag_name"],
            "--repo",
            release.REPO,
            "--yes",
        )

    def test_recent_fourth_release_is_kept_and_old_draft_is_removed(self):
        now = datetime(2026, 10, 10, tzinfo=UTC)
        existing = [
            {
                "tag_name": f"fork-v0.163.0-alpha.5.fork.{i}",
                "draft": False,
                "published_at": (now - timedelta(days=i)).isoformat(),
            }
            for i in range(1, 5)
        ]
        existing.append(
            {
                "tag_name": "fork-v0.163.0-alpha.4.fork",
                "draft": True,
                "published_at": None,
                "created_at": (now - timedelta(days=8)).isoformat(),
            }
        )
        _, expired = release.expired_releases(existing, now)
        self.assertEqual(expired, [existing[-1]])


class PublicationTests(unittest.TestCase):
    def test_new_draft_without_tag_can_publish_verified_package(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "fork-dist"
            dist.mkdir()
            archive = b"verified package bytes"
            metadata = {
                "tag": "fork-v0.162.0-alpha.14.fork.1",
                "version": "0.162.0-alpha.14.fork.1",
                "source_commit": "a" * 40,
                "upstream_tag": "rust-v0.162.0-alpha.14",
                "upstream_commit": "b" * 40,
                "asset": "package.tar.xz",
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
                    return subprocess.CompletedProcess(
                        args,
                        0,
                        stdout=json.dumps(
                            {
                                "isDraft": True,
                                "targetCommitish": metadata["source_commit"],
                            }
                        ),
                    )
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

    def test_published_version_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fork-dist").mkdir()
            metadata = {
                "tag": "fork-v0.163.0-alpha.5.fork.1",
                "source_commit": "a" * 40,
            }
            (root / "fork-dist/fork-release.json").write_text(json.dumps(metadata))
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(
                    release, "api", return_value={"object": {"sha": "a" * 40}}
                ),
                mock.patch.object(release, "run") as write,
                mock.patch.object(
                    release.subprocess,
                    "run",
                    return_value=subprocess.CompletedProcess(
                        [],
                        0,
                        stdout=json.dumps(
                            {"isDraft": False, "targetCommitish": "a" * 40}
                        ),
                    ),
                ),
                self.assertRaises(ValueError),
            ):
                release.publish()
            write.assert_not_called()

    def test_both_platform_packages_are_verified_and_recorded(self):
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            for target in release.TARGETS:
                package = dist / target
                package.mkdir()
                contents = target.encode()
                asset = f"codex-package-{target}.tar.xz"
                metadata = {
                    "target": target,
                    "version": "0.163.0-alpha.5.fork.1",
                    "tag": "fork-v0.163.0-alpha.5.fork.1",
                    "source_commit": "a" * 40,
                    "upstream_tag": "rust-v0.163.0-alpha.5",
                    "upstream_commit": "b" * 40,
                    "asset": asset,
                    "sha256": hashlib.sha256(contents).hexdigest(),
                }
                (package / asset).write_bytes(contents)
                (package / "fork-release.json").write_text(json.dumps(metadata))
                (package / "install.sh").write_text("installer")
            result = release.collect_packages(dist)
            self.assertEqual(set(result["packages"]), set(release.TARGETS))
            for target in release.TARGETS:
                self.assertEqual(
                    (dist / result["packages"][target]["asset"]).read_bytes(),
                    target.encode(),
                )
            (dist / release.TARGETS[1] / "fork-release.json").unlink()
            with self.assertRaisesRegex(ValueError, "Both platform packages"):
                release.collect_packages(dist)

    def test_platform_packages_must_share_the_same_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            for index, target in enumerate(release.TARGETS):
                directory = dist / target
                directory.mkdir()
                (directory / "fork-release.json").write_text(
                    json.dumps(
                        {
                            "target": target,
                            "version": f"0.163.0-alpha.5.fork.{index + 1}",
                            "tag": "test",
                            "source_commit": "a" * 40,
                            "upstream_tag": "test",
                            "upstream_commit": "b" * 40,
                        }
                    )
                )
            with self.assertRaisesRegex(ValueError, "different build identities"):
                release.collect_packages(dist)

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
            (
                '#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n'
                if sys.platform == "darwin"
                else '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n'
            ),
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
if url.endswith((".tar.xz", ".tar.gz")) and os.environ.get("FAIL_PACKAGE_DOWNLOAD") == "1":
    print("curl: (22) The requested URL returned error: 503", file=sys.stderr)
    sys.exit(22)
if url.endswith((".tar.xz", ".tar.gz")) and os.environ.get("FAIL_AXEL") == "1":
    output = pathlib.Path(args[args.index("-o") + 1])
    assert not output.exists(), "axel partial file was not removed"
    assert not pathlib.Path(str(output) + ".st").exists(), "axel state was not removed"
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
            # 不让宿主机的可选下载器影响 curl 路径的安装契约。
            "PATH": os.pathsep.join(
                [str(tools)]
                + [
                    entry
                    for entry in os.environ["PATH"].split(os.pathsep)
                    if not (Path(entry) / "axel").exists()
                ]
            ),
            "CODEX_HOME": str(self.home),
            "CODEX_INSTALL_DIR": str(self.bin),
            "FAKE_NETWORK": str(self.network),
        }
        self.env.pop("CODEX_RELEASE", None)
        self.target = release.TARGET if sys.platform == "darwin" else release.TARGETS[1]
        self.version = "0.162.0-alpha.14.fork.1"
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

    def prepare_build(self, commit, compression="xz", legacy=False):
        package = self.root / f"package-{commit}"
        package.mkdir()
        (package / "codex-package.json").write_text(
            json.dumps({"version": self.version, "target": self.target})
        )
        self.executable(
            package / "bin/codex", f"#!/bin/sh\necho 'codex-cli {self.version}'\n"
        )
        for binary in [
            "bin/codex-code-mode-host",
            "codex-path/rg",
            "codex-resources/bwrap",
        ]:
            self.executable(package / binary, "#!/bin/sh\nexit 0\n")
        # Changing source without changing the software version changes the actual package.
        (package / "source.txt").write_text(commit)
        archive = self.root / f"archive.tar.{compression}"
        with tarfile.open(archive, f"w:{compression}") as handle:
            handle.add(package, arcname=".")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        asset = f"codex-package-{self.target}{'-' + digest if legacy else ''}.tar.{compression}"
        (self.network / asset).write_bytes(archive.read_bytes())
        manifest = {
            "version": self.version,
            "source_commit": commit * 40,
            "sha256": digest,
            "asset": asset,
        }
        if not legacy:
            manifest["packages"] = {self.target: {"asset": asset, "sha256": digest}}
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

    def enable_axel(self):
        self.executable(
            self.tools / "axel",
            """#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
network = pathlib.Path(os.environ["FAKE_NETWORK"])
url = next(arg for arg in args if arg.startswith("https://"))
output = pathlib.Path(args[args.index("-o") + 1])
with (network / "axel.jsonl").open("a") as log:
    log.write(json.dumps({"url": url, "args": args}) + "\\n")
if os.environ.get("FAIL_AXEL") == "1":
    output.write_bytes(b"partial")
    pathlib.Path(str(output) + ".st").write_bytes(b"resume state")
    print("axel: connection failed", file=sys.stderr)
    sys.exit(1)
shutil.copyfile(network / url.rsplit("/", 1)[1], output)
if os.environ.get("CORRUPT_AXEL") == "1":
    output.write_bytes(b"corrupted download")
""",
        )

    def test_axel_downloads_only_package_with_default_connections(self):
        self.enable_axel()
        manifest = self.prepare_build("a")
        result = self.install()
        requests = [
            json.loads(line)
            for line in (self.network / "axel.jsonl").read_text().splitlines()
        ]
        self.assertEqual(len(requests), 1)
        self.assertTrue(requests[0]["url"].endswith(manifest["asset"]))
        self.assertIn("-q", requests[0]["args"])
        self.assertNotIn("-n", requests[0]["args"])
        self.assertFalse(
            any(arg.startswith("--num-connections") for arg in requests[0]["args"])
        )
        self.assertEqual(result.stderr, "")
        self.assertFalse(
            any(
                json.loads(line)["url"].endswith((".tar.xz", ".tar.gz"))
                for line in (self.network / "requests.jsonl").read_text().splitlines()
            )
        )

    def test_axel_failure_falls_back_to_curl(self):
        self.enable_axel()
        self.prepare_build("a")
        self.env["FAIL_AXEL"] = "1"
        result = self.install()
        self.assertIn("axel: connection failed", result.stderr)
        self.assertIn("retrying with curl", result.stderr)
        self.assertIn("✓ Installed", result.stdout)

    def test_axel_corruption_preserves_selected_package(self):
        self.prepare_build("a")
        self.install()
        old = self.selected()
        self.enable_axel()
        self.prepare_build("b")
        self.env["CORRUPT_AXEL"] = "1"
        result = self.install(succeeds=False)
        self.assertIn("Package checksum mismatch", result.stderr)
        self.assertEqual(self.selected(), old)

    def test_axel_interactive_progress_goes_to_terminal(self):
        self.enable_axel()
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
        request = json.loads((self.network / "axel.jsonl").read_text())
        self.assertIn("-a", request["args"])
        self.assertNotIn("-q", request["args"])

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
            any(
                json.loads(line)["url"].endswith((".tar.xz", ".tar.gz"))
                for line in requests
            )
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
                request["url"].endswith((".tar.xz", ".tar.gz")),
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
        self.assertEqual(first["asset"], second["asset"])
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

    @unittest.skipUnless(sys.platform == "darwin", "Historical releases are macOS only")
    def test_explicit_version_accepts_existing_gzip_release(self):
        self.version = "0.162.0-alpha.14.fork"
        manifest = self.prepare_build("a", compression="gz", legacy=True)
        self.install("--release", self.version)
        self.assertEqual(
            json.loads((self.selected() / "fork-release.json").read_text()), manifest
        )
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

    @unittest.skipUnless(sys.platform == "darwin", "macOS advisory lock")
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
