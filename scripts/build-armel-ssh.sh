#!/bin/sh
# Cross-build the ARMv7 soft-float SSH helper from the same upstream releases as ARMHF.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
output=${1:-"$root/build/device/armel"}
mkdir -p "$output"
output=$(cd "$output" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/primitive-dropbear.XXXXXXXX")
pending=
trap 'rm -rf "$work"; if [ -n "$pending" ]; then rm -f "$pending"; fi' 0
trap 'exit 1' HUP INT TERM
export LC_ALL=C SOURCE_DATE_EPOCH=0

extract_source() {
    name=$1
    url=$2
    digest=$3
    archive=$work/$name.tar
    curl -fsSL --proto '=https' --proto-redir '=https' --retry 2 --max-time 90 "$url" -o "$archive"
    if command -v sha256sum >/dev/null; then
        actual=$(sha256sum "$archive")
    else
        actual=$(shasum -a 256 "$archive")
    fi
    [ "${actual%% *}" = "$digest" ] || { echo "Source checksum mismatch: $name" >&2; exit 1; }
    mkdir "$work/$name"
    tar -xf "$archive" --strip-components=1 -C "$work/$name"
}

extract_source zlib https://codeload.github.com/madler/zlib/tar.gz/da607da739fa6047df13e66a2af6b8bec7c2a498 \
    b9258cf6254e7f7c37f1cd61dba943a1c5ea3cff5718c789834dac359094f5f7
extract_source dropbear https://matt.ucc.asn.au/dropbear/releases/dropbear-2026.92.tar.bz2 \
    91dcb5234de8dea68dd82c55411c9fc986b457ab58372a780ee8a870419c2f7e

cc='zig cc -target arm-linux-musleabi -mcpu=generic+v7a -mfloat-abi=soft'
jobs=${KINDLE_HELPER_JOBS:-4}
(
    cd "$work/zlib"
    CC="$cc" CHOST=arm-linux-musleabi AR='zig ar' RANLIB='zig ranlib' CFLAGS=-Os ./configure --static
    make -j "$jobs" libz.a
)
(
    cd "$work/dropbear"
    build=$(sh src/config.guess)
    CC="$cc" AR='zig ar' RANLIB='zig ranlib' CFLAGS=-Os LDFLAGS='-Wl,--build-id=none,--strip-all' \
        ./configure --build="$build" --host=arm-linux-musleabi --enable-static --enable-bundled-libtom \
        --with-zlib="$work/zlib"
    # Password authentication is part of the launcher's SSH settings.
    grep -q '^#define HAVE_CRYPT 1$' config.h
    grep -q '^#define HAVE_SHADOW_H 1$' config.h
    make -j "$jobs" PROGRAMS=dropbear dropbear
)

zig_lib=$(zig env | sed -n 's/^    \.lib_dir = "\(.*\)",$/\1/p')
[ -n "$zig_lib" ] || { echo "Could not locate Zig's musl copyright notice." >&2; exit 1; }
install -m 644 "$zig_lib/libc/musl/COPYRIGHT" "$output/MUSL-LICENSE.txt"
pending=$(mktemp "$output/.dropbear.XXXXXXXX")
install -m 755 "$work/dropbear/dropbear" "$pending"
mv "$pending" "$output/dropbear"
printf 'Built %s\n' "$output/dropbear"
