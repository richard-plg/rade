# Live E2E harness (ADR-0006, extended to the real desktop): drives a SEPARATE
# RADE instance on the owner's GNOME Wayland session with real input (ydotool
# via /dev/uinput), real IBus, real clipboard, real notification server and
# real Agents. Sourced by tests/live/run.sh. See tests/live/README.md.
#
# Safety (never violated):
# - The owner's RADE (its socket, state, config) is never contacted: every
#   `rade` call goes to $RADE_SOCKET = a per-run socket, the app runs with
#   temp XDG_DATA/CONFIG/CACHE_HOME; binaries are copied so rebuilds can't
#   swap them mid-run.
# - Before EVERY key press, text, or click, `guard` asserts the test window is
#   the active toplevel (`rade debug window`); if not, it tries
#   `rade debug action present` once, then aborts the whole run.
# - Clicks go only where `rade debug window` reports the real pointer is
#   (window coordinates, closed-loop placement in pointer.py).
# - Cleanup quits the test app and everything the run started.
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
HERE=$ROOT/tests/live
OUT=${OUT:-$ROOT/target/live}
DATE=$(date +%F)

die() { echo "live: $*" >&2; exit 2; }
[[ -n ${WAYLAND_DISPLAY:-} ]] || die "needs a Wayland session (WAYLAND_DISPLAY unset)"
[[ -w /dev/uinput ]] || die "/dev/uinput not writable; the owner must run: pkexec setfacl -m u:$USER:rw /dev/uinput"
for c in ydotool ydotoold jq wl-copy wl-paste dbus-monitor gdbus ibus python3; do
    command -v "$c" >/dev/null || die "missing $c"
done
[[ ${NO_BUILD:-} == 1 ]] || cargo build --release --manifest-path "$ROOT/Cargo.toml" -q || die "build failed"

REAL_XDG_CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
OWNER_SOCKET=${RADE_SOCKET:-${XDG_RUNTIME_DIR:?}/rade/rade.sock}
ib() { XDG_CONFIG_HOME=$REAL_XDG_CONFIG_HOME ibus "$@"; } # ibus finds its bus via ~/.config

# Short path: the ydotool socket must fit in sun_path.
T=$(mktemp -d /tmp/rade-live.XXXXXX)
mkdir -p "$T"/{bin,data,config/rade,cache,state,files} && rm -rf "$OUT" && mkdir -p "$OUT"
cp "$ROOT"/target/release/{rade,rade-app} "$T/bin/"
export XDG_DATA_HOME=$T/data XDG_CONFIG_HOME=$T/config XDG_CACHE_HOME=$T/cache
# The Usage store too (M39: $XDG_STATE_HOME/rade-dev/usage), never ~/.local/state.
export XDG_STATE_HOME=$T/state
export RADE_SOCKET=$XDG_RUNTIME_DIR/rade-live-$$.sock
[[ $RADE_SOCKET != "$OWNER_SOCKET" && $RADE_SOCKET != "$XDG_RUNTIME_DIR/rade/rade.sock" ]] || die "socket clash"
unset RADE_PANE_ID
export RADE_DEBUG_SOCKET=1 # the test app serves `debug.*` (ADR-0021); its Panes never see it
export PATH=$T/bin:$PATH
[[ $(command -v rade) == "$T/bin/rade" ]] || die "rade is not the test copy"
STATE=$XDG_DATA_HOME/rade/state.json
CONFIG=$XDG_CONFIG_HOME/rade/config.toml
# A body that meets the Definition of Ready (R59) for `ready` Tasks.
READY=$T/ready-body.md
printf '## Goal\nLive work.\n\n## Done when\n- [ ] the change is committed\n\n## Verify\n- `true` → exits 0\n' >"$READY"
# `make_ready FILE`: replace a Task's description with $READY (frontmatter and Log kept).
make_ready() { { awk 'NR==1 || !d {print} /^---$/ && NR>1 {d=1}' "$1"; cat "$READY"; echo; sed -n '/^## Log$/,$p' "$1"; } >"$1.new" && mv "$1.new" "$1"; }
# The owner's config (Agent Presets like the `clw` claude command), plus test bits.
[[ -f $REAL_XDG_CONFIG_HOME/rade/config.toml ]] && cp "$REAL_XDG_CONFIG_HOME/rade/config.toml" "$CONFIG"
cat >> "$CONFIG" <<EOF

# --- live harness ---
[limits]
max_children = 10 # M2-6: 9 Subagents → 8 tiled + 1 Title Bar

[viewer]
editor = "code --user-data-dir $T/vscode --extensions-dir $T/vscode-ext"
EOF

APP_PID= YDO_PID= MON_PID=
EXTRA_PIDS=()
IBUS_PREV=$(ib engine 2>/dev/null || true)
# (timeouts: a busy clipboard owner can stall wl-paste)
CLIP_TYPES=$(timeout 3 wl-paste --list-types 2>/dev/null || true)
CLIP_TEXT=
grep -q '^text/plain' <<<"$CLIP_TYPES" && CLIP_TEXT=$(timeout 3 wl-paste -n 2>/dev/null; echo x) && CLIP_TEXT=${CLIP_TEXT%x}

cleanup() {
    set +e
    if [[ -n $APP_PID ]]; then
        rade debug quit >/dev/null 2>&1
        for _ in $(seq 50); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.1; done
        kill "$APP_PID" 2>/dev/null
    fi
    for p in "${EXTRA_PIDS[@]}"; do kill "$p" 2>/dev/null; done
    vscode_kill
    [[ -n $MON_PID ]] && kill "$MON_PID" 2>/dev/null
    [[ -n $CAPS_WAS_ON ]] && ! caps_on && ydotool key 58:1 58:0 >/dev/null
    [[ -n $YDO_PID ]] && kill "$YDO_PID" 2>/dev/null
    [[ -n $IBUS_PREV && $IBUS_PREV != "No engine is set." ]] && ib engine "$IBUS_PREV"
    # Only if a test replaced it (M5); an image clipboard is not restored.
    if [[ -n ${CLIP_TOUCHED:-} && -n $CLIP_TEXT ]]; then printf '%s' "$CLIP_TEXT" | wl-copy; fi
    rm -f "$RADE_SOCKET"
    [[ ${KEEP:-} == 1 ]] && echo "kept $T" || rm -rf "$T"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Caps Lock on would change every typed letter: off for the run, put back
# at the end (cleanup).
caps_on() { grep -qx 1 /sys/class/leds/*::capslock/brightness 2>/dev/null; }
CAPS_WAS_ON=
ydotoold --socket-path="$T/ydotool.sock" --socket-own="$(id -u):$(id -g)" >"$OUT/ydotoold.log" 2>&1 &
YDO_PID=$!
export YDOTOOL_SOCKET=$T/ydotool.sock
for _ in $(seq 50); do [[ -S $YDOTOOL_SOCKET ]] && break; sleep 0.1; done
[[ -S $YDOTOOL_SOCKET ]] || die "ydotoold did not start"
if caps_on; then CAPS_WAS_ON=1; ydotool key 58:1 58:0 >/dev/null; sleep 0.2; fi

# Desktop notifications RADE sends to the real server (read-only monitor).
NOTIFY_LOG=$OUT/notify.log
dbus-monitor --session "type='method_call',interface='org.freedesktop.Notifications'" >"$NOTIFY_LOG" 2>&1 &
MON_PID=$!

# ---- results -------------------------------------------------------------
PASS=0 FAIL=0 NA=0
RESULTS=$OUT/results.txt
: >"$RESULTS"
ok() { PASS=$((PASS + 1)); echo "  PASS $*"; echo "PASS $*" >>"$RESULTS"; }
bad() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; echo "FAIL $*" >>"$RESULTS"; }
na() { NA=$((NA + 1)); echo "  N/A  $*"; echo "N/A $*" >>"$RESULTS"; }
check() { # name, command...
    local name=$1; shift
    if "$@"; then ok "$name"; else bad "$name"; fi
}
eventually() { # timeout_s, command...
    local t=$1; shift
    for _ in $(seq $((t * 10))); do "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done
    return 1
}
is() { [[ "$(eval "$1")" == "$2" ]]; }
want() { # name, timeout_s, command string, expected — shows what it got on failure
    if eventually "$2" is "$3" "$4"; then ok "$1"; else bad "$1 (want '$4', got '$(eval "$3" 2>&1 | head -c 300)')"; fi
}
jtrue() { jq -en "$1" >/dev/null; } # a jq expression is true
screen_has() { rade read "$1" --scrollback | grep -qF -- "$2"; }
shot() {
    local i
    for i in 1 2 3 4 5; do
        rade debug screenshot "$OUT/$1.png" >/dev/null 2>&1 && { echo "  shot ${OUT#$ROOT/}/$1.png"; return 0; }
        sleep 0.3
    done
}
state() { rade debug state --json; }
# jq over `debug state`: wt = Worktree on screen, tab = its Tab on screen.
JQ_DEFS='def wt: . as $s | $s.sources[] | select(.id == $s.active_source) | . as $p | $p.worktrees[] | select(.id == $p.active_worktree);
def tab: wt | . as $w | $w.tabs[] | select(.id == $w.active_tab);
def leaves: [.. | objects | select(.type? == "leaf") | .pane.id];'
q() { state | jq -r "$JQ_DEFS $1"; }
cur_tab() { q 'tab | .id'; }
cur_pane() { q 'tab | .focused'; }
tab_panes() { q 'tab | .layout | leaves | join(" ")'; }
ntabs() { q '[wt | .tabs[]] | length'; }

# ---- app -----------------------------------------------------------------
start_app() { # [ENV=value...] — launched fresh, so GNOME gives it focus
    env "$@" rade-app >>"$OUT/app.log" 2>&1 &
    APP_PID=$!
    eventually 20 rade list || die "app did not start (see $OUT/app.log)"
    sleep 1
    # Fullscreen: the window covers the monitor, so the real pointer is always
    # over it and nothing else can receive a stray click.
    guard
    rade debug action fullscreen >/dev/null
    eventually 5 is 'rade debug window --json | jq -r .fullscreen' true || die "fullscreen failed"
    sleep 0.5
}
stop_app() {
    rade debug quit >/dev/null 2>&1 || true
    wait "$APP_PID" 2>/dev/null || true
    APP_PID=
}

# ---- focus guard + real input ----------------------------------------------
active() { [[ $(rade debug window --json 2>/dev/null | jq -r .active) == true ]]; }
guard() {
    active && return 0
    rade debug action present >/dev/null 2>&1
    sleep 0.7
    active && return 0
    echo "ABORT: the live test window is not the active window (checked before each input); stopping." | tee -a "$RESULTS" >&2
    exit 3
}
# Linux input keycodes (linux/input-event-codes.h).
declare -A KC=(
    [ctrl]=29 [shift]=42 [alt]=56 [super]=125 [enter]=28 [esc]=1 [tab]=15 [space]=57
    [backspace]=14 [pagedown]=109 [pageup]=104 [left]=105 [right]=106 [up]=103 [down]=108
    [a]=30 [b]=48 [c]=46 [d]=32 [e]=18 [f]=33 [g]=34 [h]=35 [i]=23 [j]=36 [k]=37 [l]=38 [m]=50
    [f12]=88
    [n]=49 [o]=24 [p]=25 [q]=16 [r]=19 [s]=31 [t]=20 [u]=22 [v]=47 [w]=17 [x]=45 [y]=21 [z]=44
    [1]=2 [2]=3 [3]=4 [4]=5 [5]=6 [6]=7 [7]=8 [8]=9 [9]=10 [0]=11
)
key() { # key ctrl+shift+t [enter ...] — each combo pressed and released in order
    local combo k down up
    for combo; do
        guard
        down=() up=()
        IFS=+ read -ra ks <<<"$combo"
        for k in "${ks[@]}"; do
            [[ -n ${KC[$k]:-} ]] || die "unknown key '$k'"
            down+=("${KC[$k]}:1")
            up=("${KC[$k]}:0" "${up[@]}")
        done
        ydotool key "${down[@]}" "${up[@]}" >/dev/null
        sleep 0.15
    done
}
type_text() { # a few characters at a time, the guard before each chunk
    local s=$1 i
    for ((i = 0; i < ${#s}; i += 4)); do
        guard
        ydotool type --key-delay 15 -- "${s:i:4}" >/dev/null
    done
    sleep 0.2
    guard # focus lost while typing: stop here, and say so
}
# Real pointer to window point (x, y); needs the pointer over the window.
pointer_to() { python3 "$HERE/pointer.py" "$1" "$2"; }
at() { # x y → true if the real pointer is within 2 px of it
    rade debug window --json | jq -e --argjson x "$1" --argjson y "$2" \
        '.pointer != null and ((.pointer.x - $x) | fabs) <= 2 and ((.pointer.y - $y) | fabs) <= 2' >/dev/null
}
click() { # x y [button: left|middle|right] [mods...] — aim, verify, click
    local x=$1 y=$2 b=${3:-left} m; shift 3 2>/dev/null || shift $#
    pointer_to "$x" "$y" || { echo "  (pointer could not reach $x,$y)"; return 1; }
    at "$x" "$y" || return 1
    guard
    for m; do ydotool key "${KC[$m]}:1" >/dev/null; done
    case $b in
        left) ydotool click 0xC0 >/dev/null ;;
        middle) ydotool click 0xC2 >/dev/null ;;
        right) ydotool click 0xC1 >/dev/null ;;
    esac
    for m; do ydotool key "${KC[$m]}:0" >/dev/null; done
    sleep 0.2
}
drag() { # x1 y1 x2 y2 — left button held from one window point to another
    pointer_to "$1" "$2" && at "$1" "$2" || return 1
    guard
    ydotool click 0x40 >/dev/null
    sleep 0.1
    python3 "$HERE/pointer.py" "$3" "$4"
    local r=$?
    ydotool click 0x80 >/dev/null
    sleep 0.3
    return $r
}
# Centre of a Terminal's text / a widget, in window coordinates.
text_xy() { rade debug window --pane "$1" --text "$2" --json | jq -r '"\(.target.x) \(.target.y)"'; }
widget_xy() { rade debug window --widget "$1" --nth "${2:-0}" --json | jq -r '.widget | "\(.x + .width / 2) \(.y + .height / 2)"'; }
pane_box() { rade debug window --pane "$1" --json | jq -r '.pane | "\(.x) \(.y) \(.width) \(.height)"'; }

# A Terminal echoing its raw input like `cat -v` (ESC = ^[), with bracketed
# paste and, with `kitty`, the kitty keyboard protocol enabled.
cat >"$T/sink.sh" <<'SH'
stty raw -echo
printf 'SINK READY\r\n\033[?2004h'
[ "$1" = kitty ] && printf '\033[>1u'
exec cat -v
SH
sink() { # name [kitty] → pane id, focused
    local p
    p=$(rade spawn --cmd "bash $T/sink.sh ${2:-}" --name "$1" --json | jq -r .pane)
    rade focus "$p" >/dev/null
    eventually 10 screen_has "$p" 'SINK READY' || true
    echo "$p"
}
sink_text() { rade read "$1" --scrollback | sed -n '/SINK READY/,$p' | tail -n +2 | tr -d '\n'; }

# Separate VS Code instance (M4-5): its processes carry $T/vscode in their
# arguments (Electron rewrites the main one's argv, so match anywhere), plus
# their descendants. The owner's VS Code (~/.config/Code) never matches.
vscode_pids() {
    local all new p
    new=$(ps -eo pid=,args= | VS="$T/vscode" awk 'index($0, ENVIRON["VS"]) { print $1 }')
    all=$new
    while [[ -n $new ]]; do
        new=$(for p in $new; do ps -o pid= --ppid "$p"; done)
        all+=" $new"
    done
    echo $all
}
vscode_kill() {
    local pids
    pids=$(vscode_pids)
    [[ -z $pids ]] && return 0
    kill $pids 2>/dev/null
    for _ in $(seq 20); do [[ -z $(vscode_pids) ]] && return 0; sleep 0.25; done
    pids=$(vscode_pids)
    [[ -n $pids ]] && kill -9 $pids 2>/dev/null
    return 0
}

# The banner's Focus button / body = the app action app.focus-pane(<pane>),
# which GLib's freedesktop backend runs on ActionInvoked. Invoke that same
# action on the test app's own bus connection (the Notify sender).
# Notify calls to the real server whose strings include $1 (gnome-shell's own
# forward of each call is not counted), and CloseNotification calls.
notified() {
    awk -v s="string \"$1\"" '/^(method call|method return|signal|error)/ {
        b = ($0 ~ /destination=org.freedesktop.Notifications/ && $0 ~ /member=Notify/) }
        b && index($0, s) { n++ } END { print n + 0 }' "$NOTIFY_LOG"
}
# Notify calls whose strings include $1 (like `notified`), split by kind:
# $2 = new (replaces_id 0: a new banner) or replace (updates one in place).
notify_calls() {
    awk -v s="string \"$1" -v want="$2" '/^(method call|method return|signal|error)/ {
        b = ($0 ~ /destination=org.freedesktop.Notifications/ && $0 ~ /member=Notify/); k = 0; r = ""; next }
        b { k++; if (k == 2) r = $2; if (index($0, s) && ((want == "replace") == (r != "0"))) n++ }
        END { print n + 0 }' "$NOTIFY_LOG"
}
withdrawn() { grep -c 'destination=org.freedesktop.Notifications.*member=CloseNotification' "$NOTIFY_LOG"; }
app_bus() { # unique bus name of the test app
    local s
    for s in $(grep -o 'sender=:[0-9.]* -> destination=org.freedesktop.Notifications' "$NOTIFY_LOG" | cut -d= -f2 | cut -d' ' -f1 | sort -u); do
        [[ $(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
            --method org.freedesktop.DBus.GetConnectionUnixProcessID "$s" 2>/dev/null) == "(uint32 $APP_PID,)" ]] && { echo "$s"; return; }
    done
}
focus_action() { gdbus call --session --dest "$(app_bus)" --object-path /dev/rade/App \
    --method org.gtk.Actions.Activate focus-pane "[<'$1'>]" '{}' >/dev/null; }

summary() {
    echo "== live: $PASS passed, $FAIL failed, $NA not verifiable (results: ${RESULTS#$ROOT/})"
    [[ $FAIL -eq 0 ]]
}
