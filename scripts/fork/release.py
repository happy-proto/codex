#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///
"""构建、验收和发布基于上游 alpha 的个人 fork。"""

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Annotated
lazy import selectors

import typer

app = typer.Typer(add_completion=False, pretty_exceptions_enable=False)

ROOT = Path(__file__).resolve().parents[2]
REPO = "happy-proto/codex"
TARGET = "aarch64-apple-darwin"
TARGETS = (TARGET, "x86_64-unknown-linux-gnu")
VERSION_PATTERN = r"\d+\.\d+\.\d+-alpha(?:\.\d+){1,2}"
RELEASES_TO_KEEP = 10


def run(*args, **kwargs):
    kwargs.setdefault("env", {**os.environ, "CODEX_REPO_ROOT": str(ROOT)})
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def output(*args):
    return run(*args, capture_output=True, text=True).stdout.strip()


def fork_version(upstream_tag, revision=1):
    if not re.fullmatch("rust-v" + VERSION_PATTERN, upstream_tag):
        raise ValueError(f"Not an upstream alpha tag: {upstream_tag}")
    if not isinstance(revision, int) or revision < 1:
        raise ValueError("Fork revision must be positive")
    return upstream_tag.removeprefix("rust-v") + f".fork.{revision}"


def releases():
    pages = api(f"repos/{REPO}/releases?per_page=100", "--paginate", "--slurp")
    return [*page for page in pages]


def next_version(upstream_tag, existing):
    prefix = "fork-v" + fork_version(upstream_tag).removesuffix("1")
    revisions = [
        int(tag[len(prefix) :])
        for release in existing
        if (tag := release["tag_name"]).startswith(prefix)
        and re.fullmatch(r"[1-9]\d*", tag[len(prefix) :])
    ]
    return fork_version(upstream_tag, max(revisions, default=0) + 1)


def selected_version():
    upstream_tag = (ROOT / "scripts/fork/upstream-tag").read_text().strip()
    pinned = os.environ.get("CODEX_FORK_VERSION")
    if pinned:
        prefix = fork_version(upstream_tag).removesuffix("1")
        if not pinned.startswith(prefix) or not re.fullmatch(
            r"[1-9]\d*", pinned[len(prefix) :]
        ):
            raise ValueError("Pinned fork version does not match selected alpha")
        return pinned
    existing = releases()
    pages = api(
        f"repos/{REPO}/git/matching-refs/tags/fork-v?per_page=100",
        "--paginate",
        "--slurp",
    )
    existing.extend(
        {"tag_name": ref["ref"].removeprefix("refs/tags/")}
        for page in pages
        for ref in page
    )
    return next_version(upstream_tag, existing)


def plan(target=TARGET):
    if target not in TARGETS:
        raise ValueError(f"Unsupported fork target: {target}")
    upstream_tag = (ROOT / "scripts/fork/upstream-tag").read_text().strip()
    version = selected_version()
    upstream_commit = output("git", "rev-parse", f"{upstream_tag}^{{commit}}")
    source_commit = output("git", "rev-parse", "HEAD")
    run("git", "merge-base", "--is-ancestor", upstream_commit, source_commit)
    return {
        "schema_version": 1,
        "version": version,
        "tag": f"fork-v{version}",
        "upstream_tag": upstream_tag,
        "upstream_commit": upstream_commit,
        "source_commit": source_commit,
        "target": target,
    }


def sha256(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def archive_package(package, dist):
    archive = dist / "package.tar.xz"
    run(
        "tar",
        "--use-compress-program",
        "xz -8 -T0 --memlimit-compress=50%",
        "-cf",
        str(archive),
        "-C",
        str(package),
        ".",
    )
    return archive


def smoke(package):
    version = json.loads((package / "codex-package.json").read_text())["version"]
    with tempfile.TemporaryDirectory() as home:
        env = {**os.environ, "CODEX_HOME": home}
        cli = package / "bin/codex"
        actual = run(str(cli), "--version", env=env, capture_output=True, text=True)
        if actual.stdout.strip() != f"codex-cli {version}":
            raise ValueError(f"CLI version mismatch: {actual.stdout}")
        run(str(package / "bin/codex-code-mode-host"), "--help", env=env)
        run(str(package / "codex-path/rg"), "--version", env=env)
        if (package / "codex-resources/bwrap").exists():
            run(str(package / "codex-resources/bwrap"), "--version", env=env)
        initialize_app_server(cli, env, version)


def initialize_app_server(cli, env, expected_version):
    request = {
        "id": 1,
        "method": "initialize",
        "params": {"clientInfo": {"name": "fork-smoke", "version": "1"}},
    }
    server = subprocess.Popen(
        [str(cli), "app-server"],
        cwd=ROOT,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        server.stdin.write((json.dumps(request) + "\n").encode())
        server.stdin.flush()
        stdout = b""
        stderr = b""
        initialized = False
        deadline = time.monotonic() + 60
        with selectors.DefaultSelector() as streams:
            streams.register(server.stdout, selectors.EVENT_READ, "stdout")
            streams.register(server.stderr, selectors.EVENT_READ, "stderr")
            while not initialized:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not streams.get_map():
                    raise ValueError(
                        f"app-server initialize failed: {stdout!r} {stderr!r}"
                    )
                for key, _ in streams.select(remaining):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        streams.unregister(key.fileobj)
                    elif key.data == "stderr":
                        stderr += chunk
                    else:
                        stdout += chunk
                        while b"\n" in stdout:
                            line, stdout = stdout.split(b"\n", 1)
                            response = json.loads(line)
                            if response.get("id") == 1 and "result" in response:
                                user_agent = response["result"].get("userAgent", "")
                                product = user_agent.split(" ", 1)[0]
                                actual = product.rpartition("/")[2]
                                if actual != expected_version:
                                    raise ValueError(
                                        f"app-server version mismatch: {user_agent}"
                                    )
                                initialized = True
        # EOF starts server shutdown, so send it only after initialization.
        server.stdin.close()
        server.stdin = None
        _, stderr_tail = server.communicate(timeout=60)
        if server.returncode:
            raise ValueError(f"app-server shutdown failed: {stderr + stderr_tail!r}")
    finally:
        if server.poll() is None:
            server.kill()
            server.wait(timeout=5)
        for stream in (server.stdin, server.stdout, server.stderr):
            if stream is not None:
                stream.close()


def build(target=TARGET):
    metadata = plan(target)
    dist = ROOT / "fork-dist"
    package = dist / "package"
    dist.mkdir(exist_ok=True)
    # 发布版本写入包清单；保留内部 Cargo 版本，避免每个 fork 序号使缓存失效。
    os.environ["STABLE_GIT_COMMIT"] = metadata["source_commit"]
    run(
        sys.executable,
        "scripts/build_codex_package.py",
        "--target",
        target,
        "--cargo",
        str(ROOT / "scripts/fork/cargo-timed.sh"),
        "--cargo-profile",
        "release",
        "--package-version",
        metadata["version"],
        "--package-dir",
        str(package),
        "--force",
    )
    if target.endswith("apple-darwin"):
        for relative in [
            "bin/codex",
            "bin/codex-code-mode-host",
            "codex-path/rg",
        ]:
            binary = package / relative
            args = ["codesign", "--force", "--sign", "-", "--options", "runtime"]
            entitlements = (
                ROOT
                / ".github/scripts/macos-signing"
                / f"{binary.name}.entitlements.plist"
            )
            if entitlements.exists():
                args.extend(["--entitlements", str(entitlements)])
            run(*args, str(binary))
            run("codesign", "--verify", "--strict", str(binary))
    smoke(package)
    archive = archive_package(package, dist)
    metadata["sha256"] = sha256(archive)
    metadata["asset"] = f"codex-package-{target}.tar.xz"
    archive.rename(dist / metadata["asset"])
    (dist / "fork-release.json").write_text(json.dumps(metadata, indent=2) + "\n")
    run("cp", "scripts/fork/install.sh", str(dist / "install.sh"))


def api(path, *args):
    return json.loads(output("gh", "api", path, *args))


def collect_packages(dist):
    manifests = [
        json.loads(path.read_text())
        for path in sorted(dist.glob("*/fork-release.json"))
    ]
    if not manifests:
        return json.loads((dist / "fork-release.json").read_text())
    if {manifest["target"] for manifest in manifests} != set(TARGETS) or len(
        manifests
    ) != len(TARGETS):
        raise ValueError("Both platform packages are required")
    identity = ("version", "tag", "source_commit", "upstream_tag", "upstream_commit")
    if any(any(m[key] != manifests[0][key] for key in identity) for m in manifests):
        raise ValueError("Platform packages have different build identities")
    metadata = next(m for m in manifests if m["target"] == TARGET).copy()
    metadata["packages"] = {
        m["target"]: {key: m[key] for key in ("asset", "sha256")} for m in manifests
    }
    for manifest in manifests:
        package = dist / manifest["target"] / manifest["asset"]
        if sha256(package) != manifest["sha256"]:
            raise ValueError("Local package checksum mismatch")
        run("cp", str(package), str(dist / manifest["asset"]))
    run("cp", str(dist / TARGET / "install.sh"), str(dist / "install.sh"))
    (dist / "fork-release.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata


def publish():
    dist = ROOT / "fork-dist"
    metadata = collect_packages(dist)
    source = metadata["source_commit"]
    branch = api(f"repos/{REPO}")["default_branch"]
    # A queued older run must never publish over a newer branch head.
    current = api(f"repos/{REPO}/git/ref/heads/{branch}")["object"]["sha"]
    if source != current:
        print("Skipping superseded fork build.")
        return
    tag = metadata["tag"]
    existing = subprocess.run(
        [
            "gh",
            "release",
            "view",
            tag,
            "--repo",
            REPO,
            "--json",
            "isDraft,targetCommitish",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if existing.returncode == 0:
        release = json.loads(existing.stdout)
        if not release["isDraft"] or release["targetCommitish"] != source:
            raise ValueError(
                "Release version already exists; rerun with a fresh fork revision"
            )
    elif (
        "(HTTP 404)" not in existing.stderr
        and "release not found" not in existing.stderr.lower()
    ):
        existing.check_returncode()
    if existing.returncode:
        run(
            "gh",
            "release",
            "create",
            tag,
            "--repo",
            REPO,
            "--target",
            source,
            "--prerelease",
            "--draft",
            "--title",
            metadata["version"],
        )
    packages = metadata.get("packages", {metadata.get("target", TARGET): metadata})
    for package in packages.values():
        run(
            "gh",
            "release",
            "upload",
            tag,
            str(dist / package["asset"]),
            "--repo",
            REPO,
            "--clobber",
        )
        with tempfile.TemporaryDirectory() as directory:
            run(
                "gh",
                "release",
                "download",
                tag,
                "--repo",
                REPO,
                "--pattern",
                package["asset"],
                "--dir",
                directory,
            )
            if sha256(Path(directory) / package["asset"]) != package["sha256"]:
                raise ValueError("Uploaded package failed verification")
    current = api(f"repos/{REPO}/git/ref/heads/{branch}")["object"]["sha"]
    if source != current:
        print("Skipping publication of superseded fork build.")
        return
    # GitHub does not create a new tag when saving a release as a draft.
    reference = subprocess.run(
        ["gh", "api", f"repos/{REPO}/git/ref/tags/{tag}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if reference.returncode == 0:
        if json.loads(reference.stdout)["object"]["sha"] != source:
            raise ValueError("Fork tag already points at a different source")
    elif "(HTTP 404)" in reference.stderr:
        api(
            f"repos/{REPO}/git/refs",
            "--method",
            "POST",
            "-f",
            f"ref=refs/tags/{tag}",
            "-f",
            f"sha={source}",
        )
    else:
        reference.check_returncode()
    run(
        "gh",
        "release",
        "upload",
        tag,
        str(dist / "install.sh"),
        "--repo",
        REPO,
        "--clobber",
    )
    run(
        "gh",
        "release",
        "upload",
        tag,
        str(dist / "fork-release.json"),
        "--repo",
        REPO,
        "--clobber",
    )
    notes = dist / "notes.md"
    notes.write_text(
        f"基于上游 `{metadata['upstream_tag']}`，支持 macOS Apple Silicon 和 Linux AMD64。\n\n"
        f"- 上游提交：openai/codex@{metadata['upstream_commit']}\n"
        f"- Fork 源提交：{source}\n\n"
        "每个上游 alpha 独立递增 fork.N；每次发布使用独立 Release 和固定平台包名。"
        "包摘要见 fork-release.json。macOS 采用 ad-hoc 签名，未经 Apple 公证。"
        f"通过 `codex update` 手动更新。仅保留最新 {RELEASES_TO_KEEP} 个已发布版本及其 tag。\n"
    )
    run(
        "gh",
        "release",
        "edit",
        tag,
        "--repo",
        REPO,
        "--draft=false",
        "--prerelease",
        "--target",
        source,
        "--notes-file",
        str(notes),
    )


def fork_tag(tag):
    return (
        re.fullmatch("fork-v" + VERSION_PATTERN + r"\.fork(?:\.[1-9]\d*)?", tag)
        is not None
    )


def expired_releases(existing):
    published = sorted(
        (item for item in existing if fork_tag(item["tag_name"]) and not item["draft"]),
        key=lambda item: datetime.fromisoformat(item["published_at"]),
        reverse=True,
    )
    keep = {item["tag_name"] for item in published[:RELEASES_TO_KEEP]}
    expired = [
        item
        for item in existing
        if fork_tag(item["tag_name"]) and item["tag_name"] not in keep
    ]
    return keep, expired


def prune(apply=False):
    existing = releases()
    keep, expired = expired_releases(existing)
    # Tags without Releases can remain after failed or historical publication.
    pages = api(
        f"repos/{REPO}/git/matching-refs/tags/fork-v?per_page=100",
        "--paginate",
        "--slurp",
    )
    known_tags = {
        ref["ref"].removeprefix("refs/tags/") for page in pages for ref in page
    }
    associated = {item["tag_name"] for item in existing}
    orphaned = sorted(
        tag for tag in known_tags if fork_tag(tag) and tag not in associated
    )
    print(
        json.dumps(
            {
                "protected": sorted(keep),
                "expired_releases": [r["tag_name"] for r in expired],
                "orphaned_tags": orphaned,
            },
            indent=2,
        )
    )
    if apply:
        for item in expired:
            run(
                "gh",
                "release",
                "delete",
                item["tag_name"],
                "--repo",
                REPO,
                "--yes",
                *(["--cleanup-tag"] if item["tag_name"] in known_tags else []),
            )
        for tag in orphaned:
            run("gh", "api", f"repos/{REPO}/git/refs/tags/{tag}", "--method", "DELETE")


class Target(str, Enum):
    """支持的原生包目标。"""

    macos = "aarch64-apple-darwin"
    linux = "x86_64-unknown-linux-gnu"


@app.command("version")
def version_command() -> None:
    """输出分发版本号；优先使用 CODEX_FORK_VERSION，否则查询远端版本。"""
    print(selected_version())


@app.command("plan")
def plan_command(
    target: Annotated[
        Target, typer.Option("--target", help="原生包目标")
    ] = Target.macos,
) -> None:
    """以 JSON 输出当前仓库的构建计划。"""
    print(json.dumps(plan(target.value), indent=2))


@app.command("build")
def build_command(
    target: Annotated[
        Target, typer.Option("--target", help="原生包目标")
    ] = Target.macos,
) -> None:
    """调用上游 package builder 构建并验收原生包。"""
    build(target.value)


@app.command("publish")
def publish_command() -> None:
    """发布 fork-dist 中已验证的平台产物，需要 GitHub 写权限。"""
    publish()


@app.command("smoke")
def smoke_command(
    package: Annotated[
        Path,
        typer.Option(
            "--package",
            exists=True,
            file_okay=False,
            resolve_path=True,
            help="已解包的原生包目录",
        ),
    ],
) -> None:
    """验证原生包资源、CLI 和 app-server。"""
    smoke(package)


@app.command("prune")
def prune_command(
    apply: Annotated[
        bool,
        typer.Option("--apply", help="实际删除过期 Release 与孤立 tag；默认仅预览"),
    ] = False,
) -> None:
    """预览或执行版本保留清理。"""
    prune(apply)


if __name__ == "__main__":
    app()
