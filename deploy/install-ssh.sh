#!/bin/sh
# Copy Primitive Launcher onto the Kindle over SSH (Wi-Fi).
#
#   deploy/install-ssh.sh [path/to/kl-launcher]
#
# Needs the SSH server running on the Kindle (Primitive Browser's Diagnostics, KOReader, or the launcher's
# Settings). The root password is asked for once: every step shares one connection. Nothing secret is
# stored in this repository.
#
# Environment: KINDLE_HOST (default 192.168.1.28), KINDLE_SSH_PORT (default 2222).
#
# Files go to /mnt/us/launcher and /mnt/us/documents/Launcher.sh, each written under a temporary name and
# renamed into place. The running launcher and supervisor run from copies in /var/tmp/kl, so they are not
# disturbed. If the launcher is running, it is restarted (TERM) to load the new build; a changed
# supervisor takes effect the next time the launcher is opened from the library.
set -eu

HERE=$(cd "$(dirname "$0")/.." && pwd)
HOST=${KINDLE_HOST:-192.168.1.28}
PORT=${KINDLE_SSH_PORT:-2222}
BIN=${1:-$HERE/target/armv7-unknown-linux-musleabihf/release/kl-launcher}
ICON_PNG="$HERE/assets/launcher-icon.png"

[ -f "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }
file "$BIN" | grep -q "ARM" || { echo "not an ARM binary: $BIN" >&2; exit 1; }
[ -f "$ICON_PNG" ] || { echo "icon not found: $ICON_PNG" >&2; exit 1; }

# The library entry carries its icon in the header, after the Author line.
ICON_LINE="# Icon: data:image/png;base64,$(base64 < "$ICON_PNG" | tr -d '\n')"
ENTRY_TMP=$(mktemp)
awk -v icon="$ICON_LINE" '{ print } /^# Author:/ { print icon }' "$HERE/deploy/library-entry.sh" > "$ENTRY_TMP"

CTL="/tmp/klssh-$$"
SSH="ssh -p $PORT -o ControlMaster=auto -o ControlPath=$CTL -o ControlPersist=120 -o StrictHostKeyChecking=accept-new root@$HOST"
cleanup() {
    ssh -p "$PORT" -o ControlPath="$CTL" -O exit "root@$HOST" 2>/dev/null || true
    rm -f "$ENTRY_TMP"
}
trap cleanup EXIT INT TERM

echo "connecting to root@$HOST:$PORT"
$SSH 'mkdir -p /mnt/us/launcher/fonts /mnt/us/launcher/cache /mnt/us/documents'

upload() {
    $SSH "cat > '$2.new' && sync && mv '$2.new' '$2'" < "$1"
    echo "copied: $2"
}

# Copy only when the device's copy differs (SHA-256).
sync_file() {
    local_sum=$(shasum -a 256 "$1" | cut -c1-64)
    remote_sum=$($SSH "sha256sum '$2' 2>/dev/null" | cut -c1-64 || true)
    if [ "$local_sum" != "$remote_sum" ]; then
        upload "$1" "$2"
    fi
}

sync_file "$BIN" /mnt/us/launcher/kl-launcher
sync_file "$HERE/deploy/supervisor.sh" /mnt/us/launcher/supervisor.sh
sync_file "$HERE/deploy/ssh.sh" /mnt/us/launcher/ssh.sh
sync_file "$HERE/deploy/boot.sh" /mnt/us/launcher/boot.sh
sync_file "$HERE/deploy/holder.lua" /mnt/us/launcher/holder.lua
sync_file "$HERE/deploy/run-koreader.sh" /mnt/us/launcher/run-koreader.sh
sync_file "$HERE/deploy/apps.toml.example" /mnt/us/launcher/apps.toml.example
sync_file "$ENTRY_TMP" /mnt/us/documents/Launcher.sh
for font in NotoSans-Regular.ttf NotoSans-Bold.ttf NOTO-LICENSE.txt; do
    sync_file "$HERE/assets/fonts/$font" "/mnt/us/launcher/fonts/$font"
done
$SSH 'chmod 755 /mnt/us/launcher/kl-launcher 2>/dev/null || true'

local_sum=$(shasum -a 256 "$BIN" | cut -c1-64)
remote_sum=$($SSH 'sha256sum /mnt/us/launcher/kl-launcher' | cut -c1-64)
if [ "$local_sum" != "$remote_sum" ]; then
    echo "checksum mismatch: local $local_sum, device $remote_sum" >&2
    exit 1
fi
echo "installed: /mnt/us/launcher/kl-launcher ($(wc -c < "$BIN" | tr -d ' ') bytes, sha256 $local_sum)"

# Restart a running launcher so it loads the new build (the supervisor copies it again).
$SSH 'p=$(cat /var/tmp/kl/launcher.pid 2>/dev/null); if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then kill -TERM "$p"; echo "launcher restarted"; else echo "launcher not running: open Launcher in the Kindle library"; fi'
