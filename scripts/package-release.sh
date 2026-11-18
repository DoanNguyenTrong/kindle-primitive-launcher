#!/bin/sh
# Package a release of Primitive Launcher for people who do not build it.
#
#   scripts/package-release.sh
#
# Needs the ARMv7 device binary first:
#   cargo zigbuild --release -p kl-kindle --target armv7-unknown-linux-musleabihf
#
# Writes dist/primitive-launcher-<version>.zip and a matching .sha256 file. The zip holds two folders,
# launcher/ and documents/, which are copied onto the root of the Kindle (the same files deploy/install-ssh.sh
# installs). See docs/INSTALL.md.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"/\1/p' Cargo.toml | head -n 1)
BIN=target/armv7-unknown-linux-musleabihf/release/kl-launcher
NAME=primitive-launcher-$VERSION

[ -f "$BIN" ] || { echo "build the device binary first (see the comment at the top of this script)" >&2; exit 1; }
if command -v file >/dev/null 2>&1; then
    file "$BIN" | grep -q ARM || { echo "not an ARM binary: $BIN" >&2; exit 1; }
fi
command -v zip >/dev/null 2>&1 || { echo "zip is needed to make the package" >&2; exit 1; }

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
L="$STAGE/$NAME/launcher"
mkdir -p "$L/fonts" "$L/icons" "$L/screensavers" "$L/ssh/bin" "$L/ssh/lib" "$L/ssh/licenses" "$STAGE/$NAME/documents"

cp "$BIN" "$L/kl-launcher"
chmod 755 "$L/kl-launcher"
for f in supervisor.sh ssh.sh dns.sh boot.sh holder.lua run-koreader.sh apps.toml.example; do
    cp "deploy/$f" "$L/$f"
done
# The SSH server (Dropbear and its zlib, with their licences). The host key and the password flag are not in the package.
cp assets/device/armv7/dropbear "$L/ssh/bin/"
chmod 755 "$L/ssh/bin/dropbear"
cp assets/device/armv7/libz.so.1 "$L/ssh/lib/"
cp assets/device/licenses/DROPBEAR-LICENSE.txt assets/device/licenses/ZLIB-LICENSE.txt "$L/ssh/licenses/"
cp assets/fonts/NotoSans-Regular.ttf assets/fonts/NotoSans-Bold.ttf assets/fonts/NOTO-LICENSE.txt "$L/fonts/"
printf 'PNG files here can be chosen as app icons: long-press an app, then Change icon.\n' > "$L/icons/README.txt"
printf 'PNG files here are shown on sleep when Settings > Sleep screen is "The launcher'"'"'s own".\n' > "$L/screensavers/README.txt"

# The library entry carries its icon in the header, after the Author line, as install-ssh.sh writes it.
ICON_LINE="# Icon: data:image/png;base64,$(base64 < assets/launcher-icon.png | tr -d '\n')"
awk -v icon="$ICON_LINE" '{ print } /^# Author:/ { print icon }' deploy/library-entry.sh > "$STAGE/$NAME/documents/Launcher.sh"
chmod 755 "$STAGE/$NAME/documents/Launcher.sh"

cp docs/INSTALL.md "$STAGE/$NAME/INSTALL.md"
cp LICENSE "$STAGE/$NAME/LICENSE"

mkdir -p dist
rm -f "dist/$NAME.zip" "dist/$NAME.zip.sha256"
(cd "$STAGE" && zip -qr "$ROOT/dist/$NAME.zip" "$NAME")
(cd dist && { shasum -a 256 "$NAME.zip" 2>/dev/null || sha256sum "$NAME.zip"; } > "$NAME.zip.sha256")

echo "packaged dist/$NAME.zip"
cat "dist/$NAME.zip.sha256"
