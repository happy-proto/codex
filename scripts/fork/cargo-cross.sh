#!/bin/sh
# 将上游 package builder 的 Cargo build 调用交给 macOS 交叉链接器。
set -eu
test "$1" = build
shift
exec cargo zigbuild --locked "$@" --timings
