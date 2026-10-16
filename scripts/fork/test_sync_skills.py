"""验证快照来源、更新失败边界及不可信归档处理。"""

import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import sync_skills


def archive(files, symlink=None):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as result:
        for name, content in files.items():
            member = tarfile.TarInfo("source/" + name)
            data = content.encode()
            member.size = len(data)
            result.addfile(member, io.BytesIO(data))
        if symlink:
            member = tarfile.TarInfo("source/" + symlink)
            member.type = tarfile.SYMTYPE
            member.linkname = "/etc/passwd"
            result.addfile(member)
    return buffer.getvalue()


class SkillSnapshotTests(unittest.TestCase):
    def test_checked_in_snapshot_matches_lock(self):
        sync_skills.check()

    def test_only_skill_tree_and_license_are_imported(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            sync_skills.extract_skill(
                archive(
                    {
                        "skills/example/SKILL.md": "skill",
                        "skills/example/references/guide.md": "guide",
                        "LICENSE": "license",
                        "README.md": "excluded",
                        "skills/other/SKILL.md": "excluded",
                    }
                ),
                "example",
                destination,
            )
            self.assertEqual(
                set(sync_skills.fingerprint(destination)),
                {"SKILL.md", "references/guide.md", "LICENSE"},
            )

    def test_archive_links_and_traversal_are_rejected(self):
        for data in [
            archive({"skills/example/../../escape": "bad"}),
            archive({}, symlink="skills/example/link"),
        ]:
            with self.subTest(data=data), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises(ValueError):
                    sync_skills.extract_skill(data, "example", Path(temporary))

    def test_failed_update_preserves_existing_snapshot_and_lock(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "skills"
            destination.mkdir()
            sentinel = destination / "sentinel"
            sentinel.write_text("preserved")
            lock = root / "lock.json"
            lock.write_text("{}\n")
            with (
                patch.object(sync_skills, "DEST", destination),
                patch.object(sync_skills, "LOCK", lock),
                patch.object(sync_skills, "resolve_commit", return_value="a" * 40),
                patch.object(
                    sync_skills,
                    "download",
                    return_value=archive({"README.md": "missing skill"}),
                ),
            ):
                with self.assertRaises(ValueError):
                    sync_skills.update(["open-browser-use"], "main")
            self.assertEqual(sentinel.read_text(), "preserved")
            self.assertEqual(lock.read_text(), "{}\n")

    def test_check_detects_added_modified_and_deleted_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "skills"
            skill = destination / "example"
            skill.mkdir(parents=True)
            content = skill / "SKILL.md"
            content.write_text("original")
            lock = root / "lock.json"
            lock.write_text(
                json.dumps(
                    {
                        "example": {
                            "repository": "https://github.com/owner/repo",
                            "ref": "main",
                            "commit": "a" * 40,
                            "path": "skills/example",
                            "files": sync_skills.fingerprint(skill),
                        }
                    }
                )
            )
            with (
                patch.object(sync_skills, "DEST", destination),
                patch.object(sync_skills, "LOCK", lock),
                patch.object(sync_skills, "SOURCES", {"example": "owner/repo"}),
            ):
                content.write_text("changed")
                with self.assertRaises(ValueError):
                    sync_skills.check()
                content.write_text("original")
                extra = skill / "extra"
                extra.write_text("extra")
                with self.assertRaises(ValueError):
                    sync_skills.check()
                extra.unlink()
                content.unlink()
                with self.assertRaises(ValueError):
                    sync_skills.check()
