#!/usr/bin/env bash
# M6 end-to-end tests (ADR-0006): one Notification per Agent event (R3.1
# coalescing, desktop banner replaced not doubled), Agent Status fallbacks
# (R10.2: idle, OSC/bell, command-finished; status_source), the Codex hint
# (R16.4), Viewer Back with lines (R7.3), focus after an Agent registers,
# status dots (R10.3, screenshots light + dark) and Session Restore after
# kill -9 (R11). Driven only through `rade`; the fake Agent stands in.
# Usage: tests/e2e/m6.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m6
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade" "$T"/p1 "$T"/p2 "$T"/p3
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 2
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
[agents.silent]
command = "FAKE_NO_HOOKS=1 $FAKE"
[agents.fagy]
command = "FAKE_NO_HOOKS=1 $FAKE"
hooks = "agy"
[agents.fclaude]
command = "FAKE_NO_HOOKS=1 $FAKE"
hooks = "claude"
[agents.fcodex]
command = "FAKE_NO_HOOKS=1 $FAKE"
hooks = "codex"
EOF

state() { rade debug state; }
field() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
entries() { state | jq -r --arg p "$1" '[.notifications[]? | select(.pane==$p)] | length'; }
last_text() { state | jq -r --arg p "$1" '[.notifications[]? | select(.pane==$p)][-1].text'; }
spawn() { rade spawn "$@" --json | jq -r .pane; }
notifyd() { grep -c "^$1.*$2" "$NOTIFYD_LOG" || true; }
kill9() { kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=; }

echo "== start"
start_app
for p in p1 p2 p3; do rade source add "$T/$p" >/dev/null; done
rade focus "$(rade list --json | jq -r '[.panes[] | select(.source=="general")][0].pane')" >/dev/null

echo "== R3.1 one Notification per Agent event"
c1=$(spawn --agent fake --source p1 --name c1 'osc PONG')
check "c1 turn done (hook)" test "$(rade wait "$c1" --until done --timeout 20)" = done
sleep 1.5 # its OSC 9 follows the Stop hook
check "hook 'finished' + OSC 9 'PONG' → one entry" test "$(entries "$c1")" = 1
check "…text 'c1 finished — PONG'" test "$(last_text "$c1")" = "c1 finished — PONG"
check "…source merged" test "$(state | jq -r --arg p "$c1" '[.notifications[] | select(.pane==$p)][0].source')" = merged
check "one desktop banner for c1 (hidden Tab, smart)" test "$(notifyd NOTIFY "'c1 finished")" = 1
check "…replaced in place with the merged text, no second banner" eventually 3 grep -q "^REPLACE.*'c1 finished — PONG'" "$NOTIFYD_LOG"
check "…no banner with the bare OSC text" test "$(grep -c "^NOTIFY.*	'PONG'" "$NOTIFYD_LOG" || true)" = 0
check "debug desktop logs the replace" bash -c "rade debug desktop --json | jq -e '[.log[] | select(.event==\"replace\")] | length == 1' >/dev/null"
sleep 1.2 # Ghostty: one OSC notification per second
c2=$(spawn --agent fake --source p1 --name c2 'oscfirst need you')
check "c2 waiting (hook)" test "$(rade wait "$c2" --until waiting --timeout 20)" = waiting
sleep 1
check "OSC 777 first, then the hook → one entry" test "$(entries "$c2")" = 1
check "…'c2 needs input — Fake: need you'" test "$(last_text "$c2")" = "c2 needs input — Fake: need you"
sleep 1.2
sh=$(spawn --source p1 --name sh)
eventually 10 screen_has "$sh" '$' || true
rade send "$sh" "printf '\\e]9;plain hello\\a'" --enter >/dev/null
check "plain shell OSC 9 still a Notification" eventually 5 is "last_text $sh" "plain hello"
rade send "$sh" "printf '\\a'" --enter >/dev/null
check "plain shell BEL (hidden) still a Notification" eventually 5 is "last_text $sh" "Bell"
check "…two entries, no status" test "$(entries "$sh"),$(field "$sh" status)" = "2,null"

echo "== R10.2 fallbacks for Agents without the relevant hook"
cx=$(spawn --agent fcodex --source p2 --name cx 'spin') # for R16.4 below
sleep 1
check "R16.4 no Codex hint at the start of a turn" test "$(rade debug window --json | jq -r .banner.revealed)" = false
s1=$(spawn --agent silent --source p2 --name s1 'hello')
check "no hooks: output idle 2 s while running → done" test "$(rade wait "$s1" --until done --timeout 15)" = done
check "…status_source heuristic" test "$(field "$s1" status_source)" = heuristic
check "…raised 's1 finished'" test "$(last_text "$s1")" = "s1 finished"
sleep 1.2
s2=$(spawn --agent silent --source p2 --name s2 'oscfirst please')
check "no hooks: OSC while running → waiting" test "$(rade wait "$s2" --until waiting --timeout 15)" = waiting
sleep 1
check "…one entry 's2 needs input — Fake: please'" test "$(entries "$s2"),$(last_text "$s2")" = "1,s2 needs input — Fake: please"
ag=$(spawn --agent fagy --source p2 --name ag)
cl=$(spawn --agent fclaude --source p2 --name cl)
eventually 10 screen_has "$ag" '> ' || true
eventually 10 screen_has "$cl" '> ' || true
rade status running --pane "$ag" >/dev/null # a hook: agy reports running/done only
rade status running --pane "$cl" >/dev/null # claude-style hooks report everything
sleep 5
check "within 10 s of a hook report: no fallback" test "$(field "$ag" status)" = running
check "agy-style: idle while running → waiting (no waiting hook)" test "$(rade wait "$ag" --until waiting --timeout 15)" = waiting
check "…status_source heuristic" test "$(field "$ag" status_source)" = heuristic
check "claude-style hooks: never second-guessed" test "$(field "$cl" status),$(field "$cl" status_source)" = "running,hook"
hs=$(spawn --source p2 --name hs)
eventually 10 screen_has "$hs" '$' || true
# `clw` typed in a shell: the Agent's SessionStart hook registers it, then
# it exits back to the prompt (OSC 133 command finished).
rade send "$hs" "bash -c 'rade status --agent fake </dev/null; sleep 2'" --enter >/dev/null
check "hand-started Agent registered" eventually 5 is "field $hs agent" fake
check "its command finished → exited (OSC 133)" eventually 10 is "field $hs status" exited
check "…status_source heuristic" test "$(field "$hs" status_source)" = heuristic

echo "== R16.4 Codex hint"
check "Codex turn 30 s with no hook → banner" eventually 35 is "rade debug window --json | jq -r .banner.revealed" true
check "…says what to do" bash -c "rade debug window --json | jq -r .banner.title | grep -q '/hooks'"
shot codex-hint
rade debug action dismiss_banner >/dev/null
sleep 2.5
check "dismissed, and not shown again this session" test "$(rade debug window --json | jq -r .banner.revealed)" = false
rade close "$cx" --force >/dev/null

echo "== R7.3 Viewer Back restores the line"
rade focus "$sh" >/dev/null
for i in $(seq 1 80); do echo "line $i"; done > "$T/p1/a.txt"
printf 'b\n' > "$T/p1/b.txt"
v=$(rade open "$T/p1/a.txt:42" --pane "$sh" --json | jq -r .pane)
rade open "$T/p1/b.txt" --pane "$sh" >/dev/null
rade debug viewer "$v" back >/dev/null
check "Back → a.txt:42 in the model" eventually 5 is "state | jq -r '.. | objects | select(.id?==\"$v\") | \"\\(.kind.path):\\(.kind.line)\"'" "$T/p1/a.txt:42"
check "…and on screen (cursor line 42)" eventually 5 is "rade debug viewer $v info | jq -r .cursor_line" 42

echo "== focus stays in the Terminal when it becomes an Agent Pane (file tree shown)"
rade debug action toggle_file_tree >/dev/null
kf=$(spawn --source p1 --name kf --split right)
sleep 1
rade status --agent fake --pane "$kf" >/dev/null
sleep 1
check "keyboard focus still on kf's Terminal" test "$(rade debug window --json | jq -r '.focus.pane')" = "$kf"
rade debug action toggle_file_tree >/dev/null

echo "== R10.3 status dots (look at the screenshots)"
rade focus "$ag" >/dev/null
sleep 1
shot dots-light
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not start"; exit 1; }
# Statuses are cleared on restart (ADR-0005): set a mix again for the look.
for x in "$s1:done" "$s2:waiting" "$ag:running" "$cl:error"; do
    rade status "${x#*:}" --pane "${x%%:*}" >/dev/null 2>&1 || true
done
rade focus "$ag" >/dev/null
sleep 1.5
shot dots-dark
check "rade list: hook-set statuses are status_source hook" test "$(field "$cl" status),$(field "$cl" status_source)" = "error,hook"

echo "== R11 Session Restore after kill -9"
r1=$(spawn --agent fake --source p3 --name r1 'hi')
rade wait "$r1" --until done --timeout 20 >/dev/null
sid=$(state | jq -r --arg p "$r1" '.. | objects | select(.id?==$p) | .session_id')
rade open "$T/p1/a.txt:7" --pane "$r1" >/dev/null
rade open "$T/p1/b.txt" --pane "$r1" >/dev/null
rade debug panel toggle >/dev/null # collapse the Notification panel
rade debug panel activate "$(state | jq -r '[.notifications[] | select(.pane!="'"$r1"'")][0].id')" >/dev/null
rade focus "$r1" >/dev/null
sleep 2 # the debounced save (≤ 1 s after the last change)
# Everything the user arranged; statuses and commands change by design.
shape() { jq -S '{active_source, notifications_collapsed, file_tree,
    notifications: [.notifications[] | {id, pane, text, read}],
    sources: [.sources[] | {id, name, path, active_worktree, worktrees: [.worktrees[] | {id, active_tab, tabs: [.tabs[] | {id, name, focused, zoomed, grid,
        panes: [.. | objects | select(.kind?) | {id, name, agent, parent, kind: (.kind | del(.command))}]}]}]}]}'; }
before=$(state | shape)
kill9
check "state.json on disk = the model at kill -9" test "$(shape < "$STATE")" = "$before"
start_app
echo "$before" > "$OUT/restore-before.json"
state | shape > "$OUT/restore-after.json"
check "restart: same Sources, Tabs, Layouts, Viewers, Notifications" diff "$OUT/restore-before.json" "$OUT/restore-after.json"
check "3 Sources + General" test "$(rade source list --json | jq '.sources | length')" = 4
check "fake Agent resumed into its session ($sid)" eventually 15 screen_has "$r1" "resumed $sid"
check "Viewer restored at b.txt with a.txt:7 on its Back list" test "$(state | jq -r '[.. | objects | select(.kind?.type?=="viewer")] | map(select(.kind.path=="'"$T/p1/b.txt"'"))[0].kind.back[-1] | "\(.path):\(.line)"')" = "$T/p1/a.txt:7"
check "Notification panel still collapsed" test "$(state | jq -r .notifications_collapsed)" = true
check "exited hand-started Agent comes back as a plain shell" test "$(state | jq -r --arg p "$hs" '.. | objects | select(.id?==$p) | .kind.command')" = null
rade debug panel toggle >/dev/null # expand
sleep 1.5
kill9
start_app
check "a change 1.5 s before kill -9 survives (R11.1 ≤ 5 s)" test "$(state | jq -r '.notifications_collapsed // false')" = false

stop_app
echo "M6 E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
[[ $FAIL -eq 0 ]]
