#!/bin/sh
# Start, stop or check the SSH server (port 2222) from the launcher's Settings. Uses Primitive Browser's
# dropbear and host key (installed with the browser), the same pid file as the browser's Diagnostics
# page, so either side sees the server the other started. The root password is the device's own.
#
#   ssh.sh start | stop | status

BIN=/mnt/us/browser/bin
KEYDIR=/mnt/us/browser/ssh
PID=/tmp/dropbear_browser.pid

running() { [ -f "$PID" ] && kill -0 "$(cat "$PID" 2>/dev/null)" 2>/dev/null; }

case "$1" in
    start)
        if running; then echo "running"; exit 0; fi
        if grep -i ':08AE ' /proc/net/tcp 2>/dev/null | grep -q ' 0A '; then
            echo "port 2222 is already in use"
            exit 1
        fi
        [ -x "$BIN/dropbear" ] || { echo "no dropbear in $BIN (install Primitive Browser)"; exit 1; }
        mkdir -p "$KEYDIR"
        [ -f "$KEYDIR/dropbear_ed25519_host_key" ] || cp /mnt/us/koreader/settings/SSH/dropbear_ed25519_host_key "$KEYDIR/" 2>/dev/null
        echo "started $(date)" >> "$KEYDIR/dropbear.log"
        cd "$BIN" && LD_LIBRARY_PATH=/mnt/us/browser/lib ./dropbear -E -R -r "$KEYDIR/dropbear_ed25519_host_key" -p 2222 -P "$PID" 2>> "$KEYDIR/dropbear.log"
        sleep 1
        if running; then echo "running"; else echo "failed"; exit 1; fi
        ;;
    stop)
        if running; then kill "$(cat "$PID")"; fi
        rm -f "$PID"
        echo "stopped"
        ;;
    *)
        if running; then echo "running"; else echo "stopped"; fi
        ;;
esac
