#!/usr/bin/env python3
"""Build and publish the alpha-based personal fork using the canonical package."""

import argparse
import hashlib
import json
import os
import re
import selectors
import subprocess
import tempfile
import time
from datetime import UTC, datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPO = "happy-proto/codex"
TARGET = "aarch64-apple-darwin"
TARGETS = (TARGET, "x86_64-unknown-linux-gnu")
VERSION_PATTERN = r"\d+\.\d+\.\d+-alpha(?:\.\d+){1,2}"


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
    return [release for page in pages for release in page]


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
    if output("git", "rev-parse", "origin/main") != upstream_commit:
        raise ValueError("origin/main is not the selected upstream alpha commit")
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
        "xz -9 -T0",
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
        initialize_app_server(cli, env)


def initialize_app_server(cli, env):
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
    # cargo-edit updates inherited workspace versions and Cargo.lock together.
    run(
        "cargo",
        "set-version",
        "--manifest-path",
        "codex-rs/Cargo.toml",
        "-p",
        "codex-cli",
        metadata["version"],
    )
    os.environ["STABLE_GIT_COMMIT"] = metadata["source_commit"]
    run(
        "python3",
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
    # A queued older run must never publish over a newer branch head.
    current = api(f"repos/{REPO}/git/ref/heads/fork")["object"]["sha"]
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
    current = api(f"repos/{REPO}/git/ref/heads/fork")["object"]["sha"]
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
        "通过 `codex update` 手动更新。超过 7 天的 fork Release 和 tag 会清理，最新三个版本始终保留。\n"
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


def parse_date(value):
    return datetime.fromisoformat(value)


def expired_releases(existing, now):
    published = sorted(
        (item for item in existing if fork_tag(item["tag_name"]) and not item["draft"]),
        key=lambda item: parse_date(item["published_at"]),
        reverse=True,
    )
    keep = {item["tag_name"] for item in published[:3]}
    cutoff = now - timedelta(days=7)
    expired = [
        item
        for item in existing
        if fork_tag(item["tag_name"])
        and item["tag_name"] not in keep
        and parse_date(item["published_at"] or item["created_at"]) < cutoff
    ]
    return keep, expired


def prune(apply=False):
    existing = releases()
    now = datetime.now(UTC)
    keep, expired = expired_releases(existing, now)
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
    orphaned = []
    for reference in (ref for page in pages for ref in page):
        tag = reference["ref"].removeprefix("refs/tags/")
        if not fork_tag(tag) or tag in keep or tag in associated:
            continue
        obj = reference["object"]
        if obj["type"] == "tag":
            date = api(f"repos/{REPO}/git/tags/{obj['sha']}")["tagger"]["date"]
        else:
            date = api(f"repos/{REPO}/git/commits/{obj['sha']}")["committer"]["date"]
        if parse_date(date) < now - timedelta(days=7):
            orphaned.append(tag)
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "action", choices=["version", "plan", "build", "publish", "smoke", "prune"]
    )
    parser.add_argument("--package", type=Path)
    parser.add_argument("--target", choices=TARGETS, default=TARGET)
    parser.add_argument(
        "--apply", action="store_true", help="Apply release/tag retention cleanup"
    )
    args = parser.parse_args()
    if args.action == "version":
        print(selected_version())
    elif args.action == "prune":
        prune(args.apply)
    elif args.action == "plan":
        print(json.dumps(plan(args.target), indent=2))
    elif args.action == "build":
        build(args.target)
    elif args.action == "publish":
        publish()
    else:
        if args.package is None:
            parser.error("smoke requires --package")
        smoke(args.package.resolve())


if __name__ == "__main__":
    main()
