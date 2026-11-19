#!/bin/sh
# Package a release of Primitive Launcher for people who do not build it.
#
#   scripts/package-release.sh
#
# Needs the ARMv7 device binary first:
#   cargo zigbuild --release -p kl-launcher --target armv7-unknown-linux-musleabihf
#
# Writes dist/primitive-launcher-<version>.zip and a matching .sha256 file. The zip holds two folders,
# launcher/ and documents/, which are copied onto the root of the Kindle (the same files deploy/install-ssh.sh
# installs). See docs/INSTALL.md.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"/\1/p' Cargo.toml | head -n 1)
NAME=primitive-launcher-$VERSION

command -v zip >/dev/null 2>&1 || { echo "zip is needed to make the package" >&2; exit 1; }

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
sh scripts/stage.sh armhf "$STAGE/$NAME"

cp docs/INSTALL.md "$STAGE/$NAME/INSTALL.md"
cp LICENSE "$STAGE/$NAME/LICENSE"
cp LICENSE-DOCS "$STAGE/$NAME/LICENSE-DOCS"

mkdir -p dist
rm -f "dist/$NAME.zip" "dist/$NAME.zip.sha256"
(cd "$STAGE" && zip -qr "$ROOT/dist/$NAME.zip" "$NAME")
(cd dist && { shasum -a 256 "$NAME.zip" 2>/dev/null || sha256sum "$NAME.zip"; } > "$NAME.zip.sha256")

echo "packaged dist/$NAME.zip"
cat "dist/$NAME.zip.sha256"
