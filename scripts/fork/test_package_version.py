"""Verify release identity without recompiling or changing Cargo package versions."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


@unittest.skipUnless(
    os.environ.get("CODEX_FORK_TEST_BINARY"), "requires a built native CLI"
)
class PackagedVersionTests(unittest.TestCase):
    def test_one_binary_reports_each_package_manifest_version(self):
        source = Path(os.environ["CODEX_FORK_TEST_BINARY"]).resolve(strict=True)
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory)
            binary = package / "bin/codex"
            binary.parent.mkdir()
            shutil.copy2(source, binary)
            for version in ("1.2.3-alpha.4.fork.1", "1.2.3-alpha.4.fork.2"):
                with self.subTest(version=version):
                    (package / "codex-package.json").write_text(
                        json.dumps({"version": version})
                    )
                    result = subprocess.run(
                        [str(binary), "--version"],
                        env={**os.environ, "CODEX_HOME": str(package / "home")},
                        capture_output=True,
                        text=True,
                        check=True,
                        timeout=15,
                    )
                    self.assertEqual(result.stdout.strip(), f"codex-cli {version}")


if __name__ == "__main__":
    unittest.main()
