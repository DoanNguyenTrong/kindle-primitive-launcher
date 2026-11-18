#!/bin/sh
# Primitive Launcher supervisor.
#
# Started by documents/Launcher.sh (from the Kindle library) or, later, by the boot job. It runs from a
# copy in $RUN (tmpfs), so installs can replace the files on /mnt/us while it runs.
#
#   1. Stops the stock framework (lab126_gui). volumd keeps running, so a USB cable gives the Kindle's drive mode
#      as the stock UI does. While another program runs (an app from the grid), volumd is paused, so a cable
#      cannot take /mnt/us from under it. Both are recorded in marker files first.
#   2. Runs the launcher (kl-launcher, copied to $RUN). Exit 10: run the app described in $RUN/next, then
#      start the launcher again. Exit 0: the user asked for the Kindle UI. TERM (143): restart the launcher
#      (used by the installer to load a new build). Anything else is a crash: restart, up to
#      $MAX_CRASHES in $CRASH_WINDOW seconds, then give up.
#   3. On leaving, for any reason, resumes volumd and starts the framework again (also when it was already
#      stopped at start: the framework must not be left down).
#
# A watchdog process outlives this script. If the supervisor dies (killed, crashed), the watchdog gives
# the UI back. It also kills a launcher whose heartbeat file stops changing.
#
# Environment: KL_DIR (default /mnt/us/launcher), KL_RUN (default /var/tmp/kl), KL_DOCUMENTS.

DIR=${KL_DIR:-/mnt/us/launcher}
RUN=${KL_RUN:-/var/tmp/kl}
LOG=$DIR/log-launcher.txt
MARK=$RUN/framework-stopped
VOLUMD_MARK=$RUN/volumd-paused
PIDFILE=$RUN/supervisor.pid
LAUNCHER_PID=$RUN/launcher.pid
APP_PID=$RUN/app.pid
HOLDER_PID=$RUN/holder.pid
WAIT_PID=$RUN/wait.pid
MAX_CRASHES=${KL_MAX_CRASHES:-3}
CRASH_WINDOW=60
HEARTBEAT_LIMIT=${KL_HEARTBEAT_LIMIT:-30}
WATCH_EVERY=${KL_WATCH_EVERY:-5}

mkdir -p "$RUN"
export KL_DIR="$DIR" KL_RUN="$RUN"
# Nothing of this script may hold /mnt/us: the Kindle unmounts it to show its drive on a computer (a USB cable), and
# it kills whatever still holds it. The working directory is /, and long-running programs log through a pipe.
cd /
LOG_FIFO=$RUN/log.fifo
LOG_PENDING=$RUN/log-pending.txt
LOG_OUT=$LOG

log() { echo "[launcher $(date '+%H:%M:%S')] $*" >> "$LOG"; }

# The launcher, the holder and the wait screen write into a pipe; this reader appends each line to the log and closes
# the log again, so no long-running program keeps a file open on /mnt/us. While /mnt/us is away (the drive is on a
# computer), lines wait in $LOG_PENDING and are added when it is back.
logger_start() {
    rm -f "$LOG_FIFO"
    mkfifo "$LOG_FIFO" 2>/dev/null || return 0
    sup=$$
    (
        while [ -p "$LOG_FIFO" ] && kill -0 "$sup" 2>/dev/null; do
            while IFS= read -r line; do
                if [ -s "$LOG_PENDING" ] && cat "$LOG_PENDING" >> "$LOG" 2>/dev/null; then
                    rm -f "$LOG_PENDING"
                fi
                echo "$line" >> "$LOG" 2>/dev/null || echo "$line" >> "$LOG_PENDING"
            done < "$LOG_FIFO"
        done
    ) &
    LOGGER=$!
    LOG_OUT=$LOG_FIFO
}

logger_stop() {
    [ -n "${LOGGER:-}" ] && kill "$LOGGER" 2>/dev/null
    rm -f "$LOG_FIFO"
    LOG_OUT=$LOG
}

# Keep the log small on the Kindle's flash: keep the last ~100 KB of a log over ~200 KB.
if [ -f "$LOG" ] && [ "$(wc -c < "$LOG")" -gt 204800 ]; then
    tail -c 102400 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi

if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE" 2>/dev/null)" 2>/dev/null; then
    log "already running (pid $(cat "$PIDFILE")); nothing to do"
    exit 0
fi
echo $$ > "$PIDFILE"
log "start (pid $$)"

if [ -e /etc/init.d/framework ]; then INIT=sysv; else INIT=upstart; fi

pause_us() { usleep "$1" 2>/dev/null || sleep 1; }

framework_running() {
    if [ "$INIT" = upstart ] && command -v status >/dev/null 2>&1; then
        status lab126_gui 2>/dev/null | grep -q "start/" && return 0
    fi
    pidof cvm >/dev/null 2>&1
}

install_traps() {
    trap 'on_signal' TERM INT HUP
}

# Holds com.lab126.kaf frameworkStarted while the framework is stopped, so powerd answers the power button
# (see holder.lua). Optional: without lua or holder.lua the launcher still runs, but the button may not sleep.
holder_start() {
    holder_stop
    refresh_copy holder.lua
    if [ ! -f "$RUN/holder.lua" ] || ! command -v lua >/dev/null 2>&1; then
        log "holder not started: lua or holder.lua missing"
        return
    fi
    lua "$RUN/holder.lua" >> "$LOG_OUT" 2>&1 &
    echo $! > "$HOLDER_PID"
    log "holder started (pid $!)"
}

# The holder must be gone before the framework starts: it holds com.lab126.kaf, the framework's own lipc name,
# and the framework cannot register while it is taken ("Duplicate service name"). TERM first, then KILL.
holder_stop() {
    hp=$(cat "$HOLDER_PID" 2>/dev/null)
    if [ -n "$hp" ]; then
        kill "$hp" 2>/dev/null
        i=0
        while kill -0 "$hp" 2>/dev/null && [ $i -lt 10 ]; do
            pause_us 200000
            i=$((i + 1))
        done
        if kill -0 "$hp" 2>/dev/null; then
            kill -KILL "$hp" 2>/dev/null
            log "holder did not stop on TERM; killed (pid $hp)"
        else
            log "holder stopped (pid $hp)"
        fi
    fi
    rm -f "$HOLDER_PID"
}

framework_stop() {
    log "stopping the framework ($INIT)"
    echo stopped > "$MARK"
    t0=$(date +%s)
    # Stopping the framework can signal processes it started; this script must survive that.
    trap "" TERM
    if [ "$INIT" = sysv ]; then
        /etc/init.d/framework stop >> "$LOG" 2>&1
    else
        stop lab126_gui >> "$LOG" 2>&1
        pause_us 1250000
        # Stopping the framework starts the stock boot actions again, and their progress bar (BootActionsApplication,
        # drawn by animator) would draw over the launcher: stop them as well.
        stop bootactions >> "$LOG" 2>&1 || true
        killall -q -s KILL animator 2>/dev/null || true
        killall -q -s KILL BootActionsApplication 2>/dev/null || true
    fi
    install_traps
    log "framework stopped after $(( $(date +%s) - t0 ))s"
    holder_start
}

# The stock lipc-daemon answers com.lab126.system properties (usid and others) by forking small scripts. It
# has been seen wedged after a launcher session: every script hangs, the framework never becomes ready (it
# waits for usid), and the library shows "application error". Check it before starting the framework, and
# restart it if it does not answer. The state is logged to help find what wedges it.
usid_answers() {
    if command -v timeout >/dev/null 2>&1; then
        timeout 5 lipc-get-prop com.lab126.system usid >/dev/null 2>&1
    else
        lipc-get-prop com.lab126.system usid >/dev/null 2>&1
    fi
}

lipcd_check() {
    [ "$INIT" = upstart ] || return 0
    command -v lipc-get-prop >/dev/null 2>&1 || return 0
    usid_answers && return 0
    log "lipc-daemon does not answer (usid); restarting lipcd"
    for p in $(pidof lipc-daemon 2>/dev/null); do
        log "lipc-daemon $p state $(awk '{print $3}' /proc/$p/stat 2>/dev/null) wchan $(cat /proc/$p/wchan 2>/dev/null)"
    done
    log "volumd state $(awk '{print $3}' /proc/$(pidof volumd 2>/dev/null)/stat 2>/dev/null)"
    restart lipcd >> "$LOG" 2>&1 || { stop lipcd; start lipcd; } >> "$LOG" 2>&1
    pause_us 2000000
    if usid_answers; then
        log "lipc-daemon answers again"
    else
        log "lipc-daemon still does not answer"
    fi
}

framework_start() {
    log "starting the framework ($INIT)"
    lipcd_check
    t0=$(date +%s)
    if [ "$INIT" = sysv ]; then
        (cd / && /etc/init.d/framework start) >> "$LOG" 2>&1
    else
        (cd / && start lab126_gui) >> "$LOG" 2>&1
    fi
    rm -f "$MARK"
    log "framework start returned after $(( $(date +%s) - t0 ))s"
}

volumd_pause() {
    if pidof volumd >/dev/null 2>&1; then
        killall -STOP volumd 2>/dev/null && echo paused > "$VOLUMD_MARK"
    fi
}

volumd_resume() {
    # Resumed whether or not the marker is there: a volumd stopped by an earlier run must run again.
    killall -CONT volumd 2>/dev/null
    rm -f "$VOLUMD_MARK"
}

# The Kindle UI is ready only after the framework emits framework_ready (about 40 s after the start), the
# library app (job kppmainapp) starts, registers its lipc service com.lab126.KPPMainApp, and then its
# properties (load, go). If this script ends earlier, the library's script runner ends with it and appmgrd sends
# "load" to a library that cannot take it yet ("ServiceUnknown", then "no such property"): "application error".
# Wait until the library app offers "load", up to 120 s, then a little longer.
library_registered() {
    command -v lipc-probe >/dev/null 2>&1 || return 0
    if command -v timeout >/dev/null 2>&1; then
        timeout 5 lipc-probe com.lab126.KPPMainApp 2>/dev/null | grep -qw "load"
    else
        lipc-probe com.lab126.KPPMainApp 2>/dev/null | grep -qw "load"
    fi
}

framework_wait_ready() {
    [ "$INIT" = upstart ] || return 0
    command -v initctl >/dev/null 2>&1 || return 0
    i=0
    while [ $i -lt 120 ]; do
        if initctl status kppmainapp 2>/dev/null | grep -q "start/running" && library_registered; then
            sleep 2
            log "the Kindle UI is ready after ${i}s"
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    log "the Kindle UI was not ready after 120s"
}

# The waiting screen ("Opening the Kindle UI" with a moving bar) covers the minute the stock UI takes to start.
wait_screen_start() {
    [ -x "$RUN/kl-launcher" ] || return 0
    "$RUN/kl-launcher" --wait-screen >> "$LOG_OUT" 2>&1 &
    echo $! > "$WAIT_PID"
}

wait_screen_stop() {
    wp=$(cat "$WAIT_PID" 2>/dev/null)
    if [ -n "$wp" ]; then
        kill "$wp" 2>/dev/null
        pause_us 300000
        kill -0 "$wp" 2>/dev/null && kill -KILL "$wp" 2>/dev/null
        log "wait screen stopped"
    fi
    rm -f "$WAIT_PID"
}

# Give the UI back: undo exactly what the markers record.
restore() {
    volumd_resume
    holder_stop
    if [ -f "$MARK" ] || ! framework_running; then
        wait_screen_start
        framework_start
        framework_wait_ready
        wait_screen_stop
    fi
}

# Copy a file from $DIR to $RUN when it is newer there (or the copy is missing).
refresh_copy() {
    if [ ! -f "$RUN/$1" ] || [ "$DIR/$1" -nt "$RUN/$1" ]; then
        cp "$DIR/$1" "$RUN/$1.new" 2>/dev/null && mv "$RUN/$1.new" "$RUN/$1" && log "copied $1 to $RUN"
    fi
}

refresh_files() {
    mkdir -p "$RUN/fonts"
    refresh_copy kl-launcher
    chmod 755 "$RUN/kl-launcher" 2>/dev/null
    refresh_copy fonts/NotoSans-Regular.ttf
    refresh_copy fonts/NotoSans-Bold.ttf
    refresh_copy holder.lua
}

# Logs what the USB cable and the drive do, and whether the launcher and this script are alive, each time one of
# them changes. It is the record for a launcher that disappears during a USB session.
usb_watch() {
    last=""
    while kill -0 "$1" 2>/dev/null; do
        cable=$(cat /sys/class/power_supply/max77796-charger/online 2>/dev/null)
        mounted=$(grep -c " /mnt/base-us " /proc/mounts 2>/dev/null)
        vstate=$(awk '{print $3}' /proc/$(pidof volumd 2>/dev/null)/stat 2>/dev/null)
        lp=$(cat "$LAUNCHER_PID" 2>/dev/null)
        lalive=no
        [ -n "$lp" ] && kill -0 "$lp" 2>/dev/null && lalive=yes
        now="cable=$cable userstore=${mounted:-?} volumd=${vstate:-none} launcher=${lp:-none}/$lalive supervisor=$1"
        if [ "$now" != "$last" ]; then
            log "usb: $now"
            last="$now"
        fi
        sleep 1
    done
}

watchdog() {
    trap '' HUP INT
    sup=$1
    strikes=0
    while kill -0 "$sup" 2>/dev/null; do
        sleep "$WATCH_EVERY"
        lp=$(cat "$LAUNCHER_PID" 2>/dev/null)
        if [ -n "$lp" ] && kill -0 "$lp" 2>/dev/null; then
            hb=$(cat "$RUN/heartbeat" 2>/dev/null)
            age=$(( $(date +%s) - ${hb:-0} ))
            # Two checks in a row: after a wake from sleep the heartbeat is old until the launcher's next tick.
            if [ "$age" -gt "$HEARTBEAT_LIMIT" ]; then
                strikes=$((strikes + 1))
                if [ "$strikes" -ge 2 ]; then
                    echo "[watchdog $(date '+%H:%M:%S')] no heartbeat for ${age}s; stopping the launcher (pid $lp)" >> "$LOG"
                    kill -KILL "$lp" 2>/dev/null
                    strikes=0
                fi
            else
                strikes=0
            fi
        else
            strikes=0
        fi
    done
    if [ -f "$MARK" ] || [ -f "$VOLUMD_MARK" ]; then
        echo "[watchdog $(date '+%H:%M:%S')] supervisor gone; giving the UI back" >> "$LOG"
        restore
    fi
}

finish() {
    restore
    wait_screen_stop
    logger_stop
    rm -f "$PIDFILE" "$LAUNCHER_PID" "$APP_PID" "$HOLDER_PID"
    log "end"
}

on_signal() {
    log "signal received; stopping"
    for f in "$LAUNCHER_PID" "$APP_PID"; do
        p=$(cat "$f" 2>/dev/null)
        [ -n "$p" ] && kill -TERM "$p" 2>/dev/null
    done
    finish
    exit 0
}

run_app() {
    if [ ! -f "$RUN/next" ]; then
        log "launch requested but $RUN/next is missing"
        return
    fi
    { read -r adir; read -r kind; read -r arg; } < "$RUN/next"
    rm -f "$RUN/next"
    log "app start: $kind $arg (in $adir)"
    t0=$(date +%s)
    # While an app runs, a cable must not take /mnt/us from under it: volumd is paused until the app ends.
    volumd_pause
    (
        cd "$adir" 2>/dev/null || cd /mnt/us 2>/dev/null || cd /
        export PRIMITIVE_LAUNCHER=1 PRIMITIVE_FRAMEWORK=stopped
        case "$kind" in
            script) exec /bin/sh "$arg" ;;
            hooks) exec /bin/sh -c '. "$1"; on_run' sh "$arg" ;;
            shell) exec /bin/sh -c "$arg" ;;
            *) echo "unknown kind: $kind"; exit 1 ;;
        esac
    ) >> "$LOG" 2>&1 &
    apid=$!
    echo "$apid" > "$APP_PID"
    wait "$apid"
    rc=$?
    rm -f "$APP_PID"
    log "app exit: rc=$rc after $(( $(date +%s) - t0 ))s"
    # Apps written for the stock UI may start it again on exit, or resume volumd.
    if framework_running; then
        log "the app left the framework running; stopping it again"
        framework_stop
    fi
    volumd_resume
}

logger_start
install_traps
if framework_running; then
    framework_stop
else
    # Already stopped (an earlier run left it so): the marker makes leaving start it again, and the holder is what
    # keeps the launcher's sleep and wake working without the framework.
    log "framework not running; it will be started when the launcher leaves"
    echo stopped > "$MARK"
    holder_start
fi
volumd_resume
watchdog $$ &
usb_watch $$ &
trap 'log "supervisor exit (status $?)"' EXIT

crashes=""
while :; do
    refresh_files
    if [ ! -x "$RUN/kl-launcher" ]; then
        log "no launcher binary in $DIR; giving up"
        break
    fi
    date +%s > "$RUN/heartbeat"
    rm -f "$RUN/next"
    "$RUN/kl-launcher" >> "$LOG_OUT" 2>&1 &
    lp=$!
    echo "$lp" > "$LAUNCHER_PID"
    wait "$lp"
    rc=$?
    rm -f "$LAUNCHER_PID"
    case "$rc" in
        10)
            run_app
            ;;
        0)
            log "the Kindle UI was requested"
            break
            ;;
        143)
            log "launcher stopped (TERM); starting it again"
            ;;
        *)
            now=$(date +%s)
            recent=""
            for t in $crashes; do
                [ $((now - t)) -lt "$CRASH_WINDOW" ] && recent="$recent $t"
            done
            crashes="$recent $now"
            count=$(echo $crashes | wc -w)
            log "launcher crashed (rc=$rc), $count in ${CRASH_WINDOW}s"
            if [ "$count" -ge "$MAX_CRASHES" ]; then
                log "too many crashes; giving up"
                break
            fi
            sleep 1
            ;;
    esac
done

finish
exit 0
