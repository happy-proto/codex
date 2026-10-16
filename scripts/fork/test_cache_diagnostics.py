import json
import tempfile
import unittest
from pathlib import Path

from cache_diagnostics import classify_error, codegen_options, summarize


class CacheDiagnosticsTests(unittest.TestCase):
    def test_read_only_storage_errors_are_distinguished_from_unknown_errors(self):
        for message in (
            "Cannot write to read-only storage",
            "storage is read-only",
        ):
            with self.subTest(message=message):
                self.assertEqual(classify_error(message, set())["kind"], "ReadOnly")

    def test_effective_server_cache_mode_is_reported_without_raw_log(self):
        for mode in ("ReadOnly", "ReadWrite"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                cwd = Path(directory)
                log = cwd / "daemon.log"
                log.write_text(
                    "PRIVATE_STARTUP_SENTINEL\n"
                    f"[2026-10-29T10:50:00.000Z INFO sccache::server] server has setup with {mode}\n"
                )
                report = summarize(log, cwd)
                self.assertEqual(report["server_cache_mode"], mode)
                self.assertNotIn("PRIVATE_STARTUP_SENTINEL", json.dumps(report))

    def test_compiler_options_exclude_private_link_arguments(self):
        self.assertEqual(
            codegen_options(
                [
                    "-C",
                    "opt-level=0",
                    "-Ccodegen-units=256",
                    "-C",
                    "debuginfo=0",
                    "-C",
                    "debug-assertions=on",
                    "-C",
                    "link-arg=PRIVATE_SENTINEL",
                    "--cfg",
                    "PRIVATE_SENTINEL",
                ]
            ),
            {
                "opt-level": "0",
                "codegen-units": "256",
                "debuginfo": "0",
                "debug-assertions": "on",
            },
        )

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
                "-Copt-level=0",
                "-C",
                "debuginfo=0",
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
                first["codegen_options"], {"opt-level": "0", "debuginfo": "0"}
            )
            self.assertEqual(
                first["dependencies"]["dependency"]["file"], dependency.name
            )
            dependency.write_bytes(b"metadata v2")
            second = summarize(log, cwd)["units"][0]
            self.assertNotEqual(
                first["dependencies"]["dependency"]["sha256"],
                second["dependencies"]["dependency"]["sha256"],
            )

    def test_hidden_backend_read_warning_is_reported_without_private_data(self):
        with tempfile.TemporaryDirectory() as directory:
            cwd = Path(directory)
            key = "e" * 64
            private = "PRIVATE_READ_URL_SENTINEL"
            log = cwd / "daemon.log"
            log.write_text(
                f"[2026-10-17T06:00:00.000Z DEBUG sccache::compiler::compiler] [codex_tui]: Hash key: {key}\n"
                f"[2026-10-17T06:00:00.001Z WARN sccache::cache::cache] Got unexpected error: PermissionDenied (permanent) at read, context: {{path: {key}, status: 403}} => https://storage.invalid/blob?sig={private}\n"
                "[codex_tui]: Cache miss in 0.125 s\n"
            )
            report = summarize(log, cwd)
            self.assertEqual(
                report["backend_read_error_kinds"], {"PermissionDenied": 1}
            )
            self.assertEqual(report["backend_read_errors"][0]["http_status"], 403)
            self.assertEqual(report["backend_read_errors"][0]["keys"], [key])
            self.assertNotIn(private, json.dumps(report))
            self.assertNotIn("storage.invalid", json.dumps(report))

    def test_storage_lookup_miss_and_missing_blob_are_distinct(self):
        with tempfile.TemporaryDirectory() as directory:
            cwd = Path(directory)
            first, second = "a" * 64, "b" * 64
            private = "PRIVATE_BLOB_URL_SENTINEL"
            log = cwd / "daemon.log"
            log.write_text(
                f"[codex_tui]: Hash key: {first}\n"
                f"[2026-10-17T06:00:00.001Z WARN opendal::services] service=ghac name=test path=a/a/a/{first}: read failed NotFound (permanent) => GetCacheEntryDownloadURL returns non-ok, the key doesn't exist\n"
                f"[codex_core]: Hash key: {second}\n"
                f"[2026-10-17T06:00:00.002Z WARN opendal::services] service=ghac name=test path=b/b/b/{second}: read failed NotFound (permanent) at Reader::read, context: {{status: 404}} => BlobNotFound https://storage.invalid/blob?sig={private}\n"
                f"[2026-10-17T06:00:00.003Z WARN opendal::services] service=ghac name=test path=b/b/b/{second}: write failed AlreadyExists (permanent) => already exists\n"
            )
            report = summarize(log, cwd)
            failures = report["storage_read_failures"]
            self.assertEqual(len(failures), 2)
            self.assertEqual(failures[0]["keys"], [first])
            self.assertEqual(failures[0]["flags"], ["lookup_non_ok"])
            self.assertEqual(failures[1]["keys"], [second])
            self.assertEqual(failures[1]["flags"], ["blob_not_found"])
            self.assertEqual(failures[1]["http_status"], 404)
            self.assertNotIn(private, json.dumps(report))
            self.assertNotIn("storage.invalid", json.dumps(report))

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
