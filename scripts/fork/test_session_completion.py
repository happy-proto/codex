"""直接执行补全使用的 SQL，验证 UUID 子串和可安全填入的标题。"""

import re
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORAGE = ROOT / "codex-rs/state/src/completion.rs"
FIRST = "01a1582d-6c70-7e92-b3fa-c461ac05b237"
SECOND = "01a1582d-6c71-7000-8000-abcdef123456"


class SessionCompletionTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.row_factory = sqlite3.Row
        self.db.execute(
            "CREATE TABLE threads (id TEXT, name TEXT, preview TEXT, cwd TEXT, "
            "archived INTEGER, source TEXT, recency_at_ms INTEGER)"
        )
        self.db.executemany(
            "INSERT INTO threads VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                (FIRST, "修复 Shell 补全", "", "/project", 0, "cli", 30),
                (SECOND, "另一个会话", "", "/other", 0, "vscode", 20),
                ("exec-id", "脚本会话", "", "/project", 0, "exec", 40),
                ("unnamed-id", None, "只有预览", "/project", 0, "cli", 10),
                ("archived-id", "归档", "", "/project", 1, "cli", 50),
                ("agent-id", "子代理", "", "/project", 0, "subagent", 60),
            ],
        )

    def query(self, query, cwd=None, include_non_interactive=False):
        sql = re.search(r'r#"(.*?)"#', STORAGE.read_text(), re.S).group(1)
        return self.db.execute(
            sql, (query, query, include_non_interactive, cwd, cwd, query, query)
        ).fetchall()

    def test_uuid_middle_suffix_and_case(self):
        for query in ["6c70-7e92", "c461ac05b237", "C461AC05B237"]:
            with self.subTest(query=query):
                self.assertEqual([row["id"] for row in self.query(query)], [FIRST])

    def test_title_substring_and_case(self):
        for query in ["补全", "shell", "SHELL"]:
            rows = self.query(query)
            self.assertEqual([row["id"] for row in rows], [FIRST])
            self.assertEqual(rows[0]["label"], "修复 Shell 补全")
            self.assertTrue(rows[0]["unique_label"])

    def test_ambiguous_titles_do_not_hide_matching_ids(self):
        self.db.execute(
            "UPDATE threads SET name = 'same title' WHERE id IN (?, ?)", (FIRST, SECOND)
        )
        self.assertEqual(self.query("same", cwd="/project"), [])
        self.assertEqual([row["id"] for row in self.query("c461ac05b237")], [FIRST])

    def test_previews_and_non_interactive_titles_are_not_resume_names(self):
        self.assertEqual(self.query("只有预览"), [])
        self.assertEqual(self.query("脚本", include_non_interactive=True), [])
        self.assertEqual(
            [row["id"] for row in self.query("exec-id", include_non_interactive=True)],
            ["exec-id"],
        )

    def test_filters_order_and_empty_query(self):
        self.assertEqual(
            [row["id"] for row in self.query("")], [FIRST, SECOND, "unnamed-id"]
        )
        self.assertEqual(
            [
                row["id"]
                for row in self.query("", cwd="/project", include_non_interactive=True)
            ],
            ["exec-id", FIRST, "unnamed-id"],
        )
        self.assertEqual(self.query("c461ac05b237", cwd="/other"), [])

    def test_title_resolution_accounts_for_trimmed_names_and_unnamed_previews(self):
        self.db.execute(
            "UPDATE threads SET name = '  修复 Shell 补全  ' WHERE id = ?", (FIRST,)
        )
        self.assertEqual(self.query("Shell")[0]["label"], "修复 Shell 补全")
        self.db.execute(
            "UPDATE threads SET preview = '修复 Shell 补全' WHERE id = 'unnamed-id'"
        )
        self.assertEqual(self.query("Shell"), [])

    def test_limit_applies_after_matching_and_ambiguity_filtering(self):
        self.db.executemany(
            "INSERT INTO threads VALUES (?, 'duplicate', '', '/project', 0, 'cli', ?)",
            [(f"bulk-{index:03}", index + 100) for index in range(105)],
        )
        self.assertEqual(self.query("duplicate"), [])
        rows = self.query("bulk-")
        self.assertEqual(len(rows), 100)
        self.assertEqual(rows[0]["id"], "bulk-104")
        self.assertEqual(rows[-1]["id"], "bulk-005")


if __name__ == "__main__":
    unittest.main()
