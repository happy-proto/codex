import json
import tempfile
import unittest
from pathlib import Path

from cache_diagnostics import classify_error, summarize


class CacheDiagnosticsTests(unittest.TestCase):
    def test_cache_hit_has_no_compile_duration(self):
        with tempfile.TemporaryDirectory() as directory:
            cwd = Path(directory)
            log = cwd / "daemon.log"
            log.write_text(
                f"[codex_tui]: Hash key: {'d' * 64}\n"
                "[codex_tui]: Cache hit in 0.125 s\n"
            )
            record = summarize(log, cwd)["units"][0]
            self.assertEqual(record["lookup"], "hit")
            self.assertEqual(record["lookup_seconds"], 0.125)
            self.assertNotIn("compile_seconds", record)

    def test_cache_outcome_and_dependency_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            cwd = Path(directory)
            dependency = cwd / "libdependency.rmeta"
            dependency.write_bytes(b"metadata v1")
            key = "a" * 64
            args = [
                "--crate-name",
                "codex_core",
                "--extern",
                f"dependency={dependency}",
            ]
            log = cwd / "daemon.log"
            log.write_text(
                "\n".join(
                    [
                        f"[2026-10-17T05:00:00.000Z DEBUG sccache::compiler::compiler] [codex_core]: get_cached_or_compile: {json.dumps(args)}",
                        f"[2026-10-17T05:00:00.000Z DEBUG sccache::compiler::compiler] [codex_core]: Hash key: {key}",
                        "[2026-10-17T05:00:00.000Z DEBUG sccache::compiler::compiler] [codex_core]: Cache miss in 0.125 s",
                        "[2026-10-17T05:00:00.000Z DEBUG sccache::compiler::compiler] [codex_core]: Compiled in 492.440 s, storing in cache",
                        "[2026-10-17T05:00:00.000Z DEBUG sccache::compiler::compiler] [codex_core]: Stored in cache successfully!",
                    ]
                )
            )
            first = summarize(log, cwd)["units"][0]
            self.assertEqual(first["key"], key)
            self.assertEqual(first["lookup"], "miss")
            self.assertEqual(first["compile_seconds"], 492.44)
            self.assertTrue(first["stored"])
            self.assertEqual(
                first["dependencies"]["dependency"]["file"], dependency.name
            )
            dependency.write_bytes(b"metadata v2")
            second = summarize(log, cwd)["units"][0]
            self.assertNotEqual(
                first["dependencies"]["dependency"]["sha256"],
                second["dependencies"]["dependency"]["sha256"],
            )

    def test_private_error_details_never_exported(self):
        key = "b" * 64
        secret = "PRIVATE_RUNTIME_TOKEN_SENTINEL"
        error = f"RateLimited (temporary) at Writer::write, context: {{path: {key}, status: 429}} => https://storage.invalid/blob?sig={secret} Authorization: Bearer {secret}"
        result = classify_error(error, {key})
        self.assertEqual(result["kind"], "RateLimited")
        self.assertEqual(result["http_status"], 429)
        self.assertEqual(result["keys"], [key])
        self.assertNotIn(secret, json.dumps(result))
        self.assertNotIn("storage.invalid", json.dumps(result))
        with tempfile.TemporaryDirectory() as directory:
            cwd = Path(directory)
            log = cwd / "daemon.log"
            log.write_text(
                f"[2026-10-17T05:00:00.000Z DEBUG sccache::server] Error executing cache write: {error}\n{secret}\n"
            )
            report = summarize(log, cwd)
            self.assertEqual(report["write_error_kinds"], {"RateLimited": 1})
            self.assertEqual(
                report["write_errors"][0]["observed_at"], "2026-10-17T05:00:00.000Z"
            )
            self.assertNotIn(secret, json.dumps(report))
            self.assertNotIn("storage.invalid", json.dumps(report))

    def test_unknown_error_and_unrecognized_digest_are_not_exported(self):
        result = classify_error("private-error-text " + "c" * 64, set())
        self.assertEqual(
            result, {"kind": "unknown", "http_status": None, "flags": [], "keys": []}
        )

    def test_create_non_ok_is_distinct_from_reserve_conflict(self):
        result = classify_error(
            "Unexpected (permanent) => create cache entry returns non-ok", set()
        )
        self.assertEqual(result["flags"], ["create_non_ok"])
        self.assertIsNone(result["http_status"])


if __name__ == "__main__":
    unittest.main()
