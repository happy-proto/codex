#!/bin/sh
# Install the personal fork. macOS supplies plutil; no Python or jq is required.
set -eu

REPO=happy-proto/codex
API="https://api.github.com/repos/$REPO"
DOWNLOAD="https://github.com/$REPO/releases/download"
CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
ROOT="$CODEX_HOME_DIR/packages/standalone"
BIN_DIR="${CODEX_INSTALL_DIR:-$HOME/.local/bin}"
release="${CODEX_RELEASE:-latest}"
rollback=false
case "${1:-}" in
  --release) release="$2"; shift 2 ;;
  --rollback) rollback=true; shift ;;
  --help)
    echo 'Usage: install.sh [--release VERSION | --rollback]'
    echo 'CODEX_HOME and CODEX_INSTALL_DIR select the installation directories.'
    exit 0 ;;
esac
[ "$#" = 0 ] || { echo 'Unexpected installer argument.' >&2; exit 1; }
platform="$(uname -s)"
case "$platform:$(uname -m)" in
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  Darwin:x86_64)
    [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ] || exit 1
    target=aarch64-apple-darwin ;;
  Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-gnu ;;
  *) echo 'Supported platforms: macOS Apple Silicon and Linux AMD64 (glibc).' >&2; exit 1 ;;
esac
if [ "$platform" = Darwin ]; then command -v plutil >/dev/null
else command -v python3 >/dev/null; command -v flock >/dev/null; fi
mkdir -p "$ROOT/releases" "$BIN_DIR"
ROOT="$(cd -P "$ROOT" && pwd)"
tmp="$(mktemp -d)"
locked=false
cleanup() {
  rm -rf "$tmp"
  if [ "$locked" = true ]; then rmdir "$ROOT/fork-install.lock"; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# Refuse simultaneous installation rather than replacing a selected package.
[ ! -e "$ROOT/install.lock.d" ] || { echo 'An official installer is running.' >&2; exit 1; }
# Share the official macOS installer's advisory lock across both distributions.
exec 9<>"$ROOT/install.lock"
if [ "$platform" = Darwin ]; then
  lockf -t 0 9 || { echo 'Another installer is running.' >&2; exit 1; }
else
  flock -n 9 || { echo 'Another installer is running.' >&2; exit 1; }
fi
mkdir "$ROOT/fork-install.lock" || { echo 'Another fork installer is running.' >&2; exit 1; }
locked=true

download() {
  if [ "${3:-}" = package ] && command -v axel >/dev/null 2>&1; then
    echo '  Using axel…'
    axel_progress=-q
    if [ -t 2 ]; then axel_progress=-a; fi
    if axel "$axel_progress" -T 15 -o "$2" "$1" >&2; then
      return 0
    fi
    echo 'Axel download failed; retrying with curl.' >&2
    # 不把 axel 的残留文件或续传状态交给另一个下载器。
    rm -f "$2" "$2.st"
  fi
  progress=-s
  # 仅安装包在交互终端显示单行进度；元数据及日志输出保持简洁。
  if [ "${3:-}" = package ] && [ -t 2 ]; then progress=--progress-bar; fi
  curl -fSL "$progress" --retry 3 --connect-timeout 15 --max-time 600 "$1" -o "$2"
}
download_api() {
  # 优先复用 gh 的 GitHub.com 登录；未安装或未登录时允许匿名安装。
  token=''
  if command -v gh >/dev/null 2>&1; then
    token="$(gh auth token --hostname github.com 2>/dev/null)" || token=''
  fi
  if [ -n "$token" ]; then
    # 凭据写入私有临时文件，不出现在 curl 参数或输出中。
    (umask 077; printf 'Authorization: Bearer %s\n' "$token" > "$tmp/github-api.headers")
    unset token
    # API 请求不跟随重定向，认证不会传给 Release 下载地址。
    curl -fsS --retry 3 --connect-timeout 15 --max-time 600 \
      --header "@$tmp/github-api.headers" "$1" -o "$2"
  else
    unset token
    curl -fsS --retry 3 --connect-timeout 15 --max-time 600 "$1" -o "$2"
  fi
}
extract() {
  if [ "$platform" = Darwin ]; then
    plutil -extract "$1" raw -o - "$2"
  else
    python3 - "$1" "$2" <<'PYJSON'
import json, sys
value = json.load(open(sys.argv[2]))
for key in sys.argv[1].split("."):
    value = value[int(key)] if isinstance(value, list) else value[key]
print(str(value).lower() if isinstance(value, bool) else value)
PYJSON
  fi
}
link() {
  ln -s "$1" "$2.tmp.$$"
  if [ "$platform" = Darwin ]; then mv -fh "$2.tmp.$$" "$2"
  else mv -fT "$2.tmp.$$" "$2"; fi
}
select_package() {
  # Remove the official latest-channel marker: this installation is manually updated.
  rm -f "$ROOT/auto-update-version"
  # A cached previous commit must not offer a stale update after selecting a new build.
  rm -f "$CODEX_HOME_DIR/fork-version.json"
  link "$1" "$ROOT/current"
  link "$ROOT/current/bin/codex" "$BIN_DIR/codex"
  link "$ROOT/current/bin/codex-code-mode-host" "$BIN_DIR/codex-code-mode-host"
}
check_path() {
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf 'Add %s to PATH to use codex.\n' "$BIN_DIR" ;;
  esac
}

if [ "$rollback" = true ]; then
  previous="$(cd -P "$ROOT/fork-previous" && pwd)"
  "$previous/bin/codex" --version
  select_package "$previous"
  echo 'Previous CLI selected; configuration and sessions were preserved.'
  exit 0
fi

echo '  Checking release…'
if [ "$release" = latest ]; then
  download_api "$API/releases?per_page=100" "$tmp/releases.json"
  i=0
  tag=''
  while candidate="$(extract "$i.tag_name" "$tmp/releases.json" 2>/dev/null)"; do
    case "$candidate" in
      fork-v*-alpha.*.fork|fork-v*-alpha.*.fork.[0-9]*)
        if [ "$(extract "$i.draft" "$tmp/releases.json")" = false ]; then
          tag="$candidate"
          break
        fi ;;
    esac
    i=$((i + 1))
  done
  [ -n "$tag" ] || { echo 'No published fork alpha release.' >&2; exit 1; }
else
  tag="fork-v${release#fork-v}"
fi
case "$tag" in
  fork-v*-alpha.*.fork|fork-v*-alpha.*.fork.[0-9]*) ;;
  *) echo 'Expected a fork alpha version.' >&2; exit 1 ;;
esac
case "$tag" in *[!a-zA-Z0-9.-]*) exit 1 ;; esac
download "$DOWNLOAD/$tag/fork-release.json" "$tmp/manifest.json"
version="$(extract version "$tmp/manifest.json")"
commit="$(extract source_commit "$tmp/manifest.json")"
if extract "packages.$target.asset" "$tmp/manifest.json" >/dev/null 2>&1; then
  digest="$(extract "packages.$target.sha256" "$tmp/manifest.json")"
  asset="$(extract "packages.$target.asset" "$tmp/manifest.json")"
else
  [ "$target" = aarch64-apple-darwin ] || { echo 'Release has no Linux package.' >&2; exit 1; }
  digest="$(extract sha256 "$tmp/manifest.json")"
  asset="$(extract asset "$tmp/manifest.json")"
fi
[ "fork-v$version" = "$tag" ] || { echo 'Release version mismatch.' >&2; exit 1; }
[ "${#commit}" = 40 ] && [ "${#digest}" = 64 ] || exit 1
case "$commit$digest" in *[!0-9a-f]*) exit 1 ;; esac
case "$asset" in
  "codex-package-$target.tar.xz"|"codex-package-$target-$digest.tar.xz"|"codex-package-$target-$digest.tar.gz") ;;
  *) exit 1 ;;
esac
name="$version-$commit-$digest-$target"
destination="$ROOT/releases/$name"
short_commit="$(printf '%.8s' "$commit")"

# 完整构建标识相同才算最新；仍验证已安装的 CLI 并修复入口链接。
if [ -L "$ROOT/current" ] && [ -f "$destination/fork-release.json" ] &&
  [ "$(cd -P "$ROOT/current" && pwd)" = "$destination" ]; then
  [ "$("$destination/bin/codex" --version)" = "codex-cli $version" ]
  select_package "$destination"
  printf '\nAlready up to date: %s (%s).\n' "$version" "$short_commit"
  check_path
  exit 0
fi

if [ ! -f "$destination/fork-release.json" ]; then
  printf '  Downloading %s…\n' "$version"
  download "$DOWNLOAD/$tag/$asset" "$tmp/package.tar" package || {
    echo 'Package download failed; current CLI unchanged.' >&2; exit 1;
  }
  echo '  Verifying and installing…'
  actual="$(shasum -a 256 "$tmp/package.tar" | awk '{print $1}')"
  [ "$actual" = "$digest" ] || { echo 'Package checksum mismatch; current CLI unchanged.' >&2; exit 1; }
  # Only the repository's canonical relative-path package layout is accepted.
  tar -tf "$tmp/package.tar" > "$tmp/entries"
  if LC_ALL=C awk '/^\// || /(^|\/)\.\.(\/|$)/ {bad=1} END {exit !bad}' "$tmp/entries"; then
    echo 'Unsafe archive paths.' >&2; exit 1
  fi
  stage="$ROOT/releases/.fork-staging.$$"
  mkdir "$stage"
  tar -xf "$tmp/package.tar" -C "$stage"
  [ "$(extract version "$stage/codex-package.json")" = "$version" ]
  [ "$(extract target "$stage/codex-package.json")" = "$target" ]
  [ -x "$stage/bin/codex-code-mode-host" ]
  [ -x "$stage/codex-path/rg" ]
  if [ "$platform" = Linux ]; then [ -x "$stage/codex-resources/bwrap" ]; fi
  [ "$("$stage/bin/codex" --version)" = "codex-cli $version" ]
  cp "$tmp/manifest.json" "$stage/fork-release.json"
  ln -s bin/codex "$stage/codex"
  mv "$stage" "$destination"
fi
[ "$("$destination/bin/codex" --version)" = "codex-cli $version" ]
if [ -L "$ROOT/current" ]; then
  previous="$(cd -P "$ROOT/current" && pwd)"
  if [ "$previous" != "$destination" ]; then
    link "$previous" "$ROOT/fork-previous"
    case "$(basename "$previous")" in
      *.fork-*|*.fork.[0-9]*-*) ;;
      *)
        if [ ! -e "$ROOT/official-before-fork" ]; then
          link "$previous" "$ROOT/official-before-fork"
        fi ;;
    esac
  fi
fi
select_package "$destination"
printf '\n✓ Installed %s (%s)\n' "$version" "$short_commit"
echo '  Restart Codex to use the new version.'
check_path
