# M14 live (R20): the redesign on the real desktop and GPU — real clicks on
# sidebar Agent rows, the Toast's Show, the Status Bar's hints, the header's
# Command Palette button and the Right Panel switcher; the Attention Ring
# drawn above a real GPU Terminal. Fake Agents only (no tokens), in a temp
# Source and General.
X=$T/files/m14
mkdir -p "$X"
cat >>"$CONFIG" <<EOF

[agents.fake14]
command = "$HERE/../e2e/fake-agent.sh"
EOF
sleep 1 # config.toml is reloaded on change
rade source add "$X" --name l14 >/dev/null
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}
ui() { rpc debug.ui "${1:-{\}}"; }
palette_open() { rade debug palette --ui info --json | jq -r .open; }

R=$(rade spawn --source l14 --agent fake14 --name runner "spin" --json | jq -r .pane)
W=$(rade spawn --source l14 --agent fake14 --name asker "ask which file" --json | jq -r .pane)
want "asker waiting" 15 "rade list --json | jq -r '.panes[] | select(.pane==\"$W\") | .status'" waiting
rade focus "$R" >/dev/null
sleep 1

echo "-- R20-c real clicks on sidebar Agent rows"
read -r x y <<<"$(widget_xy "rade-agent-$W")"
if click "$x" "$y"; then
    want "R20-c click on asker's row focuses its Pane" 3 cur_pane "$W"
else
    bad "R20-c pointer over asker's row (not placed)"
fi
read -r x y <<<"$(widget_xy "rade-agent-$R")"
if click "$x" "$y"; then
    want "…and runner's row focuses runner" 3 cur_pane "$R"
fi

echo "-- R20.3 the Attention Ring on the real GPU"
rade focus "$W" >/dev/null # its Tab on screen (root spawns get their own Tab)
sleep 1
want "asker has the ring" 3 "ui '{\"op\":\"pane\",\"pane\":\"$W\"}' | jq -r .pane.ring" true
shot m14-ring
read -r bx by bw bh <<<"$(pane_box "$W")"
WW=$(rade debug window --json | jq -r .width)
rade debug screenshot "$OUT/m14-ring-px.png" >/dev/null
amber=$(python3 - "$OUT/m14-ring-px.png" "$bx" "$by" "$bw" "$bh" "$WW" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
k = im.width / float(sys.argv[6])  # screenshot pixels per window unit
x, y, w, h = (float(v) * k for v in sys.argv[2:6])
# the ring is the overlay's 2 px border: the Terminal's left and bottom edges
pts = [(int(x + k), int(y + h / 2)), (int(x + w / 2), int(y + h - k))]
def amber(p):
    r, g, b = im.getpixel(p)
    return r > 140 and 90 < g < 230 and b < 90
print(int(all(amber(p) for p in pts)))
PY
)
check "R20-b the ring is drawn above the GPU Terminal (amber pixels on its edges)" test "$amber" = 1

echo "-- R20-b the Toast's Show (real click)"
key "alt+$(rade source list --json | jq -r '[.sources[].name] | index("l14") + 1')"
O=$(rade spawn --source general --agent fake14 --name offscreen "ask about it" --json | jq -r .pane)
want "a Toast for offscreen" 15 "ui | jq -r '.chrome.toasts[-1].pane'" "$O"
sleep 0.5
shot m14-toast
read -r x y <<<"$(widget_xy Show)"
if click "$x" "$y"; then
    want "R20-b Show → General, offscreen focused" 3 "echo \$(state | jq -r .active_source) \$(cur_pane)" "general $O"
else
    bad "R20-b pointer over Show (not placed)"
fi
rade close "$O" --force >/dev/null

echo "-- R20.9 / R20.11 Status Bar and header buttons (real clicks)"
read -r x y <<<"$(widget_xy Commands)"
if click "$x" "$y"; then
    want "Status Bar 'Commands' opens the palette" 3 palette_open true
    key esc
    want "…Esc closes it" 3 palette_open false
fi
read -r x y <<<"$(widget_xy "Command Palette (Ctrl+Shift+P)")"
if click "$x" "$y"; then
    want "header search button opens the palette" 3 palette_open true
    key esc
fi
want "Status Bar counts" 3 "ui | jq -r '.chrome.status_bar.counts | join(\",\")'" "1 needs input,1 running"
rade debug action toggle_file_tree >/dev/null
sleep 0.5
read -r x y <<<"$(widget_xy "Changes (Right Panel)")"
if click "$x" "$y"; then
    want "R20.10 Right Panel switcher → Changes" 3 "ui | jq -r .chrome.right_panel.page" changes
    shot m14-right-panel
fi
read -r x y <<<"$(widget_xy "Files (Right Panel)")"
click "$x" "$y" && want "…→ Files" 3 "ui | jq -r .chrome.right_panel.page" files
rade debug action toggle_file_tree >/dev/null
shot m14-main
rade close "$R" --force >/dev/null
rade close "$W" --force >/dev/null
rade source remove l14 >/dev/null
