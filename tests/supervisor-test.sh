#!/bin/sh
# Runs deploy/supervisor.sh on the Mac against stub Kindle commands (start, stop, status, pidof, killall,
# usleep) and a fake launcher whose exit codes are scripted. Checks the exit-code protocol, the framework
# and volumd markers, the crash limit, the watchdog and the heartbeat.
#
#   sh tests/supervisor-test.sh
set -u

HERE=$(cd "$(dirname "$0")/.." && pwd)
SUP="$HERE/deploy/supervisor.sh"
PASS=0
FAIL=0

setup() {
    ROOT=$(mktemp -d /tmp/kl-sup-test.XXXXXX)
    export KL_DIR="$ROOT/dir" KL_RUN="$ROOT/run" STATE="$ROOT/state"
    mkdir -p "$KL_DIR/fonts" "$STATE" "$ROOT/bin"
    : > "$KL_DIR/fonts/NotoSans-Regular.ttf"
    : > "$KL_DIR/fonts/NotoSans-Bold.ttf"
    : > "$KL_DIR/holder.lua"
    echo running > "$STATE/fw"
    : > "$STATE/volumd"
    : > "$STATE/calls"
    cat > "$ROOT/bin/status" <<'EOF'
#!/bin/sh
if [ "$(cat "$STATE/fw")" = running ]; then echo "$1 start/running, process 99"; else echo "$1 stop/waiting"; fi
EOF
    cat > "$ROOT/bin/stop" <<'EOF'
#!/bin/sh
echo "stop $*" >> "$STATE/calls"; echo stopped > "$STATE/fw"
EOF
    cat > "$ROOT/bin/start" <<'EOF'
#!/bin/sh
echo "start $*" >> "$STATE/calls"; echo running > "$STATE/fw"
EOF
    cat > "$ROOT/bin/pidof" <<'EOF'
#!/bin/sh
case "$1" in
    volumd) [ -f "$STATE/volumd" ] ;;
    cvm) [ "$(cat "$STATE/fw")" = running ] ;;
    *) exit 1 ;;
esac
EOF
    cat > "$ROOT/bin/killall" <<'EOF'
#!/bin/sh
echo "killall $*" >> "$STATE/calls"
EOF
    cat > "$ROOT/bin/lipc-set-prop" <<'EOF'
#!/bin/sh
echo "lipc-set-prop $*" >> "$STATE/calls"
EOF
    # lipc-daemon: answers usid unless the test marks it wedged; "restart lipcd" clears that.
    cat > "$ROOT/bin/lipc-get-prop" <<'EOF'
#!/bin/sh
[ -f "$STATE/lipcd-wedged" ] && exit 1
echo G000TEST
EOF
    cat > "$ROOT/bin/restart" <<'EOF'
#!/bin/sh
echo "restart $*" >> "$STATE/calls"
[ "$1" = lipcd ] && rm -f "$STATE/lipcd-wedged"
EOF
    cat > "$ROOT/bin/usleep" <<'EOF'
#!/bin/sh
exit 0
EOF
    # The fake launcher: takes the next scripted code from $STATE/codes.
    cat > "$KL_DIR/kl-launcher" <<'EOF'
#!/bin/sh
if [ "$1" = --wait-screen ]; then echo "wait screen shown" >> "$STATE/calls"; exec sleep 1000; fi
code=$(head -n 1 "$STATE/codes")
tail -n +2 "$STATE/codes" > "$STATE/codes.tmp"; mv "$STATE/codes.tmp" "$STATE/codes"
echo "launcher run: $code" >> "$STATE/calls"
case "$code" in
    10) printf '%s\nscript\n%s\n' "$STATE" "$STATE/app.sh" > "$KL_RUN/next"; exit 10 ;;
    term) kill -TERM $$ ;;
    hang) while :; do sleep 1; done ;;
    "") exit 0 ;;
    *) exit "$code" ;;
esac
EOF
    cat > "$STATE/app.sh" <<'EOF'
echo "app ran: PRIMITIVE_LAUNCHER=$PRIMITIVE_LAUNCHER pwd=$(pwd)" >> "$STATE/calls"
[ -f "$STATE/app-starts-framework" ] && start lab126_gui
exit 3
EOF
    # The holder: a stub lua that records its start and sleeps (exec, so the pid is the sleep's pid). With
    # $STATE/holder-ignores-term it ignores TERM, as when it inherited an ignored TERM.
    cat > "$ROOT/bin/lua" <<'EOF'
#!/bin/sh
echo "holder run: $*" >> "$STATE/calls"
if [ -f "$STATE/holder-ignores-term" ]; then
    trap '' TERM
    while :; do sleep 1; done
fi
exec sleep 1000
EOF
    # lipc-probe: the library app's lipc service exists once the framework runs and the app has registered
    # ($STATE/registered-after holds how many probes to fail first).
    cat > "$ROOT/bin/lipc-probe" <<'EOF'
#!/bin/sh
# "lipc-probe com.lab126.KPPMainApp" lists the library app's properties once it is ready.
n=$(cat "$STATE/registered-after" 2>/dev/null || echo 0)
if [ "$n" -gt 0 ]; then echo $((n - 1)) > "$STATE/registered-after"; printf '\tw\tStr\tpause\n'; exit 0; fi
[ "$(cat "$STATE/fw")" = running ] && printf '\tw\tStr\tpause\n\tw\tStr\tload\n\tw\tStr\tgo\n'
exit 0
EOF
    # initctl: the library app is running once the framework has been started.
    cat > "$ROOT/bin/initctl" <<'EOF'
#!/bin/sh
if [ "$1 $2" = "status kppmainapp" ] && [ "$(cat "$STATE/fw")" = running ]; then echo "kppmainapp start/running, process 7"; else echo "$2 stop/waiting"; fi
EOF
    chmod 755 "$ROOT/bin"/* "$KL_DIR/kl-launcher"
    export PATH="$ROOT/bin:$PATH"
}

teardown() {
    rm -rf "$ROOT"
}

check() {
    if eval "$2"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        echo "FAIL: $1"
        echo "--- calls"; cat "$STATE/calls"; echo "--- log"; cat "$KL_DIR/log-launcher.txt" 2>/dev/null
    fi
}

run_sup() {
    # A time limit, so a broken supervisor cannot hang the test.
    sh "$SUP" &
    sp=$!
    i=0
    while kill -0 "$sp" 2>/dev/null && [ $i -lt 30 ]; do sleep 1; i=$((i + 1)); done
    kill -0 "$sp" 2>/dev/null && { echo "supervisor did not end"; kill -9 "$sp"; }
    sleep 1
}

echo "test: launch an app, then ask for the Kindle UI"
setup
printf '10\n0\n' > "$STATE/codes"
run_sup
check "framework stopped first" 'head -n 1 "$STATE/calls" | grep -q "stop lab126_gui"'
check "volumd paused" 'grep -q "killall -STOP volumd" "$STATE/calls"'
check "app ran with the env" 'grep -q "app ran: PRIMITIVE_LAUNCHER=1 pwd=$STATE" "$STATE/calls"'
check "launcher ran twice" '[ "$(grep -c "launcher run" "$STATE/calls")" = 2 ]'
check "framework running at the end" '[ "$(cat "$STATE/fw")" = running ]'
check "volumd resumed" 'grep -q "killall -CONT volumd" "$STATE/calls"'
check "markers removed" '[ ! -f "$KL_RUN/framework-stopped" ] && [ ! -f "$KL_RUN/volumd-paused" ] && [ ! -f "$KL_RUN/supervisor.pid" ]'
check "binary copied to run" '[ -x "$KL_RUN/kl-launcher" ]'
teardown

echo "test: the holder runs while the framework is stopped, and stops before the UI returns"
setup
printf '10\n0\n' > "$STATE/codes"
run_sup
check "holder started" 'grep -q "holder run" "$STATE/calls"'
check "holder stopped" 'grep -q "holder stopped" "$KL_DIR/log-launcher.txt"'
check "holder pid removed" '[ ! -f "$KL_RUN/holder.pid" ]'
check "holder copied to run" '[ -f "$KL_RUN/holder.lua" ]'
teardown

echo "test: a wedged lipc-daemon is restarted before the framework starts"
setup
: > "$STATE/lipcd-wedged"
printf '0\n' > "$STATE/codes"
run_sup
check "lipcd restarted" 'grep -q "restart lipcd" "$STATE/calls"'
check "logged and recovered" 'grep -q "lipc-daemon answers again" "$KL_DIR/log-launcher.txt"'
check "restart before the framework start" '[ "$(grep -n "restart lipcd" "$STATE/calls" | cut -d: -f1)" -lt "$(grep -n "start lab126_gui" "$STATE/calls" | cut -d: -f1)" ]'
teardown

echo "test: a healthy lipc-daemon is left alone"
setup
printf '0\n' > "$STATE/codes"
run_sup
check "no lipcd restart" '! grep -q "restart lipcd" "$STATE/calls"'
teardown

echo "test: a holder that ignores TERM is killed before the framework starts"
setup
: > "$STATE/holder-ignores-term"
printf '0\n' > "$STATE/codes"
run_sup
check "killed with KILL" 'grep -q "holder did not stop on TERM; killed" "$KL_DIR/log-launcher.txt"'
check "no holder left" '! pgrep -f "$KL_RUN/holder.lua" >/dev/null'
teardown

echo "test: the supervisor waits until the library app has registered"
setup
printf '0\n' > "$STATE/codes"
echo 3 > "$STATE/registered-after"
run_sup
check "ready logged once the library offers load" 'grep -q "the Kindle UI is ready after 3s" "$KL_DIR/log-launcher.txt"'
teardown

echo "test: the waiting screen covers the Kindle UI start and is stopped after"
setup
printf '0\n' > "$STATE/codes"
run_sup
check "shown" 'grep -q "wait screen shown" "$STATE/calls"'
check "stopped" 'grep -q "wait screen stopped" "$KL_DIR/log-launcher.txt"'
check "stopped after the Kindle UI was ready" '[ "$(grep -n "wait screen stopped" "$KL_DIR/log-launcher.txt" | cut -d: -f1)" -gt "$(grep -n "the Kindle UI is ready" "$KL_DIR/log-launcher.txt" | cut -d: -f1)" ]'
check "no wait pid left" '[ ! -f "$KL_RUN/wait.pid" ]'
teardown

echo "test: three crashes give up and restore"
setup
printf '1\n1\n1\n1\n' > "$STATE/codes"
run_sup
check "three runs" '[ "$(grep -c "launcher run" "$STATE/calls")" = 3 ]'
check "gave up" 'grep -q "too many crashes" "$KL_DIR/log-launcher.txt"'
check "framework restored" '[ "$(cat "$STATE/fw")" = running ]'
teardown

echo "test: TERM restarts without counting as a crash"
setup
printf 'term\nterm\nterm\n0\n' > "$STATE/codes"
run_sup
check "four runs" '[ "$(grep -c "launcher run" "$STATE/calls")" = 4 ]'
check "no give up" '! grep -q "too many crashes" "$KL_DIR/log-launcher.txt"'
teardown

echo "test: an app that starts the framework is undone"
setup
touch "$STATE/app-starts-framework"
printf '10\n0\n' > "$STATE/codes"
run_sup
check "noticed" 'grep -q "left the framework running" "$KL_DIR/log-launcher.txt"'
check "stopped twice" '[ "$(grep -c "stop lab126_gui" "$STATE/calls")" = 2 ]'
teardown

echo "test: the watchdog restores the UI when the supervisor is killed"
setup
export KL_WATCH_EVERY=1
printf 'hang\n' > "$STATE/codes"
sh "$SUP" &
sp=$!
sleep 2
check "framework stopped while running" '[ "$(cat "$STATE/fw")" = stopped ]'
kill -9 "$sp"
sleep 3
check "watchdog restored" '[ "$(cat "$STATE/fw")" = running ]'
check "watchdog logged" 'grep -q "supervisor gone" "$KL_DIR/log-launcher.txt"'
check "watchdog released the holder" 'grep -q "holder stopped" "$KL_DIR/log-launcher.txt"'
pkill -f "$KL_RUN/kl-launcher" 2>/dev/null
teardown

echo "test: a launcher without heartbeat is killed and restarted"
setup
export KL_WATCH_EVERY=1 KL_HEARTBEAT_LIMIT=1
printf 'hang\n0\n' > "$STATE/codes"
run_sup
check "killed by the watchdog" 'grep -q "no heartbeat" "$KL_DIR/log-launcher.txt"'
check "restarted and ended" '[ "$(grep -c "launcher run" "$STATE/calls")" = 2 ] && [ "$(cat "$STATE/fw")" = running ]'
unset KL_WATCH_EVERY KL_HEARTBEAT_LIMIT
teardown

echo "test: a second start does nothing"
setup
printf 'hang\n' > "$STATE/codes"
mkdir -p "$KL_RUN"
sleep 100 &
holder=$!
echo "$holder" > "$KL_RUN/supervisor.pid"
sh "$SUP"
check "already running" 'grep -q "already running" "$KL_DIR/log-launcher.txt"'
check "framework untouched" '[ "$(cat "$STATE/fw")" = running ] && [ ! -s "$STATE/calls" ]'
kill "$holder"
teardown

echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
