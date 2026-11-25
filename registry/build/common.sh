# shellcheck shell=bash
# Shared helpers for registry/build/*.sh (sourced, not executed).
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

# `<os>-<arch>` key used in manifests.
platform() {
  local os arch
  case "$(uname -s)" in Linux) os=linux ;; Darwin) os=macos ;; *) die "unsupported OS $(uname -s)" ;; esac
  case "$(uname -m)" in x86_64 | amd64) arch=x64 ;; aarch64 | arm64) arch=arm64 ;; *) die "unsupported arch $(uname -m)" ;; esac
  echo "$os-$arch"
}

jobs() { getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu; }

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

# fetch <url> <sha256> <file>: download and verify against a pinned checksum.
fetch() {
  curl -fsSL --retry 3 -o "$3" "$1"
  local got
  got=$(sha256 "$3")
  [ "$got" = "$2" ] || die "checksum mismatch for $1 (got $got)"
}

# Archives the staged tree as <out>/<name>-<version>-<platform>.tar.gz with one top folder.
package() {
  local name=$1 version=$2 stage=$3 out=$4 top
  top="$name-$version-$(platform)"
  mkdir -p "$out"
  rm -rf "${out:?}/$top"
  cp -R "$stage" "$out/$top"
  tar -C "$out" -czf "$out/$top.tar.gz" "$top"
  rm -rf "${out:?}/$top"
  echo "$out/$top.tar.gz"
}

export MACOSX_DEPLOYMENT_TARGET=12.0
