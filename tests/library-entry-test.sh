#!/bin/sh
# Checks deploy/library-entry.sh on the Mac with a stub supervisor: the entry waits until the supervisor
# ends (so the Kindle's app manager leaves the stock UI alone), ignores a stale pid file, and returns at once
# when a supervisor is already running.
#   sh tests/library-entry-test.sh
HERE=$(cd "$(dirname "$0")/.." && pwd)
PASS=0
FAIL=0
check() { if eval "$2"; then PASS=$((PASS + 1)); else FAIL=$((FAIL + 1)); echo "FAIL: $1"; fi; }

ROOT=$(mktemp -d /tmp/kl-entry-test.XXXXXX)
export KL_DIR="$ROOT/dir" KL_RUN="$ROOT/run"
mkdir -p "$KL_DIR" "$KL_RUN"
# The stub supervisor writes its pid like the real one, runs 4 s, removes the pid file and ends.
cat > "$KL_DIR/supervisor.sh" <<'SUP'
#!/bin/sh
echo $$ > "$KL_RUN/supervisor.pid"
# Does TERM reach a child of the supervisor? (It must: the supervisor stops the holder and launcher with it.)
sleep 30 &
child=$!
kill -TERM "$child" 2>/dev/null
sleep 1
if kill -0 "$child" 2>/dev/null; then echo ignored > "$KL_RUN/term"; kill -KILL "$child"; else echo delivered > "$KL_RUN/term"; fi
sleep 3
rm -f "$KL_RUN/supervisor.pid"
SUP

echo "test: the entry waits for the supervisor, even with a stale pid file"
echo 999999 > "$KL_RUN/supervisor.pid"
t0=$(date +%s)
sh "$HERE/deploy/library-entry.sh"
took=$(( $(date +%s) - t0 ))
check "waited for the supervisor (took ${took}s)" '[ "$took" -ge 4 ]'
check "did not wait much longer than needed (took ${took}s)" '[ "$took" -le 9 ]'
check "TERM still works inside the supervisor" '[ "$(cat "$KL_RUN/term" 2>/dev/null)" = delivered ]'

echo "test: a running supervisor means the entry returns at once"
sleep 30 &
holder=$!
echo "$holder" > "$KL_RUN/supervisor.pid"
t0=$(date +%s)
sh "$HERE/deploy/library-entry.sh"
took=$(( $(date +%s) - t0 ))
check "returned at once (took ${took}s)" '[ "$took" -le 1 ]'
kill "$holder" 2>/dev/null

rm -rf "$ROOT"
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
