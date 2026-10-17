"""执行构建，并定期输出不包含原始命令行的 runner 资源与缓存快照。"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import time
from datetime import UTC, datetime
from pathlib import Path

COMPILERS = {
    "rustc",
    "cargo",
    "sccache",
    "clang",
    "clang++",
    "ld",
    "ld.lld",
    "lld",
    "rust-lld",
    "codesign",
}
COUNTERS = {
    "compile_requests",
    "requests_executed",
    "cache_hits",
    "cache_misses",
    "cache_read_errors",
    "cache_write_errors",
    "cache_writes",
    "compile_fails",
}


def compiler_processes(output: str) -> list[dict]:
    result = []
    for line in output.splitlines():
        fields = line.split(maxsplit=4)
        if len(fields) < 4:
            continue
        pid, cpu, rss, executable = fields[:4]
        name = Path(executable).name
        if name not in COMPILERS:
            continue
        try:
            record = {
                "pid": int(pid),
                "executable": name,
                "cpu_percent": float(cpu),
                "rss_mib": round(int(rss) / 1024, 1),
            }
        except ValueError:
            continue
        arguments = fields[4] if len(fields) == 5 else ""
        crate = re.search(
            r"(?:^|\s)--crate-name[= ]([A-Za-z_][A-Za-z0-9_]{0,127})(?:\s|$)", arguments
        )
        if name == "rustc" and crate:
            record["crate"] = crate[1]
        result.append(record)
    return sorted(result, key=lambda item: item["pid"])


def read_command(command: list[str]) -> str:
    return subprocess.run(
        command, capture_output=True, text=True, timeout=5, check=True
    ).stdout


def cache_counters(value: dict) -> dict:
    result = {}
    for name in COUNTERS:
        counter = value.get("stats", {}).get(name)
        if isinstance(counter, dict):
            counter = counter.get("counts", counter)
            if isinstance(counter, dict):
                counter = sum(item for item in counter.values() if type(item) is int)
        if type(counter) is int:
            result[name] = counter
    return result


def snapshot() -> dict:
    # 统计整个 runner 的编译进程；sccache daemon 的 rustc 不一定是 Cargo 的子进程。
    result = {
        "scope": "runner",
        "cpu_count": os.cpu_count(),
        "load_average": list(os.getloadavg()),
        "disk_free_gib": round(shutil.disk_usage(Path.cwd()).free / 1024**3, 2),
    }
    try:
        result["processes"] = compiler_processes(
            read_command(["ps", "-ww", "-axo", "pid=,pcpu=,rss=,ucomm=,args="])
        )
    except (OSError, subprocess.SubprocessError):
        result["process_sample_unavailable"] = True
    if shutil.which("vm_stat"):
        try:
            output = read_command(["vm_stat"])
            result["vm_pages"] = {
                name: int(value)
                for name, value in re.findall(
                    r"^(Pages free|Pages active|Pages inactive|Pages wired down|Pages occupied by compressor):\s+(\d+)\.",
                    output,
                    re.MULTILINE,
                )
            }
            size = re.search(r"page size of (\d+) bytes", output)
            if size:
                result["vm_page_bytes"] = int(size[1])
            swap = re.search(
                r"\bused\s*=\s*([0-9.]+)M",
                read_command(["sysctl", "-n", "vm.swapusage"]),
            )
            if swap:
                result["swap_used_mib"] = float(swap[1])
        except (OSError, subprocess.SubprocessError):
            result["memory_sample_unavailable"] = True
    elif Path("/proc/meminfo").is_file():
        try:
            result["memory_kib"] = {
                name: int(value)
                for name, value in re.findall(
                    r"^(MemAvailable|SwapTotal|SwapFree):\s+(\d+) kB",
                    Path("/proc/meminfo").read_text(),
                    re.MULTILINE,
                )
            }
        except OSError:
            result["memory_sample_unavailable"] = True
    if shutil.which("sccache"):
        try:
            result["sccache"] = cache_counters(
                json.loads(
                    read_command(["sccache", "--show-stats", "--stats-format=json"])
                )
            )
        except (
            OSError,
            subprocess.SubprocessError,
            ValueError,
            AttributeError,
            TypeError,
        ):
            result["sccache_sample_unavailable"] = True
    return result


def observe(command: list[str], output: Path, interval: float) -> int:
    started = time.monotonic()
    try:
        output.parent.mkdir(parents=True, exist_ok=True)
        stream = output.open("a")
    except OSError:
        stream = None
    child = subprocess.Popen(command, start_new_session=True)
    received_signal = None

    def forward(signum, _frame):
        nonlocal received_signal
        received_signal = signum
        try:
            os.killpg(child.pid, signum)
        except ProcessLookupError:
            pass

    previous = {
        number: signal.signal(number, forward)
        for number in (signal.SIGINT, signal.SIGTERM)
    }
    try:
        try:
            while True:
                try:
                    record = snapshot()
                except Exception:  # noqa: BLE001 - 采样必须 fail open，不能替换构建结果。
                    # 观测失败不能覆盖构建结果，也不输出可能含环境信息的异常文本。
                    record = {"sample_unavailable": True}
                record.update(
                    event="fork-build-observation",
                    observed_at=datetime.now(UTC).isoformat(),
                    elapsed_seconds=round(time.monotonic() - started, 1),
                )
                code = child.poll()
                if code is not None:
                    record["exit_code"] = code
                line = json.dumps(record, sort_keys=True)
                try:
                    if stream:
                        stream.write(line + "\n")
                        stream.flush()
                except OSError:
                    pass
                try:
                    print(line, flush=True)
                except OSError:
                    pass
                if code is not None:
                    return (
                        128 + received_signal
                        if received_signal
                        else (128 - code if code < 0 else code)
                    )
                try:
                    child.wait(timeout=interval)
                except subprocess.TimeoutExpired:
                    continue
        finally:
            if stream:
                try:
                    stream.close()
                except OSError:
                    pass
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--interval", type=float, default=60)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or args.interval <= 0:
        parser.error("需要构建命令和大于零的采样间隔")
    raise SystemExit(observe(command, args.output, args.interval))


if __name__ == "__main__":
    main()
