#!/bin/sh
# Name: Launcher
# Author: Primitive AI
# DontUseFBInk
# PrimitiveLauncherEntry
#
# Opens Primitive Launcher from the Kindle library: the stock Kindle UI stops and the launcher takes the
# screen. The supervisor is copied to tmpfs and started detached, so it does not depend on this script.
#
# This script then stays running until the supervisor ends. The library runs it through its script runner
# (an app of appmgrd, which keeps running while the framework is stopped). Returning at once made appmgrd
# switch back to the library at the moment the supervisor stopped it: the library never answered, appmgrd
# recorded a failure, showed "application error" later and refused to load the home screen when the Kindle
# UI came back (docs/device-notes.md). Waiting keeps appmgrd's hands off, as KOReader's library entry does;
# when the supervisor ends, the framework is already starting and appmgrd goes back to the library.

DIR=${KL_DIR:-/mnt/us/launcher}
RUN=${KL_RUN:-/var/tmp/kl}

# The library runs this script from /mnt/us/documents, and a shell keeps its script open while it runs. /mnt/us must
# be free to unmount when a USB cable shows the drive on a computer, so the script runs again from a copy in $RUN.
case "$0" in
    "$RUN"/*) ;;
    *) mkdir -p "$RUN" && cp "$0" "$RUN/Launcher.sh" && cd / && exec /bin/sh "$RUN/Launcher.sh" ;;
esac
cd /

if [ -f "$RUN/supervisor.pid" ] && kill -0 "$(cat "$RUN/supervisor.pid" 2>/dev/null)" 2>/dev/null; then
    exit 0
fi
[ -f "$DIR/supervisor.sh" ] || exit 1
# The boot start: the copy on /var/local and the boot job. Nothing is written when they are already the same.
[ -f "$DIR/setup-boot.sh" ] && sh "$DIR/setup-boot.sh" "$DIR"
mkdir -p "$RUN"
cp "$DIR/supervisor.sh" "$RUN/supervisor.sh" || exit 1
# No supervisor is running (checked above), so a pid file left over is stale: remove it, so the wait below
# cannot mistake it for the new supervisor having ended.
rm -f "$RUN/supervisor.pid"

if command -v setsid >/dev/null 2>&1; then
    setsid /bin/sh "$RUN/supervisor.sh" < /dev/null > /dev/null 2>&1 &
else
    nohup /bin/sh "$RUN/supervisor.sh" < /dev/null > /dev/null 2>&1 &
fi

# Stopping the framework may signal this script; it must keep waiting. Set only after the supervisor has
# started: a signal ignored when a shell starts cannot be trapped again, and the supervisor, the launcher
# and the holder would all inherit TERM as ignored (the holder then survived its stop, v0.1.7).
trap '' TERM HUP INT

# Wait for the supervisor to write its pid (up to 10 s), then for it to end.
i=0
while [ ! -s "$RUN/supervisor.pid" ] && [ $i -lt 10 ]; do
    sleep 1
    i=$((i + 1))
done
while [ -s "$RUN/supervisor.pid" ] && kill -0 "$(cat "$RUN/supervisor.pid" 2>/dev/null)" 2>/dev/null; do
    sleep 2
done
exit 0
