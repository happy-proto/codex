#!/bin/sh
# 检查 macOS 程序的动态库加载命令，阻止可链接但被 dyld 拒绝启动的包。
set -eu
export LC_ALL=C
if [ "$#" -eq 0 ]; then
  echo 'Usage: check-macho-libraries.sh <Mach-O binary> [...]' >&2
  exit 2
fi
if command -v llvm-objdump >/dev/null 2>&1; then
  tool=llvm-objdump
elif command -v otool >/dev/null 2>&1; then
  tool=otool
else
  echo '需要 llvm-objdump 或 otool' >&2
  exit 2
fi
for binary in "$@"; do
  case $(file -b "$binary") in
    'Mach-O 64-bit arm64 executable'* | 'Mach-O 64-bit executable arm64'*) ;;
    *)
      echo "需要 macOS ARM64 可执行文件：$binary" >&2
      exit 1
      ;;
  esac
  if [ "$tool" = llvm-objdump ]; then
    listing=$("$tool" --macho --dylibs-used "$binary")
  else
    listing=$("$tool" -L "$binary")
  fi
  libraries=$(printf '%s\n' "$listing" | sed -n 's/^[[:space:]]*\(.*\) (compatibility version.*/\1/p')
  if [ -z "$libraries" ]; then
    echo "未解析到动态库依赖：$binary" >&2
    exit 1
  fi
  duplicates=$(printf '%s\n' "$libraries" | sort | uniq -d)
  if [ -n "$duplicates" ]; then
    printf '重复的动态库加载命令：%s\n%s\n' "$binary" "$duplicates" >&2
    exit 1
  fi
  echo "动态库加载命令校验通过：$binary"
done
