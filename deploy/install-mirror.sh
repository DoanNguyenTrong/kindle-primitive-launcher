#!/bin/sh
# Keep a copy of what the boot needs on /var/local: the boot guard, the supervisor, the launcher, its fonts, and the SSH
# server. /var/local is writable and kept across reboots; /mnt/us is not there while the Kindle shows its drive to a
# computer, so the boot job starts from this copy when /mnt/us is missing. Copied only when different (SHA-256).
#
#   deploy/install-mirror.sh [path/to/kl-launcher]
set -eu

HERE=$(cd "$(dirname "$0")/.." && pwd)
HOST=${KINDLE_HOST:-192.168.1.28}
PORT=${KINDLE_SSH_PORT:-2222}
BIN=${1:-$HERE/target/armv7-unknown-linux-musleabihf/release/kl-launcher}
M=/var/local/primitive-launcher

[ -f "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }

CTL="/tmp/klssh-mirror-$$"
SSH="ssh -p $PORT -o ControlMaster=auto -o ControlPath=$CTL -o ControlPersist=120 -o StrictHostKeyChecking=accept-new root@$HOST"
cleanup() { ssh -p "$PORT" -o ControlPath="$CTL" -O exit "root@$HOST" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

$SSH "mkdir -p $M/fonts $M/ssh/bin $M/ssh/lib"

# Copy only when the device's copy differs (SHA-256).
sync_file() {
    local_sum=$(shasum -a 256 "$1" | cut -c1-64)
    remote_sum=$($SSH "sha256sum '$2' 2>/dev/null" | cut -c1-64 || true)
    if [ "$local_sum" != "$remote_sum" ]; then
        $SSH "cat > '$2.new' && sync && mv '$2.new' '$2'" < "$1"
        echo "copied: $2"
    fi
}

sync_file "$BIN" "$M/kl-launcher"
sync_file "$HERE/deploy/supervisor.sh" "$M/supervisor.sh"
sync_file "$HERE/deploy/boot.sh" "$M/boot.sh"
sync_file "$HERE/deploy/holder.lua" "$M/holder.lua"
sync_file "$HERE/deploy/ssh.sh" "$M/ssh.sh"
for font in NotoSans-Regular.ttf NotoSans-Bold.ttf; do
    sync_file "$HERE/assets/fonts/$font" "$M/fonts/$font"
done

# The SSH server (Dropbear, its zlib and the host key) is the launcher's copy on /mnt/us, taken on the Kindle when it
# is there. Copied only when different.
$SSH "S=/mnt/us/launcher/ssh; if [ -d \$S ]; then for f in bin/dropbear lib/libz.so.1 dropbear_ed25519_host_key; do if [ -f \$S/\$f ] && ! cmp -s \$S/\$f $M/ssh/\$f; then cp \$S/\$f $M/ssh/\$f && echo \"copied: $M/ssh/\$f\"; fi; done; fi; chmod 755 $M/kl-launcher $M/supervisor.sh $M/boot.sh $M/ssh.sh $M/ssh/bin/dropbear 2>/dev/null || true; sync"
echo "mirror ready in $M"
