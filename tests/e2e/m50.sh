#!/usr/bin/env bash
# M50 end-to-end tests (ADR-0006): Staged Session Restore (R130). 62 fake
# Claude Agents in 7 Workspaces (the owner's 1.4.0 shape, made up here) are
# restored after a relaunch while tests/e2e/probe-latency.py calls the socket
# every 50 ms: the shown Workspace starts first, the others follow at
# AGENT_GAP_MS per Agent, the window answers within 250 ms throughout, a
# socket call or a Workspace switch starts a waiting Pane at once, a quit in
# the middle keeps every session, a paused Source and a closed Pane never run.
# Usage: tests/e2e/m50.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
[[ -n ${DEBUG_M50:-} ]] && set -x
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m50
source "$HERE/lib.sh"

mkdir -p "$T/bin" "$XDG_CONFIG_HOME/rade"
{
    echo '#!/usr/bin/env bash'
    echo 'FAKE_AGENT=claude'
    tail -n +2 "$HERE/fake-agent.sh"
} >"$T/bin/claude"
chmod +x "$T/bin/claude"
export PATH=$T/bin:$PATH FAKE_ARGV=$T/argv.log FAKE_PROMPTS=$T/prompts.log
: >"$FAKE_ARGV"
: >"$FAKE_PROMPTS"
printf 'idle_seconds = 60\n' >"$XDG_CONFIG_HOME/rade/config.toml"
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock

repo() { mkdir -p "$1" && git -C "$1" init -q -b main && git -C "$1" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init; }
panes_of() { rade list --all --json | jq -r --arg w "$1" '.panes[] | select(.workspace == $w and .agent != null) | .pane'; }
ws_of() { rade list --all --json | jq -r --arg p "$1" '.panes[] | select(.pane == $p) | .workspace'; }
restore() { rpc debug.restore; }
resumed() { grep -c -- "^$1 .*--resume" "$FAKE_ARGV" || true; }

start_app
echo "== setup: 62 Agents in 7 Workspaces"
repo "$T/src/w0"
rade source add "$T/src/w0" >/dev/null
for i in 1 2 3 4 5 6; do
    root=$HOME/workspaces/w$i
    mkdir -p "$root/.rade"
    printf 'name = "w%s"\n' "$i" >"$root/.rade/workspace.toml"
    rade workspace add "$root" >/dev/null
    repo "$root/s$i"
    rade source add "$root/s$i" >/dev/null
done
n=0
for i in 0 1 2 3 4 5 6; do
    count=9; [[ $i == 0 ]] && count=8
    for k in $(seq "$count"); do
        tab=current; ((k % 3 == 1)) && tab=new
        src=s$i; [[ $i == 0 ]] && src=w0
        rade spawn --agent claude --source "$src" --name "a$i-$k" --tab "$tab" "hello $i $k" >/dev/null
        n=$((n + 1))
    done
done
check "62 Agents spawned" test "$n" = 62
eventually 90 is 'rade list --all --json | jq "[.panes[] | select(.status == \"done\")] | length"' 62 || true
check "all 62 finished their turn" is 'rade list --all --json | jq "[.panes[] | select(.status == \"done\")] | length"' 62
rade list --all --json | jq -c '[.panes[] | {pane, workspace, source, status, agent}]' >"$OUT/setup.json"
rade workspace switch w3 >/dev/null
W3=($(panes_of w3)); W2=($(panes_of w2)); W5=($(panes_of w5)); W6=($(panes_of w6))
rade focus "${W3[4]}" >/dev/null
rade source pause s6 --force >/dev/null
check "w6 paused" test "$(rpc debug.state | jq -r '.sources[] | select(.id == "s6") | .paused')" = true
sleep 1.5 # saved
stop_app

echo "== R130-c: relaunch with the probe running, hands off"
: >"$FAKE_ARGV"
python3 "$HERE/probe-latency.py" "$SOCK" --every 50 --for 35 >"$OUT/probe.json" &
PROBE=$!
start_app
wait "$PROBE"
r=$(restore)
check "R130-c the queue is empty" test "$(jq '.pending | length' <<<"$r")" = 0
# Order: every queued w3 Agent started before any Agent of another Workspace
# (the Panes of the Tab on screen start as GTK shows them, not in this log).
started=$(jq -r '.started[].pane' <<<"$r")
first_other=$(i=0; for p in $started; do i=$((i + 1)); [[ " ${W3[*]} " == *" $p "* ]] || { echo $i; break; }; done)
last_w3=$(i=0; l=0; for p in $started; do i=$((i + 1)); [[ " ${W3[*]} " == *" $p "* ]] && l=$i; done; echo $l)
check "R130-c the shown Workspace w3 starts first (last w3 #$last_w3 < first other #$first_other)" test "$last_w3" -lt "${first_other:-999}"
miss=0
for w in default w1 w2 w3 w4 w5; do
    for p in $(panes_of "$w"); do [[ $(resumed "$p") -ge 1 ]] || miss=$((miss + 1)); done
done
check "R130-c every Agent resumed within 35 s (missing: $miss)" test "$miss" = 0
python3 - "$OUT/probe.json" >"$OUT/probe.txt" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
after = [l for t, l in d["probes"] if t >= d["shown"]]
print(f"{max(after):.3f} {len(after)} {sorted(after)[int(len(after) * .95)]:.3f}")
PY
read -r worst count p95 <"$OUT/probe.txt"
echo "  probe: $count calls after the first frame, p95 ${p95}s, worst ${worst}s"
check "R130-c the probe ran after the first frame" test "$count" -gt 100
perf_check "R130-c no socket call waits over 250 ms after the first frame (worst ${worst}s)" python3 -c "import sys; sys.exit(0 if $worst <= 0.25 else 1)"
stop_app

echo "== R130-d / R130-f: a send, a Workspace switch and a close while the queue runs"
: >"$FAKE_ARGV"; : >"$FAKE_PROMPTS"
start_app
# R130-f: a Pane closed while still waiting never runs.
CLOSED=${W2[3]}
rade close "$CLOSED" >/dev/null
# R130-d: a socket call starts a waiting Pane of another Workspace at once.
TARGET=${W5[7]}
check "R130-d the w5 Pane is still waiting" is "restore | jq '.pending | index(\"$TARGET\") != null'" true
rade send "$TARGET" "ping from m50" --enter >/dev/null
eventually 5 grep -q "^$TARGET ping from m50" "$FAKE_PROMPTS" || true
check "R130-d send reaches a waiting Pane of w5" grep -q "^$TARGET ping from m50" "$FAKE_PROMPTS"
# R130-d: showing a Workspace starts its visible Panes at once.
W4=($(panes_of w4))
check "R130-d w4's Panes are still waiting" is "restore | jq '.pending | index(\"${W4[1]}\") != null'" true
rade workspace switch w4 >/dev/null
rade focus "${W4[0]}" >/dev/null # the Tab holding it is shown
sleep 1
W4_SHOWN=$(rpc debug.state | jq -r '.sources[] | select(.id == "s4") | . as $s | .worktrees[] | select(.id == $s.active_worktree) | . as $w | .tabs[] | select(.id == $w.active_tab) | [.. | .pane? | objects | .id] | .[]' | grep -v "^${W4[0]}$")
check "R130-d w4 shows a Tab with Panes" test -n "$W4_SHOWN"
pend=$(restore | jq -c .pending)
left=0; for p in $W4_SHOWN; do jq -e --arg p "$p" 'index($p) == null' <<<"$pend" >/dev/null || left=$((left + 1)); done
check "R130-d the shown Tab of w4 is no longer waiting ($left left)" test "$left" = 0
shot restore-w4
eventually 40 is 'restore | jq ".pending | length"' 0 || true
sleep 1
check "R130-f the closed Pane never ran" test "$(grep -c "^$CLOSED " "$FAKE_ARGV" || true)" = 0
ran6=0; for p in "${W6[@]}"; do ran6=$((ran6 + $(grep -c "^$p " "$FAKE_ARGV" || true))); done
check "R130-f the paused Source w6 ran nothing" test "$ran6" = 0
stop_app

echo "== R130-e: quit while Panes still wait, relaunch"
before=$(jq -c '[.. | objects | select(has("session_id") and .session_id != null) | {id, session_id}] | sort_by(.id)' "$STATE")
: >"$FAKE_ARGV"
start_app
sleep 1
agents=$(rade list --all --json | jq -r '.panes[] | select(.agent != null) | .pane')
waiting=$(restore | jq -r '.pending[]' | grep -Fxf <(echo "$agents") || true)
nwait=$(wc -w <<<"$waiting")
check "R130-e many Panes still wait 1 s after start ($nwait)" test "$nwait" -ge 20
stop_app
after=$(jq -c '[.. | objects | select(has("session_id") and .session_id != null) | {id, session_id}] | sort_by(.id)' "$STATE")
check "R130-e every session is kept in state.json" test "$before" = "$after"
: >"$FAKE_ARGV"
start_app
eventually 40 is 'restore | jq ".pending | length"' 0 || true
missing() { local m=0 p; for p in $waiting; do [[ $(resumed "$p") -ge 1 ]] || m=$((m + 1)); done; echo $m; }
eventually 10 is missing 0 || true
miss=$(missing)
check "R130-e the Panes that waited resume after the relaunch (missing: $miss)" test "$miss" = 0
stop_app

echo
echo "m50: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
