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
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPO = "happy-proto/codex"
TARGET = "aarch64-apple-darwin"


def run(*args, **kwargs):
    kwargs.setdefault("env", {**os.environ, "CODEX_REPO_ROOT": str(ROOT)})
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def output(*args):
    return run(*args, capture_output=True, text=True).stdout.strip()


def fork_version(upstream_tag):
    if not re.fullmatch(r"rust-v\d+\.\d+\.\d+-alpha(?:\.\d+){1,2}", upstream_tag):
        raise ValueError(f"Not an upstream alpha tag: {upstream_tag}")
    return upstream_tag.removeprefix("rust-v") + ".fork"


def plan():
    upstream_tag = (ROOT / "scripts/fork/upstream-tag").read_text().strip()
    version = fork_version(upstream_tag)
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
        "target": TARGET,
    }


def sha256(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


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
        run(
            str(package / "codex-resources/zsh/bin/zsh"),
            "-fc",
            "echo fork-smoke",
            env=env,
        )
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


def build():
    metadata = plan()
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
        TARGET,
        "--cargo-profile",
        "release",
        "--package-version",
        metadata["version"],
        "--package-dir",
        str(package),
        "--force",
    )
    for relative in [
        "bin/codex",
        "bin/codex-code-mode-host",
        "codex-path/rg",
        "codex-resources/zsh/bin/zsh",
    ]:
        binary = package / relative
        args = ["codesign", "--force", "--sign", "-", "--options", "runtime"]
        entitlements = (
            ROOT / ".github/scripts/macos-signing" / f"{binary.name}.entitlements.plist"
        )
        if entitlements.exists():
            args.extend(["--entitlements", str(entitlements)])
        run(*args, str(binary))
        run("codesign", "--verify", "--strict", str(binary))
    smoke(package)
    archive = dist / "package.tar.gz"
    run("tar", "-czf", str(archive), "-C", str(package), ".")
    metadata["sha256"] = sha256(archive)
    metadata["asset"] = f"codex-package-{TARGET}-{metadata['sha256']}.tar.gz"
    archive.rename(dist / metadata["asset"])
    (dist / "fork-release.json").write_text(json.dumps(metadata, indent=2) + "\n")
    run("cp", "scripts/fork/install.sh", str(dist / "install.sh"))


def api(path, *args):
    return json.loads(output("gh", "api", path, *args))


def publish():
    dist = ROOT / "fork-dist"
    metadata = json.loads((dist / "fork-release.json").read_text())
    source = metadata["source_commit"]
    # A queued older run must never publish over a newer branch head.
    current = api(f"repos/{REPO}/git/ref/heads/fork")["object"]["sha"]
    if source != current:
        print("Skipping superseded fork build.")
        return
    tag = metadata["tag"]
    existing = subprocess.run(
        ["gh", "release", "view", tag, "--repo", REPO],
        cwd=ROOT,
        capture_output=True,
        check=False,
    )
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
    # Keep build assets immutable. Updating the small manifest is the commit point.
    run(
        "gh",
        "release",
        "upload",
        tag,
        str(dist / metadata["asset"]),
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
            metadata["asset"],
            "--dir",
            directory,
        )
        if sha256(Path(directory) / metadata["asset"]) != metadata["sha256"]:
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
        api(
            f"repos/{REPO}/git/refs/tags/{tag}",
            "--method",
            "PATCH",
            "-f",
            f"sha={source}",
            "-F",
            "force=true",
        )
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
        f"Personal fork of upstream `{metadata['upstream_tag']}`. macOS Apple Silicon only.\n\n"
        f"- Upstream: openai/codex@{metadata['upstream_commit']}\n"
        f"- Fork source: {source}\n"
        f"- Package SHA-256: `{metadata['sha256']}`\n\n"
        "Version numbers are reused within an upstream alpha. The manifest identifies the actual build; "
        "older content-addressed packages remain available. Ad-hoc signed; not Apple notarized. "
        "Updates are manual via `codex update`.\n"
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
        "--notes-file",
        str(notes),
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["plan", "build", "publish", "smoke"])
    parser.add_argument("--package", type=Path)
    args = parser.parse_args()
    if args.action == "plan":
        print(json.dumps(plan(), indent=2))
    elif args.action == "build":
        build()
    elif args.action == "publish":
        publish()
    else:
        if args.package is None:
            parser.error("smoke requires --package")
        smoke(args.package.resolve())


if __name__ == "__main__":
    main()
