"""临时 runner 实验：下载已有 binary 包，对比 xz 单线程和自动并行。"""

import argparse
import hashlib
import json
import math
import os
import platform
import re
import resource
import statistics
import subprocess
import time
from pathlib import Path

REPO = "happy-proto/codex"


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def resolve(root):
    releases = json.loads(output("gh", "api", f"repos/{REPO}/releases?per_page=100"))
    candidates = []
    for release in releases:
        match = re.fullmatch(
            r"fork-v(\d+)\.(\d+)\.(\d+)-alpha\.(\d+)\.fork", release["tag_name"]
        )
        if match and not release["draft"]:
            candidates.append((tuple(map(int, match.groups())), release))
    release = max(candidates, key=lambda item: item[0])[1]
    tag = release["tag_name"]
    base = f"https://github.com/{REPO}/releases/download/{tag}"
    manifest = json.loads(output("curl", "-fLsS", f"{base}/fork-release.json"))
    sha = manifest["sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", sha):
        raise ValueError("发布清单 SHA-256 无效")
    asset = f"codex-package-aarch64-apple-darwin-{sha}.tar.gz"
    if manifest["asset"] != asset or manifest["tag"] != tag:
        raise ValueError("发布清单与 binary 包身份不一致")
    snapshot = {
        "manifest": manifest,
        "url": f"{base}/{asset}",
        "release_url": release["html_url"],
    }
    (root / "snapshot.json").write_text(json.dumps(snapshot, indent=2) + "\n")
    print(json.dumps(snapshot, indent=2), flush=True)


def prepare(root):
    snapshot = json.loads((root / "snapshot.json").read_text())
    archive = root / "package.tar.gz"
    subprocess.run(
        ["axel", "-n", "8", "-q", "-o", str(archive), snapshot["url"]], check=True
    )
    if digest(archive) != snapshot["manifest"]["sha256"]:
        raise ValueError("下载包的 SHA-256 不匹配")
    with (root / "package.tar").open("wb") as stream:
        subprocess.run(["gzip", "-dc", str(archive)], stdout=stream, check=True)
    print(output("tar", "-tvf", str(root / "package.tar")), flush=True)


def measure(root, threads):
    snapshot = json.loads((root / "snapshot.json").read_text())
    raw = root / "package.tar"
    expected = digest(raw)
    command = [
        "xz",
        "-9",
        f"-T{threads}",
        "--memlimit-compress=50%",
        "--no-adjust",
        "-vv",
        "-c",
        str(raw),
    ]
    environment = {
        "runner_os": os.environ.get("RUNNER_OS"),
        "image_version": os.environ.get("ImageVersion"),
        "platform": platform.platform(),
        "cpu": output("sysctl", "-n", "machdep.cpu.brand_string"),
        "logical_cpus": int(output("sysctl", "-n", "hw.logicalcpu")),
        "physical_cpus": int(output("sysctl", "-n", "hw.physicalcpu")),
        "ram_bytes": int(output("sysctl", "-n", "hw.memsize")),
        "xz_version": output("xz", "--version"),
        "xz_memory_info": output("xz", "--info-memory"),
    }
    print(json.dumps(environment, indent=2), flush=True)
    samples = []
    for repetition in range(1, 4):
        compressed = root / f"package-T{threads}.tar.xz"
        log = root / f"T{threads}-run{repetition}.log"
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        start = time.perf_counter()
        with compressed.open("wb") as stream, log.open("w") as stderr:
            subprocess.run(command, stdout=stream, stderr=stderr, check=True)
        elapsed = time.perf_counter() - start
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        cpu_seconds = (
            after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime
        )
        print(log.read_text(), flush=True)
        start = time.perf_counter()
        with subprocess.Popen(
            ["xz", "-dc", str(compressed)], stdout=subprocess.PIPE
        ) as process:
            actual = hashlib.file_digest(process.stdout, "sha256").hexdigest()
            process.stdout.close()
            if process.wait() != 0 or actual != expected:
                raise ValueError("解压内容与原始 tar 不一致")
        decompression_seconds = time.perf_counter() - start
        sample = {
            "repetition": repetition,
            "bytes": compressed.stat().st_size,
            "sha256": digest(compressed),
            "compression_seconds": elapsed,
            "cpu_seconds": cpu_seconds,
            "average_cpu_cores": cpu_seconds / elapsed,
            "decompression_and_hash_seconds": decompression_seconds,
            "xz_list": output("xz", "--robot", "--list", str(compressed)),
            "roundtrip_verified": True,
        }
        samples.append(sample)
        print(json.dumps(sample, indent=2), flush=True)
        compressed.unlink()
    if len({sample["sha256"] for sample in samples}) != 1:
        raise ValueError("三次压缩结果不一致")
    result = {
        "snapshot": snapshot,
        "environment": environment,
        "command": command,
        "threads": threads,
        "tar_bytes": raw.stat().st_size,
        "tar_sha256": expected,
        "gzip_bytes": (root / "package.tar.gz").stat().st_size,
        "default_multithread_block_bytes": 3 * 64 * 1024 * 1024,
        "default_multithread_block_count": math.ceil(
            raw.stat().st_size / (3 * 64 * 1024 * 1024)
        ),
        "samples": samples,
        "median_compression_seconds": statistics.median(
            sample["compression_seconds"] for sample in samples
        ),
    }
    (root / f"result-T{threads}.json").write_text(json.dumps(result, indent=2) + "\n")


def compare(root):
    single = json.loads((root / "result-T1.json").read_text())
    auto = json.loads((root / "result-T0.json").read_text())
    for key in ("snapshot", "tar_sha256", "tar_bytes", "gzip_bytes"):
        if single[key] != auto[key]:
            raise ValueError(f"两边输入不同：{key}")
    if single["environment"]["xz_version"] != auto["environment"]["xz_version"]:
        raise ValueError("两边 xz 版本不同")
    lines = [
        "# xz runner 压缩实验",
        "",
        f"Release: [{single['snapshot']['manifest']['tag']}]({single['snapshot']['release_url']})",
        f"Binary 包 SHA-256: `{single['snapshot']['manifest']['sha256']}`",
        f"未压缩 tar: {single['tar_bytes'] / 1e6:.2f} MB；原始 gzip: {single['gzip_bytes'] / 1e6:.2f} MB。",
        "",
        "仅重压缩已有 binary 包；下载、解 gzip 和校验不计入压缩耗时。两个独立 macos-15 job，各重复三次；MB 为十进制。",
        "",
        "| 模式 | 体积 MB | 原始/压缩体积 | 相对 gzip 节省 | 压缩秒（中位数） | 平均 CPU 核数（中位数） |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for result in (single, auto):
        size = result["samples"][0]["bytes"]
        cpu = statistics.median(
            sample["average_cpu_cores"] for sample in result["samples"]
        )
        lines.append(
            f"| -9 -T{result['threads']} | {size / 1e6:.2f} | {result['tar_bytes'] / size:.3f} | {1 - size / result['gzip_bytes']:.2%} | {result['median_compression_seconds']:.2f} | {cpu:.2f} |"
        )
    speedup = single["median_compression_seconds"] / auto["median_compression_seconds"]
    growth = auto["samples"][0]["bytes"] / single["samples"][0]["bytes"] - 1
    lines.extend(
        [
            "",
            f"T0 相对 T1 的加速比：{speedup:.2f}×；体积变化：{growth:+.2%}。",
            "",
            "所有样本解压 SHA-256 校验通过；三次压缩产物一致。",
        ]
    )
    for result in (single, auto):
        lines.extend(
            [
                "",
                f"## T{result['threads']} 环境与样本",
                "",
                "```json",
                json.dumps(result, indent=2),
                "```",
            ]
        )
    report = "\n".join(lines) + "\n"
    (root / "comparison.md").write_text(report)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a") as stream:
            stream.write(report)
    print(report, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "operation", choices=["resolve", "prepare", "measure", "compare"]
    )
    parser.add_argument("directory", type=Path)
    parser.add_argument("--threads", type=int, choices=[0, 1], default=1)
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    if args.operation == "measure":
        measure(args.directory, args.threads)
    else:
        {"resolve": resolve, "prepare": prepare, "compare": compare}[args.operation](
            args.directory
        )
