#!/bin/sh
# Build and deploy a device release of Primitive Launcher.
#
#   scripts/build-device.sh
#
# Every device build is a new patch version (0.1.0 -> 0.1.1 -> ...): the workspace version in Cargo.toml,
# shown in Settings. The bump is undone if the tests, the build or the install fail, so a failed deploy
# does not use up a version number.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"
MANIFEST="$ROOT/Cargo.toml"

current=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"/\1/p' "$MANIFEST" | head -n 1)
next=$(echo "$current" | awk -F. '{ printf "%d.%d.%d", $1, $2, $3 + 1 }')

set_version() {
    sed -i '' "s/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/version = \"$1\"/" "$MANIFEST"
}

echo "version: $current -> $next"
set_version "$next"
undo() { set_version "$current"; echo "build or install failed: version left at $current"; }
trap undo ERR INT

cd "$ROOT"
cargo test --offline -j 4 --workspace --quiet
sh tests/supervisor-test.sh > /dev/null
cargo zigbuild --offline -j 4 --release -p kl-kindle --target armv7-unknown-linux-musleabihf 2>&1 | grep -E "^(error|warning: unused)" && false || true
[ -f target/armv7-unknown-linux-musleabihf/release/kl-launcher ]
sh deploy/install-ssh.sh target/armv7-unknown-linux-musleabihf/release/kl-launcher
trap - ERR INT
echo "deployed Primitive Launcher v$next"
