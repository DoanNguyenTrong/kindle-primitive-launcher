#!/bin/sh
# Runs deploy/setup-boot.sh on the Mac in temporary folders: the copy on /var/local, the boot job, the second run
# (nothing copied), a changed file, and the root filesystem not being writable.
#
#   sh tests/setup-boot-test.sh
set -u

HERE=$(cd "$(dirname "$0")/.." && pwd)
SETUP="$HERE/deploy/setup-boot.sh"
PASS=0
FAIL=0

check() {
    if [ "$2" = "$3" ]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        echo "FAIL: $1 (expected '$3', got '$2')"
    fi
}

ROOT=$(mktemp -d /tmp/kl-setup-test.XXXXXX)
trap 'rm -rf "$ROOT"' EXIT

DIR="$ROOT/launcher"
M="$ROOT/mirror"
JOB="$ROOT/upstart/primitive-launcher.conf"
LOG="$DIR/log-launcher.txt"
export KL_MIRROR="$M" KL_JOB="$JOB" KL_LOG="$LOG"

mkdir -p "$DIR/fonts" "$DIR/ssh/bin" "$DIR/ssh/lib" "$ROOT/upstart"
echo "launcher 1" > "$DIR/kl-launcher"
echo "supervisor" > "$DIR/supervisor.sh"
echo "boot" > "$DIR/boot.sh"
echo "holder" > "$DIR/holder.lua"
echo "ssh" > "$DIR/ssh.sh"
echo "regular" > "$DIR/fonts/NotoSans-Regular.ttf"
echo "bold" > "$DIR/fonts/NotoSans-Bold.ttf"
echo "dropbear" > "$DIR/ssh/bin/dropbear"
echo "zlib" > "$DIR/ssh/lib/libz.so.1"
echo "job" > "$DIR/primitive-launcher.conf"

# A stub mntroot that records its calls and succeeds.
STUB="$ROOT/bin"
mkdir -p "$STUB"
cat > "$STUB/mntroot" <<EOF
#!/bin/sh
echo "\$*" >> "$ROOT/mntroot.calls"
EOF
chmod 755 "$STUB/mntroot"

# First run: everything is copied, and the job is written through mntroot rw and ro.
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "launcher copied" "$(cat "$M/kl-launcher" 2>/dev/null)" "launcher 1"
check "supervisor copied" "$(cat "$M/supervisor.sh" 2>/dev/null)" "supervisor"
check "boot copied" "$(cat "$M/boot.sh" 2>/dev/null)" "boot"
check "holder copied" "$(cat "$M/holder.lua" 2>/dev/null)" "holder"
check "ssh.sh copied" "$(cat "$M/ssh.sh" 2>/dev/null)" "ssh"
check "font copied" "$(cat "$M/fonts/NotoSans-Bold.ttf" 2>/dev/null)" "bold"
check "dropbear copied" "$(cat "$M/ssh/bin/dropbear" 2>/dev/null)" "dropbear"
check "zlib copied" "$(cat "$M/ssh/lib/libz.so.1" 2>/dev/null)" "zlib"
check "launcher binary is executable" "$([ -x "$M/kl-launcher" ] && echo yes)" "yes"
check "job written" "$(cat "$JOB" 2>/dev/null)" "job"
check "mntroot rw then ro" "$(cat "$ROOT/mntroot.calls" | tr '\n' ' ')" "rw ro "

# Second run: nothing changed, so nothing is copied or written.
: > "$LOG"
rm -f "$ROOT/mntroot.calls"
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "second run copies nothing" "$(grep -c copied "$LOG" 2>/dev/null)" "0"
check "second run does not touch the root filesystem" "$([ -f "$ROOT/mntroot.calls" ] && echo called || echo none)" "none"

# A changed launcher is copied again; the job is still the same.
echo "launcher 2" > "$DIR/kl-launcher"
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "changed launcher copied" "$(cat "$M/kl-launcher")" "launcher 2"

# A changed job is written again.
echo "job 2" > "$DIR/primitive-launcher.conf"
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "changed job written" "$(cat "$JOB")" "job 2"

# No mntroot (the root filesystem cannot be made writable here): the job is not written, and the run still ends cleanly.
echo "job 3" > "$DIR/primitive-launcher.conf"
PATH="/usr/bin:/bin" sh "$SETUP" "$DIR"
status=$?
check "no mntroot: exit status 0" "$status" "0"
check "no mntroot: job left as it was" "$(cat "$JOB")" "job 2"
check "no mntroot: logged" "$(grep -c 'not writable' "$LOG")" "1"

# A missing launcher folder file is skipped, not an error.
rm -f "$DIR/fonts/NotoSans-Regular.ttf"
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "missing file: exit status 0" "$?" "0"

echo "setup-boot tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
