#!/usr/bin/env python3
"""更新或校验 fork 内置的第三方 skill 快照；沿用 fork 脚本的标准库工具链。"""

import argparse
import hashlib
import io
import json
import re
import shutil
import subprocess
import tarfile
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "codex-rs/skills/src/assets/fork"
LOCK = ROOT / "scripts/fork/skills.lock.json"
SOURCES = {
    "open-browser-use": "iFurySt/open-browser-use",
    "open-computer-use": "iFurySt/open-computer-use",
}


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "codex-fork-skills"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def resolve_commit(repo, ref):
    value = subprocess.check_output(
        [
            "gh",
            "api",
            f"repos/{repo}/commits/{urllib.parse.quote(ref, safe='')}",
            "--jq",
            ".sha",
        ],
        text=True,
    ).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ValueError(f"无效上游 commit: {value}")
    return value


def extract_skill(data, name, destination):
    """只导入 skill 文件及仓库许可证，拒绝链接和路径穿越。"""
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive.getmembers():
            parts = PurePosixPath(member.name).parts
            relative = None
            if len(parts) > 3 and parts[1:3] == ("skills", name):
                relative = PurePosixPath(*parts[3:])
            elif len(parts) == 2 and parts[1] in (
                "LICENSE",
                "LICENSE.md",
                "LICENSE.txt",
            ):
                relative = PurePosixPath(parts[1])
            if relative is None or member.isdir():
                continue
            if not member.isfile() or relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"不支持的 skill 归档条目: {member.name}")
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(member) as source:
                target.write_bytes(source.read())
    if not (destination / "SKILL.md").is_file():
        raise ValueError(f"上游缺少 skills/{name}/SKILL.md")
    if not any(destination.glob("LICENSE*")):
        raise ValueError(f"上游缺少许可证: {name}")


def fingerprint(directory):
    if any(path.is_symlink() for path in directory.rglob("*")):
        raise ValueError(f"skill 快照中不允许链接: {directory}")
    return {
        str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def update(names, ref):
    lock = json.loads(LOCK.read_text()) if LOCK.exists() else {}
    with tempfile.TemporaryDirectory(prefix="codex-fork-skills-") as temporary:
        staging = Path(temporary)
        for name in names:
            repo = SOURCES[name]
            commit = resolve_commit(repo, ref)
            extract_skill(
                download(f"https://codeload.github.com/{repo}/tar.gz/{commit}"),
                name,
                staging / name,
            )
            lock[name] = {
                "repository": f"https://github.com/{repo}",
                "ref": ref,
                "commit": commit,
                "path": f"skills/{name}",
                "files": fingerprint(staging / name),
            }
        # 所有下载和校验成功后才替换本地快照；lock 最后写入。
        DEST.mkdir(parents=True, exist_ok=True)
        for name in names:
            target = DEST / name
            if target.exists():
                shutil.rmtree(target)
            shutil.copytree(staging / name, target)
            print(f"已更新 {name}: {lock[name]['commit']}")
        LOCK.write_text(json.dumps(lock, indent=2, sort_keys=True) + "\n")


def check(upstream=False):
    lock = json.loads(LOCK.read_text())
    if set(lock) != set(SOURCES) or {p.name for p in DEST.iterdir()} != set(SOURCES):
        raise ValueError("skill 清单与快照目录不一致")
    for name, repo in SOURCES.items():
        item = lock[name]
        if (
            item["repository"] != f"https://github.com/{repo}"
            or item["path"] != f"skills/{name}"
        ):
            raise ValueError(f"skill 来源不一致: {name}")
        if not re.fullmatch(r"[0-9a-f]{40}", item["commit"]):
            raise ValueError(f"无效 commit: {name}")
        if fingerprint(DEST / name) != item["files"]:
            raise ValueError(f"skill 快照与 lock 不一致: {name}")
        print(f"校验通过 {name}: {item['commit']}")
        if upstream:
            latest = resolve_commit(repo, item["ref"])
            print(
                f"  上游 {item['ref']}: {latest}"
                + ("（有更新）" if latest != item["commit"] else "（最新）")
            )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    updater = commands.add_parser(
        "update", help="更新全部或指定 skill，生成固定 commit 和文件校验值"
    )
    updater.add_argument(
        "skill", nargs="?", choices=sorted(SOURCES), help="省略则更新全部 skill"
    )
    updater.add_argument(
        "--ref", default="main", help="上游分支、tag 或 commit（默认 main）"
    )
    checker = commands.add_parser("check", help="离线校验来源清单及全部快照文件")
    checker.add_argument(
        "--upstream",
        action="store_true",
        help="同时联网查看锁定 ref 是否有更新；不修改文件",
    )
    args = parser.parse_args()
    if args.command == "update":
        update([args.skill] if args.skill else list(SOURCES), args.ref)
    else:
        check(args.upstream)


if __name__ == "__main__":
    main()
