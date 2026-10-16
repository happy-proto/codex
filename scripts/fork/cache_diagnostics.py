#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///
"""提取缓存键、命中结果和依赖摘要，不导出原始日志。"""

import hashlib
import json
import os
import re
import subprocess
import tomllib
from collections import Counter
from pathlib import Path
from typing import Annotated

import typer

app = typer.Typer(add_completion=False, pretty_exceptions_enable=False)


SELECTED = {
    "ts_rs",
    "codex_utils_absolute_path",
    "v8",
    "codex_code_mode",
    "codex_config",
    "codex_protocol",
    "codex_core",
    "codex_tui",
    "codex_cli",
}
KINDS = {
    "Unexpected",
    "Unsupported",
    "ConfigInvalid",
    "NotFound",
    "PermissionDenied",
    "AlreadyExists",
    "RateLimited",
    "IsADirectory",
    "NotADirectory",
    "ConditionNotMatch",
    "RangeNotSatisfied",
}
FLAGS = {
    "create_non_ok": "create cache entry returns non-ok",
    "already_exists": "already exists",
    "reserve_conflict": "unable to reserve",
    "rate_limit": "rate limit",
    "quota": "quota",
    "expired": "expired",
    "lookup_non_ok": "getcacheentrydownloadurl returns non-ok",
    "blob_not_found": "blobnotfound",
}


def file_digest(path: Path) -> dict:
    """通过标准库流式计算摘要，避免 Python 层的分块循环。"""
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256")
    return {"bytes": path.stat().st_size, "sha256": digest.hexdigest()}


def classify_error(text: str, known_keys: set[str], *, key_context: str = "") -> dict:
    text = text.strip()
    first = text.split(maxsplit=1)[0] if text else ""
    kind = first if first in KINDS else "unknown"
    if text in {"Cannot write to read-only storage", "storage is read-only"}:
        kind = "ReadOnly"
    status = re.search(r"\bstatus(?:_code)?\s*[:=]\s*([1-5][0-9]{2})\b", text)
    return {
        "kind": kind,
        "http_status": int(status[1]) if status else None,
        "flags": sorted(
            name for name, phrase in FLAGS.items() if phrase in text.lower()
        ),
        "keys": sorted(
            set(re.findall(r"\b[a-f0-9]{64}\b", text + " " + key_context)) & known_keys
        ),
    }


def codegen_options(arguments: list[str]) -> dict:
    allowed = {
        "opt-level",
        "codegen-units",
        "debuginfo",
        "debug-assertions",
        "lto",
        "strip",
    }
    result = {}
    values = iter(arguments)
    for argument in values:
        option = (
            next(values, "")
            if argument == "-C"
            else argument[2:]
            if argument.startswith("-C")
            else ""
        )
        name, separator, value = option.partition("=")
        if (
            name in allowed
            and separator
            and re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", value)
        ):
            result[name] = value
    return result


def dependency_fingerprints(arguments: list[str], cwd: Path) -> dict:
    result = {}
    for i, argument in enumerate(arguments):
        spec = (
            arguments[i + 1]
            if argument == "--extern" and i + 1 < len(arguments)
            else None
        )
        if argument.startswith("--extern="):
            spec = argument.removeprefix("--extern=")
        if not spec or "=" not in spec:
            continue
        name, filename = spec.split("=", 1)
        path = cwd / filename
        if path.is_file():
            result[name] = {"file": path.name, **file_digest(path)}
    return result


def summarize(log: Path, cwd: Path) -> dict:
    server_cache_mode = None
    records = {}
    current = {}
    pending_arguments = {}
    pending_hash_seconds = {}
    raw_errors = []
    raw_read_errors = []
    raw_storage_read_failures = []
    event_pattern = re.compile(r"\[([A-Za-z_][A-Za-z0-9_.-]*)\]: (.*)")
    with log.open(errors="replace") as stream:
        for line in stream:
            if mode := re.search(
                r"\bsccache::server\] server has setup with (ReadOnly|ReadWrite)\b",
                line,
            ):
                server_cache_mode = mode[1]
            stamp = re.match(r"^\[([0-9T:.,Z+-]+)\s", line)
            observed_at = stamp[1] if stamp else None
            if "Error executing cache write: " in line:
                raw_errors.append(
                    (
                        observed_at,
                        line.split("Error executing cache write: ", 1)[1].strip(),
                    )
                )
            if "sccache::cache::cache" in line and "Got unexpected error: " in line:
                raw_read_errors.append(
                    (
                        observed_at,
                        line.split("Got unexpected error: ", 1)[1].strip(),
                    )
                )
            if "opendal::services" in line and (
                failure := re.search(r": (?:read|Reader::read) failed (.*)", line)
            ):
                raw_storage_read_failures.append((observed_at, failure[1], line))
            match = event_pattern.search(line)
            if not match:
                continue
            crate, event = match.groups()
            if event.startswith("get_cached_or_compile: ") and crate in SELECTED:
                try:
                    pending_arguments[crate] = json.loads(event.split(": ", 1)[1])
                except json.JSONDecodeError:
                    pass
            elif hash_time := re.fullmatch(
                r"generate_hash_key took ([0-9.]+)\s*s", event
            ):
                pending_hash_seconds[crate] = float(hash_time[1])
            elif re.fullmatch(r"Hash key: [a-f0-9]{64}", event):
                key = event.removeprefix("Hash key: ")
                record = records.setdefault(key, {"crate": crate, "key": key})
                record["observed_at"] = observed_at
                record["hash_seconds"] = pending_hash_seconds.get(crate)
                current[crate] = record
                if crate in pending_arguments:
                    arguments = pending_arguments[crate]
                    record["arguments_sha256"] = hashlib.sha256(
                        json.dumps(arguments).encode()
                    ).hexdigest()
                    record["dependencies"] = dependency_fingerprints(arguments, cwd)
                    record["codegen_options"] = codegen_options(arguments)
            elif crate in current:
                record = current[crate]
                if lookup := re.fullmatch(r"Cache (hit|miss) in ([0-9.]+)\s*s", event):
                    record["lookup"] = lookup[1]
                    record["lookup_seconds"] = float(lookup[2])
                elif event == "Stored in cache successfully!":
                    record["stored"] = True
                    record["stored_at"] = observed_at
                elif artifact := re.fullmatch(
                    r"Created cache artifact in ([0-9.]+)\s*s", event
                ):
                    record["artifact_seconds"] = float(artifact[1])
                elif match_time := re.fullmatch(
                    r"Compiled in ([0-9.]+)\s*s, storing in cache", event
                ):
                    record["compile_seconds"] = float(match_time[1])
    errors = [
        classify_error(error, set(records)) | {"observed_at": stamp}
        for stamp, error in raw_errors
    ]
    read_errors = [
        classify_error(error, set(records)) | {"observed_at": stamp}
        for stamp, error in raw_read_errors
    ]
    storage_read_failures = [
        classify_error(error, set(records), key_context=context)
        | {"observed_at": stamp}
        for stamp, error, context in raw_storage_read_failures
    ]
    return {
        "schema_version": 1,
        "server_cache_mode": server_cache_mode,
        "units": list(records.values()),
        "write_errors": errors,
        "write_error_kinds": dict(Counter(error["kind"] for error in errors)),
        "backend_read_errors": read_errors,
        "backend_read_error_kinds": dict(
            Counter(error["kind"] for error in read_errors)
        ),
        "storage_read_failures": storage_read_failures,
        "storage_read_failure_kinds": dict(
            Counter(error["kind"] for error in storage_read_failures)
        ),
    }


@app.command()
def main(
    log: Annotated[
        Path, typer.Option("--log", help="sccache daemon 日志路径；允许缺失")
    ],
    cwd: Annotated[
        Path,
        typer.Option(
            "--cwd",
            exists=True,
            file_okay=False,
            help="Rust workspace 目录，用于读取 Cargo.toml 和 Git 信息",
        ),
    ],
    output: Annotated[Path, typer.Option("--output", help="诊断 JSON 输出路径")],
) -> None:
    """输出经过脱敏的缓存诊断与构建环境。"""
    report = summarize(log, cwd)
    report["environment"] = {
        name: os.environ.get(name)
        for name in (
            "RUNNER_OS",
            "RUNNER_ARCH",
            "ImageOS",
            "ImageVersion",
            "CARGO_INCREMENTAL",
            "CARGO_BUILD_JOBS",
            "CARGO_PROFILE_RELEASE_OPT_LEVEL",
            "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
            "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_PROFILE_RELEASE_DEBUG",
            "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO",
            "CARGO_PROFILE_CI_TEST_DEBUG",
            "CARGO_PROFILE_CI_TEST_SPLIT_DEBUGINFO",
            "CARGO_PROFILE_CI_TEST_CODEGEN_UNITS",
            "RUSTC_WRAPPER",
            "SCCACHE_GHA_RW_MODE",
            "SCCACHE_SKIP_CACHE_CHECK",
        )
    }
    report["rustc"] = subprocess.check_output(
        ["rustc", "-vV"], cwd=cwd, text=True
    ).strip()
    report["rust_source_tree"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD:codex-rs"], cwd=cwd, text=True
    ).strip()
    report["source_commit"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=cwd, text=True
    ).strip()
    report["workspace_version"] = tomllib.loads((cwd / "Cargo.toml").read_text())[
        "workspace"
    ]["package"]["version"]
    report["package_version"] = os.environ.get("CODEX_FORK_VERSION")
    output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    app()
