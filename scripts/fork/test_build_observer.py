import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from build_observer import (
    StorageLogCounters,
    cache_counters,
    compiler_processes,
    observe,
    progress_summary,
    snapshot,
)


class BuildObserverTests(unittest.TestCase):
    def test_console_summary_keeps_details_in_artifact(self):
        summary = progress_summary(
            {
                "invocation_id": "build-1",
                "elapsed_seconds": 3.5,
                "exit_code": 7,
                "private": "PRIVATE_SENTINEL",
                "processes": [{"pid": 123, "rss_mib": 12.5, "crate": "codex_core"}],
            }
        )
        self.assertEqual(summary["compiler_rss_sum_mib"], 12.5)
        self.assertEqual(summary["active_crates"], ["codex_core"])
        self.assertEqual(summary["exit_code"], 7)
        self.assertNotIn("processes", summary)
        self.assertNotIn("PRIVATE_SENTINEL", json.dumps(summary))

    def test_appended_commands_have_separate_invocation_ids(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            patch("build_observer.snapshot", side_effect=dict),
        ):
            output = Path(directory) / "progress.ndjson"
            with patch(
                "build_observer.progress_summary", side_effect=RuntimeError("PRIVATE")
            ):
                for status in (0, 7):
                    self.assertEqual(
                        observe(
                            [sys.executable, "-c", f"raise SystemExit({status})"],
                            output,
                            0.01,
                        ),
                        status,
                    )
            records = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(len({record["invocation_id"] for record in records}), 2)
            self.assertEqual(
                [record["exit_code"] for record in records if "exit_code" in record],
                [0, 7],
            )
            self.assertNotIn("PRIVATE", output.read_text())

    def test_cargo_wrapper_preserves_arguments_environment_cwd_and_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "repo with spaces/scripts/fork"
            scripts.mkdir(parents=True)
            wrapper = scripts / "cargo-observed.sh"
            shutil.copy2(Path(__file__).with_name(wrapper.name), wrapper)
            (scripts / "build_observer.py").write_text(
                "import os, pathlib, sys\n"
                "assert sys.argv[1] == '--output' and sys.argv[3] == '--'\n"
                "assert pathlib.Path(sys.argv[2]).resolve() == pathlib.Path(__file__).resolve().parents[2] / 'fork-sccache/progress.ndjson'\n"
                "os.execvpe(sys.argv[4], sys.argv[4:], os.environ)\n"
            )
            binaries = root / "bin"
            binaries.mkdir()
            cargo = binaries / "cargo"
            cargo.write_text(
                f"#!{sys.executable}\n"
                "import json, os, pathlib, sys\n"
                "pathlib.Path(os.environ['FORK_TEST_CAPTURE']).write_text(json.dumps({'args': sys.argv[1:], 'cwd': os.getcwd(), 'debug': os.environ['CARGO_PROFILE_CI_TEST_DEBUG']}))\n"
                "raise SystemExit(7)\n"
            )
            cargo.chmod(0o755)
            capture = root / "args.json"
            arguments = [
                "test",
                "--profile",
                "ci-test",
                "--",
                "filter with spaces",
                "$literal",
            ]
            result = subprocess.run(
                [str(wrapper), *arguments],
                check=False,
                cwd=root,
                env={
                    **os.environ,
                    "PATH": str(binaries) + os.pathsep + os.environ["PATH"],
                    "FORK_TEST_CAPTURE": str(capture),
                    "CARGO_PROFILE_CI_TEST_DEBUG": "0",
                },
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertEqual(
                json.loads(capture.read_text()),
                {"args": arguments, "cwd": str(root.resolve()), "debug": "0"},
            )

    def test_live_storage_errors_are_incremental_and_private(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "daemon.log"
            private = "PRIVATE_STORAGE_SENTINEL"
            warning = f"[WARN sccache::cache::cache] Got unexpected error: RateLimited (temporary) at read, context: {{status: 429}} => https://private.invalid/?sig={private}\n"
            log.write_text(warning)
            counters = StorageLogCounters(log)
            first = counters.sample()
            self.assertEqual(first["backend_read"], {"RateLimited/429": 1})
            self.assertEqual(counters.sample(), first)
            with log.open("a") as stream:
                stream.write(warning.rstrip("\n"))
            self.assertEqual(counters.sample(), first)
            with log.open("a") as stream:
                stream.write(
                    "\n[WARN opendal::services] service=ghac: read failed NotFound (permanent) at Reader::read, context: {status: 404}\n"
                    "[DEBUG sccache::server] Error executing cache write: Cannot write to read-only storage\n"
                )
            result = counters.sample()
            self.assertEqual(result["backend_read"], {"RateLimited/429": 2})
            self.assertEqual(result["storage_read"], {"NotFound/404": 1})
            self.assertEqual(result["cache_write"], {"ReadOnly/None": 1})
            self.assertNotIn(private, json.dumps(result))
            self.assertNotIn("private.invalid", json.dumps(result))

    def test_live_process_names_are_not_truncated_paths(self):
        with patch(
            "build_observer.COMPILERS",
            {
                Path(sys.executable).name,
                Path(sys.executable).resolve().name,
                f"python{sys.version_info.major}.{sys.version_info.minor}",
                "python3",
                "python",
                "Python",
            },
        ):
            processes = snapshot()["processes"]
        self.assertIn(os.getpid(), [item["pid"] for item in processes])

    def test_process_sample_exports_only_compiler_fields(self):
        private = "PRIVATE_ARGUMENT_SENTINEL"
        result = compiler_processes(
            f"101 125.5 2048 /tools/rustc /tools/rustc --crate-name codex_tui --private {private}\n"
            f"102 0.1 4096 curl curl https://private.invalid/?sig={private}\n"
        )
        self.assertEqual(
            result,
            [
                {
                    "pid": 101,
                    "executable": "rustc",
                    "cpu_percent": 125.5,
                    "rss_mib": 2.0,
                    "crate": "codex_tui",
                }
            ],
        )
        self.assertNotIn(private, json.dumps(result))
        self.assertNotIn("private.invalid", json.dumps(result))

    def test_cache_sample_does_not_export_locations_or_unknown_fields(self):
        result = cache_counters(
            {
                "cache_location": "PRIVATE",
                "stats": {
                    "cache_hits": {"counts": {"Rust": 2, "C/C++": 3}},
                    "cache_misses": {"Rust": 1},
                    "cache_writes": 4,
                    "private": "PRIVATE",
                },
            }
        )
        self.assertEqual(
            result, {"cache_hits": 5, "cache_misses": 1, "cache_writes": 4}
        )

    def test_observation_failure_preserves_child_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "progress.ndjson"
            with patch("build_observer.snapshot", side_effect=RuntimeError("PRIVATE")):
                code = observe(
                    [sys.executable, "-c", "raise SystemExit(7)"], output, 0.01
                )
            self.assertEqual(code, 7)
            records = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(records[-1]["exit_code"], 7)
            self.assertNotIn("PRIVATE", output.read_text())

    def test_cancellation_is_forwarded_and_child_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "pid"
            process = subprocess.Popen(
                [
                    "uv",
                    "run",
                    "--script",
                    str(Path(__file__).with_name("build_observer.py")),
                    "--output",
                    str(root / "progress.ndjson"),
                    "--interval",
                    "0.05",
                    "--",
                    sys.executable,
                    "-c",
                    "import os, pathlib, sys, time; pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)",
                    str(pidfile),
                ],
                stdout=subprocess.DEVNULL,
            )
            try:
                deadline = time.monotonic() + 5
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(pidfile.exists())
                child = int(pidfile.read_text())
                process.send_signal(signal.SIGTERM)
                self.assertEqual(process.wait(timeout=10), 143)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child, 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()

    def test_unwritable_artifact_does_not_fail_successful_build(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            patch("build_observer.snapshot", return_value={}),
        ):
            self.assertEqual(
                observe([sys.executable, "-c", "pass"], Path(directory), 0.01), 0
            )


if __name__ == "__main__":
    unittest.main()
