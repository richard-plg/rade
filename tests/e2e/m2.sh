#!/usr/bin/env bash
# M2 end-to-end tests (ADR-0006): Agent Presets, spawn placement + Agent Grid
# + Title Bars, limits, Agent Status + wait, scrollback budget, hooks,
# `rade setup`, and close/Session Restore (post-review fixes B1, B2) — all in a temp HOME, driven only through the `rade` CLI.
# A deterministic fake Agent (fake-agent.sh) stands in for real ones.
# Usage: tests/e2e/m2.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m2
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade" "$XDG_CONFIG_HOME/ghostty"
# The owner's real Ghostty setting: 100000 bytes keeps only ~850 lines (M0 row 6).
echo 'scrollback-limit = 100000' > "$XDG_CONFIG_HOME/ghostty/config"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
config_env = ["FAKE_CONFIG_DIR"]
value_flags = ["--model"]
[agents.typed]
command = "$FAKE"
prompt = "type"
[agents.lead]
command = "bash"
EOF

state() { rade debug state; }
pane_of() { rade list --json | jq -r --arg n "$1" '[.panes[] | select(.name==$n)][0].pane'; }
info() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
tab_json() { state | jq --arg p "$1" '[.. | objects | select(has("layout")) | select([.. | objects | select(.id?==$p)] | length > 0)][0]'; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
as_pane() { local p=$1; shift; RADE_PANE_ID=$p RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock "$@"; }

echo "== start"
start_app
shell=$(rade list --json | jq -r '.panes[0].pane')

echo "== R2.3 root spawn opens a new Tab"
lead=$(rade spawn --agent lead --name lead --json | jq -r .pane)
check "root spawn: new Tab" test "$(info "$lead" tab)" != "$(info "$shell" tab)"
check "lead marked as Agent 'lead'" test "$(info "$lead" agent)" = lead
eventually 10 screen_has "$lead" '$' || true

echo "== R2-a: from an Agent Pane, spawn --agent fake --name rev \"say hi\""
rade send "$lead" 'rade spawn --agent fake --name rev "say hi" && rade wait rev --until done && rade read rev | grep -q "REPLY: SAY HI" && echo R2A-$((40+2))' --enter
check "R2-a inside lead: spawn + wait done + read reply" eventually 20 screen_has "$lead" R2A-42
rev=$(pane_of rev)
check "rev in lead's Tab" test "$(info "$rev" tab)" = "$(info "$lead" tab)"
check "rev parent = lead" test "$(info "$rev" parent)" = "$lead"
check "Agent Grid layout R(lead,rev), grid parent lead" test "$(tab_json "$rev" | jq -r '[.layout.type, .layout.dir, .layout.a.pane.id, .layout.b.pane.id, .grid] | join(",")')" = "split,right,$lead,$rev,$lead"
check "wait --until done (outside) prints done" test "$(rade wait rev --until done --timeout 10)" = done
check "read rev contains the reply" screen_has "$rev" "REPLY: SAY HI"
sid=$(state | jq -r --arg p "$rev" '.. | objects | select(.id?==$p) | .session_id')
check "R16.4 pre-assigned --session-id reached the Agent ($sid)" screen_has "$rev" "fake-agent session $sid"
check "session id is a v4 UUID" bash -c "[[ $sid =~ ^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$ ]]"

echo "== send follow-up → running → done"
rade send rev "second task" --enter
check "status running right after send --enter" test "$(info "$rev" status)" = running
check "wait done after follow-up" test "$(rade wait rev --until done --timeout 10)" = done
check "follow-up reply" screen_has "$rev" "REPLY: SECOND TASK"

echo "== R10: waiting raises a Notification; regex, idle, timeout"
n0=$(info "$rev" unread) # each earlier `done` raised "rev finished" (R3.1)
rade send rev "ask something" --enter
check "wait --until waiting" test "$(rade wait rev --until waiting --timeout 10)" = waiting
check "waiting → 1 more unread Notification" test "$(info "$rev" unread)" = $((n0 + 1))
got=$(rade wait rev --until '/QUESTION: [a-zA-Z ]+/' --timeout 5 | cut -f2); echo "  regex got: [$got]"
check "wait /regex/ prints the match" test "$got" = "QUESTION: which file should I use"
check "wait --until idle (idle_seconds = 1)" test "$(rade wait rev --until idle --timeout 10 | cut -f2)" = idle
check "wait timeout exits 6" test "$(exit_code rade wait rev --until exited --timeout 1)" = 6
check "timeout message names the condition" grep -q "until exited" "$T/err"
rade focus "$rev"
check "focus reads the Notification" test "$(info "$rev" unread)" = 0

echo "== R16.1 prompt mode 'type' (typed after start)"
as_pane "$lead" rade spawn --agent typed --name ty "typed hello" >/dev/null
check "typed prompt answered" test "$(rade wait ty --until done --timeout 20)" = done
check "typed reply" screen_has "$(pane_of ty)" "REPLY: TYPED HELLO"

echo "== R2.4 Agent Grid: up to 8 Subagents tiled (Title Bars only past a raised limit; unit-tested)"
for n in c3 c4 c5 c6; do as_pane "$lead" rade spawn --agent fake --name $n >/dev/null; done
check "6 Subagents of lead" test "$(rade list --json | jq --arg p "$lead" '[.panes[] | select(.parent==$p)] | length')" = 6
check "layout R(lead, R(D,D)) = 2 columns of 3" test "$(tab_json "$lead" | jq -r '[.layout.b.dir, .layout.b.a.dir, .layout.b.b.dir] | join(",")')" = "right,down,down"
check "no Title Bars with 6" test "$(tab_json "$lead" | jq -r '.title_bars | length')" = 0
c6=$(pane_of c6)
eventually 10 screen_has "$c6" 'fake-agent session' || true
rade focus "$lead"
sleep 1
shot grid-6-children
rade debug action zoom >/dev/null
check "tree lists Subagents under lead" bash -c "rade list --tree | grep -q \"^  $rev rev \\[fake\""

echo "== R2-b / R2.5 limits"
for n in c7 c8; do as_pane "$lead" rade spawn --agent fake --name $n >/dev/null; done
check "R2-b 9th live Subagent exits 5" test "$(exit_code as_pane "$lead" rade spawn --agent fake --name c9)" = 5
check "R2-b message names the limit" grep -q "limits.max_children = 8" "$T/err"
c3=$(pane_of c3)
rade send "$c3" quit --enter
check "process exit → status exited" test "$(rade wait c3 --until exited --timeout 10)" = exited
check "exited Pane stays readable" eventually 5 screen_has "$c3" "Process exited"
check "exited child frees a slot: 9th spawn now OK" test "$(exit_code as_pane "$lead" rade spawn --agent fake --name c9)" = 0
d2=$(as_pane "$rev" rade spawn --cmd bash --name d2)
d3=$(as_pane "$d2" rade spawn --cmd bash --name d3)
check "depth 3 reached" test "$(state | jq --arg p "$d3" '.. | objects | select(.id?==$p) | .depth')" = 3
check "depth 4 exits 5" test "$(exit_code as_pane "$d3" rade spawn --cmd bash)" = 5
check "depth message names the limit" grep -q "limits.max_depth = 3" "$T/err"

echo "== addressing: AMBIGUOUS names"
X=/tmp/rade-e2e-m2-$$
mkdir -p "$X"
rade source add "$X" --name x >/dev/null
rade spawn --source x --name rev --cmd bash >/dev/null
check "global 'rev' is AMBIGUOUS (exit 4)" test "$(exit_code rade read rev)" = 4
check "AMBIGUOUS lists both ids" grep -q "$rev" "$T/err"
check "from lead, 'rev' resolves in its Worktree" test "$(exit_code as_pane "$lead" rade read rev)" = 0

echo "== R2.7 scrollback budget for Agent Panes (user scrollback-limit = 100000)"
rade send "$lead" 'clear; seq 1 20000' --enter
eventually 15 bash -c "rade read $lead | grep -qx 20000"
kept=$(rade read "$lead" --scrollback | grep -cE '^[0-9]+$' || true)
last=$(rade read "$lead" --scrollback | grep -E '^[0-9]+$' | tail -1)
echo "  Agent Pane kept $kept numeric lines, last $last"
check "Agent Pane read --scrollback >= 10000 lines" test "$kept" -ge 10000
check "... ending at 20000" test "$last" = 20000
rade send "$shell" 'clear; seq 1 20000' --enter
eventually 15 bash -c "rade read $shell | grep -qx 20000"
plain=$(rade read "$shell" --scrollback | grep -cE '^[0-9]+$' || true)
echo "  plain shell kept $plain lines (user limit applies)"
check "plain shell keeps the user's smaller limit" test "$plain" -lt 10000

echo "== status / notify / hooks"
rade status done --pane ty --session-id s-x --agent codex
check "status sets session id + agent" test "$(state | jq -r --arg p "$(pane_of ty)" '.. | objects | select(.id?==$p) | [.session_id, .agent] | join(",")')" = "s-x,codex"
n0=$(info "$(pane_of ty)" unread) # "ty finished" from the done above
rade notify "build broke" --pane ty --status error
check "notify stores + sets status (R3.1: one entry with 'ty failed')" test "$(info "$(pane_of ty)" status),$(info "$(pane_of ty)" unread)" = "error,$((n0 + 1))"
check "R3.1 coalesced text 'ty failed — build broke'" test "$(state | jq -r '.notifications[-1].text')" = "ty failed — build broke"
check "bad status exits 2" test "$(exit_code rade status busy --pane ty)" = 2
out=$(env -u RADE_PANE_ID rade status done --hook; echo "rc=$?")
check "hook outside RADE: silent, exit 0" test "$out" = "rc=0"
out=$(echo '{}' | RADE_PANE_ID=p_1 RADE_SOCKET=$T/nope.sock rade status done --hook; echo "rc=$?")
check "hook with RADE not running: silent, exit 0" test "$out" = "rc=0"
check "USAGE: --worktree new in General (no git) exits 2" test "$(exit_code rade spawn --source general --worktree new)" = 2
check "unknown Preset exits 2" test "$(exit_code rade spawn --agent nope)" = 2

echo "== R16.3 rade setup (temp HOME only)"
ALL=(--agent claude --agent codex --agent agy --agent opencode)
mkdir -p "$HOME/.claude"
echo '{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"my-stop.sh"}]}]}}' > "$HOME/.claude/settings.json"
rade setup "${ALL[@]}" --dry-run > "$OUT/setup-dry-run.txt"
check "dry run shows a diff" grep -q '^+.*rade status done --hook' "$OUT/setup-dry-run.txt"
check "dry run writes nothing" bash -c "! test -e $HOME/.codex/config.toml && grep -qv 'rade status' $HOME/.claude/settings.json"
rade setup "${ALL[@]}" > "$OUT/setup.txt"
check "setup prints the Codex /hooks trust notice" grep -q "/hooks" "$OUT/setup.txt"
check "backup of the user's settings.json" bash -c "ls $HOME/.claude/settings.json.rade-bak-* >/dev/null"
check "user Stop hook kept" test "$(jq -r '.hooks.Stop[0].hooks[0].command' "$HOME/.claude/settings.json")" = my-stop.sh
check "every file installed" bash -c "test -s $HOME/.codex/config.toml && test -s $HOME/.gemini/config/hooks.json && test -s $XDG_CONFIG_HOME/opencode/plugins/rade.js && test -s $HOME/.claude/skills/rade/SKILL.md"
check "second run is a no-op" grep -q "already installed" <(rade setup "${ALL[@]}")
# Run the installed hook commands exactly as the Agents would.
stop=$(jq -r '.hooks.Stop[1].hooks[0].command' "$HOME/.claude/settings.json")
tyid=$(pane_of ty)
echo '{"session_id":"claude-sid"}' | RADE_PANE_ID=$tyid RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock sh -c "$stop"
check "installed Claude Stop hook → done + session id" test "$(state | jq -r --arg p "$tyid" '.. | objects | select(.id?==$p) | [.status, .session_id] | join(",")')" = "done,claude-sid"
out=$(echo '{"session_id":"x"}' | env -u RADE_PANE_ID sh -c "$stop"; echo "rc=$?")
check "installed hook outside RADE: silent, exit 0" test "$out" = "rc=0"
pre=$(jq -r '.rade.PreInvocation[0].command' "$HOME/.gemini/config/hooks.json")
out=$(echo '{"conversationId":"agy-c1"}' | RADE_PANE_ID=$tyid RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock sh -c "$pre")
check "agy PreInvocation hook prints {} and reports running + conversationId" test "$out,$(state | jq -r --arg p "$tyid" '.. | objects | select(.id?==$p) | [.status, .session_id, .agent] | join(",")')" = "{},running,agy-c1,agy"
codex_stop=$(python3 -c "import tomllib,sys; d=tomllib.load(open('$HOME/.codex/config.toml','rb')); print(d['hooks']['Stop'][0]['hooks'][0]['command'])")
echo '{"session_id":"codex-sid"}' | RADE_PANE_ID=$tyid RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock sh -c "$codex_stop"
check "installed Codex Stop hook → done" test "$(info "$tyid" status)" = done
# Codex appends the JSON payload as the last argv element.
RADE_PANE_ID=$tyid RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock python3 - "$HOME/.codex/config.toml" <<'PY'
import subprocess, sys, tomllib
argv = tomllib.load(open(sys.argv[1], "rb"))["notify"]
subprocess.run(argv + ['{"type":"agent-turn-complete","thread-id":"th-1"}'], check=True)
PY
check "Codex notify fallback → session id from thread-id" test "$(state | jq -r --arg p "$tyid" '.. | objects | select(.id?==$p) | .session_id')" = th-1

echo "== close: a Parent's Subagents stay"
n_children=$(rade list --json | jq --arg p "$lead" '[.panes[] | select(.parent==$p)] | length')
rade close "$lead"
check "lead closed" test "$(exit_code rade read "$lead")" = 4
check "its $n_children Subagents remain" test "$(rade list --json | jq --arg p "$lead" '[.panes[] | select(.parent==$p)] | length')" = "$n_children"

echo "== B1 close: out of the model at once, SIGHUP without blocking; --force = SIGKILL"
# Ignores SIGHUP for 5 s, like an Agent running its SessionEnd hook.
SLOW="bash -c 'trap \"sleep 5; exit\" HUP; echo SLOW-UP; while :; do sleep 0.2; done'"
has_procs() { grep -qsaz "^RADE_PANE_ID=$1\$" /proc/[0-9]*/environ; }
g1=$(rade spawn --cmd "$SLOW" --name g1); g2=$(rade spawn --cmd "$SLOW" --name g2); g3=$(rade spawn --cmd "$SLOW" --name g3)
for g in "$g1" "$g2" "$g3"; do eventually 10 screen_has "$g" SLOW-UP || true; done
t0=$(date +%s%N)
rade close --force g3
ms=$((($(date +%s%N) - t0) / 1000000))
check "close --force replies at once (${ms} ms)" test "$ms" -lt 1000
check "close --force: process group gone (SIGKILL)" eventually 2 bash -c "! grep -qsaz '^RADE_PANE_ID=$g3\$' /proc/[0-9]*/environ"

echo "== R11.2 Session Restore fixtures"
rade close ty # its Preset was switched to agy by the hook checks; no real Agents here
rs=$(rade spawn --agent fake --name rs "restore me")
rade wait rs --until done --timeout 10 >/dev/null
rsc=$(as_pane "$rs" rade spawn --agent fake --name rsc "child task")
rade wait rsc --until done --timeout 10 >/dev/null
sid_of() { state | jq -r --arg p "$1" '.. | objects | select(.id?==$p) | .session_id'; }
rs_sid=$(sid_of "$rs") rsc_sid=$(sid_of "$rsc")
# Started by hand like `clw` (= CLAUDE_CONFIG_DIR=… claude --dangerously-skip-permissions).
al=$(rade spawn --name al)
eventually 10 screen_has "$al" '$' || true
rade send "$al" "FAKE_CONFIG_DIR=$T/x $FAKE --flag-x --model m1 'alias prompt'" --enter
check "hand-started Agent reports done" test "$(rade wait al --until done --timeout 10)" = done
al_sid=$(sid_of "$al")
check "R11.2 invocation recorded (argv + config env)" test "$(state | jq -c --arg p "$al" '.. | objects | select(.id?==$p) | .invocation')" = "{\"argv\":[\"$FAKE\",\"--flag-x\",\"--model\",\"m1\",\"alias prompt\"],\"env\":{\"FAKE_CONFIG_DIR\":\"$T/x\"}}"
cm=$(rade spawn --cmd "echo CMD-RAN; sleep 1000" --name cm)
eventually 10 screen_has "$cm" CMD-RAN || true

echo "== B1 close, close, quit in quick succession"
t0=$(date +%s%N)
rade close g1; rade close g2
ms=$((($(date +%s%N) - t0) / 1000000))
alive=0; has_procs "$g1" || alive=1
rade debug quit >/dev/null
check "plain close = SIGHUP: the process still gets its grace" test "$alive" = 0
check "two closes of SIGHUP-ignoring Panes reply at once (${ms} ms)" test "$ms" -lt 1000
wait "$APP_PID" 2>/dev/null || true
APP_PID=
check "state.json has neither closed Pane" test "$(jq --arg a "$g1" --arg b "$g2" '[.. | objects | select(.id? == $a or .id? == $b)] | length' "$STATE")" = 0
start_app
check "closed Panes stay gone after restart" test "$(exit_code rade read "$g1"),$(exit_code rade read "$g2")" = 4,4

echo "== B2 / R11.2 restored Agents resume, never replay the prompt"
check "rs resumed its session ($rs_sid)" eventually 10 screen_has "$rs" "resumed $rs_sid"
check "rs command = recorded argv minus prompt/--session-id + Preset resume args" test "$(state | jq -r --arg p "$rs" '.. | objects | select(.id?==$p) | .kind.command')" = "'$FAKE' --resume '$rs_sid'"
check "rs did not re-process its prompt" bash -c "! rade read $rs --scrollback | grep -qiE 'thinking about: restore me|REPLY: RESTORE ME'"
check "rsc resumed its session" eventually 10 screen_has "$rsc" "resumed $rsc_sid"
check "rsc did not re-process its prompt" bash -c "! rade read $rsc --scrollback | grep -qi 'child task'"
check "rsc keeps Parent rs" test "$(info "$rsc" parent)" = "$rs"
check "rsc still in rs's Agent Grid" test "$(tab_json "$rsc" | jq -r .grid)" = "$rs"
check "resumed Agent answers a new prompt" bash -c "rade send $rs 'after restore' --enter && rade wait rs --until done --timeout 10 >/dev/null && rade read $rs | grep -q 'REPLY: AFTER RESTORE'"
check "R11.2 hand-started Agent resumed its session ($al_sid)" eventually 10 screen_has "$al" "resumed $al_sid"
check "... with its config env and flags" screen_has "$al" "fake-agent config=$T/x flags=--flag-x --model m1"
check "... command = env prefix + kept args + resume" test "$(state | jq -r --arg p "$al" '.. | objects | select(.id?==$p) | .kind.command')" = "FAKE_CONFIG_DIR='$T/x' '$FAKE' '--flag-x' '--model' 'm1' --resume '$al_sid'"
check "... and did not replay its prompt" bash -c "! rade read $al --scrollback | grep -qiE 'thinking about: alias prompt|REPLY: ALIAS PROMPT'"
check "--cmd Pane comes back as a shell (command not re-run)" test "$(state | jq -r --arg p "$cm" '.. | objects | select(.id?==$p) | .kind.command')" = null
rade send "$cm" 'echo SH-$((6*7))' --enter
check "... a working shell in its cwd" eventually 10 screen_has "$cm" SH-42
check "... that did not print CMD-RAN again" bash -c "! rade read $cm --scrollback | grep -qx CMD-RAN"
rade focus "$rs"
sleep 0.5
shot restored

stop_app
rm -rf "$X"
echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
