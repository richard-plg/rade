#!/usr/bin/env bash
# M10 end-to-end test of the .deb (ADR-0006): extract
# target/debian/rade_<ver>_<arch>.deb into a temp root (no install, no root)
# and run ITS rade-app and rade in headless Weston: libghostty found via
# RUNPATH $ORIGIN (LD_LIBRARY_PATH unset), Installed flavor (rade.sock,
# $XDG_DATA_HOME/rade), bundled terminfo / shell integration in a Pane
# (TERM, infocmp, OSC 133 command finished), the matching CLI on the Pane's PATH.
# Usage: tests/e2e/m10.sh   (builds the .deb unless NO_BUILD=1; KEEP=1 keeps the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=$ROOT/target/e2e-m10
if [[ -z ${RADE_E2E_BUS:-} && ${NO_BUILD:-} != 1 ]]; then "$ROOT/scripts/package-deb.sh"; fi
export NO_BUILD=1
source "$HERE/lib.sh"

DEB=$(ls "$ROOT"/target/debian/rade_*_"$(dpkg --print-architecture)".deb)
# Outside any Cargo target dir (TMPDIR may be target/tmp): a binary under a
# dir with CACHEDIR.TAG is the Devel flavor (rade-core paths.rs).
R=$(mktemp -d /tmp/rade-m10-root.XXXXXX)
m10_cleanup() { rm -rf "$R"; cleanup; }
trap m10_cleanup EXIT
dpkg-deb -x "$DEB" "$R"
unset LD_LIBRARY_PATH GHOSTTY_RESOURCES_DIR TERMINFO GHOSTTY_SHELL_INTEGRATION_XDG_DIR
BIN=$R/usr/bin # start_app runs $BIN/rade-app: the /usr/bin symlink
export PATH=$R/usr/bin:${PATH#"$ROOT/target/release:"}
field() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }

echo "== package layout"
check "rade on PATH is the extracted one" test "$(command -v rade)" = "$R/usr/bin/rade"
check "rade-app RUNPATH is \$ORIGIN" bash -c "readelf -d '$R/usr/lib/rade/rade-app' | grep -q 'RUNPATH.*\[\$ORIGIN\]'"
check "libghostty resolves next to rade-app (via the symlink too)" \
    bash -c "ldd '$R/usr/bin/rade-app' | grep -q 'libghostty-internal.so => $R/usr/lib/rade/libghostty-internal.so'"
check "no 'not found' libraries" bash -c "! ldd '$R/usr/lib/rade/rade-app' | grep -q 'not found'"
check "rade-install is in the package and executable" test -x "$R/usr/lib/rade/rade-install"
check "desktop file valid" desktop-file-validate "$R/usr/share/applications/dev.rade.App.desktop"

mkdir -p "$XDG_CONFIG_HOME/rade"
printf '[agents.fake]\ncommand = "true"\n' > "$XDG_CONFIG_HOME/rade/config.toml"

echo "== start the extracted app"
start_app
check "Installed flavor: default socket rade.sock" test -S "$XDG_RUNTIME_DIR/rade/rade.sock"
check "…not the dev socket" test ! -e "$XDG_RUNTIME_DIR/rade/rade-dev.sock"
check "…state not under rade-dev" test ! -e "$XDG_DATA_HOME/rade-dev"
p=$(rade list --json | jq -r '.panes[0].pane')
eventually 10 screen_has "$p" '$' || true
rade send "$p" 'echo "T:$TERM R:$GHOSTTY_RESOURCES_DIR C:$(command -v rade)"' --enter
check "TERM=xterm-ghostty, resources from /usr/share/rade/ghostty, rade from /usr/lib/rade" \
    eventually 5 screen_has "$p" "T:xterm-ghostty R:$R/usr/share/rade/ghostty C:$R/usr/lib/rade/rade"
rade send "$p" 'infocmp xterm-ghostty >/dev/null && echo INFOCMP-$((6*7))' --enter
check "infocmp xterm-ghostty (bundled terminfo)" eventually 5 screen_has "$p" INFOCMP-42
shot m10-extracted
# Shell integration: a command typed in the shell that registers as an Agent
# and returns to the prompt → `exited` via OSC 133 D (as M6).
rade send "$p" "bash -c 'rade status --agent fake </dev/null; sleep 1'" --enter >/dev/null
check "hand-started Agent registered" eventually 5 is "field $p agent" fake
check "OSC 133 command finished → exited" eventually 10 is "field $p status" exited
# A second launch must not touch the running app (review M10-1): it shows
# the first window and exits; socket and state stay.
before=$(rade list --json)
check "second instance exits 0 at once" timeout 10 "$BIN/rade-app"
check "…the first one still answers on its socket" test "$(rade list --json)" = "$before"

# Dev and installed side by side: own sockets, own state dirs, each CLI
# reaches its own app.
"$ROOT/target/release/rade-app" >>"$OUT/app-dev.log" 2>&1 &
DEV_PID=$!
check "dev app serves rade-dev.sock next to it" eventually 15 "$ROOT/target/release/rade" list
check "…installed CLI still reaches the installed app" test "$(rade list --json)" = "$before"
"$ROOT/target/release/rade" debug quit >/dev/null || true
wait "$DEV_PID" 2>/dev/null || true
check "…dev state saved under rade-dev" test -f "$XDG_DATA_HOME/rade-dev/state.json"
check "…installed app unaffected" test "$(rade list --json)" = "$before"

# Desktop notifications keep the freedesktop backend (M10 decision); GLib
# names the app by its id in the desktop-entry hint → dev.rade.App.desktop.
dbus-monitor --session "interface='org.freedesktop.Notifications',member='Notify'" > "$OUT/notify-monitor.log" 2>&1 &
MON=$!
sleep 0.5
RADE_PANE_ID=$p rade notify "from the package" --title M10 >/dev/null
check "desktop notification sent" eventually 5 grep -q "M10" "$NOTIFYD_LOG"
check "…with desktop-entry hint dev.rade.App" eventually 5 \
    bash -c "grep -A1 '\"desktop-entry\"' '$OUT/notify-monitor.log' | grep -q '\"dev.rade.App\"'"
kill "$MON" 2>/dev/null || true
stop_app
check "state saved under \$XDG_DATA_HOME/rade (Installed)" test -f "$XDG_DATA_HOME/rade/state.json"
# The packaged app started normally (no test mode, ADR-0021) refuses debug.*.
RADE_DEBUG_SOCKET= start_app
check "installed app without RADE_DEBUG_SOCKET=1: rade debug state → exit 2" bash -c "rade debug state >/dev/null 2>'$T/dbg.err'; test \$? = 2"
check "…prints the hint" grep -qF "start rade-app with RADE_DEBUG_SOCKET=1" "$T/dbg.err"
check "…public methods work" rade list
kill "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=

echo "== m10: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
