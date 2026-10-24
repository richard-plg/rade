#!/usr/bin/env bash
# M7 end-to-end tests (ADR-0006): Worktrees (R12: add/list/import, setup
# command, spawn --worktree new, remove with dirty refusal / branch delete /
# lock, prune, Session Restore) and Diff review (R13: Review Pane contents,
# hunks, Review Comments from the UI path and the CLI, Send to Agent → one
# prompt). Temp git repo, temp HOME, driven only through the `rade` CLI.
# Usage: tests/e2e/m7.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m7
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
RECV=$T/received.txt
: >"$RECV"
# A stand-in Agent that records every byte it is sent (the prompt).
printf '#!/bin/bash\necho SINK READY\nexec cat >>%q\n' "$RECV" >"$T/sink.sh"
chmod +x "$T/sink.sh"
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
[agents.typed]
command = "$FAKE"
prompt = "type"
[agents.sink]
command = "$T/sink.sh"
[sources.proj]
setup = "echo SETUP-RAN-\$((6*7)); touch .setup-done"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
R=$T/repo
mkdir -p "$R/src"
printf 'fn main() {\n    let x = 1;\n    println!("{}", x);\n}\n' >"$R/src/a.rs"
printf 'def f():\n    return 1\n\n\ndef g():\n    return 2\n' >"$R/src/b.py"
printf '# Repo\n' >"$R/README.md"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init

state() { rade debug state; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
info() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
wtl() { rade worktree list proj --json | jq -r --arg w "$1" ".worktrees[] | select(.worktree==\$w) | .$2"; }
WTS=$XDG_DATA_HOME/rade-dev/worktrees/proj

echo "== start"
start_app
rade source add "$R" --name proj >/dev/null

echo "== R12.1 worktree add"
out=$(rade worktree add proj feat-x)
check "prints the path under \$XDG_DATA_HOME/rade-dev/worktrees/proj/feat-x (ADR-0007)" test "$out" = "$WTS/feat-x"
check "git checkout exists on branch feat-x" test "$(git -C "$WTS/feat-x" branch --show-current)" = feat-x
check "worktree list: main + feat-x" test "$(rade worktree list proj --json | jq -r '[.worktrees[].worktree] | join(",")')" = "main,feat-x"
check "R12 AC: branch feat-x reported" test "$(wtl feat-x branch)" = feat-x
check "setup pending in state.json until a Terminal opens" test "$(state | jq -r '.sources[0].worktrees[1].setup')" = 'echo SETUP-RAN-$((6*7)); touch .setup-done'
check "invalid name 'a b' exits 2" test "$(exit_code rade worktree add proj 'a b')" = 2
check "…and says why" grep -q "invalid worktree name" "$T/err"
check "'new' is reserved (exit 2)" test "$(exit_code rade worktree add proj new)" = 2
check "duplicate name exits 2" test "$(exit_code rade worktree add proj feat-x)" = 2
check "General exits 2" test "$(exit_code rade worktree add general x)" = 2
check "unknown source exits 4" test "$(exit_code rade worktree add nope x)" = 4
G -C "$R" branch old-br
check "existing branch + --base exits 2" test "$(exit_code rade worktree add proj old-br --base main)" = 2
check "existing branch is checked out, not created" test "$(rade worktree add proj old-br --json | jq -r .created_branch)" = false
check "--base: a new branch from another branch (exit 0)" test "$(exit_code rade worktree add proj again-x --base feat-x)" = 0
check "a new branch from --base" test "$(git -C "$WTS/again-x" rev-parse HEAD)" = "$(git -C "$R" rev-parse feat-x)"

echo "== R12 worktrees created outside RADE are imported"
G -C "$R" worktree add -q -b ext-br "$T/ext wt"
check "git worktree add outside → listed as ext-br" test "$(wtl ext-br path)" = "$T/ext wt"

echo "== R12.2 setup runs in the first Terminal, R12 AC pwd"
wsh=$(rade spawn --source proj --worktree feat-x --name wsh --json | jq -r .pane)
check "setup output visible in the first Terminal" eventually 10 screen_has "$wsh" SETUP-RAN-42
check "setup ran in the Worktree" test -f "$WTS/feat-x/.setup-done"
eventually 5 screen_has "$wsh" '$' || true
rade send "$wsh" 'pwd' --enter
check "R12 AC: pwd = the worktree path" eventually 5 screen_has "$wsh" "$WTS/feat-x"
check "RADE_WORKTREE=feat-x in its env" test "$(info "$wsh" worktree)" = feat-x
check "setup consumed (not in state.json)" test "$(state | jq -r '.sources[0].worktrees[1].setup // "none"')" = none
w2=$(rade spawn --source proj --worktree feat-x --name w2 --json | jq -r .pane)
eventually 5 screen_has "$w2" '$' || true
sleep 0.5
check "second Terminal does not run setup again" bash -c "! rade read $w2 --scrollback | grep -q SETUP-RAN"
rade close "$w2" >/dev/null

echo "== R2.3 spawn --worktree new"
sp=$(rade spawn --source proj --worktree new --agent fake "hello" --json)
fk=$(jq -r .pane <<<"$sp")
check "auto-named wt-fake-1" test "$(jq -r .worktree <<<"$sp")" = wt-fake-1
check "fake Agent replied in it" test "$(rade wait "$fk" --until done --timeout 20)" = done
check "its cwd is the new worktree" test "$(info "$fk" cwd)" = "$WTS/wt-fake-1"
out=$(rade read "$fk" --scrollback)
check "setup ran before the Agent" bash -c "grep -n SETUP-RAN-42 <<<'$out' | head -1 | cut -d: -f1 | xargs -I{} test {} -lt \$(grep -n 'fake-agent session' <<<'$out' | cut -d: -f1)"
check "new branch wt-fake-1" test "$(git -C "$WTS/wt-fake-1" branch --show-current)" = wt-fake-1
nw=$(rade spawn --source proj --worktree new --name named-wt --json)
check "--worktree new --name names it" test "$(jq -r .worktree <<<"$nw")" = named-wt
check "--worktree <existing> targets it" test "$(rade spawn --source proj --worktree feat-x --cmd bash --json | jq -r .worktree)" = feat-x

ty=$(rade spawn --source proj --worktree new --name ty-wt --agent typed "typed hello" --json | jq -r .pane)
check "prompt=type Preset: typed after the setup ran" test "$(rade wait "$ty" --until done --timeout 20)" = done
check "…the Agent got it" eventually 5 screen_has "$ty" "REPLY: TYPED HELLO"

echo "== R1.2 sidebar Worktree level (screenshot)"
rade focus "$wsh" >/dev/null
sleep 1
shot sidebar-worktrees-light

echo "== R13.1 Review Pane"
W=$WTS/feat-x
# Committed on the branch, then uncommitted + untracked on top.
printf 'fn main() {\n    let count = 1;\n    println!("{}", count);\n}\n' >"$W/src/a.rs"
G -C "$W" commit -qam "rename x"
printf 'def f():\n    return 1\n\n\ndef g():\n    return 20\n' >"$W/src/b.py"
printf 'new file\nsecond line\n' >"$W/src/new.txt"
git -C "$W" rm -q README.md
rv=$(rade review open --source proj --worktree feat-x)
rinfo() { rade debug review "$rv" "${@:-info}" --json; }
check "Review Pane opened in its own Tab (M14: full width)" test "$(info "$rv" tab)" != "$(info "$wsh" tab)"
check "diff loads" eventually 10 is "rinfo | jq -r '.files | length'" 5
check "files: status and +/- counts" test "$(rinfo | jq -r '[.files[] | "\(.status)\(.path)+\(.added)-\(.removed)"] | join(" ")')" = "U.setup-done+0-0 DREADME.md+0-1 Msrc/a.rs+2-2 Msrc/b.py+1-1 Usrc/new.txt+2-0"
echo "  (files: $(rinfo | jq -c '[.files[] | .status + .path]'))"
rade debug review "$rv" select src/a.rs >/dev/null
check "a.rs diff shows old and new lines" bash -c "rade debug review $rv info --json | jq -r .visible_text | grep -qx '    let count = 1;'"
check "gutter shows old/new numbers" bash -c "rade debug review $rv info --json | jq -r '.gutter[]' | grep -q '^ *2 *|−\$'"
check "a.rs highlighted as Rust" test "$(rinfo | jq -r .language)" = rust
check "title says vs main" bash -c "rade debug review $rv info --json | jq -r .title | grep -q 'vs main'"
rade debug review "$rv" toggle "@@" >/dev/null
check "hunk collapses (lines hidden)" bash -c "! rade debug review $rv info --json | jq -r .visible_text | grep -q 'let count'"
rade debug review "$rv" toggle "@@" >/dev/null
check "and expands" bash -c "rade debug review $rv info --json | jq -r .visible_text | grep -q 'let count'"
# Live refresh: a change on disk shows within the poll interval.
printf 'third\n' >>"$W/src/new.txt"
check "refreshes on file change" eventually 8 is "rinfo | jq -r '.files[] | select(.path==\"src/new.txt\") | .added'" 3

echo "== R13.2 Review Comments → Agent"
sk=$(rade spawn --source proj --worktree feat-x --agent sink --name sk --json | jq -r .pane)
eventually 10 screen_has "$sk" 'SINK READY' || true
rade debug review "$rv" select src/a.rs >/dev/null
rade debug review "$rv" comment "let count = 1|count);|Use a clearer name than count." >/dev/null
check "UI comment: anchored at src/a.rs:2-3" test "$(rade review list --source proj --worktree feat-x --json | jq -r '.comments[0] | "\(.path):\(.line)-\(.end_line)"')" = "src/a.rs:2-3"
check "…with the quoted code" test "$(rade review list --source proj --worktree feat-x --json | jq -r '.comments[0].excerpt | join("|")')" = '+    let count = 1;|+    println!("{}", count);'
out=$(cd "$W" && rade review add src/b.py:6 "Why 20?" --source proj --worktree feat-x)
check "CLI comment on src/b.py:6" test "$out" = "src/b.py:6"
check "CLI excerpt from the diff" test "$(rade review list --source proj --worktree feat-x --json | jq -r '.comments[1].excerpt[0]')" = "+    return 20"
check "Review Pane lists 2 comments, Request changes enabled" test "$(rinfo | jq -r '"\(.comments) \(.primary)"')" = "2 Request changes (2)"
check "commented lines marked in the gutter" bash -c "rade debug review $rv info --json | jq -r '.gutter[]' | grep -q '●'"
rade debug review "$rv" select src/a.rs >/dev/null
sleep 0.3
shot review-light
check "comment outside the Worktree exits 2" test "$(exit_code rade review add /etc/hosts:1 x --source proj --worktree feat-x)" = 2
stop_app
start_app
check "R13.2 comments persist across a restart" test "$(rade review list --source proj --worktree feat-x --json | jq '.comments | length')" = 2
rv=$(state | jq -r '[.. | objects | select(.kind?.type? == "review") | .id][0]')
check "Review Pane restored" test -n "$rv"
# The sink Agent was restored (its Preset re-launched): the only Agent there.
check "sink Agent restored" eventually 10 screen_has "$sk" 'SINK READY'
sleep 0.3
: >"$RECV"
rade debug review "$rv" send >/dev/null
check "R13 AC: Agent received both path:line references" eventually 5 bash -c "grep -q 'src/a.rs:2-3' $RECV && grep -q 'src/b.py:6' $RECV"
check "…in ONE prompt" test "$(grep -c 'comments on the changes in this worktree' "$RECV")" = 1
check "…with quotes and comments" eventually 5 bash -c "grep -qF '> +    return 20' $RECV && grep -q 'Why 20?' $RECV && grep -q 'clearer name' $RECV"
check "sent comments stay, with their round (M44, R119.1)" test "$(rade review list --source proj --worktree feat-x --json | jq -r '[.comments[] | .round] | map(tostring) | join(",")')" = "1,1"
check "…and no drafts are left to send (no prompt)" test "$(rade review list --source proj --worktree feat-x --json | jq -r .prompt)" = null
check "Agent Status running after the send" test "$(info "$sk" status)" = running
check "nothing to send → exit 2" test "$(exit_code rade review send --source proj --worktree feat-x)" = 2
rade review add "$W/src/b.py:6" "again" --source proj --worktree feat-x >/dev/null
rade spawn --source proj --worktree feat-x --agent sink --name sk3 >/dev/null
check "several Agents → AMBIGUOUS (exit 4)" test "$(exit_code rade review send --source proj --worktree feat-x)" = 4
check "--pane picks one" test "$(rade review send --source proj --worktree feat-x --pane sk3 --json | jq -r .comments)" = 1
rade review add "$WTS/again-x/src/a.rs:1" "no agent here" --source proj --worktree again-x >/dev/null
check "no Agent → NOT_FOUND (exit 4)" test "$(exit_code rade review send --source proj --worktree again-x)" = 4
fk2=$(rade review send --source proj --worktree again-x --agent fake --json | jq -r .pane)
check "--agent spawns one with the prompt" eventually 10 screen_has "$fk2" "COMMENTS ON THE CHANGES IN THIS WORKTREE"

echo "== R13 look in dark"
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not start"; exit 1; }
rade focus "$rv" >/dev/null
eventually 10 is "rade debug review $rv info --json | jq -r '.files | length'" 5 || true
rade debug review "$rv" select src/b.py >/dev/null
sleep 1
shot review-dark
rade focus "$wsh" >/dev/null
sleep 0.5
shot sidebar-worktrees-dark

echo "== R12.3 remove"
check "dirty worktree refused (exit 2)" test "$(exit_code rade worktree remove proj feat-x)" = 2
check "…naming the files and --force" grep -q "uncommitted or untracked.*--force" "$T/err"
check "…its Panes stay" test "$(info "$wsh" worktree)" = feat-x
G -C "$R" worktree lock "$WTS/again-x" --reason busy
check "locked worktree refused even with --force" test "$(exit_code rade worktree remove proj again-x --force)" = 2
check "…saying so" grep -q "locked (busy)" "$T/err"
G -C "$R" worktree unlock "$WTS/again-x"
r=$(rade worktree remove proj feat-x --force --delete-branch --json)
check "--force removes it" test ! -e "$WTS/feat-x"
check "its Panes closed" test -z "$(rade list --json | jq -r '.panes[] | select(.worktree=="feat-x") | .pane')"
check "--force --delete-branch deletes the unmerged branch (-D)" test "$(jq -r .branch_deleted <<<"$r")" = true
check "branch gone" bash -c "! git -C $R rev-parse --verify -q refs/heads/feat-x"
printf 'x\n' >"$WTS/named-wt/n.txt"
G -C "$WTS/named-wt" add -A && G -C "$WTS/named-wt" commit -qm n
rade worktree remove proj named-wt --delete-branch >"$T/out" 2>"$T/err"
check "clean worktree removed without --force" test ! -e "$WTS/named-wt"
check "unmerged branch kept with -d, and said" grep -q "branch named-wt kept.*not fully merged" "$T/err"
G -C "$WTS/old-br" checkout -q --detach
r=$(rade worktree remove proj old-br --delete-branch --json)
check "detached HEAD: no branch to delete (not the commit hash)" test "$(jq -r '"\(.branch) \(.branch_deleted)"' <<<"$r")" = "null false"
check "…branch old-br untouched" git -C "$R" rev-parse -q --verify refs/heads/old-br
check "main worktree can't be removed (exit 2)" test "$(exit_code rade worktree remove proj main)" = 2

echo "== R11 restore + missing folder → prune"
stop_app
rm -rf "$WTS/wt-fake-1"
start_app
check "worktrees restored" test "$(rade worktree list proj --json | jq -r '[.worktrees[].worktree] | join(",")')" = "main,again-x,ext-br,wt-fake-1,ty-wt"
check "vanished folder reported missing" test "$(wtl wt-fake-1 missing)" = true
check "Tabs of the other worktrees restored" test "$(state | jq -r '[.sources[0].worktrees[] | select(.name=="again-x") | .tabs[]] | length')" -ge 1
rade focus "$(rade list --json | jq -r '.panes[] | select(.worktree=="again-x") | .pane' | head -1)" >/dev/null
sleep 0.8
shot sidebar-missing
r=$(rade worktree remove proj wt-fake-1 --json)
check "remove on a missing folder prunes" test "$(jq -r .pruned <<<"$r")" = true
check "git forgot it" bash -c "! git -C $R worktree list | grep -q wt-fake-1"
check "model forgot it" test -z "$(wtl wt-fake-1 worktree)"

echo "== R12.3 Remove Worktree… on a clean Worktree with a submodule"
S=$T/subrepo
mkdir -p "$S" && G -C "$S" init -q -b main && printf 's\n' >"$S/s.txt" && G -C "$S" add -A && G -C "$S" commit -qm s
rade worktree add proj sub-wt >/dev/null
G -C "$WTS/sub-wt" -c protocol.file.allow=always submodule add -q "$S" lib >/dev/null 2>&1
G -C "$WTS/sub-wt" commit -qm "add lib"
check "R12.3 the Worktree is clean and has an initialized submodule" \
    bash -c "[[ -z \$(git -C '$WTS/sub-wt' status --porcelain) && -e '$WTS/sub-wt/lib/s.txt' ]]"
sleep 1
rade debug menu worktree proj/sub-wt activate "Remove Worktree" >/dev/null
eventually 3 bash -c "rade debug dialog --json | jq -e '.open' >/dev/null"
rade debug dialog --json >"$T/sub-dialog.json"
check "R12.3 the dialog says its submodules go with it and offers Remove" \
    bash -c "jq -r .body '$T/sub-dialog.json' | grep -qF 'Its submodules are removed with it.' && jq -e '.responses | index(\"submodules\")' '$T/sub-dialog.json' >/dev/null"
rade debug dialog respond submodules >/dev/null
check "R12.3 Remove removes it (git needed --force), no error dialog" \
    eventually 8 bash -c "[[ ! -e '$WTS/sub-wt' && -z \$(wtl sub-wt worktree) ]]"
check "R12.3 …the branch stays (Also delete branch was off)" git -C "$R" rev-parse -q --verify refs/heads/sub-wt
check "R12.3 …and no 'Could not remove' dialog" test "$(rade debug dialog --json | jq -r .open)" = false

echo "== general tests still hold"
check "spawn --worktree new in General exits 2" test "$(exit_code rade spawn --source general --worktree new)" = 2

stop_app
echo "== M7: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
