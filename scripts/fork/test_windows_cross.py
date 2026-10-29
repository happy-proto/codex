import unittest

from rich.text import Text
from typer.testing import CliRunner
from windows_cross import app, deb_data


class WindowsCrossTests(unittest.TestCase):
    def test_debian_archive_reads_data_after_odd_sized_control_member(self):
        def member(name, data):
            header = (
                f"{name + '/':<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(data):<10}`\n"
            )
            return header.encode() + data + (b"\n" if len(data) % 2 else b"")

        payload = b"target library archive\x00"
        archive = (
            b"!<arch>\n"
            + member("control.tar.xz", b"x")
            + member("data.tar.zst", payload)
        )
        self.assertEqual(deb_data(archive), payload)
        with self.assertRaises(ValueError):
            deb_data(b"not a Debian archive")
        with self.assertRaises(ValueError):
            deb_data(b"!<arch>\n" + member("control.tar.xz", b"x"))

    def test_cli_exposes_targets_and_rejects_missing_linux_sysroot(self):
        runner = CliRunner()
        help_result = runner.invoke(app, ["build", "--help"], terminal_width=160)
        self.assertEqual(help_result.exit_code, 0)
        help_text = Text.from_ansi(help_result.stdout).plain
        self.assertIn("aarch64-apple-darwin", help_text)
        self.assertIn("unknown-linux-gnu", help_text)
        invalid = runner.invoke(
            app, ["build", "--target", "aarch64-pc-windows-msvc", "--output", "unused"]
        )
        self.assertNotEqual(invalid.exit_code, 0)
        missing = runner.invoke(
            app, ["build", "--target", "x86_64-unknown-linux-gnu", "--output", "unused"]
        )
        self.assertNotEqual(missing.exit_code, 0)
        self.assertIn("--sysroot", missing.output)
