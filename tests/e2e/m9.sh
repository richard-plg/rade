#!/usr/bin/env bash
# M9 end-to-end tests (ADR-0006): Fan-out, Judge, Winner (R15). `rade fanout`
# with the stand-in Agent (it appends a README line in its Worktree), the
# Fan-out Tab and bar, a stand-in Judge that prints `WINNER: fake-2`,
# choosing a Winner (squash onto main, dirty-target refusal, keep),
# discard, the palette's Fan-out… form, Session Restore mid-fan-out, and
# screenshots light + dark. Temp git repo, temp HOME, driven by the CLI.
# Usage: tests/e2e/m9.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m9
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
[agents.fakejudge]
command = "FAKE_WINNER=fake-2 FAKE_RANKING='fanout-1-2, fanout-1-1' $FAKE"
[agents.ghost]
command = "rade-no-such-agent"
[fanout]
judge = "fakejudge"
[sources.proj]
setup = "echo FANOUT-SETUP-\$((6*7))"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
R=$T/repo
mkdir -p "$R"
printf '# Repo\n' >"$R/README.md"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init
WTS=$XDG_DATA_HOME/rade-dev/worktrees/proj

exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
st() { rade fanout status "$1" --json | jq -r "$2"; }
bar() { rade debug fanout "$1" info | jq -r "$2"; }
state() { rade debug state; }

echo "== start"
start_app
rade source add "$R" --name proj >/dev/null

echo "== R15.1 checks before anything is created"
check "one Agent is refused (exit 2)" test "$(exit_code rade fanout --source proj --agents fake 'x')" = 2
check "…saying at least 2" grep -q "at least 2" "$T/err"
check "unknown Preset is refused (exit 2)" test "$(exit_code rade fanout --source proj --agents fake,nope 'x')" = 2
check "unavailable program is refused (exit 2)" test "$(exit_code rade fanout --source proj --agents fake,ghost 'x')" = 2
check "…naming it" grep -q "not available" "$T/err"
check "9 Agents exceed limits.max_children (exit 5)" \
    test "$(exit_code rade fanout --source proj --agents fake,fake,fake,fake,fake,fake,fake,fake,fake 'x')" = 5
check "General is refused (exit 2)" test "$(exit_code rade fanout --source general --agents fake,fake 'x')" = 2
check "a bad base is refused (exit 2)" test "$(exit_code rade fanout --source proj --base nope --agents fake,fake 'x')" = 2
check "nothing was created" test "$(git -C "$R" worktree list | wc -l)" = 1

echo "== R15 AC: rade fanout --agents fake,fake \"add a README line\""
J=$(rade fanout --source proj --agents fake,fake "add a README line" --json)
echo "$J" >"$OUT/start.json"
check "id fanout-1" test "$(jq -r .fanout <<<"$J")" = fanout-1
check "Worktrees fanout-1-1, fanout-1-2" test "$(jq -r '.worktrees | join(",")' <<<"$J")" = "fanout-1-1,fanout-1-2"
check "labels fake-1, fake-2" test "$(jq -r '.labels | join(",")' <<<"$J")" = "fake-1,fake-2"
P1=$(jq -r '.panes[0]' <<<"$J") P2=$(jq -r '.panes[1]' <<<"$J") TAB=$(jq -r .tab <<<"$J")
check "2 git worktrees on branches fanout-1-1/-2" \
    test "$(git -C "$R" worktree list --porcelain | grep -c '^branch refs/heads/fanout-1-')" = 2
check "in RADE's data folder (ADR-0007)" test -d "$WTS/fanout-1-2"
check "1 Tab, in the main Worktree, named fanout-1" \
    test "$(state | jq -r --arg t "$TAB" '[.sources[0].worktrees[0].tabs[] | select(.id==$t) | .name] | join(",")')" = fanout-1
check "…holding the 2 Panes side by side" \
    test "$(state | jq -r --arg t "$TAB" '.sources[0].worktrees[0].tabs[] | select(.id==$t) | .layout | "\(.type) \(.dir) \(.a.pane.id) \(.b.pane.id)"')" = "split right $P1 $P2"
check "each Pane runs in its Worktree (cwd)" test "$(rade list --json | jq -r --arg p "$P2" '.panes[] | select(.pane==$p) | .cwd')" = "$WTS/fanout-1-2"
check "both Agents received the prompt" eventually 10 bash -c "rade read $P1 | grep -q 'thinking about: add a README line' && rade read $P2 | grep -q 'thinking about: add a README line'"
check "the Source's setup ran first in each (R12.2)" bash -c "rade read $P1 --scrollback | grep -q FANOUT-SETUP-42"

echo "== progress, wait, comparison"
check "rade fanout wait returns when both are done" bash -c "rade fanout wait fanout-1 --timeout 30 >/dev/null"
check "phase finished, 2/2" test "$(st fanout-1 '"\(.phase) \(.done)/\(.total)"')" = "finished 2/2"
check "each changed 1 file, +1 −0" test "$(st fanout-1 '[.members[] | "\(.files)+\(.added)-\(.removed)"] | join(" ")')" = "1+1-0 1+1-0"
check "each Worktree has its own README line" grep -q "by $P2" "$WTS/fanout-1-2/README.md"
check "main is untouched" test "$(cat "$R/README.md")" = "# Repo"
rade focus "$P1" >/dev/null
sleep 1
check "bar: 2/2 done" eventually 5 is "bar fanout-1 .progress" "2/2 done"
check "bar: chips with sizes" eventually 8 is "bar fanout-1 '.members[1].stats'" "1 file +1 −0"
check "bar: Judge ready, Choose Winner enabled" test "$(bar fanout-1 '"\(.buttons.judge.sensitive) \(.buttons.choose.sensitive)"')" = "true true"
check "sidebar: the Fan-out Worktrees are listed" test "$(rade worktree list proj --json | jq -r '[.worktrees[].worktree] | join(",")')" = "main,fanout-1-1,fanout-1-2"
shot fanout-light

echo "== R15.3 Diff = that Worktree's Review Pane"
rade debug fanout fanout-1 diff fake-2 >/dev/null
RV=$(rade list --json | jq -r --arg w "$WTS/fanout-1-2" '.panes[] | select(.cwd==$w and .name==null) | .pane' | head -1)
check "a Review Pane of fanout-1-2 opened" test -n "$RV"
check "…listing README.md +1" eventually 5 bash -c "rade debug review $RV info | jq -e '.files[0].path==\"README.md\" and .files[0].added==1' >/dev/null"
shot fanout-diff-light
rade close "$RV" >/dev/null

echo "== R15.3 Judge"
JU=$(rade fanout judge fanout-1)
check "a Judge Pane started" test -n "$JU"
check "…in the main Worktree (cwd = repo)" test "$(rade list --json | jq -r --arg p "$JU" '.panes[] | select(.pane==$p) | .cwd')" = "$R"
check "…below the Agents in the Fan-out Tab" \
    test "$(state | jq -r --arg t "$TAB" '.sources[0].worktrees[0].tabs[] | select(.id==$t) | .layout | "\(.dir) \(.b.pane.id)"')" = "down $JU"
check "rade fanout wait --until judged" bash -c "rade fanout wait fanout-1 --until judged --timeout 30 >/dev/null"
check "pick fake-2 parsed from its answer" test "$(st fanout-1 .judge.pick)" = fake-2
check "ranking fake-2, fake-1" test "$(st fanout-1 '.judge.ranking | join(",")')" = "fake-2,fake-1"
check "phase judged" test "$(st fanout-1 .phase)" = judged
check "the Judge saw worktree names, not Agent names" bash -c "D=\$(rade fanout status fanout-1 --json | jq -r .judge.dir); grep -q 'fanout-1-2' \$D/prompt.md && ! grep -q 'fake-' \$D/prompt.md && test -s \$D/fanout-1-1.diff"
check "bar: Judge picks fake-2, chip marked" eventually 5 is "bar fanout-1 '\"\(.verdict) \(.members[1].picked)\"'" "Judge picks fake-2 true"
shot fanout-judged-light
rade debug fanout fanout-1 click judge >/dev/null
check "Judge Again (bar button) replaces the Judge" eventually 10 bash -c "test \"\$(rade fanout status fanout-1 --json | jq -r .judge.pane)\" != $JU"
check "…and gives its verdict again" bash -c "rade fanout wait fanout-1 --until judged --timeout 30 >/dev/null && test \"\$(rade fanout status fanout-1 --json | jq -r .judge.pick)\" = fake-2"
check "the old Judge Pane is closed" bash -c "! rade list --json | jq -e --arg p $JU '.panes[] | select(.pane==\$p)' >/dev/null"

echo "== Session Restore mid-fan-out (and dark look)"
rade fanout --source proj --name spin --agents fake,fake "spin" >/dev/null
check "a second, still running Fan-out" eventually 5 is "st spin .phase" running
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not restart"; exit 1; }
sleep 1
check "fanout-1 restored: judged, pick fake-2, 2/2" test "$(st fanout-1 '"\(.phase) \(.judge.pick) \(.done)/\(.total)"')" = "judged fake-2 2/2"
check "spin restored: its mid-turn Agents are interrupted, not done (M11)" test "$(st spin '"\(.phase) \(.done)/\(.total) \(.interrupted)"')" = "running 0/2 2"
check "wait --until judged without a Judge is exit 2" test "$(exit_code rade fanout wait spin --until judged --timeout 5)" = 2
check "its Tab and Panes are back" test "$(state | jq -r '[.sources[0].worktrees[0].tabs[] | select(.name=="spin") | .layout.a.pane.name] | join(",")')" = fake-1
check "its Agents run in their Worktrees again" test "$(rade list --json | jq -r '.panes[] | select(.name=="fake-2" and (.cwd | endswith("/spin-2"))) | .cwd')" = "$WTS/spin-2"
rade focus "$P1" >/dev/null
sleep 1.5
shot fanout-judged-dark
rade debug fanout fanout-1 diff fake-1 >/dev/null
sleep 1
shot fanout-diff-dark
rade focus "$P1" >/dev/null

echo "== discard"
D=$(rade fanout discard spin --json)
check "discard removed spin-1 and spin-2" test "$(jq -r '.removed | join(",")' <<<"$D")" = "spin-1,spin-2"
check "their folders and branches are gone" bash -c "! test -e $WTS/spin-1 && ! git -C $R rev-parse --verify -q refs/heads/spin-2"
check "its Tab closed and it is no longer listed" bash -c "test \"\$(rade debug state | jq '[.sources[0].worktrees[0].tabs[] | select(.name==\"spin\")] | length')\" = 0 && ! rade fanout status spin 2>/dev/null"

echo "== R15.4 Choose Winner (squash; never a commit)"
printf 'dirty\n' >>"$R/README.md"
check "a dirty target is refused (exit 2)" test "$(exit_code rade fanout choose fanout-1)" = 2
check "…saying so" grep -q "uncommitted changes" "$T/err"
G -C "$R" checkout -q -- README.md
G -C "$R" checkout -q --detach
check "a detached HEAD is refused (exit 2)" test "$(exit_code rade fanout choose fanout-1)" = 2
G -C "$R" checkout -q main
check "an unknown Winner is exit 4" test "$(exit_code rade fanout choose fanout-1 fake-9)" = 4
HEAD0=$(git -C "$R" rev-parse HEAD)
C=$(rade fanout choose fanout-1 --json)
echo "$C" >"$OUT/choose.json"
check "default Winner = the Judge's pick (fake-2)" test "$(jq -r .winner <<<"$C")" = fake-2
check "staged on main, no conflicts" test "$(jq -r '"\(.target) \(.conflicts | length) \(.staged)"' <<<"$C")" = "main 0 1"
check "target branch has the Winner's change (staged)" bash -c "git -C $R diff --cached | grep -q '+add a README line by $P2'"
check "…and not the other Agent's" bash -c "! git -C $R diff --cached | grep -q 'by $P1'"
check "no commit was made (HEAD unchanged)" test "$(git -C "$R" rev-parse HEAD)" = "$HEAD0"
check "the next step is printed (git commit)" grep -q "git -C" <<<"$(jq -r .next <<<"$C")"
check "phase chosen" test "$(st fanout-1 .phase)" = chosen
check "bar: Winner fake-2, Clean Up…" eventually 5 is "bar fanout-1 '\"\(.verdict) \(.buttons.choose.visible) \(.buttons.discard.label)\"'" "Winner fake-2 false Clean Up…"
check "a second choose is refused" test "$(exit_code rade fanout choose fanout-1 fake-1)" = 2
shot fanout-chosen-dark
D=$(rade fanout discard fanout-1 --json)
check "clean up removes both Worktrees and branches" bash -c "test '$(jq -r '.removed | join(",")' <<<"$D")' = fanout-1-1,fanout-1-2 && ! git -C $R rev-parse -q --verify refs/heads/fanout-1-2"
check "the staged change stays" bash -c "git -C $R diff --cached | grep -q 'by $P2'"
G -C "$R" commit -qm "fanout-1 winner"

echo "== R15.4 keep + cleanup in one step"
K=$(rade fanout --source proj --name keepme --agents fake,fake "add another line" --json)
KP2=$(jq -r '.panes[1]' <<<"$K")
rade fanout wait keepme --timeout 30 >/dev/null
KC=$(rade fanout choose keepme keepme-2 --strategy keep --cleanup --json)
check "keep: the Winner's Worktree stays" test "$(jq -r '.cleaned.kept' <<<"$KC")" = keepme-2
check "…the other one is removed" test "$(jq -r '.cleaned.removed | join(",")' <<<"$KC")" = keepme-1
check "…its branch keeps the work (uncommitted in its Worktree)" grep -q "add another line by $KP2" "$WTS/keepme-2/README.md"
check "its Agent moved to a Tab of its own Worktree, still running" \
    test "$(state | jq -r --arg p "$KP2" '.sources[0].worktrees[] | select(.name=="keepme-2") | [.tabs[].layout | .. | objects | select(.id?==$p)] | length')" = 1
check "…and answers" bash -c "rade send $KP2 'hello' --enter && sleep 1.5 && rade read $KP2 | grep -q 'REPLY: HELLO'"
check "the Fan-out is gone" test "$(rade fanout list --json | jq '.fanouts | length')" = 0
rade worktree remove proj keepme-2 --force --delete-branch >/dev/null

echo "== R14.2 / R15.1 palette Fan-out… form"
rade focus "$P1" 2>/dev/null >/dev/null || rade source list >/dev/null
rade debug action command_palette >/dev/null
rade debug palette --ui type ">fan-out" >/dev/null
F=$(rade debug palette --ui key enter)
check "Fan-out… opens a form: prompt, agents (counts), base, name" \
    test "$(jq -r '[.form.fields[] | "\(.key):\(.kind)"] | join(",")' <<<"$F")" = "prompt:text,agents:counts,base:text,name:text"
check "an empty prompt can't be submitted" test "$(jq -r .form.can_submit <<<"$F")" = false
rade debug palette --ui field "prompt=add a palette line" >/dev/null
F=$(rade debug palette --ui field "agents=fake=2")
check "fake × 2 set through the spin buttons" test "$(jq -r '.form.fields[1].value' <<<"$F")" = "fake=2"
shot fanout-form-dark
rade debug palette --ui key enter >/dev/null
check "Enter starts it (fanout-1 again: the name is free)" eventually 10 is "rade fanout list --json | jq -r '.fanouts[0].fanout'" fanout-1
check "…focused on its first Agent" eventually 5 bash -c "rade debug state | jq -e '.sources[0].worktrees[0] as \$w | \$w.tabs[] | select(.id==\$w.active_tab) | .name==\"fanout-1\"' >/dev/null"
rade fanout wait fanout-1 --timeout 30 >/dev/null
check "bar sizes refresh as soon as the Agents finish" eventually 3 is "bar fanout-1 '.members[0].stats'" "1 file +1 −0"
shot fanout-palette-dark
rade fanout discard fanout-1 >/dev/null
check "no Fan-out Worktree left" test "$(git -C "$R" worktree list | wc -l)" = 1
stop_app

echo "== M9: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
