#!/bin/sh
# Checks deploy/run-koreader.sh on the Mac with a stub KOReader and stub start/stop.
#   sh tests/koreader-wrapper-test.sh
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(mktemp -d /tmp/kl-kr-test.XXXXXX)
PASS=0
FAIL=0
check() { if eval "$2"; then PASS=$((PASS + 1)); else FAIL=$((FAIL + 1)); echo "FAIL: $1"; fi; }

mkdir -p "$ROOT/koreader" "$ROOT/bin"
cat > "$ROOT/bin/start" <<'STUB'
#!/bin/sh
echo "real start $*" >> "$CALLS"
STUB
cat > "$ROOT/koreader/koreader.sh" <<'KO'
#!/bin/sh
echo "koreader args: $*" >> "$CALLS"
start lab126_gui
stop lab126_gui
start other-job
KO
chmod 755 "$ROOT/bin/start" "$ROOT/koreader/koreader.sh"
export CALLS="$ROOT/calls" KL_KOREADER_DIR="$ROOT/koreader" KL_SHIM_DIR="$ROOT/shim"
: > "$CALLS"
# The stub start is the "real" one the shim should reach; put it on the PATH the wrapper sees.
PATH="$ROOT/bin:$PATH" sh "$HERE/deploy/run-koreader.sh"

check "KOReader got the framework-stopped flag" 'grep -q "koreader args: --framework_stop" "$CALLS"'
check "start lab126_gui was swallowed" '! grep -q "real start lab126_gui" "$CALLS"'
check "stop lab126_gui was swallowed" '! grep -q "real stop lab126_gui" "$CALLS"'
check "other jobs pass through" 'grep -q "real start other-job" "$CALLS"'
check "shims are executable" '[ -x "$ROOT/shim/start" ] && [ -x "$ROOT/shim/stop" ]'
rm -rf "$ROOT"
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
