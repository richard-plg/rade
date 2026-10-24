#!/usr/bin/env bash
# M15 end-to-end tests (ADR-0006): Workspaces + Sources (R23, R24). A v1
# state.json migrates to v2 with a one-time backup and its old XDG Worktree
# still works (R24-a, R24-d); the `rade project` alias is gone since M16
# (R24-c became R31-a); Workspaces from `<root>/.rade/workspace.toml` give each Pane its
# env and `gh` account (R23-a); new Worktrees go under the root and get the
# Workspace's git identity (R23-b); switching shows one Workspace while the
# other keeps running (R23-c); a hidden Agent waiting raises the switcher's
# `!`, Unread and a desktop notification naming its Workspace (R23-d);
# `rade source add org/x` clones with a fake `gh` under the Workspace env,
# idempotently (R24-b). Screenshots light + dark of the switcher and its
# popover with the badge.
# Usage: tests/e2e/m15.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m15
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin" "$T/remotes"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
EOF
# A fake GitHub CLI first on PATH (Panes inherit the app's PATH).
ln -s "$HERE/fake-gh.sh" "$T/bin/gh"
export PATH=$T/bin:$PATH FAKE_GH_LOG=$T/gh.log FAKE_GH_REMOTES=$T/remotes
unset GH_CONFIG_DIR GH_TOKEN WS_NAME
: >"$FAKE_GH_LOG"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
spawn() { rade spawn "$@" --json | jq -r .pane; }
state() { rade debug state; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
shown_ws() { state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .workspace'; }
# The Sources the Navigator shows (1.0.0, R104: links, Worktrees and Scratch rows name them).
sidebar_sources() { ui sidebar | jq -r '[.sidebar[] | .target.source? // empty] | unique | join(",")'; }
focused() { state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .active_worktree as $w | .worktrees[] | select(.id==$w) | .active_tab as $t | .tabs[] | select(.id==$t) | .focused'; }

echo "== R24-a v1 state.json migrates to v2 with a one-time backup"
DEMO=$HOME/code/demo
mkdir -p "$DEMO" && G -C "$DEMO" init -q -b main && G -C "$DEMO" commit -q --allow-empty -m init
OLD_WT=$XDG_DATA_HOME/rade-dev/worktrees/demo/feat
mkdir -p "$(dirname "$OLD_WT")"
G -C "$DEMO" worktree add -q -b feat "$OLD_WT"
mkdir -p "$(dirname "$STATE")"
sed -e "s|@HOME@|$HOME|g" -e "s|@DATA@|$XDG_DATA_HOME|g" "$HERE/fixtures/state-v1.json" >"$STATE"
cp "$STATE" "$T/state-v1.json"
start_app
BAK=$STATE.v1.bak
check "state.json.v1.bak written" test -f "$BAK"
check "backup byte-identical to the v1 file" cmp -s "$BAK" "$T/state-v1.json"
check "model is the current version (3, R101.3)" test "$(state | jq .version)" = 3
check "one Workspace: default at \$HOME" test "$(state | jq -c '[.workspaces[] | [.id, .root]]')" = "[[\"default\",\"$HOME\"]]"
check "every Source in default" test "$(state | jq -r '[.sources[].workspace] | unique | join(",")')" = default
check "Tabs and Panes restored (p_3, p_4 in t_2)" test "$(rade list --json | jq -r '[.panes[] | select(.tab=="t_2") | .pane] | sort | join(",")')" = p_3,p_4
check "Agent resumed its session id" eventually 10 screen_has p_4 "resumed sess-v1"
check "palette recents renamed" test "$(state | jq -r '.palette_recent | join(",")')" = "source:demo,action:add_source,worktree:demo/w_5"

echo "== R24-d the v1 Worktree under XDG keeps working"
check "listed" test "$(rade worktree list demo --json | jq -r '.worktrees[] | select(.worktree=="feat") | .path')" = "$OLD_WT"
p=$(spawn --source demo --worktree feat)
rade send "$p" 'pwd' --enter
check "a Terminal opens in it" eventually 5 screen_has "$p" "$OLD_WT"
rade close "$p" --force >/dev/null
check "removed" test "$(code rade worktree remove demo feat --force)" = 0
check "its folder is gone" test ! -e "$OLD_WT"
stop_app
check "saved as the current version (3, R101.3)" test "$(jq .version "$STATE")" = 3
check "saved keys are the new ones" test "$(jq -r 'has("sources") and has("active_source") and (has("projects") | not)' "$STATE")" = true
touch -d '2001-01-01' "$BAK"
start_app
check "backup not rewritten on the next start" test "$(stat -c %Y "$BAK")" = "$(date -d 2001-01-01 +%s)"
check "still identical" cmp -s "$BAK" "$T/state-v1.json"

echo "== R24-c → R31 the M15 project aliases are gone (M16)"
hint=$(rade project add ./demo 2>&1 >/dev/null; echo "rc=$?")
check "rade project add <path> hints at rade source add" test "$hint" = "rade: 'rade project add <path>' is now 'rade source add <path>'
rc=2"
check "no project alias keys in results" test "$(rade source list --json | jq -r '.sources[0] | has("project")')" = false
check "--source still lists" test "$(rade list --source demo --json | jq -r '.panes | length')" = 2

echo "== R23.1 Workspaces: add moves Sources in, General each"
A=$HOME/workspaces/acme B=$HOME/workspaces/bravo
mkdir -p "$A/.rade" "$B/.rade" "$HOME/.config/gh-acme" "$HOME/.config/gh-bravo"
cat >"$A/.rade/workspace.toml" <<'EOF'
name = "Acme"
color = "orange"
[env]
GH_CONFIG_DIR = "~/.config/gh-acme"
[clone]
default_owner = "acme-org"
[identity]
email = "ignored@unknown.key"
EOF
cat >"$B/.rade/workspace.toml" <<'EOF'
name = "Bravo"
color = "blue"
[env]
GH_CONFIG_DIR = "~/.config/gh-bravo"
EOF
echo '{"login":"alice-acme"}' >"$HOME/.config/gh-acme/login"
echo '{"login":"bob-bravo"}' >"$HOME/.config/gh-bravo/login"
# git identity per Workspace through includeIf (as on the owner's machine).
printf '[user]\n\temail = global@example.test\n\tname = T\n[commit]\n\tgpgsign = false\n[includeIf "gitdir:~/workspaces/acme/"]\n\tpath = ~/.gitconfig-acme\n' >"$HOME/.gitconfig"
printf '[user]\n\temail = me@acme.test\n' >"$HOME/.gitconfig-acme"
API=$A/api
mkdir -p "$API" && git -C "$API" init -q -b main && git -C "$API" commit -q --allow-empty -m init
check "source added before its Workspace exists → default" test "$(rade source add "$API" --json | jq -r .workspace)" = default
r=$(rade workspace add "$A" --json)
check "id from workspace.toml's name" test "$(jq -r .workspace <<<"$r")" = acme
check "Sources under the root moved in" test "$(jq -c .moved <<<"$r")" = '["api"]'
check "second Workspace" test "$(rade workspace add "$B" --json | jq -r .workspace)" = bravo
check "a General per Workspace" test "$(state | jq -r '[.sources[] | select(.id|startswith("general")) | .id] | join(",")')" = general,general@acme,general@bravo
check "General@bravo rooted at its root" test "$(rade source list --workspace bravo --json | jq -r '.sources[0].path')" = "$B"
check "workspace list: keys and colors" test "$(rade workspace list --json | jq -c '[.workspaces[] | [.workspace, .name, .key, .color]]')" = '[["default","default","Ctrl+Alt+1","#77767b"],["acme","Acme","Ctrl+Alt+2","#ff7800"],["bravo","Bravo","Ctrl+Alt+3","#3584e4"]]'
check "remove refuses a Workspace with Sources" test "$(code rade workspace remove acme)" = 2
check "… naming them" grep -q "api" "$T/err"
check "default can't be removed" test "$(code rade workspace remove default)" = 2

echo "== R23-a env per Pane: GH_CONFIG_DIR and gh account per Workspace"
pa=$(spawn --source api --name sh-a)
pb=$(spawn --source general@bravo --name sh-b)
for p in "$pa" "$pb"; do rade send "$p" 'echo "WS=$RADE_WORKSPACE GH=$GH_CONFIG_DIR SRC=$RADE_SOURCE OLD=[$RADE_PROJECT]"; gh api user' --enter; done
check "A Pane: its GH_CONFIG_DIR + RADE_WORKSPACE, no RADE_PROJECT (R31-c)" eventually 5 screen_has "$pa" "WS=acme GH=$HOME/.config/gh-acme SRC=api OLD=[]"
check "A Pane: gh api user = alice" eventually 5 screen_has "$pa" '"login":"alice-acme"'
check "B Pane: its GH_CONFIG_DIR" eventually 5 screen_has "$pb" "WS=bravo GH=$HOME/.config/gh-bravo SRC=general@bravo"
check "B Pane: gh api user = bob" eventually 5 screen_has "$pb" '"login":"bob-bravo"'
check "a default Pane gets no GH_CONFIG_DIR" bash -c "p=\$(rade spawn --source demo --json | jq -r .pane); rade send \$p 'echo \"D=[\$GH_CONFIG_DIR] W=\$RADE_WORKSPACE\"' --enter; sleep 1; rade read \$p | grep -qF 'D=[] W=default'"
check "workspace.list env, ~ expanded" test "$(rade workspace list --json | jq -r '.workspaces[1].env.GH_CONFIG_DIR')" = "$HOME/.config/gh-acme"

echo "== R23-b new Worktrees under the root, with the Workspace's identity"
wt=$(rade worktree add api feat-x)
check "path under acme/.worktrees-dev (dev flavor)" test "$wt" = "$A/.worktrees-dev/api/feat-x"
check "git user.email there = acme's (includeIf)" test "$(git -C "$wt" config user.email)" = me@acme.test
check "outside the clone (git status of api clean)" test -z "$(git -C "$API" status --porcelain)"
check "default Source keeps XDG" test "$(rade worktree add demo d1)" = "$XDG_DATA_HOME/rade-dev/worktrees/demo/d1"

echo "== R23-c one Workspace shown; the other keeps running"
lead=$(spawn --source api --agent fake --name lead "hello")
check "lead running in acme" eventually 10 screen_has "$lead" "HELLO"
rade focus "$lead"
tab_a=$(rade list --all --json | jq -r --arg p "$lead" '.panes[] | select(.pane==$p) | .tab')
# The Agent's process: the fake agent whose environment names its Pane.
pid=$(for x in $(pgrep -f fake-agent.sh); do tr '\0' '\n' <"/proc/$x/environ" 2>/dev/null | grep -qx "RADE_PANE_ID=$lead" && echo "$x"; done | head -1 || true)
check "shown: acme" test "$(shown_ws)" = acme
check "sidebar: acme's Sources only" test "$(sidebar_sources)" = api,general@acme
rade debug action workspace_3
check "Ctrl+Alt+3 → bravo shown" test "$(shown_ws)" = bravo
check "sidebar: bravo's Sources only" test "$(sidebar_sources)" = general@bravo
check "rade list: the shown Workspace's Panes" test "$(rade list --json | jq -r '[.panes[].workspace] | unique | join(",")')" = bravo
check "rade list --all: every Workspace" test "$(rade list --all --json | jq -r '[.panes[].workspace] | unique | join(",")')" = acme,bravo,default
check "lead's process alive while hidden" kill -0 "${pid:-none}"
rade send "$lead" "while hidden" --enter
check "rade read shows what it printed meanwhile" eventually 10 screen_has "$lead" "WHILE HIDDEN"
shot ws-bravo
rade debug action workspace_2
check "Ctrl+Alt+2 → acme again" test "$(shown_ws)" = acme
check "same Tab" test "$(state | jq -r '.sources[] | select(.id=="api") | .worktrees[0].active_tab')" = "$tab_a"
check "same focus" test "$(focused)" = "$lead"
check "Alt+1 = acme's first Source" bash -c "rade debug action source_1; [[ \$(rade debug state | jq -r .active_source) == api ]]"

echo "== R23-d hidden Agent waiting → badge, Unread, desktop notification naming it"
rade debug action workspace_3
check "no badge yet" test "$(ui workspaces | jq -r .workspaces.badge)" = false
before=$(grep -c '^NOTIFY' "$NOTIFYD_LOG" || true)
# What its Agent hook does (`rade status waiting --hook`), timed from there.
t0=$(date +%s%N)
rade status waiting --pane "$lead"
check "switcher ! within 1 s" eventually 1 is "ui workspaces | jq -r .workspaces.badge" true
ms=$(( ($(date +%s%N) - t0) / 1000000 ))
echo "  (badge after ${ms} ms)"
check "measured under 1 s" test "$ms" -lt 1000
check "acme row: Unread 1, waiting glyph" test "$(ui workspaces | jq -c '.workspaces.rows[] | select(.workspace=="acme") | [.unread, .glyph]')" = '[1,"waiting"]'
check "badge tooltip names the Agent and Workspace" test "$(ui workspaces | jq -r .workspaces.badge_tooltip)" = "lead needs input in Acme"
check "Toast names the Workspace" eventually 3 is "ui chrome | jq -r '.chrome.toasts[-1].title'" "[Acme] <b>lead</b> needs input"
check "desktop notification sent" eventually 5 bash -c "[[ \$(grep -c '^NOTIFY' '$NOTIFYD_LOG') -gt $before ]]"
line=$(grep '^NOTIFY' "$NOTIFYD_LOG" | tail -1)
title=$(cut -f2 <<<"$line")
echo "  (title: $title)"
check "its title names Acme" test "${title#\[Acme\] }" != "$title"
shot ws-badge
# The popover is its own surface: rendered alone.
popshot() { sleep 0.4; rpc debug.ui "{\"op\": \"switcher\", \"arg\": \"shot\", \"path\": \"$OUT/$1.png\"}" >/dev/null && echo "  shot ${OUT#$ROOT/}/$1.png"; }
rpc debug.ui '{"op": "switcher", "arg": "open"}' >/dev/null
check "popover open" eventually 3 is "ui workspaces | jq -r .workspaces.open" true
popshot ws-popover
rade debug action color_scheme_dark
sleep 0.3
shot ws-badge-dark
popshot ws-popover-dark
rpc debug.ui '{"op": "switcher", "arg": "close"}' >/dev/null
rade debug action color_scheme_light
nid=$(cut -f1 <<<"$line" | cut -d' ' -f2)
gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Invoke "$nid" default >/dev/null
check "activating it switches to acme" eventually 5 is shown_ws acme
check "… and focuses lead" eventually 5 is focused "$lead"
check "badge gone" eventually 3 is "ui workspaces | jq -r .workspaces.badge" false
rade send "$lead" "yes" --enter

echo "== R23.5 switcher clicks and palette"
rpc debug.ui '{"op": "switcher", "arg": "bravo"}' >/dev/null
check "switcher row click → bravo" eventually 3 is shown_ws bravo
check "palette row 'Switch Workspace: Acme' with its key" test "$(rade debug palette -- '>switch workspace acme' | jq -r '.results[0] | [.title, .keys] | join("|")')" = "Switch Workspace: Acme|Ctrl+Alt+2"
check "palette Source rows: the shown Workspace's" test "$(rade debug palette -- '/' | jq -r '[.results[] | select(.kind=="source") | .title] | join(",")')" = "~/workspaces/bravo"
rade workspace switch acme >/dev/null
check "rade workspace switch" test "$(shown_ws)" = acme

echo "== R24-b rade source add org/x clones with gh under the Workspace env"
G init -q --bare "$T/remotes/web.git"
G clone -q "$T/remotes/web.git" "$T/seed" 2>/dev/null; G -C "$T/seed" commit -q --allow-empty -m first; G -C "$T/seed" push -q origin HEAD:main 2>/dev/null
G --git-dir="$T/remotes/web.git" symbolic-ref HEAD refs/heads/main
: >"$FAKE_GH_LOG"
(cd "$T" && RADE_PANE_ID=$pb rade source add web-org/web --json) >"$T/add.json" 2>"$T/add.err" || true
check "cloned under bravo's root (the caller's Workspace)" test -d "$B/web/.git"
check "registered in bravo" test "$(jq -r '[.source, .workspace, .existing] | join(",")' "$T/add.json")" = web,bravo,false
check "gh ran with bravo's GH_CONFIG_DIR" grep -qx "GH_CONFIG_DIR=$HOME/.config/gh-bravo repo clone web-org/web $B/web" "$FAKE_GH_LOG"
: >"$FAKE_GH_LOG"
check "again: exit 0" test "$(code env RADE_PANE_ID="$pb" rade source add web-org/web)" = 0
check "… prints the Source" test "$(cat "$T/out")" = web
check "… no second clone" test ! -s "$FAKE_GH_LOG"
check "… one Source" test "$(rade source list --all --json | jq '[.sources[] | select(.path=="'"$B"'/web")] | length')" = 1
G clone -q --bare "$T/remotes/web.git" "$T/remotes/tools.git"
check "bare repo name gets default_owner (acme)" test "$(code rade source add tools --workspace acme)" = 0
check "… gh got acme-org/tools with acme's env" grep -qx "GH_CONFIG_DIR=$HOME/.config/gh-acme repo clone acme-org/tools $A/tools" "$FAKE_GH_LOG"
check "no default_owner → USAGE" test "$(code rade source add lonely --workspace bravo)" = 2
check "… says how" grep -q "default_owner" "$T/err"
check "a path still registers directly" test "$(rade source add "$DEMO" --json | jq -r .existing)" = true

echo "== R23.1 remove an empty Workspace"
check "bravo has a Source: refused" test "$(code rade workspace remove bravo)" = 2
rade source remove web >/dev/null
rade workspace switch bravo >/dev/null
check "empty → removed" test "$(code rade workspace remove bravo)" = 0
check "its General is gone" test "$(state | jq '[.sources[] | select(.id=="general@bravo")] | length')" = 0
check "shown falls back to default" test "$(shown_ws)" = default
check "its Panes closed" test "$(rade list --all --json | jq --arg p "$pb" '[.panes[] | select(.pane==$p)] | length')" = 0

echo "== restart keeps Workspaces and the shown one"
rade workspace switch acme >/dev/null
stop_app
start_app
check "Workspaces restored" test "$(rade workspace list --json | jq -r '[.workspaces[].workspace] | join(",")')" = default,acme
check "acme shown" test "$(shown_ws)" = acme

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
