import json
import tempfile
import unittest
from pathlib import Path

from build_report import resource_summary, slowest_units


class BuildReportTests(unittest.TestCase):
    def test_command_time_uses_each_invocation_once(self):
        with tempfile.TemporaryDirectory() as directory:
            progress = Path(directory) / "progress.ndjson"
            rows = [
                {
                    "invocation_id": "first",
                    "elapsed_seconds": 5,
                    "processes": [{"rss_mib": 20}],
                },
                {
                    "invocation_id": "first",
                    "elapsed_seconds": 10,
                    "processes": [{"rss_mib": 30}, {"rss_mib": 20}],
                },
                {
                    "invocation_id": "second",
                    "elapsed_seconds": 4,
                    "memory_kib": {
                        "MemAvailable": 1024,
                        "SwapTotal": 2048,
                        "SwapFree": 1024,
                    },
                },
            ]
            progress.write_text(
                "\n".join(json.dumps(row) for row in rows) + '\n{"incomplete":'
            )
            report = resource_summary(progress)
            self.assertEqual(report["invocations"], 2)
            self.assertEqual(report["observed_command_seconds"], 14)
            self.assertEqual(report["sampled_peak_compiler_rss_sum_mib"], 50)
            self.assertEqual(report["sampled_peak_swap_mib"], 1)
            self.assertEqual(report["sampled_min_available_memory_mib"], 1)

    def test_all_cargo_invocations_are_counted_without_latest_alias(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, duration in [
                ("cargo-timing-first.html", 100),
                ("cargo-timing-second.html", 5),
                ("cargo-timing.html", 5),
            ]:
                (root / name).write_text(
                    "const UNIT_DATA = "
                    + json.dumps(
                        [
                            {
                                "name": "codex-cli",
                                "target": "codex bin",
                                "duration": duration,
                            }
                        ]
                    )
                    + ";"
                )
            (root / "cargo-timing-partial.html").write_text("const UNIT_DATA = [")
            self.assertEqual(
                slowest_units(root),
                [
                    {
                        "name": "codex-cli",
                        "target": "codex bin",
                        "seconds": 105,
                        "executions": 2,
                    }
                ],
            )


if __name__ == "__main__":
    unittest.main()
