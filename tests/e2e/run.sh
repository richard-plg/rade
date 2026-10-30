#!/usr/bin/env bash
# End-to-end tests (ADR-0006): run the real rade-app in a headless Weston and
# drive it only through the `rade` CLI. Screenshots land in target/e2e/.
# Usage: tests/e2e/run.sh            (builds release first unless NO_BUILD=1)
set -euo pipefail

source "$(dirname "$0")/lib.sh"

# R17.2: Terminal appearance comes from the user's Ghostty config.
mkdir -p "$XDG_CONFIG_HOME/ghostty"
echo 'background = #102030' > "$XDG_CONFIG_HOME/ghostty/config"
pixel() { python3 -c "from PIL import Image; print('%02x%02x%02x' % Image.open('$1').convert('RGB').getpixel(($2, $3)))"; }

echo "== start"
start_app
first=$(rade list --json | jq -r '.panes[0].pane')
check "first start opens a Terminal in General" test "$(rade list --json | jq -r '.panes[0].source')" = general

echo "== R1-b: General Terminal pwd is \$HOME"
eventually 10 screen_has "$first" '$' || true
rade send "$first" 'pwd' --enter
check "R1-b pwd = $HOME" eventually 5 screen_has "$first" "$HOME"
shot r1b-general
check "R17.2 Ghostty config background used" test "$(pixel "$OUT/r1b-general.png" 1000 700)" = 102030

echo "== R1-a / R1.5: source add, Terminal cwd = Source root"
X=/tmp/rade-e2e-x-$$ Y=/tmp/rade-e2e-y-$$
mkdir -p "$X/sub" "$Y"
git -C "$X" init -q -b feat-x 2>/dev/null || git -C "$X" init -q
check "source add prints id" test "$(rade source add "$X" --name x)" = x
rade source add "$Y" --name y >/dev/null
check "General stays last" test "$(rade source list --json | jq -r '[.sources[].source] | join(",")')" = "x,y,general"
check "branch read from HEAD" test "$(rade source list --json | jq -r '.sources[0].branch')" = feat-x
xa=$(rade spawn --source x --json | jq -r .pane)
eventually 10 screen_has "$xa" '$' || true
rade send "$xa" 'pwd' --enter
check "R1-a pwd = $X (Tab never shown: R2.8 process started hidden)" eventually 5 screen_has "$xa" "$X"
rade send "$xa" 'echo "E:$RADE_PANE_ID:$RADE_SOURCE:$RADE_WORKTREE:$RADE_WORKTREE_PATH"' --enter
check "Terminal env has RADE_PANE_ID/SOURCE/WORKTREE/WORKTREE_PATH" \
    eventually 5 screen_has "$xa" "E:$xa:x:main:$X"
rade send "$xa" 'test -S "$RADE_SOCKET" && command -v rade >/dev/null && echo SOCK-OK' --enter
check "RADE_SOCKET is live and rade is on PATH" eventually 5 screen_has "$xa" SOCK-OK

echo "== R1.3 / R1.4: split, cwd = Worktree root even after cd"
rade send "$xa" "cd $X/sub" --enter
xb=$(rade spawn --source x --tab current --split right --json | jq -r .pane)
eventually 10 screen_has "$xb" '$' || true
rade send "$xb" 'pwd' --enter
check "split Terminal pwd = $X (not $X/sub)" eventually 5 screen_has "$xb" "$X"
check "split pane is not in sub" bash -c "! rade read $xb | grep -qF '$X/sub'"
xc=$(rade spawn --source x --split down --name logs --json | jq -r .pane)
check "layout R(xa,D(xb,xc))" test "$(rade debug state | jq -r --arg x x '.sources[] | select(.id==$x) | .worktrees[0].tabs[0].layout | [.type, .dir, .b.dir] | join(",")')" = "split,right,down"
check "pane addressable by name" test "$(rade list --json | jq -r '.panes[] | select(.name=="logs") | .pane')" = "$xc"
rade spawn --source x --tab new >/dev/null
rade focus "$xb"
sleep 0.5
shot x-splits

echo "== R1.7: Zoom leaves the Layout unchanged"
check "focus switched to x" test "$(rade debug state | jq -r .active_source)" = x
before=$(rade debug state)
rade debug action zoom >/dev/null   # same code path as Ctrl+Shift+Z
check "zoomed pane is the focused one" test "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[0].zoomed')" = "$xb"
sleep 0.3
shot x-zoomed
rade debug action zoom >/dev/null
check "un-Zoom restores the identical Layout" test "$(rade debug state)" = "$before"

echo "== R1-c: switch Sources and back restores Tabs, Layout, focus"
ya=$(rade spawn --source y --json | jq -r .pane)
rade spawn --source y --tab new >/dev/null
yc=$(rade spawn --source y --tab new --json | jq -r .pane)
rade focus "$ya"
x_before=$(rade debug state | jq -S '.sources[] | select(.id=="x")' | norm)
rade focus "$yc"
sleep 0.3
shot y-source
check "on y" test "$(rade debug state | jq -r .active_source)" = y
rade focus "$xb"
sleep 0.3
x_after=$(rade debug state | jq -S '.sources[] | select(.id=="x")' | norm)
check "R1-c x identical after switching away and back" test "$x_before" = "$x_after"
check "R1-c focus is xb again" test "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[0].focused')" = "$xb"

echo "== close"
rade close "$xc"
check "close collapses split" test "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[0].layout.b.type')" = leaf
check "close unknown pane exits 4" bash -c "rade close p_999 2>/dev/null; test \$? -eq 4"
rade send "$xb" 'exit' --enter
check "shell exit closes its Pane" eventually 5 bash -c "! rade list --json | jq -e '.panes[] | select(.pane==\"$xb\")' >/dev/null"

echo "== read options"
check "read --lines 1 returns one line" test "$(rade read "$xa" --lines 1 | wc -l)" = 1
check "read reports a cursor" bash -c "rade read $xa --json | jq -e '.cursor.row >= 0 and .cursor.col >= 0' >/dev/null"

echo "== keybinding actions (same code as the shortcuts)"
rade focus "$xa"
n0=$(rade list --json | jq '.panes | length')
rade debug action split_right >/dev/null
check "split_right adds a Pane" test "$(rade list --json | jq '.panes | length')" = $((n0 + 1))
new=$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[0].focused')
rade debug action focus_left >/dev/null
check "focus_left moves back to xa" test "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[0].focused')" = "$xa"
rade debug action next_tab >/dev/null
check "next_tab selects Tab 2" test "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].active_tab')" = "$(rade debug state | jq -r '.sources[] | select(.id=="x") | .worktrees[0].tabs[1].id')"
rade debug action prev_tab >/dev/null
rade debug action source_3 >/dev/null
check "source_3 (Alt+3) shows General" test "$(rade debug state | jq -r .active_source)" = general
rade debug action source_1 >/dev/null
rade focus "$new"
rade debug action close_pane >/dev/null
check "close_pane asks first (R19.9)" eventually 3 bash -c "rade debug dialog | jq -e .open >/dev/null"
rade debug dialog respond close >/dev/null
check "close_pane closes the focused Pane" test "$(rade list --json | jq '.panes | length')" = "$n0"
mkdir -p "$XDG_CONFIG_HOME/rade"
printf '[keybindings]\nnew_tab = "<Ctrl><Shift>y"\n' > "$XDG_CONFIG_HOME/rade/config.toml"
check "R17.1 config.toml live-reloads" eventually 5 grep -q "reloaded $XDG_CONFIG_HOME/rade/config.toml" "$OUT/app.log"

echo "== errors and exit codes"
check "unknown source exits 4" bash -c "rade spawn --source nope 2>/dev/null; test \$? -eq 4"
check "remove General exits 2" bash -c "rade source remove general 2>/dev/null; test \$? -eq 2"
check "add non-dir exits 2" bash -c "rade source add /nonexistent-$$ 2>/dev/null; test \$? -eq 2"
check "bad usage exits 2" bash -c "rade spawn --split sideways 2>/dev/null; test \$? -eq 2"
check "socket mode 0600" test "$(stat -c %a "$XDG_RUNTIME_DIR/rade/rade-dev.sock")" = 600

echo "== Session Restore: quit, relaunch, same Layout and cwds"
rade send "$xa" "cd $X/sub" --enter
rade spawn --source x --split down --name restored-split >/dev/null
sleep 1.5
pre=$(rade debug state | norm)
stop_app
check "state.json written" test -s "$STATE"
check "state.json has version" test "$(jq .version "$STATE")" = 3
check "cwd tracked via OSC 7" test "$(jq -r --arg p "$xa" '.. | objects | select(.id?==$p) | .kind.cwd' "$STATE")" = "$X/sub"
start_app
post=$(rade debug state | norm)
check "restored model identical" test "$pre" = "$post" || diff <(echo "$pre") <(echo "$post") | head -40
eventually 10 screen_has "$xa" '$' || true
rade send "$xa" 'pwd' --enter
check "restored Terminal starts in saved cwd" eventually 5 screen_has "$xa" "$X/sub"
rade focus "$xa"
sleep 0.5
shot restored

echo "== source remove never touches files"
rade source remove y >/dev/null
check "y gone from list" test "$(rade source list --json | jq -r '[.sources[].source] | join(",")')" = "x,general"
check "y folder still exists" test -d "$Y"
check "y panes closed" test "$(rade list --json | jq '[.panes[] | select(.source=="y")] | length')" = 0

stop_app
check "not running exits 3" bash -c "rade list 2>/dev/null; test \$? -eq 3"
rm -rf "$X" "$Y"

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
