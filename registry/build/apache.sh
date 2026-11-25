#!/usr/bin/env bash
# Builds a relocatable Apache httpd (event MPM, shared modules, bundled APR,
# static PCRE2/Expat; no mod_ssl/http2) for this machine.
# usage: registry/build/apache.sh <version> <out-dir>
# shellcheck source=registry/build/common.sh
source "$(dirname "$0")/common.sh"
version=${1:?version}
out=$(mkdir -p "${2:?out dir}" && cd "$2" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

APR=1.7.6 APR_UTIL=1.6.6
# Pinned dependencies (checksums from their GitHub releases).
PCRE2=10.49 PCRE2_SHA256=53c156e1ba416a20da8e65395daa132da0d80e76910424caca3fcdae7831d384
EXPAT=2.11.0 EXPAT_SHA256=e4fad0deffe151dbb4ba23226211a4c607765bebfbe02772db3644a928482777
apache_sum() { curl -fsSL "https://downloads.apache.org/$1.sha256" | awk '{ print $1 }'; }
fetch "https://downloads.apache.org/httpd/httpd-$version.tar.bz2" "$(apache_sum "httpd/httpd-$version.tar.bz2")" "$work/httpd.tar.bz2"
fetch "https://downloads.apache.org/apr/apr-$APR.tar.bz2" "$(apache_sum "apr/apr-$APR.tar.bz2")" "$work/apr.tar.bz2"
fetch "https://downloads.apache.org/apr/apr-util-$APR_UTIL.tar.bz2" "$(apache_sum "apr/apr-util-$APR_UTIL.tar.bz2")" "$work/apr-util.tar.bz2"
fetch "https://github.com/PCRE2Project/pcre2/releases/download/pcre2-$PCRE2/pcre2-$PCRE2.tar.bz2" \
  "$PCRE2_SHA256" "$work/pcre2.tar.bz2"
fetch "https://github.com/libexpat/libexpat/releases/download/R_${EXPAT//./_}/expat-$EXPAT.tar.bz2" \
  "$EXPAT_SHA256" "$work/expat.tar.bz2"
for f in httpd apr apr-util pcre2 expat; do tar -xjf "$work/$f.tar.bz2" -C "$work"; done

deps="$work/deps"
(cd "$work/pcre2-$PCRE2" && ./configure --prefix="$deps" --disable-shared --enable-static --with-pic >/dev/null && make -j"$(jobs)" install >/dev/null)
(cd "$work/expat-$EXPAT" && ./configure --prefix="$deps" --disable-shared --enable-static --with-pic --without-docbook \
  --without-examples --without-tests >/dev/null && make -j"$(jobs)" install >/dev/null)

src="$work/httpd-$version"
mv "$work/apr-$APR" "$src/srclib/apr"
mv "$work/apr-util-$APR_UTIL" "$src/srclib/apr-util"

prefix="$work/stage"
if [ "$(uname -s)" = Linux ]; then
  # XORIGIN is patched to $ORIGIN after the build (same length): libraries
  # are found next to the binaries wherever the module is extracted.
  export LDFLAGS="${LDFLAGS:-} -Wl,-z,origin -Wl,-rpath,XORIGIN/../lib"
fi
cd "$src" || exit
./configure --prefix="$prefix" --with-included-apr --with-expat="$deps" --with-pcre="$deps/bin/pcre2-config" \
  --with-mpm=event --enable-mods-shared=most --enable-proxy --enable-proxy-fcgi \
  --disable-ssl --disable-http2 --disable-proxy-http2 --disable-brotli --disable-lua --disable-md \
  --disable-xml2enc --disable-proxy-html --disable-session-crypto --disable-ldap --disable-authnz-ldap
make -j"$(jobs)"
make install

# Trim what a dev server does not need.
rm -rf "$prefix/manual" "$prefix/man" "$prefix/build" "$prefix/include" "$prefix/cgi-bin" "$prefix/logs" "$prefix/htdocs"
rm -rf "$prefix"/lib/*.a "$prefix"/lib/*.la "$prefix"/lib/*.exp "$prefix/lib/pkgconfig"
cp LICENSE NOTICE "$prefix/"

if [ "$(uname -s)" = Linux ]; then
  # libtool also records the build folder ($prefix/lib) in RUNPATH. Left in,
  # a local user could plant libraries there on the target machine, so it is
  # overwritten in place with an equal-length "$ORIGIN/..////lib".
  for f in "$prefix"/bin/* "$prefix"/lib/*.so* "$prefix"/modules/*.so; do
    if [ -f "$f" ] && ! [ -L "$f" ] && file -b "$f" | grep -q ELF; then
      size=$(wc -c <"$f")
      BUILD_LIB="$prefix/lib" perl -pi -e '
        BEGIN { $b = $ENV{BUILD_LIB}; $r = q($ORIGIN/..) . ("/" x (length($b) - 13)) . "lib" }
        s/XORIGIN/\$ORIGIN/g; s/\Q$b\E(?=[\0:])/$r/g' "$f"
      [ "$(wc -c <"$f")" = "$size" ] || die "patching changed the size of $f"
      if readelf -d "$f" | grep -E "R(UN)?PATH" | grep -q "$prefix"; then die "build path left in $f"; fi
    fi
  done
else
  # Point every Mach-O at @rpath and add an rpath relative to the file.
  for f in "$prefix"/bin/* "$prefix"/lib/*.dylib "$prefix"/modules/*.so; do
    if [ -L "$f" ] || ! [ -f "$f" ] || ! file -b "$f" | grep -q Mach-O; then continue; fi
    for dep in $(otool -L "$f" | awk 'NR > 1 { print $1 }' | grep "^$prefix/lib/"); do
      install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$f"
    done
    case "$f" in
      *.dylib) install_name_tool -id "@rpath/$(basename "$f")" "$f" ;;
    esac
    for rp in $(otool -l "$f" | awk '/LC_RPATH/ { getline; getline; print $2 }' | grep "^$prefix"); do
      install_name_tool -delete_rpath "$rp" "$f"
    done
    install_name_tool -add_rpath "@loader_path/../lib" "$f" 2>/dev/null || true
    codesign --force -s - "$f"
  done
fi
if [ "$(uname -s)" = Darwin ] && otool -L "$prefix"/bin/* "$prefix"/modules/*.so 2>/dev/null | grep -q "$prefix"; then
  die "build path left in a Mach-O load command"
fi
"$prefix/bin/httpd" -v
package apache "$version" "$prefix" "$out"
