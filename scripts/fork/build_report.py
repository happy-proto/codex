#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "typer>=0.27.3",
# ]
# ///
"""把缓存、资源和 Cargo timing 明细汇总到 artifact 与 Actions job summary。"""

import json
import os
from collections import defaultdict
from pathlib import Path
from typing import Annotated

import typer
from build_observer import cache_counters

app = typer.Typer(add_completion=False, pretty_exceptions_enable=False)


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text())
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def resource_summary(path: Path) -> dict:
    durations = defaultdict(float)
    rss_samples, swap_samples, compressor_samples = [], [], []
    available = []
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return {}
    for line in lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if not isinstance(row, dict):
            continue
        elapsed = row.get("elapsed_seconds")
        if isinstance(elapsed, (int, float)):
            identity = row.get("invocation_id", "legacy")
            durations[identity] = max(durations[identity], elapsed)
        if "processes" in row:
            rss_samples.append(sum(item.get("rss_mib", 0) for item in row["processes"]))
        memory = row.get("memory_kib", {})
        if "MemAvailable" in memory:
            available.append(memory["MemAvailable"] / 1024)
        if "swap_used_mib" in row:
            swap_samples.append(row["swap_used_mib"])
        if "SwapTotal" in memory and "SwapFree" in memory:
            swap_samples.append((memory["SwapTotal"] - memory["SwapFree"]) / 1024)
        if "vm_pages" in row and "vm_page_bytes" in row:
            compressor_samples.append(
                row["vm_pages"].get("Pages occupied by compressor", 0)
                * row["vm_page_bytes"]
                / 1024**2
            )
    return {
        "invocations": len(durations),
        "observed_command_seconds": round(sum(durations.values()), 1)
        if durations
        else None,
        "sampled_peak_compiler_rss_sum_mib": round(max(rss_samples), 1)
        if rss_samples
        else None,
        "sampled_peak_swap_mib": round(max(swap_samples), 1) if swap_samples else None,
        "sampled_peak_compressor_mib": round(max(compressor_samples), 1)
        if compressor_samples
        else None,
        "sampled_min_available_memory_mib": round(min(available), 1)
        if available
        else None,
    }


def slowest_units(directory: Path) -> list[dict]:
    totals = {}
    for path in directory.glob("cargo-timing-*.html"):
        try:
            text = path.read_text()
            marker = "const UNIT_DATA = "
            if marker not in text:
                continue
            units, _ = json.JSONDecoder().raw_decode(text.split(marker, 1)[1])
        except (OSError, ValueError):
            continue
        for unit in units:
            duration = unit.get("duration", 0)
            if duration <= 0:
                continue
            identity = (unit["name"], unit.get("target", ""))
            record = totals.setdefault(
                identity,
                {
                    "name": identity[0],
                    "target": identity[1],
                    "seconds": 0,
                    "executions": 0,
                },
            )
            record["seconds"] += duration
            record["executions"] += 1
    return [
        {**row, "seconds": round(row["seconds"], 2)}
        for row in sorted(
            totals.values(), key=lambda row: row["seconds"], reverse=True
        )[:10]
    ]


def markdown(report: dict) -> str:
    cache = report["cache"]
    resources = report["resources"]
    lines = ["### 构建诊断", "", "| 指标 | 数值 |", "| --- | --- |"]
    metrics = {
        "编译缓存命中": cache.get("cache_hits"),
        "编译缓存未命中": cache.get("cache_misses"),
        "不可缓存请求": cache.get("requests_not_cacheable"),
        "观察命令累计耗时（秒）": resources.get("observed_command_seconds"),
        "采样峰值编译进程 RSS 合计（MiB）": resources.get(
            "sampled_peak_compiler_rss_sum_mib"
        ),
        "采样峰值 swap（MiB）": resources.get("sampled_peak_swap_mib"),
        "采样峰值 macOS 压缩内存（MiB）": resources.get("sampled_peak_compressor_mib"),
    }
    lines.extend(
        f"| {name} | {value if value is not None else '—'} |"
        for name, value in metrics.items()
    )
    lines += [
        "",
        f"后端读取错误：`{json.dumps(report['backend_read_error_kinds'], sort_keys=True)}`。",
        "",
        "下表累计各次命令的单元时长；单元可以并行，不能相加作为 job 总耗时。",
        "",
        "| Cargo 单元 | 累计秒数 | 执行次数 |",
        "| --- | --- | --- |",
    ]
    for unit in report["slowest_units"]:
        name = (
            (unit["name"] + " " + unit["target"])
            .strip()
            .replace("|", "\\|")
            .replace("\n", " ")
        )
        lines.append(f"| {name} | {unit['seconds']} | {unit['executions']} |")
    return "\n".join(lines) + "\n"


@app.command()
def main(
    directory: Annotated[
        Path,
        typer.Option(
            "--directory",
            exists=True,
            file_okay=False,
            help="诊断 artifact 目录；在此写入 report.json",
        ),
    ],
    stats: Annotated[
        Path, typer.Option("--stats", help="sccache JSON 路径；相对路径基于 directory")
    ],
    timings: Annotated[
        Path, typer.Option("--timings", help="Cargo timing 报告目录；允许缺失")
    ],
) -> None:
    """生成构建汇总，并追加到 GITHUB_STEP_SUMMARY（如已设置）。"""
    try:
        diagnostics = read_json(directory / "diagnostics.json")
        report = {
            "schema_version": 1,
            "cache": cache_counters(read_json(directory / stats)),
            "backend_read_error_kinds": diagnostics.get("backend_read_error_kinds", {}),
            "resources": resource_summary(directory / "progress.ndjson"),
            "slowest_units": slowest_units(timings),
        }
        (directory / "report.json").write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        )
        if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
            with Path(summary).open("a") as stream:
                stream.write(markdown(report))
    except Exception:  # noqa: BLE001 - 诊断失败不改变构建结果，也不输出原始异常。
        print("构建汇总不可用；保留已有诊断 artifact。")


if __name__ == "__main__":
    app()
