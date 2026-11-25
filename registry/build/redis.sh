#!/usr/bin/env bash
# Builds a portable Redis (core server and tools, no TLS/modules) for this machine.
# usage: registry/build/redis.sh <version> <out-dir>
# shellcheck source=registry/build/common.sh
source "$(dirname "$0")/common.sh"
version=${1:?version}
out=$(mkdir -p "${2:?out dir}" && cd "$2" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The official source checksum is published in the redis-hashes repository.
sum=$(curl -fsSL https://raw.githubusercontent.com/redis/redis-hashes/master/README |
  awk -v f="redis-$version.tar.gz" '$2 == f && $3 == "sha256" { print $4 }')
[ -n "$sum" ] || die "no published checksum for redis $version"
fetch "https://download.redis.io/releases/redis-$version.tar.gz" "$sum" "$work/src.tar.gz"
tar -xzf "$work/src.tar.gz" -C "$work"

cd "$work/redis-$version" || exit
# Only the core in src/ (the top-level Makefile also builds the bundled modules).
make -C src -j"$(jobs)" BUILD_TLS=no USE_SYSTEMD=no
make -C src PREFIX="$work/stage" install
cp LICENSE.txt "$work/stage/" 2>/dev/null || cp COPYING "$work/stage/"
"$work/stage/bin/redis-server" --version
package redis "$version" "$work/stage" "$out"
