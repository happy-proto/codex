#!/bin/sh
# 通过上游 package builder 的 --cargo 接口记录构建耗时，保留参数和退出状态。
set -eu
exec cargo "$@" --timings
