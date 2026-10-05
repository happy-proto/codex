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
[ "$(uname -s)" = Darwin ] || { echo 'This fork supports macOS only.' >&2; exit 1; }
case "$(uname -m)" in
  arm64|aarch64) ;;
  x86_64) [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ] ;;
  *) exit 1 ;;
esac
command -v plutil >/dev/null
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
lockf -t 0 9 || { echo 'Another installer is running.' >&2; exit 1; }
mkdir "$ROOT/fork-install.lock" || { echo 'Another fork installer is running.' >&2; exit 1; }
locked=true

download() {
  curl -fSL --retry 3 --connect-timeout 15 --max-time 600 "$1" -o "$2"
}
extract() {
  plutil -extract "$1" raw -o - "$2"
}
link() {
  ln -s "$1" "$2.tmp.$$"
  mv -fh "$2.tmp.$$" "$2"
}
select_package() {
  # Remove the official latest-channel marker: this installation is manually updated.
  rm -f "$ROOT/auto-update-version"
  link "$1" "$ROOT/current"
  link "$ROOT/current/bin/codex" "$BIN_DIR/codex"
  link "$ROOT/current/bin/codex-code-mode-host" "$BIN_DIR/codex-code-mode-host"
}

if [ "$rollback" = true ]; then
  previous="$(cd -P "$ROOT/fork-previous" && pwd)"
  "$previous/bin/codex" --version
  select_package "$previous"
  echo 'Previous CLI selected; configuration and sessions were preserved.'
  exit 0
fi

if [ "$release" = latest ]; then
  download "$API/releases?per_page=100" "$tmp/releases.json"
  i=0
  tag=''
  while candidate="$(extract "$i.tag_name" "$tmp/releases.json" 2>/dev/null)"; do
    case "$candidate" in
      fork-v*-alpha.*.fork)
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
  fork-v*-alpha.*.fork) ;;
  *) echo 'Expected a fork alpha version.' >&2; exit 1 ;;
esac
case "$tag" in *[!a-zA-Z0-9.-]*) exit 1 ;; esac
download "$DOWNLOAD/$tag/fork-release.json" "$tmp/manifest.json"
version="$(extract version "$tmp/manifest.json")"
commit="$(extract source_commit "$tmp/manifest.json")"
digest="$(extract sha256 "$tmp/manifest.json")"
asset="$(extract asset "$tmp/manifest.json")"
[ "fork-v$version" = "$tag" ] || { echo 'Release version mismatch.' >&2; exit 1; }
[ "${#commit}" = 40 ] && [ "${#digest}" = 64 ] || exit 1
case "$commit$digest" in *[!0-9a-f]*) exit 1 ;; esac
[ "$asset" = "codex-package-aarch64-apple-darwin-$digest.tar.gz" ] || exit 1
name="$version-$commit-$digest-aarch64-apple-darwin"
destination="$ROOT/releases/$name"

if [ ! -f "$destination/fork-release.json" ]; then
  download "$DOWNLOAD/$tag/$asset" "$tmp/package.tar.gz"
  actual="$(shasum -a 256 "$tmp/package.tar.gz" | awk '{print $1}')"
  [ "$actual" = "$digest" ] || { echo 'Package checksum mismatch; current CLI unchanged.' >&2; exit 1; }
  # Only the repository's canonical relative-path package layout is accepted.
  tar -tzf "$tmp/package.tar.gz" > "$tmp/entries"
  if LC_ALL=C awk '/^\// || /(^|\/)\.\.(\/|$)/ {bad=1} END {exit !bad}' "$tmp/entries"; then
    echo 'Unsafe archive paths.' >&2; exit 1
  fi
  stage="$ROOT/releases/.fork-staging.$$"
  mkdir "$stage"
  tar -xzf "$tmp/package.tar.gz" -C "$stage"
  [ "$(extract version "$stage/codex-package.json")" = "$version" ]
  [ "$(extract target "$stage/codex-package.json")" = aarch64-apple-darwin ]
  [ -x "$stage/bin/codex-code-mode-host" ]
  [ -x "$stage/codex-path/rg" ]
  [ -x "$stage/codex-resources/zsh/bin/zsh" ]
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
      *.fork-*) ;;
      *)
        if [ ! -e "$ROOT/official-before-fork" ]; then
          link "$previous" "$ROOT/official-before-fork"
        fi ;;
    esac
  fi
fi
select_package "$destination"
printf 'Installed Codex %s (%s). Background upgrades are disabled.\n' "$version" "$commit"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) printf 'Add %s to PATH to use codex.\n' "$BIN_DIR" ;;
esac
