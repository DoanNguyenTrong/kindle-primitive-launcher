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
strict=${KL_SETUP_BOOT_STRICT:-0}
failed=0
mirror_files='kl-launcher supervisor.sh boot.sh holder.lua ssh.sh fonts/NotoSans-Regular.ttf fonts/NotoSans-Bold.ttf ssh/bin/dropbear ssh/lib/libz.so.1 ssh/dropbear_ed25519_host_key'

log() { echo "[setup $(date '+%H:%M:%S')] $*" >> "$LOG" 2>/dev/null; }

# Copy $1 to $2 when they differ. The copy is written under a temporary name and then renamed into place.
copy() {
    [ -f "$1" ] || return 0
    cmp -s "$1" "$2" 2>/dev/null && return 0
    if mkdir -p "$(dirname "$2")" && cat "$1" > "$2.new" && sync && mv "$2.new" "$2"; then
        log "copied $2"
    else
        rm -f "$2.new"
        log "copy failed: $2"
        failed=1
    fi
}

mirror_has_space() {
    free=$(df -Pk "$M" | awk 'NR == 2 { print $4 }')
    case "$free" in ''|*[!0-9]*) return 1 ;; esac
    for relative in $mirror_files; do
        source=$DIR/$relative
        target=$M/$relative
        [ -f "$source" ] || continue
        cmp -s "$source" "$target" 2>/dev/null && continue
        bytes=$(wc -c < "$source") || return 1
        required=$(( (bytes + 1023) / 1024 ))
        [ "$free" -ge "$required" ] || return 1
        # Each atomic rename frees its predecessor before the next file is staged.
        previous=0
        if [ -f "$target" ]; then
            previous=$(wc -c < "$target") || return 1
        fi
        free=$((free - required + (previous + 1023) / 1024))
    done
}

mkdir -p "$M/fonts" "$M/ssh/bin" "$M/ssh/lib" 2>/dev/null || failed=1
if [ "$strict" = 1 ] && ! mirror_has_space; then
    log "cannot stage the Launcher boot copy: check free space in $M"
    exit 1
fi
# A USB install may have no host key yet. The mirror generates one when SSH first starts.
for f in $mirror_files; do
    copy "$DIR/$f" "$M/$f"
done
for f in kl-launcher supervisor.sh boot.sh ssh.sh ssh/bin/dropbear; do
    [ ! -f "$M/$f" ] || chmod 755 "$M/$f" 2>/dev/null || failed=1
done

# The boot job. The root filesystem is read-only except between "mntroot rw" and "mntroot ro".
if [ -f "$DIR/primitive-launcher.conf" ] && ! cmp -s "$DIR/primitive-launcher.conf" "$JOB" 2>/dev/null; then
    if command -v mntroot >/dev/null 2>&1 && mntroot rw; then
        if cat "$DIR/primitive-launcher.conf" > "$JOB.new" && sync && mv "$JOB.new" "$JOB"; then
            log "boot job written: $JOB"
        else
            rm -f "$JOB.new"
            log "boot job not written: $JOB"
            failed=1
        fi
        mntroot ro || failed=1
    else
        log "boot job not written: the root filesystem is not writable here"
        failed=1
    fi
fi
[ "$strict" != 1 ] || exit "$failed"
exit 0
