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

# A trial needs the caller to see mirror and job failures before accepting the candidate.
KL_SETUP_BOOT_STRICT=1 PATH="/usr/bin:/bin" sh "$SETUP" "$DIR"
check "strict mode reports the job failure" "$?" "1"
check "strict mode leaves the old job" "$(cat "$JOB")" "job 2"

# A missing launcher folder file is skipped, not an error.
rm -f "$DIR/fonts/NotoSans-Regular.ttf"
PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "missing file: exit status 0" "$?" "0"

# A full mirror filesystem must leave the previous copy and boot job intact.
cat > "$STUB/df" <<'EOF'
#!/bin/sh
printf '%s\n' 'Filesystem 1024-blocks Used Available Capacity Mounted on' 'fixture 1024 1024 0 100% /var/local'
EOF
chmod 755 "$STUB/df"
echo "launcher 3" > "$DIR/kl-launcher"
echo "job 4" > "$DIR/primitive-launcher.conf"
KL_SETUP_BOOT_STRICT=1 PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "no mirror space: strict failure" "$?" "1"
check "no mirror space: launcher kept" "$(cat "$M/kl-launcher")" "launcher 2"
check "no mirror space: job kept" "$(cat "$JOB")" "job 3"
check "no mirror space: useful log" "$(grep -c 'check free space' "$LOG")" "1"
rm -f "$STUB/df"

cat > "$STUB/cat" <<'EOF'
#!/bin/sh
case "$1" in
    */kl-launcher) printf 'incomplete'; exit 1 ;;
esac
exec /bin/cat "$@"
EOF
chmod 755 "$STUB/cat"
KL_SETUP_BOOT_STRICT=1 PATH="$STUB:$PATH" sh "$SETUP" "$DIR"
check "interrupted mirror write: strict failure" "$?" "1"
check "interrupted mirror write: launcher kept" "$(cat "$M/kl-launcher")" "launcher 2"
check "interrupted mirror write: temporary copy removed" "$([ -e "$M/kl-launcher.new" ] && echo yes || echo no)" "no"

# A new launcher replaces the whole copy: a stale file from the old install goes, the SSH host key stays, and no
# temporary folder is left behind.
echo "host key" > "$M/ssh/dropbear_ed25519_host_key"
echo "leftover" > "$M/old-component.txt"
echo "launcher 3" > "$DIR/kl-launcher"
BEFORE=$(grep -c "replaced the boot copy" "$LOG")
KL_SETUP_BOOT_STRICT=0 sh "$SETUP" "$DIR"
check "replace: new launcher copied" "$(cat "$M/kl-launcher")" "launcher 3"
check "replace: stale file removed" "$([ -e "$M/old-component.txt" ] && echo yes || echo no)" "no"
check "replace: host key kept" "$(cat "$M/ssh/dropbear_ed25519_host_key")" "host key"
check "replace: no temporary folders left" "$(ls "$ROOT" | grep -c '^mirror\.')" "0"
check "replace: replacement logged" "$(( $(grep -c 'replaced the boot copy' "$LOG") - BEFORE ))" "1"

# Nothing differs: the folder is not replaced (same directory).
INO=$(ls -di "$M" | awk '{print $1}')
sh "$SETUP" "$DIR"
check "unchanged: folder not replaced" "$(ls -di "$M" | awk '{print $1}')" "$INO"

echo "setup-boot tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
