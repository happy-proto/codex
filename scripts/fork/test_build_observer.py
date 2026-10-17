import json
import os
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
    snapshot,
)


class BuildObserverTests(unittest.TestCase):
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
                )
            result = counters.sample()
            self.assertEqual(result["backend_read"], {"RateLimited/429": 2})
            self.assertEqual(result["storage_read"], {"NotFound/404": 1})
            self.assertNotIn(private, json.dumps(result))
            self.assertNotIn("private.invalid", json.dumps(result))

    def test_live_process_names_are_not_truncated_paths(self):
        with patch(
            "build_observer.COMPILERS",
            {Path(sys.executable).name, "python3.12", "python3", "python", "Python"},
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
                    sys.executable,
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
