#!/usr/bin/env bash
# E2E (M43): Agent Overview (R117) — every Agent of every Workspace in bands,
# Ctrl+Shift+Space from a Terminal, Enter → the Agent in a hidden Workspace,
# Esc back, hold-to-flip, tails, entry points (titlebar, palette, switcher !,
# Statusline count), empty / 40 Agents at 1280 px and text scale 200 %,
# budgets, Reduce Animation; screenshots dark + light → target/e2e-m43/.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m43
WESTON_WIDTH=1440
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 300
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
EOF
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
spawn() { rade spawn "$@" --json | jq -r .pane; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
ov() { rpc debug.ui "{\"op\": \"overview\", \"arg\": \"${1:-state}\"}"; }
key() { rpc debug.ui "{\"op\": \"key\", \"keys\": \"$1\"}" >/dev/null; }
state() { rade debug state; }
shown_ws() { state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .workspace'; }
focused() { state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .active_worktree as $w | .worktrees[] | select(.id==$w) | .active_tab as $t | .tabs[] | select(.id==$t) | .focused'; }
bands() { ov | jq -c '[.sections[] | [.title, [.cards[].name]]]'; }

A=$HOME/workspaces/acme B=$HOME/workspaces/bravo
mkdir -p "$A/.rade" "$B/.rade"
printf 'name = "ACME"\ncolor = "green"\n' >"$A/.rade/workspace.toml"
printf 'name = "Bravo"\ncolor = "purple"\n' >"$B/.rade/workspace.toml"
for r in "$A/shop" "$B/fleet"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && G -C "$r" commit -q --allow-empty -m init
done
start_app
rade workspace add "$A" >/dev/null
rade workspace add "$B" >/dev/null
rade source add "$A/shop" >/dev/null
rade source add "$B/fleet" >/dev/null
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null

echo "== scene: Agents in ACME and Bravo"
planner=$(spawn --source shop --agent fake --name planner "hello")
tests=$(spawn --source shop --agent fake --name tests "hi")
dev=$(spawn --source shop --agent fake --name dev --cmd "printf 'one\ntwo\nthree\n'; sleep 600")
# A screen with a line far wider than a card and a tab (a Codex prompt line).
wide=$(spawn --source fleet --agent fake --name wide --cmd "printf 'a\tb\n%0300d\n  ? for shortcuts                                        x\n' 7; sleep 600")
migrate=$(spawn --source fleet --agent fake --name migrate "hello")
indexer=$(spawn --source fleet --agent fake --name indexer "hello")
eventually 10 screen_has "$planner" HELLO
eventually 10 screen_has "$migrate" HELLO
eventually 10 screen_has "$dev" three
rade status done --pane "$planner"
rade status running --pane "$dev"
sleep 1.1
rade status running --pane "$indexer"
sleep 1.1
rade status running --pane "$wide"
rade status error --pane "$migrate"
sleep 1.2
rade status waiting --pane "$tests"
# Bravo shown, the keyboard in a Terminal running `cat -v`.
rade debug action workspace_3
cat=$(spawn --source fleet --name cat --cmd "cat -v")
rade focus "$cat"
eventually 5 screen_has "$cat" "" || true
sleep 0.5

echo "== R117-f open from a Terminal, bands, Enter → hidden Workspace, Esc back"
key "ctrl+shift+space"
check "R117-f Ctrl+Shift+Space opens Agent Overview" test "$(ov | jq -r .open)" = true
check "R117-f the Terminal received nothing" bash -c "! rade read $cat | grep -qF '^@'"
check "R117-f bands: Needs you (tests, migrate), Finished (planner), Running (wide, indexer, dev)" \
    test "$(bands)" = '[["Needs you",["tests","migrate"]],["Finished, not seen",["planner"]],["Running",["wide","indexer","dev"]]]'
check "R117-f count 6 in 2 Workspaces" test "$(ov | jq -r .count)" = "6 in 2 Workspaces"
check "R117-f card place = Source / Workspace" test "$(ov | jq -r '.sections[0].cards[0] | [.workspace, .place] | join("|")')" = "ACME|shop"
check "R117-f opens on the first Needs-you card" test "$(ov | jq -r '.sections[0].cards[0].selected')" = true
shot overview-dark
key "enter"
check "R117-f Enter: closed" test "$(ov | jq -r .open)" = false
check "R117-f Enter: ACME shown" eventually 3 is shown_ws acme
check "R117-f Enter: tests focused" test "$(focused)" = "$tests"
check "R117-f Enter: its Notification read" test "$(state | jq --arg p "$tests" '[.notifications[] | select(.pane==$p and (.read|not))] | length')" = 0
rade debug action workspace_3
rade focus "$cat"
sleep 0.3
key "ctrl+shift+space"
key "escape"
check "R117-f Esc closes" test "$(ov | jq -r .open)" = false
check "R117-f Esc: Bravo still shown" test "$(shown_ws)" = bravo
key "x"
check "R117-f after Esc typing reaches the Terminal" eventually 3 screen_has "$cat" "x"

echo "== R117-g hold to flip"
key "ctrl+shift+space"
key "ctrl+shift+space"
key "ctrl+shift+space"
third=$(ov | jq -r '[.sections[].cards[]] | map(select(.selected))[0].panes[0]')
key "release:ctrl"
check "R117-g 3 presses + release: the third card's Agent focused" eventually 3 is focused "$third"
check "R117-g … and it is closed" test "$(ov | jq -r .open)" = false
key "ctrl+shift+space"
key "release:ctrl"
check "R117-g 1 press + release: still open" test "$(ov | jq -r .open)" = true

echo "== R117-h tails"
check "R117-h dev's tail = its last 3 lines" test "$(ov | jq -c --arg p "$dev" '[.sections[].cards[] | select(.panes[0]==$p)][0].tail')" = '["one","two","three"]'
check "R117-h a wide screen: 3 tail lines, each one ellipsized line, clipped box" \
    test "$(ov | jq -c --arg p "$wide" '[.sections[].cards[] | select(.panes[0]==$p)][0].tail_widget')" = '{"lines":3,"single":true,"clip":true}'
check "R117-h no Quiet card has a tail" test "$(ov | jq '[.sections[] | select(.band=="quiet") | .cards[] | select(.tail|length>0)] | length')" = 0
check "R117.3 git facts never break the card" test "$(ov | jq -r '.sections[0].cards[0].name')" != null
key "escape"

echo "== R117.2 Quiet folds only when it does not fit"
for i in 1 2 3 4 5 6 7 8 9; do
    q=$(spawn --source fleet --agent fake --name "q$i" --cmd "sleep 600")
    rade status exited --pane "$q"
done
ov open >/dev/null
# Folded or not depends on the room the other bands leave; either way the
# title counts all, and a fold fills whole rows (cards + the tile).
quiet() { ov | jq -c --argjson c "$(ov | jq .cols)" '[.sections[] | select(.band=="quiet") | [(.cards | length), .more, .total, (((.cards | length) + 1) % $c)]][0]'; }
quiet_ok() {
    local c; c=$(ov | jq .cols)
    [[ $(ov | jq --argjson c "$c" '[.sections[] | select(.band=="quiet") | if .more == 0 then (.cards | length) == .total else ((.cards | length) + 1) % $c == 0 and .total == (.cards | length) + .more end][0]') == true ]]
}
check "R117.2 Quiet: all shown, or folded to whole rows; the title counts all" quiet_ok
# Only Quiet on screen (its chip): room for all, nothing folded.
ov "chip quiet" >/dev/null
check "R117.2 Quiet alone: all shown, none folded" test "$(quiet | jq -c '.[0:3] | [.[0] == .[2], .[1]]')" = '[true,0]'
key "escape"
key "escape"

echo "== R117-o Preview: keyboard rest, hover, live, typing, Alt+P, click"
long=$(spawn --source shop --agent fake --name long --cmd "for i in \$(seq 1 60); do echo line \$i; done; cat")
eventually 10 screen_has "$long" "line 60"
rade status running --pane "$long"
pv() { ov | jq -c ".preview | $1"; }
# The Preview's rect is clear of its card's and inside the window.
apart='(.rect.x + .rect.w <= .card.x) or (.card.x + .card.w <= .rect.x) or (.rect.y + .rect.h <= .card.y) or (.card.y + .card.h <= .rect.y)'
ov open >/dev/null
check "R117-o not at once: the selection must rest" test "$(pv '[.shown, .pending]')" = "[false,\"$tests\"]"
sleep 0.5
check "R117-o keyboard rest: the selected card's Preview" test "$(pv '[.shown, .by, .key]')" = "[true,\"keys\",\"$tests\"]"
check "R117-o column 1: right of its card, not over it" test "$(pv "[.side, ($apart)]")" = '["right",true]'
check "R117-o … top-aligned with its card, as wide as min(600, 46 %)" test "$(pv '[.rect.y == .card.y, .rect.w]')" = '[true,600]'
check "R117-o … no card moved (the selected card in place)" test "$(pv '.card.x')" = "$(rpc debug.ui '{"op":"geometry","widget":"overview-card"}' | jq -r '.x | round')"
shot preview-dark
ov "hover $long" >/dev/null
check "R117-o hover: nothing before the pointer rests 350 ms" test "$(pv '[.key, .pending]')" = "[\"$tests\",\"$long\"]"
sleep 0.6
check "R117-o hover: that card's Preview" test "$(pv '[.shown, .by, .key]')" = "[true,\"hover\",\"$long\"]"
check "R117-o 24 lines from a 60-line screen, the newest last" test "$(pv '[.lines, .text[-1], .at_end]')" = '[24,"line 60",true]'
rade send "$long" "fresh line" --enter >/dev/null
check "R117-o follows a new line within 2 s" eventually 2 is "pv '.text[-1]'" '"fresh line"'
ov leave >/dev/null
sleep 0.4
check "R117-o the pointer leaves card and Preview: gone after 150 ms" test "$(pv .shown)" = false
lastcol=$(ov | jq -r --argjson c "$(ov | jq .cols)" '[.sections[] | select(.band=="running") | .cards[]][$c - 1].panes[0]')
ov "hover $lastcol" >/dev/null
sleep 0.6
check "R117-o last column: left of its card, not over it" test "$(pv "[.side, ($apart)]")" = '["left",true]'
check "R117-o a click on the Preview leaves it to the Preview (stays open)" \
    test "$(ov "click $(pv '"\(.rect.x + .rect.w / 2) \(.rect.y + .rect.h / 2)"' | tr -d '"')" | jq -c '[.closed, .open]')" = '[false,true]'
quiet1=$(ov | jq -r '[.sections[] | select(.band=="quiet") | .cards[]][0].panes[0]')
ov "hover $quiet1" >/dev/null
sleep 0.6
check "R117-o a Quiet row has a Preview too" test "$(pv "[.shown, .pane == \"$quiet1\", ($apart)]")" = '[true,true,true]'
ov leave >/dev/null
ov "filter lo" >/dev/null
check "R117-o hidden while typing a filter" test "$(pv .shown)" = false
sleep 1.1
check "R117-o … back on the selection after a pause" test "$(pv '[.shown, .by]')" = '[true,"keys"]'
key "escape"
key "alt+p"
check "R117-o Alt+P hides the keyboard Preview" test "$(pv '[.shown, .keys]')" = '[false,false]'
check "R117-o … kept in state.json" test "$(state | jq -r .overview_no_preview)" = true
key "down"
sleep 0.5
check "R117-o … no Preview on a keyboard move" test "$(pv .shown)" = false
ov "hover $long" >/dev/null
sleep 0.6
check "R117-o … hover still shows one" test "$(pv '[.shown, .by]')" = '[true,"hover"]'
key "alt+p"
check "R117-o Alt+P again: keyboard Previews back" test "$(pv .keys)" = true
ov "hover $dev" >/dev/null
sleep 0.6
ov preview_click >/dev/null
check "R117-o a click on the Preview jumps to its Agent" eventually 3 is focused "$dev"
check "R117-o … and closes" test "$(ov | jq -r .open)" = false
rade debug action color_scheme_light >/dev/null
sleep 0.5
ov open >/dev/null
sleep 0.6
shot preview-light
key "escape"
rade debug action color_scheme_dark >/dev/null
rpc debug.ui '{"op":"layout","width":900,"height":700,"text_scale":2.0}' >/dev/null
sleep 1.2
ov open >/dev/null
sleep 0.6
# Neither side has 414 px and the content fits neither below nor above:
# beside after all, narrower (unit R117-n covers below / above).
check "R117-o 900 px at 200 %: beside, narrowed to the room left, not over its card" test "$(pv "[.side, .rect.w >= 300 and .rect.x + .rect.w == .body.x + .body.w, ($apart)]")" = '["right",true,true]'
check "R117-o 900 px at 200 %: inside the body (above the key hints)" test "$(pv '.rect.x >= .body.x and .rect.x + .rect.w <= .body.x + .body.w and .rect.y >= .body.y and .rect.y + .rect.h <= .body.y + .body.h')" = true
shot preview-900-2.0
key "escape"
rpc debug.ui '{"op":"layout","width":1440,"height":900,"text_scale":1.0}' >/dev/null
# The window is back at 1440 px before the clicks of R117.1 aim at it.
eventually 5 is "ui layout_info | jq .layout.width" 1440 || true
sleep 0.5
rade close "$long" --force >/dev/null

echo "== R117-k entry points"
ov toggle >/dev/null
check "R117-k titlebar button / toggle opens" test "$(ov | jq -r .open)" = true
ov toggle >/dev/null
rade debug action command_palette >/dev/null
rade --json debug palette --ui type ">agent overview" >/dev/null
sleep 0.3
check "R117-k palette lists Agent Overview first" test "$(rade --json debug palette --ui info | jq -r '.rows[0].title')" = "Agent Overview"
rade --json debug palette --ui key enter >/dev/null
check "R117-k palette action opens" eventually 2 is "ov | jq -r .open" true
key "escape"
ov "band needs_you" >/dev/null
check "R117-k the switcher ! opens filtered to Needs you" test "$(ov | jq -c '[.band, [.sections[].title]]')" = '["needs_you",["Needs you"]]'
key "escape"; key "escape"
ov "band running" >/dev/null
check "R117-k a Statusline count opens filtered to Running" test "$(ov | jq -c '[.band, [.sections[].title]]')" = '["running",["Running"]]'
key "escape"; key "escape"

echo "== R117.1 a click on nothing in particular closes it (macOS)"
center() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"}" | jq -r '"\(.x + .width / 2) \(.y + .height / 2)"'; }
ov open >/dev/null
check "R117.1 click on a chip: stays open" test "$(ov "click $(center overview-chip)" | jq -c '[.closed, .open]')" = '[false,true]'
check "R117.1 click on the filter field: stays open" test "$(ov "click $(center overview-filter)" | jq -c '[.closed, .open]')" = '[false,true]'
check "R117.1 click on a card: left to the card" test "$(ov "click $(center overview-card)" | jq -c '[.closed, .open]')" = '[false,true]'
check "R117.1 click on the key hints bar: closes" test "$(ov "click 700 880" | jq -c '[.closed, .open]')" = '[true,false]'
ov open >/dev/null
check "R117.1 click on empty space right of the cards: closes" test "$(ov "click 1400 600" | jq -r .closed)" = true

echo "== R117-c filter in the app"
ov open >/dev/null
ov "filter mig" >/dev/null
check "R117-c filter 'mig' → migrate" test "$(ov | jq -c '[.sections[].cards[].name]')" = '["migrate"]'
ov "filter zzz" >/dev/null
check "R117-c no match line" test "$(ov | jq -r .empty)" = 'No Agent matches "zzz" · Esc clears'
key "escape"
check "R117-c Esc clears the filter first" test "$(ov | jq -r '[.open, .filter] | join(",")')" = "true,"
# A count chip and Ctrl+Alt+n change the query from a handler (a RefCell
# borrowed twice crashed the app here once).
ov "chip running" >/dev/null
check "R117-k a count chip filters to its band (app alive)" test "$(ov | jq -c '[.band, [.sections[].title]]')" = '["running",["Running"]]'
ov "chip running" >/dev/null
check "R117-k … again: all bands" test "$(ov | jq -r '.band')" = null
key "ctrl+alt+3"
check "R117-c Ctrl+Alt+3 shows only Bravo (app alive)" test "$(ov | jq -c '[.workspace, ([.sections[].cards[].workspace] | unique)]')" = '["bravo",["Bravo"]]'
key "ctrl+alt+3"
check "R117-c … again: every Workspace" test "$(ov | jq -r .workspace)" = null
key "alt+g"
check "R117.4 Alt+G groups by Workspace" test "$(ov | jq -c '[.group, [.sections[].title]]')" = '["workspace",["ACME","Bravo"]]'
shot overview-by-workspace
key "alt+g"
key "escape"

echo "== R117-m look, light"
rade debug action color_scheme_light >/dev/null
sleep 0.5
ov open >/dev/null
sleep 0.5
shot overview-light
key "escape"
check "R117-m screenshots dark + light saved" test -s "$OUT/overview-dark.png" -a -s "$OUT/overview-light.png"
rade debug action color_scheme_dark >/dev/null

echo "== R117-j 40 Agents in 5 Workspaces, 1280 px, text scale 200 %, budgets"
for w in c d e; do
    mkdir -p "$HOME/workspaces/$w" && rade workspace add "$HOME/workspaces/$w" >/dev/null
done
srcs=(shop fleet general@c general@d general@e)
for i in $(seq 1 25); do
    s=${srcs[$((i % 5))]}
    p=$(spawn --source "$s" --agent fake --name "a$i" --cmd "sleep 600")
    case $((i % 4)) in
        0) rade status running --pane "$p" ;; 1) rade status done --pane "$p" ;;
        2) rade status running --pane "$p" ;; 3) rade status exited --pane "$p" ;;
    esac
done
check "R117-j 40 Agents listed" test "$(ov open | jq -r .count)" = "40 in 5 Workspaces"
check "R117-j 4 columns at 1440 px" test "$(ov | jq -r .cols)" = 4
check "R117-j Running folds after 8" test "$(ov | jq '[.sections[] | select(.band=="running") | .cards | length][0]')" = 8
check "R117-j Quiet folds when it does not fit, its title counts all" \
    test "$(ov | jq -c '[.sections[] | select(.band=="quiet") | [.more > 0, .total == ((.cards | length) + .more)]][0]')" = '[true,true]'
sleep 0.6
shot overview-40
key "escape"
ui perf reset >/dev/null
for _ in 1 2 3 4 5; do ov open >/dev/null; sleep 0.6; key "escape"; sleep 0.3; done
perf=$(rpc debug.ui '{"op":"perf"}')
first=$(jq -r '.samples.overview.p95 // "none"' <<<"$perf")
anim=$(jq -r '.samples["overview:anim"].p95 // "none"' <<<"$perf")
echo "  first frame p95 ${first} ms, animation frame work p95 ${anim} ms"
check "R117-j first frame within 50 ms (p95)" awk -v v="$first" 'BEGIN { exit !(v != "none" && v <= 50) }'
check "R117-j animation frame work within 16.7 ms (p95)" awk -v v="$anim" 'BEGIN { exit !(v != "none" && v <= 16.7) }'
for mode in "1280 800 1.0 3" "1440 900 2.0 2"; do
    set -- $mode
    rpc debug.ui "{\"op\":\"layout\",\"width\":$1,\"height\":$2,\"text_scale\":$3}" >/dev/null
    sleep 1.2
    ov open >/dev/null; sleep 0.5
    check "R117-j $1 px × $3: $4 columns" test "$(ov | jq -r .cols)" = "$4"
    check "R117-j $1 px × $3: no horizontal scroll" test "$(ui hscroll | jq .count)" = 0
    ui clipped >"$T/clip.json"
    jq -c '.labels[:5][] | [.text, .classes, .why]' "$T/clip.json" 2>/dev/null | sed 's/^/    clipped: /'
    check "R117-j $1 px × $3: no clipped label" test "$(jq .count "$T/clip.json")" = 0
    shot "overview-$1-$3"
    key "escape"
done
rpc debug.ui '{"op":"layout","width":1440,"height":900,"text_scale":1.0}' >/dev/null

echo "== R117-i Reduce Animation"
check "R117-i with animations: in 200 ms" test "$(ov open | jq -r .durations.in)" = 200
key "escape"
stop_app
mkdir -p "$XDG_CONFIG_HOME/gtk-4.0"
printf '[Settings]\ngtk-enable-animations=0\n' >"$XDG_CONFIG_HOME/gtk-4.0/settings.ini"
start_app
check "R117-i animations off → in 0 ms" test "$(ov open | jq -c '[.durations.animations, .durations.in]')" = '[false,0]'
key "escape"
check "R117-i … and out 0 ms, closed at once" test "$(ov | jq -c '[.open, .durations.out]')" = '[false,0]'

echo "== R117-j no Agent"
for p in $(rade list --all --json | jq -r '.panes[] | select(.agent != null) | .pane'); do rade close "$p" --force >/dev/null 2>&1 || true; done
check "R117-j empty line" test "$(ov open | jq -r .empty)" = "No Agents running · Ctrl+Shift+P > Spawn Agent"
key "escape"

stop_app
echo "M43: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
