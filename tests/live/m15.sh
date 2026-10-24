# M15 live (R23-e, R23.5): real Ctrl+Alt+<n> on GNOME switches Workspaces
# (GNOME, Ghostty and IBus leave it alone), focus comes back where it was, a
# real click opens the switcher, and an Agent waiting in the hidden
# Workspace raises the amber "!" (screenshots on the real GPU, light and
# dark — the app's own style only, the GNOME theme is never touched).
# Workspace roots live in the run's temp dir; no real ~/workspaces, no gh.
WA=$T/files/ws-live-a WB=$T/files/ws-live-b
mkdir -p "$WA/.rade" "$WB/.rade"
printf 'name = "LiveA"\ncolor = "orange"\n[env]\nLIVE_WS = "a"\n' >"$WA/.rade/workspace.toml"
printf 'name = "LiveB"\ncolor = "green"\n[env]\nLIVE_WS = "b"\n' >"$WB/.rade/workspace.toml"
cat >>"$CONFIG" <<EOF

[agents.livefake]
command = "$ROOT/tests/e2e/fake-agent.sh"
EOF
sleep 1 # config.toml is reloaded on change
shown_ws() { state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .workspace'; }
ws_ui() { python3 - "$RADE_SOCKET" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.ui", "params": {"op": "workspaces"}}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]["workspaces"]))
PY
}
rade workspace add "$WA" >/dev/null
rade workspace add "$WB" >/dev/null
la=$(rade spawn --source general@livea --agent livefake --name live-lead "hello" --json | jq -r .pane)
lb=$(rade spawn --source general@liveb --cmd bash --name live-b --json | jq -r .pane)
rade focus "$la" >/dev/null
sleep 1

echo "-- R23-e real Ctrl+Alt+<n>"
want "start: LiveA shown" 3 shown_ws livea
key ctrl+alt+3
want "real Ctrl+Alt+3 shows LiveB" 3 shown_ws liveb
want "…its Pane focused" 3 cur_pane "$lb"
rade send "$lb" 'echo "ws=$LIVE_WS/$RADE_WORKSPACE"' --enter >/dev/null
want "LiveB's Terminal has LiveB's env" 5 "rade read $lb | grep -c 'ws=b/liveb'" 1
key ctrl+alt+2
want "real Ctrl+Alt+2 back to LiveA" 3 shown_ws livea
want "…the same Pane focused" 3 cur_pane "$la"
type_text "typed"
want "the keyboard is in that Pane" 3 "rade read $la | grep -c 'typed'" 1
key backspace backspace backspace backspace backspace
key ctrl+alt+1
want "real Ctrl+Alt+1 shows default" 3 shown_ws default

echo "-- R23.5 the badge from a hidden Workspace, real click on the switcher"
rade send "$la" "ask may I go on?" --enter >/dev/null
want "LiveA's Agent waiting → the switcher's !" 5 "ws_ui | jq -r .badge" true
want "…LiveA's row: Unread 1, waiting" 3 "ws_ui | jq -c '.rows[] | select(.workspace==\"livea\") | [.unread, .glyph]'" '[1,"waiting"]'
shot m15-badge-light
read -r x y <<<"$(widget_xy rade-switcher)"
if click "$x" "$y"; then
    want "a real click opens the switcher" 3 "ws_ui | jq -r .open" true
    python3 - "$RADE_SOCKET" "$OUT/m15-popover-light.png" <<'PY' && echo "  shot target/live/m15-popover-light.png"
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.ui", "params": {"op": "switcher", "arg": "shot", "path": sys.argv[2]}}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
PY
    key esc
    want "Esc closes it" 3 "ws_ui | jq -r .open" false
else
    bad "pointer over the switcher (not placed)"
fi
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot m15-badge-dark
rade debug action color_scheme_light >/dev/null
key ctrl+alt+2
want "Ctrl+Alt+2 to the waiting Agent's Workspace: the ! goes" 3 "ws_ui | jq -r .badge" false
rade send "$la" "yes" --enter >/dev/null
rade close "$la" --force >/dev/null
rade close "$lb" --force >/dev/null
rade workspace switch default >/dev/null
rade workspace remove liveb >/dev/null
rade workspace remove livea >/dev/null
