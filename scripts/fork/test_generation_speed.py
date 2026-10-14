"""直接执行 Rust 里的 SQL，验证速度统计的跨连接读取、隔离和留存边界。"""

import re
import sqlite3
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORAGE = ROOT / "codex-rs/state/src/runtime/generation_speed.rs"
SCHEMA = STORAGE.with_suffix(".sql")


def production_query(prefix):
    queries = re.findall(r'sqlx::query\(\s*"([^"]+)"', STORAGE.read_text())
    return next(query for query in queries if query.startswith(prefix))


class GenerationSpeedStorageTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "state.sqlite"
        self.writer = sqlite3.connect(self.path)
        self.addCleanup(self.writer.close)
        self.writer.execute("PRAGMA foreign_keys = ON")
        self.writer.execute("CREATE TABLE threads (id TEXT PRIMARY KEY)")
        self.writer.executescript(SCHEMA.read_text())
        self.writer.executemany(
            "INSERT INTO threads VALUES (?)", [("first",), ("second",)]
        )
        self.writer.commit()

    def save(self, thread, turn, tokens, duration):
        self.writer.execute(
            production_query("INSERT INTO"), (thread, turn, tokens, duration)
        )
        self.writer.execute(production_query("DELETE FROM"), (thread, thread))
        self.writer.commit()

    def read(self, connection, thread):
        return connection.execute(
            production_query("SELECT turn_id"), (thread,)
        ).fetchall()

    def test_cross_connection_identity_and_thread_deletion(self):
        self.save("first", "same-turn", 200, 4_000_000)
        self.save("second", "same-turn", 300, 1_000_000)
        reader = sqlite3.connect(self.path)
        self.addCleanup(reader.close)
        self.assertEqual(self.read(reader, "first"), [("same-turn", 200, 4_000_000)])
        self.assertEqual(self.read(reader, "second"), [("same-turn", 300, 1_000_000)])
        self.assertEqual(self.read(reader, "legacy"), [])
        self.writer.execute("DELETE FROM threads WHERE id = 'first'")
        self.writer.commit()
        self.assertEqual(self.read(reader, "first"), [])
        self.assertEqual(len(self.read(reader, "second")), 1)

    def test_invalid_data_is_rejected(self):
        for tokens, duration in [(0, 1), (1, 0), (-1, 1), (1, -1)]:
            with self.assertRaises(sqlite3.IntegrityError):
                self.save("first", "invalid", tokens, duration)
            self.writer.rollback()
        self.assertEqual(self.read(self.writer, "first"), [])

    def test_retention_and_updates_are_per_thread(self):
        self.save("second", "retained", 30, 1_000_000)
        for index in range(1001):
            self.save("first", f"turn-{index}", 10, 1_000_000)
        self.save("first", "turn-1000", 40, 2_000_000)
        rows = {row[0]: row[1:] for row in self.read(self.writer, "first")}
        self.assertEqual(len(rows), 1000)
        self.assertNotIn("turn-0", rows)
        self.assertEqual(rows["turn-1000"], (40, 2_000_000))
        self.assertEqual(
            self.read(self.writer, "second"), [("retained", 30, 1_000_000)]
        )


if __name__ == "__main__":
    unittest.main()
