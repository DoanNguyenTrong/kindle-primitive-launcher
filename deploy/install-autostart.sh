#!/bin/sh
# Turn on the boot start (Phase 3). Writes the upstart job to the root filesystem over SSH.
#
#   deploy/install-autostart.sh
#
# Needs the Kindle on Wi-Fi with SSH on, and the Launcher installed (boot.sh and supervisor.sh in
# /mnt/us/launcher). The root filesystem is writable only between "mntroot rw" and "mntroot ro". Creating
# the job does not start anything now; the next boot does. To undo, run deploy/remove-autostart.sh or create
# /mnt/us/launcher/DISABLE over USB.
set -eu

HERE=$(cd "$(dirname "$0")/.." && pwd)
HOST=${KINDLE_HOST:-192.168.1.28}
PORT=${KINDLE_SSH_PORT:-2222}
JOB="$HERE/deploy/primitive-launcher.conf"
[ -f "$JOB" ] || { echo "job file not found: $JOB" >&2; exit 1; }

CTL="/tmp/klssh-autostart-$$"
SSH="ssh -p $PORT -o ControlMaster=auto -o ControlPath=$CTL -o ControlPersist=120 -o StrictHostKeyChecking=accept-new root@$HOST"
cleanup() { ssh -p "$PORT" -o ControlPath="$CTL" -O exit "root@$HOST" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

$SSH 'test -f /mnt/us/launcher/boot.sh && test -f /mnt/us/launcher/supervisor.sh' \
    || { echo "the launcher is not installed on the Kindle (boot.sh and supervisor.sh are missing)" >&2; exit 1; }
$SSH 'command -v mntroot >/dev/null' || { echo "mntroot not found on the Kindle; the job cannot be written" >&2; exit 1; }

$SSH 'mntroot rw && cat > /etc/upstart/primitive-launcher.conf.new && sync && mv /etc/upstart/primitive-launcher.conf.new /etc/upstart/primitive-launcher.conf; r=$?; mntroot ro; exit $r' < "$JOB"

local_sum=$(shasum -a 256 "$JOB" | cut -c1-64)
remote_sum=$($SSH 'sha256sum /etc/upstart/primitive-launcher.conf' | cut -c1-64)
if [ "$local_sum" != "$remote_sum" ]; then
    echo "checksum mismatch for the job file" >&2
    exit 1
fi
echo "installed /etc/upstart/primitive-launcher.conf (sha256 $local_sum); starts on the next boot"
