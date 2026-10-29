#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///
"""在 Windows ARM 实验交叉编译，并在 Linux 完成平台包验收。"""

import hashlib
import io
import json
import lzma
import os
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import urllib.request
from enum import Enum
from pathlib import Path
from typing import Annotated

import typer

ROOT = Path(__file__).resolve().parents[2]
os.environ["CODEX_REPO_ROOT"] = str(ROOT)
sys.path.insert(0, str(ROOT / "scripts"))
from codex_package.targets import TARGET_SPECS
from codex_package.v8 import resolve_codex_v8_cargo_env

app = typer.Typer(pretty_exceptions_enable=False, add_completion=False)


class Target(str, Enum):
    """与正式发布相同的目标。"""

    macos = "aarch64-apple-darwin"
    linux = "x86_64-unknown-linux-gnu"


def run(*args, **kwargs):
    """在源码根目录执行命令并保留失败。"""
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def fetch(url):
    """读取公开的 Ubuntu 仓库资源。"""
    with urllib.request.urlopen(url, timeout=120) as response:
        return response.read()


def deb_data(content):
    """从 Debian ar 包中提取 data tar，不执行维护脚本。"""
    if not content.startswith(b"!<arch>\n"):
        raise ValueError("Invalid Debian archive")
    offset = 8
    while offset + 60 <= len(content):
        header = content[offset : offset + 60]
        size = int(header[48:58])
        name = header[:16].decode().strip().rstrip("/")
        start = offset + 60
        if name.startswith("data.tar"):
            return content[start : start + size]
        offset = start + size + size % 2
    raise ValueError("Debian archive has no data tar")


@app.command("linux-sysroot")
def linux_sysroot(
    output: Annotated[
        Path, typer.Option("--output", help="Ubuntu 24.04 AMD64 系统库解包目录")
    ],
):
    """获取目标平台的 libcap、ALSA 和 Linux 头文件，核对仓库 SHA256。"""
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    wanted = {
        "libcap-dev",
        "libcap2",
        "libasound2-dev",
        "libasound2t64",
        "linux-libc-dev",
    }
    packages = {}
    base = "https://archive.ubuntu.com/ubuntu/"
    for suite in ("noble", "noble-updates"):
        index = lzma.decompress(
            fetch(f"{base}dists/{suite}/main/binary-amd64/Packages.xz")
        ).decode()
        for paragraph in index.split("\n\n"):
            fields = dict(
                line.split(": ", 1)
                for line in paragraph.splitlines()
                if ": " in line and not line.startswith(" ")
            )
            if fields.get("Package") in wanted:
                packages[fields["Package"]] = fields
    if set(packages) != wanted:
        raise ValueError(f"Missing packages: {wanted - set(packages)}")
    manifest = {}
    for name, fields in packages.items():
        print(f"Extracting {name} {fields['Version']}", flush=True)
        content = fetch(base + fields["Filename"])
        if hashlib.sha256(content).hexdigest() != fields["SHA256"]:
            raise ValueError(f"Invalid checksum: {name}")
        with tarfile.open(fileobj=io.BytesIO(deb_data(content))) as archive:
            archive.extractall(output, filter="data")
        manifest[name] = {key: fields[key] for key in ("Version", "Filename", "SHA256")}
    (output / "sources.json").write_text(json.dumps(manifest, indent=2) + "\n")


@app.command()
def build(
    target: Annotated[Target, typer.Option("--target", help="完整发布包的编译目标")],
    output: Annotated[
        Path, typer.Option("--output", help="原生二进制、Cargo timing 和缓存统计目录")
    ],
    sysroot: Annotated[
        Path | None, typer.Option("--sysroot", help="Linux 目标需要的 AMD64 系统库目录")
    ] = None,
):
    """使用原生 Windows ARM Rust 编译完整包需要的全部自有二进制。"""
    if target == Target.linux and (sysroot is None or not sysroot.is_dir()):
        raise typer.BadParameter("Linux build requires --sysroot")
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = run(
        "git", "rev-parse", "HEAD", capture_output=True, text=True
    ).stdout.strip()
    environment = {
        **os.environ,
        **resolve_codex_v8_cargo_env(TARGET_SPECS[target.value]),
        "STABLE_GIT_COMMIT": source,
    }
    if target == Target.linux:
        environment.update(
            PKG_CONFIG_ALLOW_CROSS="1",
            PKG_CONFIG_SYSROOT_DIR=sysroot.resolve().as_posix(),
            PKG_CONFIG_PATH=os.pathsep.join(
                str(sysroot.resolve() / path)
                for path in (
                    "usr/lib/x86_64-linux-gnu/pkgconfig",
                    "usr/share/pkgconfig",
                )
            ),
        )
    args = [
        "cargo",
        "zigbuild",
        "--locked",
        "--release",
        "--timings",
        "--target",
        target.value + (".2.39" if target == Target.linux else ""),
    ]
    binaries = ["codex", "codex-code-mode-host"] + (
        ["bwrap"] if target == Target.linux else []
    )
    for binary in binaries:
        args.extend(("--bin", binary))
    started = time.monotonic()
    try:
        subprocess.run(args, cwd=ROOT / "codex-rs", env=environment, check=True)
    finally:
        (output / "build-metadata.json").write_text(
            json.dumps(
                {
                    "source_commit": source,
                    "target": target.value,
                    "cargo_command_seconds": round(time.monotonic() - started, 3),
                },
                indent=2,
            )
            + "\n"
        )
        stats = subprocess.run(
            ["sccache", "--show-stats", "--stats-format=json"],
            capture_output=True,
            text=True,
            check=False,
        )
        if stats.returncode == 0:
            (output / "sccache.json").write_text(stats.stdout)
        timings = ROOT / "codex-rs/target/cargo-timings"
        if timings.is_dir():
            shutil.copytree(timings, output / "cargo-timings", dirs_exist_ok=True)
    for binary in binaries:
        shutil.copy2(
            ROOT / "codex-rs/target" / target.value / "release" / binary,
            output / binary,
        )


@app.command()
def package(
    target: Annotated[Target, typer.Option("--target", help="完整包目标")],
    input_dir: Annotated[
        Path,
        typer.Option(
            "--input",
            exists=True,
            file_okay=False,
            help="Windows job 上传的自有二进制目录",
        ),
    ],
    output: Annotated[Path, typer.Option("--output", help="完整包和 xz 归档目录")],
):
    """在 Linux 原样复用上游打包器，完成签名和目标平台验收。"""
    import release

    input_dir = input_dir.resolve()
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    package_dir = output / "package"
    version = tomllib.loads((ROOT / "codex-rs/Cargo.toml").read_text())["workspace"][
        "package"
    ]["version"]
    args = [
        sys.executable,
        "scripts/build_codex_package.py",
        "--target",
        target.value,
        "--package-version",
        version,
        "--package-dir",
        str(package_dir),
        "--entrypoint-bin",
        str(input_dir / "codex"),
        "--code-mode-host-bin",
        str(input_dir / "codex-code-mode-host"),
        "--strip",
        "all",
        "--strip-tool",
        "llvm-strip",
        "--force",
    ]
    if target == Target.linux:
        args.extend(("--bwrap-bin", str(input_dir / "bwrap")))
    run(*args)
    if target == Target.macos:
        files = [
            package_dir / "bin/codex",
            package_dir / "bin/codex-code-mode-host",
            package_dir / "codex-path/rg",
        ]
        run("scripts/fork/check-macho-libraries.sh", *map(str, files))
        for binary in files:
            args = ["rcodesign", "sign", "--code-signature-flags", "runtime"]
            entitlements = (
                ROOT
                / ".github/scripts/macos-signing"
                / f"{binary.name}.entitlements.plist"
            )
            if entitlements.is_file():
                args.extend(("--entitlements-xml-path", str(entitlements)))
            run(*args, str(binary))
    else:
        release.smoke(package_dir)
    archive = release.archive_package(package_dir, output)
    metadata = json.loads((input_dir / "build-metadata.json").read_text())
    (output / "package-metadata.json").write_text(
        json.dumps(
            {
                "target": target.value,
                "source_commit": metadata["source_commit"],
                "sha256": release.sha256(archive),
                "version": version,
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    app()
