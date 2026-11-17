#!/bin/sh
# Turn off the boot start: removes the upstart job from the root filesystem over SSH. The launcher files on
# /mnt/us are kept. Afterwards a normal stock boot is all that happens.
#
#   deploy/remove-autostart.sh
set -eu

HOST=${KINDLE_HOST:-192.168.1.28}
PORT=${KINDLE_SSH_PORT:-2222}
CTL="/tmp/klssh-remove-$$"
SSH="ssh -p $PORT -o ControlMaster=auto -o ControlPath=$CTL -o ControlPersist=60 -o StrictHostKeyChecking=accept-new root@$HOST"
cleanup() { ssh -p "$PORT" -o ControlPath="$CTL" -O exit "root@$HOST" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

$SSH 'command -v mntroot >/dev/null' || { echo "mntroot not found on the Kindle" >&2; exit 1; }
$SSH 'mntroot rw && rm -f /etc/upstart/primitive-launcher.conf; r=$?; sync; mntroot ro; exit $r'
if $SSH 'test -e /etc/upstart/primitive-launcher.conf'; then
    echo "the job file is still present" >&2
    exit 1
fi
echo "removed /etc/upstart/primitive-launcher.conf; the next boot is stock"
