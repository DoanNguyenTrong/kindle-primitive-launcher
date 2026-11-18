#!/bin/sh
# KOReader, started by Primitive Launcher: the launcher runs KOReader's library entry (KOReader.sh) through this
# wrapper unless apps.toml gives KOReader another command.
#
# The launcher has already stopped the stock framework and owns the screen, so KOReader does not need it.
# KOReader's framework-stopped mode (--framework_stop) does the stop itself, then starts the framework again on
# exit. That restart would bring the stock UI back behind the launcher, so `start lab126_gui` and
# `stop lab126_gui` are turned into no-ops for KOReader by a PATH shim. Every other start and stop is passed on.
# KOReader sets com.lab126.kaf frameworkStarted and unloads the stock screensaver itself in this mode.
#
# Environment (tests): KL_KOREADER_DIR (default /mnt/us/koreader), KL_SHIM_DIR (default /var/tmp/kl-shim).
KOREADER=${KL_KOREADER_DIR:-/mnt/us/koreader}
SHIM=${KL_SHIM_DIR:-/var/tmp/kl-shim}

mkdir -p "$SHIM" || exit 1
for cmd in start stop; do
    real=$(command -v "$cmd" 2>/dev/null)
    [ -n "$real" ] || real=/sbin/$cmd
    cat > "$SHIM/$cmd" <<SHIM_EOF
#!/bin/sh
case "\$1" in lab126_gui) exit 0 ;; esac
exec "$real" "\$@"
SHIM_EOF
    chmod 755 "$SHIM/$cmd"
done

cd "$KOREADER" || exit 1
PATH="$SHIM:$PATH" exec sh "$KOREADER/koreader.sh" --framework_stop
