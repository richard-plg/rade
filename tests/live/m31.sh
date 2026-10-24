# 0.7.1 live (code navigation as the owner uses it) on the real desktop with
# real input and a FAKE language server (tests/e2e/fake-lsp.py): a real Ctrl
# held over a name underlines it and makes the pointer a hand, a real
# pointer move with Ctrl held moves the link, letting Ctrl go clears it; a
# real drag selection survives a real Ctrl+click, which navigates; real
# clicks open call hierarchy nodes on the results popover (their callers load).
# Light and dark screenshots on the real GPU. Run it with a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m31
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LW=$T/files/ws-m31/web
mkdir -p "$LW/src"
printf '{"name": "web"}\n' >"$LW/package.json"
cat >"$LW/src/utils.ts" <<'EOF'
export const momentWithTZ = (date?: string) => new Date(date ?? 0)
export const twice = () => [momentWithTZ(), momentWithTZ()]
EOF
cat >"$LW/src/Events.ts" <<'EOF'
import { momentWithTZ } from './utils'
export function range() {
  return [momentWithTZ(), momentWithTZ()]
}
export function outer() {
  return range()
}
EOF
cat >"$LW/.fake-lsp.json" <<'EOF'
{"momentWithTZ": {"def": ["src/utils.ts:1:14", "src/utils.ts:2:14"],
   "callers": [{"name": "range", "def": "src/Events.ts:2:17", "at": ["src/Events.ts:3:11", "src/Events.ts:3:27"]}]},
 "twice": {"def": ["src/utils.ts:2:14"]},
 "range": {"def": ["src/Events.ts:2:17"], "callers": [{"name": "outer", "def": "src/Events.ts:5:17", "at": "src/Events.ts:6:10"}]},
 "outer": {"def": ["src/Events.ts:5:17"], "callers": [{"name": "main", "def": "src/utils.ts:1:1", "at": "src/utils.ts:1:1"}]}}
EOF
LG -C "$LW" init -q -b main && LG -C "$LW" add -A && LG -C "$LW" commit -qm init
cat >>"$CONFIG" <<EOF

[code.server.typescript]
command = ["python3", "$ROOT/tests/e2e/fake-lsp.py"]
EOF
rade source add "$LW" >/dev/null
rade debug action color_scheme_light >/dev/null
V=$(rade open "$LW/src/utils.ts:2")
sleep 1
vj() { rade debug viewer "$V" info --json | jq -r "$1"; }
where() { vj '"\(.path | sub(".*/web/"; "")):\(.line)"'; }
xy() { rade debug viewer "$V" locate "$1" --json 2>/dev/null | jq -r '"\(.x) \(.y)"'; }
code_rows() { python3 - "$RADE_SOCKET" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.code", "params": {}}) + "\n").encode())
b = b""
while not b.endswith(b"\n"): b += s.recv(65536)
r = json.loads(b)["result"]
print(",".join(eval(sys.argv[2], {}, {"r": x}) for x in r["rows"]))
PY
}
ctrl_up() { ydotool key 29:0 >/dev/null; }

echo "-- a real Ctrl held over a name: link look, then gone on release"
read -r x y <<<"$(xy 2:30)"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    want "no Ctrl: no link" 2 "vj .link" null
    guard
    ydotool key 29:1 >/dev/null
    want "Ctrl pressed with the pointer on momentWithTZ: underlined" 3 "vj '\"\(.link.word) \(.link.underlined)\"'" "momentWithTZ true"
    want "… the pointer is a hand" 3 "vj .pointer_cursor" pointer
    read -r x2 y2 <<<"$(xy 2:16)"
    pointer_to "$x2" "$y2" || true
    want "moving with Ctrl held: the link follows (twice)" 3 "vj .link.word" twice
    sleep 0.3
    shot m31-link-light
    ctrl_up
    want "Ctrl let go: no link, the text cursor" 3 "vj '\"\(.link) \(.pointer_cursor)\"'" "null text"
else
    bad "real click in the Viewer (pointer not placed)"
fi

echo "-- a real drag selection survives a real Ctrl+click, which navigates"
read -r a b <<<"$(xy 1:1)"
read -r c d <<<"$(xy 1:13)"
if [[ -n $a && $a != null ]] && drag "$a" "$b" "$c" "$d"; then
    want "the drag selected line 1's start" 3 "vj .selection" "1:1-1:13"
    read -r x y <<<"$(xy 2:30)"
    if click "$x" "$y" left ctrl; then
        want "Ctrl+click on momentWithTZ (2 definitions): the results popover lists them" 5 "code_rows 'r[\"kind\"]+\":\"+str(r.get(\"line\"))' | tr ',' ' '" "file:None item:1 item:2"
        want "… the selection is unchanged (no drag-select, no caret move)" 3 "vj .selection" "1:1-1:13"
    else
        bad "real Ctrl+click on momentWithTZ (pointer not placed)"
    fi
    read -r x y <<<"$(xy 2:16)"
    if click "$x" "$y" left ctrl; then
        want "Ctrl+click on twice: jumps (one definition)" 5 "vj '\"\(.line):\(.cursor_col)\"'" "2:14"
    else
        bad "real Ctrl+click on twice (pointer not placed)"
    fi
else
    bad "real drag in the Viewer (pointer not placed)"
fi

echo "-- real clicks open call hierarchy nodes"
read -r x y <<<"$(xy 1:16)"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    key alt+shift+h
    want "Alt+Shift+H: range with its 2 call sites, its caller outer loaded" 6 "code_rows 'r[\"kind\"]+\":\"+str(r.get(\"name\") or r.get(\"line\"))' | tr ',' ' '" "call:range site:3 site:3 call:outer"
    read -r x y <<<"$(widget_xy 'outer · call · src/Events.ts:5')"
    if [[ -n $x && $x != null ]] && click "$x" "$y"; then
        want "a real click on outer: its call site and its caller main" 6 "code_rows 'r[\"kind\"]+\":\"+str(r.get(\"name\") or r.get(\"line\"))' | tr ',' ' '" "call:range site:3 site:3 call:outer site:6 call:main"
        sleep 0.4
        shot m31-tree-light
        rade debug action color_scheme_dark >/dev/null
        sleep 0.8
        shot m31-tree-dark
        rade debug action color_scheme_light >/dev/null
        if click "$x" "$y"; then
            want "a second real click closes it" 4 "code_rows 'r[\"kind\"]+\":\"+str(r.get(\"name\") or r.get(\"line\"))' | tr ',' ' '" "call:range site:3 site:3 call:outer"
        fi
    else
        bad "real click on the outer node (pointer not placed)"
    fi
    read -r x y <<<"$(widget_xy 'Go to range')"
    if [[ -n $x && $x != null ]] && click "$x" "$y"; then
        want "a real click on range's Go to: Events.ts:2" 5 where "src/Events.ts:2"
    else
        bad "real click on Go to range (pointer not placed)"
    fi
else
    bad "real click before Alt+Shift+H (pointer not placed)"
fi
rade debug viewer "$V" escape >/dev/null 2>&1 || true
