#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///
"""只重编译 ts-rs，对照上游确定性修复；不构建 Codex 或修改仓库依赖。"""

import hashlib
import json
import os
import shutil
import subprocess
import time
import tomllib
from pathlib import Path
from typing import Annotated

import typer

app = typer.Typer(add_completion=False, pretty_exceptions_enable=False)


UPSTREAM_FIX = "https://github.com/Aleph-Alpha/ts-rs/pull/488"
TS_FEATURES = ("uuid-impl", "serde-json-impl", "no-serde-warnings")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], env: dict, log: Path) -> str:
    started = time.monotonic()
    result = subprocess.run(
        command, env=env, capture_output=True, text=True, check=False
    )
    log.write_text(result.stdout + result.stderr)
    print(
        json.dumps(
            {
                "step": log.stem,
                "seconds": round(time.monotonic() - started, 3),
                "exit_code": result.returncode,
            }
        ),
        flush=True,
    )
    result.check_returncode()
    return result.stdout


def patch_macro(directory: Path) -> None:
    """将 #488 的两个源文件改动应用到隔离的 11.1.0 副本。"""
    deps = directory / "src/deps.rs"
    text = deps.read_text()
    old = "use std::{collections::HashSet, rc::Rc};"
    if old not in text or text.count("HashSet") != 5:
        raise ValueError("ts-rs-macros deps.rs 与预期基线不符")
    deps.write_text(
        text.replace(old, "use std::rc::Rc;\n\nuse indexmap::IndexSet;").replace(
            "HashSet", "IndexSet"
        )
    )
    lib = directory / "src/lib.rs"
    text = lib.read_text()
    old = "use std::collections::{HashMap, HashSet};"
    if old not in text or text.count("HashSet") != 3:
        raise ValueError("ts-rs-macros lib.rs 与预期基线不符")
    lib.write_text(
        text.replace(
            old, "use std::collections::HashMap;\n\nuse indexmap::IndexSet;"
        ).replace("HashSet", "IndexSet")
    )


@app.command()
def main(
    output: Annotated[
        Path, typer.Option("--output", help="隔离实验输出目录；必须尚不存在")
    ],
    target: Annotated[
        str, typer.Option("--target", help="传给 Cargo 的 Rust target triple")
    ] = "aarch64-apple-darwin",
    patched_macros: Annotated[
        Path | None,
        typer.Option(
            "--patched-macros",
            exists=True,
            file_okay=False,
            help="实际维护的宏源码目录；省略则在副本应用上游修复",
        ),
    ] = None,
) -> None:
    """对照 ts-rs 宏修复前后的产物确定性；执行隔离的 Cargo 编译。"""
    root = output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    repository = Path(__file__).resolve().parents[2]
    lock_path = repository / "codex-rs/Cargo.lock"
    lock = tomllib.loads(lock_path.read_text())
    pinned = {(item["name"], item["version"]) for item in lock["package"]}
    versions = {
        item["name"]: item["version"]
        for item in lock["package"]
        if item["name"] in {"ts-rs", "ts-rs-macros"}
    }
    if versions != {"ts-rs": "11.1.0", "ts-rs-macros": "11.1.0"}:
        raise ValueError("此复现仅对应 fork 当前使用的 ts-rs 11.1.0")
    release = tomllib.loads((repository / "codex-rs/Cargo.toml").read_text())[
        "profile"
    ]["release"]
    profile = {
        "CARGO_PROFILE_RELEASE_LTO": "false",
        "CARGO_PROFILE_RELEASE_DEBUG": "0",
        "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO": "off",
        "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": str(release["codegen-units"]),
        "CARGO_PROFILE_RELEASE_OPT_LEVEL": str(release.get("opt-level", 3)),
        "CARGO_PROFILE_RELEASE_STRIP": str(release["strip"]).lower(),
        "CARGO_PROFILE_RELEASE_PANIC": release.get("panic", "unwind"),
    }
    env = dict(os.environ)
    # 必须真正执行 rustc，不能把恢复出的同一个缓存对象误当成可复现构建。
    for name in (
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
    ):
        env.pop(name, None)
    env.update(
        CARGO_TARGET_DIR=str(root / "target"),
        CARGO_INCREMENTAL="0",
        CARGO_BUILD_JOBS="3",
        **profile,
    )
    fixture = root / "fixture"
    run(
        [
            "cargo",
            "init",
            "--lib",
            "--vcs",
            "none",
            "--name",
            "ts_rs_cache_probe",
            str(fixture),
        ],
        env,
        root / "init.log",
    )
    # Cargo 为 fixture 裁剪锁文件，但保留工作区已有的兼容传递版本。
    shutil.copyfile(lock_path, fixture / "Cargo.lock")
    run(
        [
            "cargo",
            "add",
            "--manifest-path",
            str(fixture / "Cargo.toml"),
            "ts-rs@=11.1.0",
            "--features",
            ",".join(TS_FEATURES),
        ],
        env,
        root / "add.log",
    )
    metadata = json.loads(
        run(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                str(fixture / "Cargo.toml"),
                "--format-version",
                "1",
                "--locked",
            ],
            env,
            root / "metadata.log",
        )
    )
    macro = next(
        item for item in metadata["packages"] if item["name"] == "ts-rs-macros"
    )
    if macro["version"] != "11.1.0":
        raise ValueError("宏依赖版本未匹配仓库锁文件")
    vendor = root / "patched-macros"
    if patched_macros:
        shutil.copytree(
            patched_macros.resolve(),
            vendor,
            ignore=shutil.ignore_patterns("target", "Cargo.lock"),
        )
    else:
        shutil.copytree(Path(macro["manifest_path"]).parent, vendor)
        patch_macro(vendor)
        run(
            [
                "cargo",
                "add",
                "--manifest-path",
                str(vendor / "Cargo.toml"),
                "indexmap@2",
            ],
            env,
            root / "add-indexmap.log",
        )
    source_hashes = {
        str(path.relative_to(vendor)): digest(path)
        for path in sorted(vendor.rglob("*"))
        if path.is_file() and (path.suffix == ".rs" or path.name == "Cargo.toml")
    }
    (root / "patched-source.json").write_text(
        json.dumps(source_hashes, indent=2) + "\n"
    )
    results = []
    dependency_versions = {}
    for case in ("baseline", "patched"):
        if case == "patched":
            # 原版会裁剪未使用的 indexmap；重新提供工作区 pin 给修复组解析。
            shutil.copyfile(lock_path, fixture / "Cargo.lock")
        config = (
            []
            if case == "baseline"
            else ["--config", f'patch.crates-io.ts-rs-macros.path="{vendor}"']
        )
        for attempt in range(1, 4):
            common = [
                "--manifest-path",
                str(fixture / "Cargo.toml"),
                "--release",
                "--target",
                target,
                *config,
            ]
            run(
                ["cargo", "clean", *common, "-p", "ts-rs"],
                env,
                root / f"{case}-{attempt}-clean.log",
            )
            output = run(
                ["cargo", "build", *common, "--locked", "--message-format=json"],
                env,
                root / f"{case}-{attempt}-build.log",
            )
            if attempt == 1:
                resolved = tomllib.loads((fixture / "Cargo.lock").read_text())
                selected = {
                    (item["name"], item["version"])
                    for item in resolved["package"]
                    if item["name"] != "ts_rs_cache_probe"
                }
                if selected - pinned:
                    raise ValueError(
                        f"fixture 使用了工作区锁文件以外的版本：{sorted(selected - pinned)}"
                    )
                dependency_versions[case] = sorted(selected)
                (root / "dependency-versions.json").write_text(
                    json.dumps(dependency_versions, indent=2) + "\n"
                )
            artifacts = []
            for line in output.splitlines():
                item = json.loads(line)
                if item.get("reason") == "compiler-artifact":
                    artifacts.append(item)
            library = next(
                item for item in artifacts if item["target"]["name"] == "ts_rs"
            )
            if library["fresh"]:
                raise ValueError("ts-rs 没有重新编译，不能作为可复现构建证据")
            rmeta = next(
                path
                for name in library["filenames"]
                if (path := Path(name)).suffix == ".rmeta"
            )
            snapshot = root / f"{case}-{attempt}.rmeta"
            shutil.copyfile(rmeta, snapshot)
            dependencies = {
                Path(name).name: digest(Path(name))
                for item in artifacts
                if item["target"]["name"] != "ts_rs"
                and item["target"]["name"] != "ts_rs_cache_probe"
                for name in item["filenames"]
                if Path(name).suffix in {".rlib", ".rmeta", ".dylib", ".so"}
            }
            result = {
                "case": case,
                "attempt": attempt,
                "metadata_sha256": digest(snapshot),
                "metadata_bytes": snapshot.stat().st_size,
                "dependencies": dependencies,
            }
            results.append(result)
            (root / "results.json").write_text(json.dumps(results, indent=2) + "\n")
            print(
                json.dumps(
                    {
                        key: value
                        for key, value in result.items()
                        if key != "dependencies"
                    }
                ),
                flush=True,
            )
    summary = {
        "upstream_fix": UPSTREAM_FIX,
        "target": target,
        "workspace_lock_sha256": digest(lock_path),
        "release_profile": profile,
        "ts_rs_features": TS_FEATURES,
        "cases": {},
    }
    for case in ("baseline", "patched"):
        group = [item for item in results if item["case"] == case]
        summary["cases"][case] = {
            "distinct_metadata": len({item["metadata_sha256"] for item in group}),
            "same_dependency_bytes": all(
                item["dependencies"] == group[0]["dependencies"] for item in group
            ),
        }
    (root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary), flush=True)
    assert all(item["same_dependency_bytes"] for item in summary["cases"].values()), (
        "依赖输入变化，复现结果无效"
    )
    assert summary["cases"]["baseline"]["distinct_metadata"] > 1, (
        "未复现，不能认定宏顺序为原因"
    )
    assert summary["cases"]["patched"]["distinct_metadata"] == 1, "修复后产物仍不稳定"


if __name__ == "__main__":
    app()
