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
    queries = re.findall(
        r'sqlx::query(?:_scalar[^\n(]*)?\(\s*"([^"]+)"', STORAGE.read_text()
    )
    return next(query for query in queries if query.startswith(prefix))


class GenerationSpeedStorageTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "state.sqlite"
        self.writer = sqlite3.connect(self.path)
        self.addCleanup(self.writer.close)
        self.writer.execute("PRAGMA foreign_keys = ON")
        self.writer.execute("PRAGMA journal_mode = WAL")
        self.writer.execute("CREATE TABLE threads (id TEXT PRIMARY KEY)")
        self.writer.executescript(SCHEMA.read_text())
        self.writer.executescript(
            SCHEMA.with_name("live_generation_speed.sql").read_text()
        )
        self.writer.executemany(
            "INSERT INTO threads VALUES (?)", [("first",), ("second",)]
        )
        self.writer.commit()

    def save(self, thread, turn, tokens, duration):
        self.writer.execute(
            production_query("INSERT INTO fork_generation_speeds_v1"),
            (thread, turn, tokens, duration, 10, 14, 4000),
        )
        self.writer.execute(
            production_query("DELETE FROM fork_generation_speeds_v1\n"),
            (thread, thread),
        )
        self.writer.commit()

    def read(self, connection, thread):
        return [
            row[:3]
            for row in connection.execute(
                production_query("SELECT turn_id"), (thread,)
            ).fetchall()
        ]

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

    def test_recovery_cleanup_removes_only_the_same_turn(self):
        self.save("first", "recovered", 20, 1_000_000)
        self.save("first", "other", 30, 1_000_000)
        self.save("second", "recovered", 40, 1_000_000)
        self.writer.execute(
            production_query("DELETE FROM fork_generation_speeds_v1 WHERE"),
            ("first", "recovered"),
        )
        self.writer.commit()
        self.assertEqual(self.read(self.writer, "first"), [("other", 30, 1_000_000)])
        self.assertEqual(
            self.read(self.writer, "second"), [("recovered", 40, 1_000_000)]
        )

    def test_write_lock_leaves_old_completion_identity_readable(self):
        self.save("first", "recovered", 200, 4_000_000)
        locker = sqlite3.connect(self.path)
        self.addCleanup(locker.close)
        reader = sqlite3.connect(self.path, timeout=0.01)
        self.addCleanup(reader.close)
        locker.execute("BEGIN IMMEDIATE")
        with self.assertRaises(sqlite3.OperationalError):
            reader.execute(
                production_query("DELETE FROM fork_generation_speeds_v1 WHERE"),
                ("first", "recovered"),
            )
        old = reader.execute(production_query("SELECT turn_id"), ("first",)).fetchone()
        self.assertEqual(old[3:], (10, 14, 4000))
        # Rust/TUI tests reject a new completion even though this old row survives.
        self.assertNotEqual(old[3:], (11, 14, 4000))
        locker.rollback()

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

    def start_live(self, thread, turn, run):
        self.writer.execute(
            production_query("INSERT INTO fork_live_generation_speeds"),
            (thread, turn, run, 10000),
        )
        self.writer.commit()

    def update_live(self, thread, turn, run, speed):
        self.writer.execute(
            production_query("UPDATE fork_live_generation_speeds"),
            (speed, 10000, thread, turn, run),
        )
        self.writer.commit()

    def read_live(self, connection, thread, turn, observed_at=0):
        row = connection.execute(
            production_query("SELECT speed FROM fork_live_generation_speeds"),
            (thread, turn, max(7000, observed_at + 1), 10000),
        ).fetchone()
        return row[0] if row else None

    def test_live_cross_connection_isolation_reset_and_expiry(self):
        reader = sqlite3.connect(self.path)
        self.addCleanup(reader.close)
        self.start_live("first", "turn", "run-1")
        self.start_live("second", "turn", "run-2")
        self.update_live("first", "turn", "run-1", 42.3)
        self.update_live("second", "turn", "run-2", 10.0)
        self.assertEqual(self.read_live(reader, "first", "turn"), 42.3)
        self.assertEqual(self.read_live(reader, "second", "turn"), 10.0)
        self.assertIsNone(self.read_live(reader, "first", "different"))
        self.start_live("first", "turn", "recovered")
        self.update_live("first", "turn", "run-1", 999.0)
        self.assertIsNone(self.read_live(reader, "first", "turn"))
        self.update_live("first", "turn", "recovered", 0.0)
        self.assertEqual(self.read_live(reader, "first", "turn"), 0.0)
        self.update_live("first", "turn", "recovered", None)
        self.assertIsNone(self.read_live(reader, "first", "turn"))
        self.start_live("first", "next-turn", "next-run")
        self.update_live("first", "turn", "recovered", 999.0)
        self.assertIsNone(self.read_live(reader, "first", "next-turn"))
        self.writer.execute("DELETE FROM threads WHERE id = 'first'")
        self.writer.commit()
        self.assertEqual(
            self.writer.execute(
                "SELECT COUNT(*) FROM fork_live_generation_speeds_v1"
            ).fetchone()[0],
            1,
        )

    def test_live_reporter_heartbeat_hides_stale_or_future_snapshots(self):
        self.start_live("first", "turn", "run")
        self.update_live("first", "turn", "run", 20.0)
        for timestamp in [6999, 10001]:
            self.writer.execute(
                "UPDATE fork_live_generation_speeds_v1 SET updated_at_ms = ?",
                (timestamp,),
            )
            self.assertIsNone(self.read_live(self.writer, "first", "turn"))

    def test_live_recovery_rejects_fresh_old_row_when_reset_is_locked(self):
        self.start_live("first", "turn", "before-crash")
        self.update_live("first", "turn", "before-crash", 42.0)
        locker = sqlite3.connect(self.path)
        self.addCleanup(locker.close)
        reader = sqlite3.connect(self.path, timeout=0.01)
        self.addCleanup(reader.close)
        locker.execute("BEGIN IMMEDIATE")
        with self.assertRaises(sqlite3.OperationalError):
            reader.execute(
                production_query("INSERT INTO fork_live_generation_speeds"),
                ("first", "turn", "recovered", 10000),
            )
        self.assertEqual(self.read_live(reader, "first", "turn"), 42.0)
        self.assertIsNone(self.read_live(reader, "first", "turn", observed_at=10000))
        locker.rollback()


if __name__ == "__main__":
    unittest.main()
