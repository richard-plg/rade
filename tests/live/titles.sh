# Live: Tab label follows a real Claude Code's terminal title, and the real
# mouse + keyboard rename a Tab by double-clicking its label (R1.8,
# docs/checks/M12-titles.md). Run: tests/live/run.sh tests/live/titles.sh
rade source add "$ROOT" --name repo >/dev/null
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
# Without the Unread badge " (n)" (Claude's turn-end Notification).
label() { rpc debug.tabs | jq -r --arg t "$1" '.tabs[] | select(.tab==$t) | .label | sub(" \\([0-9]+\\)$"; "")'; }
title() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .title'; }

echo "-- R1.8 real Claude: Tab label follows its title"
C=$(rade spawn --source repo --name cl --json \
    --cmd "CLAUDE_CONFIG_DIR=$HOME/.claude-work claude 'reply with exactly: PONG'" | jq -r .pane)
rade focus "$C" >/dev/null
TC=$(rade list --json | jq -r --arg p "$C" '.panes[] | select(.pane==$p) | .tab')
want "Claude replied PONG" 120 "screen_has $C PONG && echo y" y
sleep 3
T1=$(title "$C")
echo "  (title: '$T1', label: '$(label "$TC")')"
check "Claude set a title, ✳ stripped ('$T1')" bash -c "[[ -n '$T1' && '$T1' != null && '$T1' != ✳* ]]"
want "Tab label = Claude's title" 5 "label $TC" "$T1"
shot titles-claude

echo "-- R1.8 double-click the Tab label → rename with real keys"
read -r X Y W H < <(rpc debug.tabs | jq -r '.selected_bounds | "\(.x) \(.y) \(.width) \(.height)"')
CX=$(jq -n "$X + $W * 0.35 | floor") CY=$(jq -n "$Y + $H / 2 | floor")
pointer_to "$CX" "$CY" && at "$CX" "$CY" && guard && ydotool click -r 2 -D 80 0xC0 >/dev/null
want "double-click opened the rename entry" 3 "rpc debug.tabs | jq -r .rename_open" true
shot titles-rename-open
type_text "live name"
key enter
want "label = 'live name'" 3 "label $TC" "live name"
rade send "$C" "reply with exactly: PONG2" --enter >/dev/null
want "Claude replied again" 120 "screen_has $C PONG2 && echo y" y
sleep 3
want "label still 'live name' after Claude's new titles" 1 "label $TC" "live name"
shot titles-renamed
rade close "$C" --force >/dev/null
