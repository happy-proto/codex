#!/bin/sh
# 观察 CI 测试命令的资源使用，保留 Cargo 参数、工作目录和退出状态。
set -eu
fork_script_dir=$(dirname "$0")
exec uv run --script "$fork_script_dir/build_observer.py" \
  --output "$fork_script_dir/../../fork-sccache/progress.ndjson" -- cargo "$@"
