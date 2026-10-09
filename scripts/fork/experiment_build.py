"""临时对照实验：大型 crate 的代码生成单元数，不发布更新。"""

import json
import os
import shutil
import subprocess
import time

import release

ROOT = release.ROOT
RESULTS = ROOT / "experiment-results"
PACKAGES = {
    "codex-core": "core",
    "codex-tui": "tui",
    "codex-app-server": "app-server",
    "codex-exec": "exec",
    "codex-cli": "cli",
}


def main():
    units = int(os.environ["EXPERIMENT_CODEGEN_UNITS"])
    RESULTS.mkdir(exist_ok=True)
    config = ROOT / "codex-rs/.cargo/config.toml"
    with config.open("a") as output:
        for name in PACKAGES:
            output.write(
                f'\n[profile.release.package."{name}"]\ncodegen-units = {units}\n'
            )
    hardware = {
        key: subprocess.check_output(["sysctl", "-n", key], text=True).strip()
        for key in ["hw.ncpu", "hw.memsize", "machdep.cpu.brand_string"]
    }
    metrics = {"codegen_units": units, "hardware": hardware, "phases": []}
    (RESULTS / "metadata.json").write_text(json.dumps(metrics, indent=2))
    for phase in ["prime", "changed"]:
        # 改动相同的无运行副作用常量，避免完全相同源码的缓存命中掩盖重编译成本。
        value = 0 if phase == "prime" else 1
        for folder in PACKAGES.values():
            path = ROOT / f"codex-rs/{folder}/src/lib.rs"
            text = path.read_text()
            marker = "\n#[doc(hidden)]\npub const CODEX_BUILD_TIMING_PROBE: u8 = "
            text = text.split(marker)[0]
            path.write_text(text + marker + f"{value};\n")
        directory = RESULTS / phase
        directory.mkdir()
        before = json.loads(release.output("mbx", "stats", "--json"))
        started = time.monotonic()
        # 沿用完整包构建、签名、CLI/helper/rg/app-server 验收，不调用 publish。
        command = [
            "/usr/bin/time",
            "-l",
            "python3",
            "-u",
            "scripts/fork/release.py",
            "build",
        ]
        with (directory / "resource-usage.txt").open("w") as resource:
            subprocess.run(command, cwd=ROOT, stderr=resource, check=True)
        elapsed = time.monotonic() - started
        after = json.loads(release.output("mbx", "stats", "--json"))
        data = {
            "phase": phase,
            "elapsed_seconds": elapsed,
            "mbx_before": before,
            "mbx_after": after,
        }
        metrics["phases"].append(data)
        (RESULTS / "metadata.json").write_text(json.dumps(metrics, indent=2))
        shutil.copytree(
            ROOT / "codex-rs/target/cargo-timings", directory / "cargo-timings"
        )
        sizes = {
            name: (ROOT / "fork-dist/package/bin" / name).stat().st_size
            for name in ["codex", "codex-code-mode-host"]
        }
        (directory / "binary-sizes.json").write_text(json.dumps(sizes, indent=2))
        subprocess.run(
            [
                "python3",
                "-m",
                "unittest",
                "discover",
                "-s",
                "scripts/fork",
                "-p",
                "test_terminal_cwd.py",
                "-v",
            ],
            cwd=ROOT,
            env={
                **os.environ,
                "CODEX_FORK_TEST_BINARY": str(ROOT / "fork-dist/package/bin/codex"),
            },
            check=True,
        )
        print(f"{phase}: {elapsed:.1f}s; binary sizes: {sizes}", flush=True)
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(f"CGU {units}, {phase}: {elapsed:.1f}s; sizes: {sizes}\n\n")


if __name__ == "__main__":
    main()
