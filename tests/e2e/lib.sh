# Shared E2E harness (ADR-0006), sourced by run.sh, m2.sh, m3.sh and m4.sh: temp
# HOME/XDG dirs, a private D-Bus session with a stand-in notification server
# (never the desktop's), headless Weston, helpers. Set OUT before sourcing
# to change the screenshot directory.
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN=$ROOT/target/release
OUT=${OUT:-$ROOT/target/e2e}
# Re-run the calling script inside its own session bus, with isolated user
# dirs set before the bus starts, so the bus and every service it activates
# (portals, gvfs, …) see them too: nothing touches the real session or ~/.
if [[ -z ${RADE_E2E_BUS:-} ]]; then
    [[ ${NO_BUILD:-} == 1 ]] || cargo build --release --manifest-path "$ROOT/Cargo.toml" -q
    export RADE_E2E_BUS=1 NO_BUILD=1
    RADE_E2E_T=$(mktemp -d "${TMPDIR:-/tmp}/rade-e2e.XXXXXX")
    export RADE_E2E_T HOME=$RADE_E2E_T/home XDG_CONFIG_HOME=$RADE_E2E_T/config \
        XDG_DATA_HOME=$RADE_E2E_T/data XDG_RUNTIME_DIR=$RADE_E2E_T/run XDG_CACHE_HOME=$RADE_E2E_T/cache
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR" "$XDG_CACHE_HOME"
    chmod 700 "$XDG_RUNTIME_DIR"
    # No portals, a11y bus, or gvfs FUSE (quiet, and nothing real).
    export GDK_DEBUG=no-portals ADW_DISABLE_PORTAL=1 GTK_A11Y=none NO_AT_BRIDGE=1 \
        GIO_USE_VFS=local GVFS_DISABLE_FUSE=1
    exec dbus-run-session -- "$0" "$@"
fi
rm -rf "$OUT" && mkdir -p "$OUT"
T=$RADE_E2E_T
export SHELL=/bin/bash GTK_IM_MODULE=gtk-im-context-simple RADE_SKIP_SHELL_ENV=1
# Test mode: the app serves the hidden `debug.*` methods (ADR-0021).
export RADE_DEBUG_SOCKET=1
# The system monitor (R116) reads the real machine: off, so goldens and cell
# orders stay fixed; tests/e2e/sysmon.sh turns it on over a fake proc root.
export RADE_SYSTEM_MONITOR=${RADE_SYSTEM_MONITOR:-0}
# The update check (R127) would reach github.com: off, tests/e2e/m48.sh turns it on over a fake GitHub.
export RADE_UPDATE_CHECK=${RADE_UPDATE_CHECK:-0}
printf 'PS1="$ "\n' > "$HOME/.bashrc"
# Agent config dirs must come from the temp HOME, never the real one.
unset RADE_SOCKET RADE_PANE_ID CLAUDE_CONFIG_DIR CODEX_HOME
STATE=$XDG_DATA_HOME/rade-dev/state.json  # target/release is the Devel flavor (M10): rade-dev dir, rade-dev.sock

WESTON_PID= APP_PID= NOTIFYD_PID=
cleanup() {
    [[ -n $APP_PID ]] && kill "$APP_PID" 2>/dev/null
    [[ -n $NOTIFYD_PID ]] && kill "$NOTIFYD_PID" 2>/dev/null
    [[ -n $WESTON_PID ]] && kill "$WESTON_PID" 2>/dev/null
    wait 2>/dev/null
    # The private bus's document portal mounts FUSE in our runtime dir.
    fusermount3 -u "$XDG_RUNTIME_DIR/doc" 2>/dev/null || true
    [[ ${KEEP:-} == 1 ]] && echo "kept $T" || rm -rf "$T"
}
trap cleanup EXIT

# Desktop notifications land in $NOTIFYD_LOG (tests/e2e/notifyd.py).
NOTIFYD_LOG=$OUT/notifyd.log
: > "$NOTIFYD_LOG"
python3 "$ROOT/tests/e2e/notifyd.py" "$NOTIFYD_LOG" 2>>"$OUT/notifyd.err" &
NOTIFYD_PID=$!
for _ in $(seq 50); do
    gdbus introspect --session --dest org.freedesktop.Notifications \
        --object-path /org/freedesktop/Notifications >/dev/null 2>&1 && break
    sleep 0.1
done

export WAYLAND_DISPLAY=rade-e2e-$$
weston --backend=headless --socket="$WAYLAND_DISPLAY" --width=${WESTON_WIDTH:-1400} --height=900 --idle-time=0 \
    ${WESTON_REFRESH:+--refresh-rate=$WESTON_REFRESH} \
    >"$OUT/weston.log" 2>&1 &
WESTON_PID=$!
for _ in $(seq 50); do [[ -S $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY ]] && break; sleep 0.1; done

PASS=0 FAIL=0
export PATH=$BIN:$PATH   # `rade`, also inside `bash -c` checks
ok() { PASS=$((PASS + 1)); echo "  PASS $*"; }
bad() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }
check() { # name, command...
    local name=$1; shift
    if "$@"; then ok "$name"; else bad "$name"; fi
}
# A timing budget (frame / update times in ms): skipped on hardware that
# can't meet it — E2E_SKIP_PERF=1, set by CI on GitHub-hosted runners
# (4 vCPUs, llvmpipe; ADR-0042). Local and live runs still enforce it.
perf_check() { # name, command...
    if [[ ${E2E_SKIP_PERF:-} == 1 ]]; then echo "  SKIP $1 (E2E_SKIP_PERF=1)"; return 0; fi
    check "$@"
}
# Poll until a command succeeds (terminals start asynchronously).
eventually() { # timeout_s, command...
    local t=$1; shift
    for _ in $(seq $((t * 10))); do "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done
    return 1
}
# `eventually N is '<command>' <expected>`: re-runs the command each poll
# (`eventually N test "$(cmd)" = x` would evaluate it only once).
is() { [[ "$(eval "$1")" == "$2" ]]; }
screen_has() { rade read "$1" --scrollback | grep -qF -- "$2"; }
shot() { # retries: a frame in the middle of a relayout has nothing to render
    local i
    for i in 1 2 3 4 5; do
        rade debug screenshot "$OUT/$1.png" >/dev/null 2>&1 && { echo "  shot ${OUT#$ROOT/}/$1.png"; return 0; }
        sleep 0.3
    done
    rade debug screenshot "$OUT/$1.png" >/dev/null
}

start_app() {
    "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
    APP_PID=$!
    eventually 15 rade list || { echo "app did not start"; cat "$OUT/app.log"; exit 1; }
    sleep 0.5 # first frame
}
stop_app() {
    rade debug quit >/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
    APP_PID=
}

# Strip volatile fields (split ratios move with window size).
norm() { jq -S 'del(.. | .ratio?)'; }

# Raw socket call for debug methods without a CLI subcommand (M14):
# `rpc debug.ui '{"op": "sidebar"}'` → the result JSON.
rpc() { python3 - "$XDG_RUNTIME_DIR/rade/rade-dev.sock" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
