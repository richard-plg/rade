# M30 live (R90, R92) on the real desktop with real input and a REAL
# language server (gopls from PATH; skipped with the reason without one):
# a real Ctrl+I on a name in the Viewer shows its hover; Ctrl held with the
# real pointer resting on another name shows that one's; a real Esc closes
# it; Ctrl+Shift+P, typing `$Tot` lists the Worktree's symbols from gopls
# and a real Enter opens the definition. Also: the app holds no io_uring
# ring (R89). Light and dark screenshots on the real GPU. Run it with a
# temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m30
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
pop() { rpc debug.code '{"op":"popover"}' | jq -r "$1"; }
pal() { rade --json debug palette --ui "$@"; }

echo "-- R89 no io_uring ring in the app"
want "the app holds no io_uring fd" 3 "ls -l /proc/$APP_PID/fd 2>/dev/null | grep -c io_uring" 0

if ! command -v gopls >/dev/null; then
    na "real hover / palette symbols: no gopls on PATH (go install golang.org/x/tools/gopls@latest)"
else
    LG30=$T/files/ws-m30/goapp
    mkdir -p "$LG30"
    printf 'module goapp\n\ngo 1.21\n' >"$LG30/go.mod"
    cat >"$LG30/a.go" <<'EOF'
package goapp

// Total counts the items.
func Total(xs []int) int { return len(xs) }

// Twice doubles a number.
func Twice(n int) int { return 2 * n }
EOF
    cat >"$LG30/b.go" <<'EOF'
package goapp

var N = Total([]int{1, 2})

var M = Twice(N)
EOF
    LG -C "$LG30" init -q -b main && LG -C "$LG30" add -A && LG -C "$LG30" commit -qm init
    rade source add "$LG30" >/dev/null
    rade debug action color_scheme_light >/dev/null
    SH=$(rade spawn --source goapp --name sh30 --cmd bash --json | jq -r .pane)
    rade focus "$SH" >/dev/null
    VG=$(RADE_PANE_ID=$SH rade open "$LG30/b.go:3")
    rade focus "$VG" >/dev/null
    sleep 1

    echo "-- R90 a real Ctrl+I on Total (gopls hover)"
    rade debug viewer "$VG" place 3:10 >/dev/null
    key ctrl+i
    want "Ctrl+I: gopls's hover of Total" 30 "pop '\"\(.kind)|\(.title)|\(.text | test(\"Total counts the items\"))\"'" "hover|gopls|true"
    sleep 0.6
    shot m30-hover-light
    rade debug action color_scheme_dark >/dev/null
    sleep 0.8
    shot m30-hover-dark
    rade debug action color_scheme_light >/dev/null
    key esc
    want "a real Esc closes it" 3 "pop .open" false

    echo "-- R90 Ctrl held with the real pointer resting on Twice"
    read -r x y <<<"$(rade debug viewer "$VG" locate 5:10 --json | jq -r '"\(.x) \(.y)"')"
    if [[ -n $x && $x != null ]] && pointer_to "$((${x%.*} + 20))" "$y" && at "$((${x%.*} + 20))" "$y"; then
        guard
        ydotool key 29:1 >/dev/null
        pointer_to "${x%.*}" "$y"
        sleep 1.2
        ydotool key 29:0 >/dev/null
        want "Ctrl+hover: Twice's hover, opened by the pointer" 10 "pop '\"\(.by_pointer)|\(.text | test(\"Twice doubles\"))\"'" "true|true"
        sleep 0.4
        shot m30-ctrl-hover-light
        key esc
    else
        bad "real pointer over Twice (not placed)"
    fi

    echo "-- R92 Ctrl+Shift+P, \$Tot: gopls's workspace symbols, a real Enter"
    rade focus "$VG" >/dev/null
    key ctrl+shift+p
    want "the palette is open" 3 "pal info | jq -r .open" true
    type_text '$Tot'
    want "\$Tot lists Total from gopls" 30 "pal info | jq -r '\"\(.rows[0].kind)|\(.rows[0].title)|\(.status | test(\"gopls\"))\"'" "symbol|Total|true"
    sleep 0.4
    shot m30-palette-light
    rade debug action color_scheme_dark >/dev/null
    sleep 0.8
    shot m30-palette-dark
    rade debug action color_scheme_light >/dev/null
    key enter
    want "a real Enter opens a.go at Total, caret on the name" 8 "rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\") | .kind | \"\(.path | sub(\".*/goapp/\"; \"\")):\(.line)\"] | join(\" \")' | grep -o 'a.go:4'" "a.go:4"
    rade close "$SH" --force >/dev/null 2>&1 || true
fi
