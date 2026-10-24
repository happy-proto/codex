"""验证 fork CLI 的参数、机器输出和可执行入口契约。"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import build_observer
import release
import sync_skills
from rich.text import Text
from typer.testing import CliRunner


class CliTests(unittest.TestCase):
    """验证调用方可观察的行为，禁止测试产生远端写入。"""

    def test_help_exposes_targets_skills_and_destructive_opt_in(self):
        runner = CliRunner()
        for app, command, expected in (
            (release.app, ["build", "--help"], ["--target", *release.TARGETS]),
            (release.app, ["prune", "--help"], ["--apply", "默认仅预览"]),
            (sync_skills.app, ["update", "--help"], ["--ref", *sync_skills.SOURCES]),
            (build_observer.app, ["--help"], ["--output", "--interval", "原样透传"]),
        ):
            with self.subTest(command=command):
                result = runner.invoke(
                    app, command, env={"COLUMNS": "240"}, terminal_width=240
                )
                self.assertEqual(result.exit_code, 0, result.output)
                help_text = Text.from_ansi(result.output).plain
                for value in expected:
                    self.assertIn(value, help_text)

    def test_invalid_values_are_rejected_before_work(self):
        runner = CliRunner()
        with (
            patch.object(release, "build") as build,
            patch.object(sync_skills, "update") as update,
            patch.object(build_observer, "observe") as observe,
        ):
            for app, command in (
                (release.app, ["build", "--target", "unknown"]),
                (release.app, ["smoke"]),
                (sync_skills.app, ["update", "unknown"]),
                (
                    build_observer.app,
                    ["--output", "unused", "--interval", "0", "--", "cargo"],
                ),
                (
                    build_observer.app,
                    ["--output", "unused", "--interval", "-1", "--", "cargo"],
                ),
            ):
                with self.subTest(command=command):
                    self.assertEqual(runner.invoke(app, command).exit_code, 2)
            build.assert_not_called()
            update.assert_not_called()
            observe.assert_not_called()

    def test_version_stdout_is_only_the_pinned_version(self):
        version = "0.164.0-alpha.12.fork.9"
        tag = "rust-v0.164.0-alpha.12"
        with (
            patch.dict(os.environ, CODEX_FORK_VERSION=version),
            patch.object(Path, "read_text", return_value=tag),
            patch.object(release, "releases") as query,
        ):
            result = CliRunner().invoke(release.app, ["version"])
        self.assertEqual(result.exit_code, 0, result.output)
        self.assertEqual(result.stdout, version + "\n")
        query.assert_not_called()

    def test_observer_preserves_command_arguments_and_exit_code(self):
        command = [sys.executable, "-c", "raise SystemExit(7)", "--target", "$literal"]
        with patch.object(build_observer, "observe", return_value=7) as observe:
            result = CliRunner().invoke(
                build_observer.app,
                ["--output", "progress.ndjson", "--", *command],
            )
        self.assertEqual(result.exit_code, 7, result.output)
        observe.assert_called_once_with(command, Path("progress.ndjson"), 60)

    def test_prune_requires_explicit_apply(self):
        with patch.object(release, "prune") as prune:
            runner = CliRunner()
            self.assertEqual(runner.invoke(release.app, ["prune"]).exit_code, 0)
            self.assertEqual(
                runner.invoke(release.app, ["prune", "--apply"]).exit_code, 0
            )
        self.assertEqual(
            [call.args for call in prune.call_args_list], [(False,), (True,)]
        )

    def test_entrypoints_work_outside_repository(self):
        directory = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory() as temporary:
            for name in (
                "release",
                "sync_skills",
                "build_observer",
                "build_report",
                "cache_diagnostics",
                "probe_ts_rs_cache",
                "run_tests",
            ):
                entry = directory / f"{name}.py"
                for prefix in ([str(entry)], ["uv", "run", "--script", str(entry)]):
                    with self.subTest(command=prefix):
                        result = subprocess.run(
                            [*prefix, "--help"],
                            cwd=temporary,
                            check=False,
                            capture_output=True,
                            text=True,
                            timeout=60,
                        )
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertIn("--help", Text.from_ansi(result.stdout).plain)

    def test_observer_real_entry_preserves_failure_and_ndjson(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "progress.ndjson"
            result = subprocess.run(
                [
                    str(Path(__file__).with_name("build_observer.py")),
                    "--output",
                    str(output),
                    "--",
                    sys.executable,
                    "-c",
                    "raise SystemExit(7)",
                ],
                capture_output=True,
                check=False,
                text=True,
                timeout=60,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            records = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(records[-1]["exit_code"], 7)
