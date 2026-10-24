# M13 live (R19): real right-clicks open the Tab, Source and Terminal menus
# and their items work from the keyboard; a mouse-reporting program keeps
# its right-click, Shift+right-click still opens the menu; real
# Ctrl+Shift+F + typing (plain and IBus Bamboo) searches the scrollback;
# Pause / Resume with one real tiny Agent turn (claude, else agy). Everything
# in a temp Source; the IBus engine is put back.
X=$T/files/m13
mkdir -p "$X"
rade source add "$X" --name l13 >/dev/null
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }
key "alt+$(proj_index l13)"
want "M13 Alt+N → Source l13" 3 "state | jq -r .active_source" l13
menu_open() { rade debug window --json | jq -r .menu_open; }
# debug.tabs has no CLI subcommand: a raw socket call (as tests/e2e/titles.sh).
debug_tabs() { python3 - "$RADE_SOCKET" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall(b'{"jsonrpc":"2.0","id":1,"method":"debug.tabs","params":{}}\n')
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}

echo "-- R19.6 Terminal menu by a real right-click"
P=$(cur_pane)
rade send "$P" "echo right-click-here" --enter >/dev/null
want "text printed" 5 "rade read $P | grep -cx right-click-here" 1
read -r x y <<<"$(text_xy "$P" right-click-here)"
if click "$x" "$y" right; then
    want "R19.6 right-click opens the Terminal menu" 3 menu_open true
    sleep 1
    key down
    sleep 0.3
    want "M14 the Terminal menu takes the keyboard: Down → its first item" 2 "rade debug window --json | jq -r .focus.widget" GtkModelButton
    want "…still open" 2 menu_open true
    shot m13-terminal-menu
    key esc
    want "Esc closes it" 3 menu_open false
    want "…keyboard back on the Terminal" 3 "rade debug window --json | jq -r .focus.pane" "$P"
else
    bad "R19.6 pointer over the Terminal (not placed)"
fi
rade send "$P" "printf '\\e[?1000h'; command cat -v" --enter >/dev/null
sleep 0.8
read -r x y <<<"$(text_xy "$P" right-click-here)"
if click "$x" "$y" right; then
    want "R19.6 mouse reporting on: the program gets the right-click (no menu)" 2 menu_open false
    want "…cat -v shows the mouse report" 3 "rade read $P | grep -c '\\^\\[\\[M'" 1
fi
if click "$x" "$y" right shift; then
    want "R19.6 Shift+right-click opens the menu over it" 3 menu_open true
    key esc
fi
key ctrl+c
rade send "$P" "printf '\\e[?1000l'" --enter >/dev/null
sleep 0.3
rade send "$P" clear --enter >/dev/null
sleep 0.3

echo "-- R19.5 Ctrl+Shift+F, real typing (plain and Bamboo)"
rade send "$P" 'for i in $(seq 1 7); do echo "needle $i Tiếng"; done; seq 1 200' --enter >/dev/null
sleep 1
key ctrl+shift+f
want "R19.5 Ctrl+Shift+F opens the search bar, keys in its entry" 3 "rade debug search $P | jq -r '\"\\(.open) \\(.focused)\"'" "true true"
type_text "needle"
want "R19.5 typed 'needle': 8 matches (7 lines + the command), scrollback included" 5 "rade debug search $P | jq -r .count | cut -d/ -f2" 8
key enter
want "Enter selects a match" 3 "rade debug search $P | jq -r .count | grep -cE '^[1-9]'" 1
shot m13-search
key ctrl+a backspace
ib engine Bamboo
sleep 0.3
type_text "Tieesng"
sleep 0.4
ib engine xkb:us::eng
want "R19.5 Bamboo in the entry: 'Tiếng' composed" 3 "rade debug search $P | jq -r .text" "Tiếng"
want "…and found (8)" 5 "rade debug search $P | jq -r .count | cut -d/ -f2" 8
key esc
want "Esc closes it, keys back to the Terminal" 3 "rade debug search $P | jq -r .open" false
want "…focus on the Terminal" 3 "rade debug window --json | jq -r .focus.pane" "$P"

echo "-- R19.1 Tab menu by a real right-click"
key ctrl+shift+t # a second Tab: the tab bar hides with one
sleep 0.8
N0=$(ntabs)
read -r x y <<<"$(debug_tabs | jq -r '.selected_bounds | "\(.x + .width / 2) \(.y + .height / 2)"')"
if click "$x" "$y" right; then
    sleep 1 # the first key is lost if sent before the popover has the keyboard
    want "R19.1 right-click on a Tab opens its menu (keyboard in it)" 2 "rade debug window --json | jq -r .focus.widget" GtkModelButton
    # M14: arrows move through the items (AdwTabBar's own menu closed on
    # Down / Tab, docs/checks/M13.md acceptance).
    key down
    sleep 0.3
    want "M14 Down keeps the Tab menu open" 2 menu_open true
    key tab
    sleep 0.3
    want "M14 Tab keeps it open too" 2 menu_open true
    key esc
    want "Esc closes the Tab menu" 3 menu_open false
    click "$x" "$y" right
    sleep 1
    rade debug window --json | jq -c .focus >>"$OUT/m13-menu-focus.txt"
    key down
    sleep 0.3
    rade debug window --json | jq -c .focus >>"$OUT/m13-menu-focus.txt"
    want "M14 Down moves to the next item (Duplicate)" 2 "rade debug window --json | jq -r .focus.label" Duplicate
    key enter
    want "M14 Tab menu by keyboard: Down, Enter → Duplicate" 3 ntabs $((N0 + 1))
    key ctrl+shift+w
    sleep 0.5
    click "$x" "$y" right
    sleep 1
    key enter # Rename…
    sleep 0.6
    type_text "live-tab"
    key enter
    want "R19.1 Tab menu → Rename…: label live-tab" 3 "debug_tabs | jq -r '.tabs[] | select(.selected) | .label'" live-tab
else
    bad "R19.1 pointer over the Tab (not placed)"
fi

echo "-- R19.2 Source menu by a real right-click"
read -r x y <<<"$(widget_xy rade-row-l13)"
if click "$x" "$y" right; then
    sleep 1
    key enter # Rename…, the first item, is preselected
    sleep 0.4
    type_text "live13"
    key enter
    want "R19.2 Source menu → Rename…: name live13" 3 "rade source list --json | jq -r '.sources[] | select(.source==\"l13\") | .name'" live13
else
    bad "R19.2 pointer over the Source row (not placed)"
fi

echo "-- R19.3 Pause / Resume with a real Agent turn"
AGENT=claude
command -v claude >/dev/null || AGENT=agy
A=$(rade spawn --agent "$AGENT" --source l13 --name p13 --json 'reply with exactly: PONG' | jq -r .pane)
want "$AGENT turn done" 1 "rade wait $A --until done --timeout 300" done
want "$AGENT said PONG" 3 "rade read $A --scrollback | grep -c PONG | awk '{print (\$1 > 0)}'" 1
rade source pause l13 >/dev/null
want "R19.3 paused" 3 "rade source list --json | jq -r '.sources[] | select(.source==\"l13\") | .paused'" true
want "R19.3 the Agent's process is gone" 15 "grep -lsaz '^RADE_PANE_ID=$A\$' /proc/[0-9]*/environ | xargs -r grep -lsaz \"^RADE_SOCKET=\$RADE_SOCKET\\\$\" | wc -l" 0
rade focus "$A" >/dev/null
sleep 0.5
shot m13-paused
rade source resume l13 >/dev/null
sleep 8
rade send "$A" 'reply with exactly: PONG2' --enter >/dev/null
want "R19.3 resumed $AGENT answers in its session" 1 "rade wait $A --until done --timeout 300" done
want "…PONG2" 3 "rade read $A --scrollback | grep -c PONG2 | awk '{print (\$1 > 0)}'" 1
rade read "$A" --scrollback | tail -25 >"$OUT/m13-agent.txt"
rade close "$A" --force >/dev/null
rade source remove l13 >/dev/null
