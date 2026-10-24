# M8 live: the command palette (R14) with the real keyboard — Ctrl+Shift+P,
# typed queries, arrows + Enter, Tab across categories, Vietnamese typed
# through IBus Bamboo into the search entry (engine restored after), the
# Spawn Agent… form driven by keys (a stand-in Agent: no tokens), and Esc /
# Ctrl+Shift+P giving the keyboard back to the Pane. Everything lives in the
# run's temp dir.
X=$T/files/m8repo
mkdir -p "$X/src"
printf 'fn live() {}\n' >"$X/src/live_palette.rs"
printf '# m8\n' >"$X/README.md"
git -C "$X" init -q -b main
rade source add "$X" --name lm8 >/dev/null
LM=$(rade source list --json | jq -r '.sources[] | select(.name == "lm8") | .source')
if ! grep -q '^\[agents.livefake\]' "$CONFIG"; then
    printf '\n[agents.livefake]\ncommand = "%s"\n' "$ROOT/tests/e2e/fake-agent.sh" >>"$CONFIG"
    sleep 1 # live reload
fi
pal() { rade --json debug palette --ui "$@"; }
pinfo() { rade --json debug palette --ui info | jq -r "$1"; }
wfocus() { rade debug window --json | jq -r '"\(.focus.widget)/\(.focus.pane)"'; }
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }

key "alt+$(proj_index lm8)"
want "M8 Alt+N → Source lm8" 3 "state | jq -r .active_source" "$LM"
A=$(rade spawn --source lm8 --cmd cat --name "Tiếng Việt" --json | jq -r .pane)
B=$(rade spawn --source lm8 --name shell --json | jq -r .pane)
rade focus "$B" >/dev/null
sleep 0.5

echo "-- M8-1 Ctrl+Shift+P, typed query, Enter runs an action"
n=$(tab_panes | wc -w)
key ctrl+shift+p
want "M8-1 Ctrl+Shift+P opens the palette" 3 "pinfo .open" true
want "M8-1 the keyboard is in its search entry" 3 wfocus "GtkText/null"
type_text ">split r"
want "M8-1 typed text reached the entry" 3 "pinfo .query" ">split r"
want "M8-1 Split Right is the first row, with its key" 3 "pinfo '.rows[0] | \"\(.title) \(.keys)\"'" "Split Right Ctrl+Shift+Enter"
shot m8-palette-actions
key enter
want "M8-1 Enter ran Split Right" 3 "tab_panes | wc -w" $((n + 1))
want "M8-1 the new Pane has the real keyboard" 3 "rade debug window --json | jq -r .focus.pane" "$(cur_pane)"

echo "-- M8-2 arrows + Enter open a file"
key ctrl+shift+p
type_text "#live_pal"
want "M8-2 file row first" 3 "pinfo '.rows[0].title'" live_palette.rs
key down up
want "M8-2 Down, Up → row 0 again" 3 "pinfo .selected" 0
key enter
want "M8-2 the Viewer shows the file" 5 "q 'tab | [.. | objects | select(.type? == \"viewer\") | .path][0]'" "$X/src/live_palette.rs"

echo "-- M8-3 Esc gives the keyboard back to the Pane"
rade focus "$B" >/dev/null
sleep 0.3
key ctrl+shift+p
type_text "zz"
key esc
want "M8-3 Esc closed it" 3 "pinfo .open" false
want "M8-3 keyboard focus back on the shell Pane" 3 "rade debug window --json | jq -r .focus.pane" "$B"
type_text "echo LIVE-M8-\$((6*7))"
key enter
want "M8-3 typing reaches the Pane's shell" 3 "screen_has $B LIVE-M8-42 && echo y" y
key ctrl+shift+p
want "M8-3 Ctrl+Shift+P opens…" 3 "pinfo .open" true
key ctrl+shift+p
want "M8-3 …and closes it again" 3 "pinfo .open" false
want "M8-3 focus back on the shell Pane" 3 "rade debug window --json | jq -r .focus.pane" "$B"

echo "-- M8-4 Tab cycles categories; other RADE keys stay out while typing"
key ctrl+shift+p
key tab
want "M8-4 Tab → Actions ('>')" 3 "pinfo .filter" actions
key tab tab
want "M8-4 Tab, Tab → Panes ('@')" 3 "pinfo .filter" panes
key shift+tab
want "M8-4 Shift+Tab → Sources" 3 "pinfo .filter" sources
tabs=$(ntabs)
key ctrl+shift+t
sleep 0.3
check "M8-4 Ctrl+Shift+T while the palette is open opens no Tab" test "$(ntabs)" = "$tabs"
key esc

echo "-- M8-5 Vietnamese (IBus Bamboo) in the search entry"
rade focus "$B" >/dev/null
key ctrl+shift+p
type_text "@"
ib engine Bamboo
sleep 0.3
type_text "Tieesng Vieejt "
sleep 0.5
shot m8-vietnamese
ib engine xkb:us::eng
sleep 0.3
want "M8-5 Bamboo text in the entry: '@Tiếng Việt '" 3 "pinfo .query" "@Tiếng Việt "
want "M8-5 the Pane named 'Tiếng Việt' is first" 3 "pinfo '.rows[0].title'" "Tiếng Việt"
key enter
want "M8-5 Enter focused it" 3 cur_pane "$A"
want "M8-5 …with the real keyboard" 3 "rade debug window --json | jq -r .focus.pane" "$A"

echo "-- M8-6 Spawn Agent… form by keyboard (stand-in Agent)"
rade focus "$B" >/dev/null
key ctrl+shift+p
type_text ">spawn agent"
want "M8-6 'Spawn Agent…' first" 3 "pinfo '.rows[0].title'" "Spawn Agent…"
key enter
want "M8-6 the form opens, focus on Agent" 3 "pinfo '\"\(.form.title) \(.form.focus)\"'" "Spawn Agent agent"
pal field agent=livefake >/dev/null # the drop-down pick (its popover list order depends on your Presets)
key tab
want "M8-6 Tab → Prompt field" 3 "pinfo .form.focus" prompt
type_text "live palette"
key tab tab tab
want "M8-6 Tab ×3 more wraps inside the form back to Agent (palette still open)" 3 "pinfo '\"\(.open) \(.form.focus)\"'" "true agent"
key shift+tab
want "M8-6 Shift+Tab wraps to the Spawn button" 3 "pinfo '\"\(.open) \(.form.focus)\"'" "true null"
key shift+tab shift+tab shift+tab
want "M8-6 back on the Agent drop-down" 3 "pinfo .form.focus" agent
shot m8-form
before=$(rade list --json | jq '.panes | length')
key enter # on the drop-down: submits
want "M8-6 Enter on the Agent drop-down spawned it (palette closed)" 5 "rade list --json | jq '.panes | length'" $((before + 1))
NEWP=$(cur_pane)
want "M8-6 the stand-in Agent got the prompt" 10 "screen_has $NEWP 'REPLY: LIVE PALETTE' && echo y" y
want "M8-6 keyboard focus on the new Agent Pane" 3 "rade debug window --json | jq -r .focus.pane" "$NEWP"
for p in "$NEWP" "$A" "$B"; do rade close "$p" --force >/dev/null 2>&1; done
