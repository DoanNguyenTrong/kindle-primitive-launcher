#!/bin/sh
# Build and deploy a device release of Primitive Launcher.
#
#   scripts/build-device.sh           # next patch version: 0.2.0 -> 0.2.1
#   scripts/build-device.sh --minor   # next minor version, a new phase: 0.1.11 -> 0.2.0
#
# Every device build is a new version: the workspace version in Cargo.toml, shown in Settings. A new phase
# starts a new minor version (Phase 2 is 0.2.x). The bump is undone if the tests, the build or the install
# fail, so a failed deploy does not use up a version number.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"
MANIFEST="$ROOT/Cargo.toml"

current=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"/\1/p' "$MANIFEST" | head -n 1)
case "${1:-}" in
    --minor) next=$(echo "$current" | awk -F. '{ printf "%d.%d.0", $1, $2 + 1 }') ;;
    "") next=$(echo "$current" | awk -F. '{ printf "%d.%d.%d", $1, $2, $3 + 1 }') ;;
    *) echo "usage: $0 [--minor]" >&2; exit 2 ;;
esac

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
sh tests/setup-boot-test.sh > /dev/null
# Build the launcher binary itself (kl-launcher). The old file is removed first, and the new one must carry the
# version being deployed: a stale binary once passed this check.
BIN=target/armv7-unknown-linux-musleabihf/release/kl-launcher
rm -f "$BIN"
cargo zigbuild --offline -j 4 --release -p kl-launcher --target armv7-unknown-linux-musleabihf 2>&1 | grep -E "^(error|warning: unused)" && false || true
[ -f "$BIN" ] && grep -a -q "$next" "$BIN"
sh deploy/install-ssh.sh target/armv7-unknown-linux-musleabihf/release/kl-launcher
trap - ERR INT
echo "deployed Primitive Launcher v$next"
