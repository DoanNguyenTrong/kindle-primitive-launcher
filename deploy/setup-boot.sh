#!/bin/sh
# Sets up the boot start on the Kindle from the launcher's own files, so no computer or SSH is needed. The library
# entry (Launcher.sh) runs it each time Launcher opens. It copies what the boot needs to /var/local/primitive-launcher
# and writes the boot job to /etc/upstart when they differ from what is there. Nothing is written when they are the
# same, so running it every time costs little.
#
#   setup-boot.sh [/mnt/us/launcher]
#
# Environment (tests): KL_MIRROR (default /var/local/primitive-launcher), KL_JOB (default
# /etc/upstart/primitive-launcher.conf), KL_LOG (default the launcher's log in the folder).
DIR=${1:-/mnt/us/launcher}
M=${KL_MIRROR:-/var/local/primitive-launcher}
JOB=${KL_JOB:-/etc/upstart/primitive-launcher.conf}
LOG=${KL_LOG:-$DIR/log-launcher.txt}

log() { echo "[setup $(date '+%H:%M:%S')] $*" >> "$LOG" 2>/dev/null; }

# Copy $1 to $2 when they differ. The copy is written under a temporary name and then renamed into place.
copy() {
    [ -f "$1" ] || return 0
    cmp -s "$1" "$2" 2>/dev/null && return 0
    if mkdir -p "$(dirname "$2")" && cat "$1" > "$2.new" && sync && mv "$2.new" "$2"; then
        log "copied $2"
    else
        log "copy failed: $2"
        return 1
    fi
}

mkdir -p "$M/fonts" "$M/ssh/bin" "$M/ssh/lib" 2>/dev/null
copy "$DIR/kl-launcher" "$M/kl-launcher"
for f in supervisor.sh boot.sh holder.lua ssh.sh; do
    copy "$DIR/$f" "$M/$f"
done
for f in NotoSans-Regular.ttf NotoSans-Bold.ttf; do
    copy "$DIR/fonts/$f" "$M/fonts/$f"
done
# The SSH server and its host key, when the launcher's folder has them (a USB install has no host key yet: the
# server makes one on its first start in the copy).
copy "$DIR/ssh/bin/dropbear" "$M/ssh/bin/dropbear"
copy "$DIR/ssh/lib/libz.so.1" "$M/ssh/lib/libz.so.1"
copy "$DIR/ssh/dropbear_ed25519_host_key" "$M/ssh/dropbear_ed25519_host_key"
chmod 755 "$M/kl-launcher" "$M/supervisor.sh" "$M/boot.sh" "$M/ssh.sh" "$M/ssh/bin/dropbear" 2>/dev/null || true

# The boot job. The root filesystem is read-only except between "mntroot rw" and "mntroot ro".
if [ -f "$DIR/primitive-launcher.conf" ] && ! cmp -s "$DIR/primitive-launcher.conf" "$JOB" 2>/dev/null; then
    if command -v mntroot >/dev/null 2>&1 && mntroot rw; then
        if cat "$DIR/primitive-launcher.conf" > "$JOB.new" && sync && mv "$JOB.new" "$JOB"; then
            log "boot job written: $JOB"
        else
            log "boot job not written: $JOB"
        fi
        mntroot ro
    else
        log "boot job not written: the root filesystem is not writable here"
    fi
fi
exit 0
