#!/usr/bin/env bash
# M38 end-to-end tests (ADR-0006, ADR-0035): Agent Accounts (PRD R108).
# A temp HOME with two Workspaces: `acme` (accounts acme-work claude default
# ~/.claude-work, acme-team claude ~/.claude-team, acme codex default
# ~/.codex) and `personal` (personal claude ~/.claude, personal codex
# ~/.codex). The fake Agent (tests/e2e/fake-agent.sh) is the `claude` and
# `codex` Presets' command; its session-start hook names the `claude` Preset.
# Part 1 (Group 2): R108-g, h, d, e, c, l, j (also for Fan-out Members and the
# Judge), f, k. Part 2 (Group 3): R108-i,
# the `+` menu on Project Shop / shop-web / Worktree checkout-v2: its rows and
# spec F geometry (tests/e2e/spec_f.py) in dark and light, the locked
# `personal` row, activating `acme-team`; screenshots → target/e2e-m38/.
# Usage: tests/e2e/m38.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m38
export WESTON_WIDTH=1440
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
export FAKE_AGENT=claude FAKE_ARGV=$T/argv.log
: >"$FAKE_ARGV"
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade"
preset_cmd() { # the claude Preset's command (R108-j changes it)
    printf 'idle_seconds = 1\n\n[agents.claude]\ncommand = "%s"\n\n[agents.codex]\ncommand = "%s"\n' "$1" "$FAKE" >"$CONF"
}
preset_cmd "$FAKE"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
as_pane() { local p=$1; shift; RADE_PANE_ID=$p "$@"; }
npanes() { rade list --all --json | jq '.panes | length'; }
state() { rade debug state; }
acct_of() { jq -r --arg p "$1" '.. | objects | select(.id? == $p) | .account // "none"' "$STATE"; }
# A Pane's variable, read from its processes' environment (/proc).
penv() { python3 - "$1" "$2" <<'PY'
import os, sys
pane, var = sys.argv[1], sys.argv[2]
for pid in sorted(p for p in os.listdir("/proc") if p.isdigit()):
    try:
        raw = open(f"/proc/{pid}/environ", "rb").read().split(b"\0")
    except OSError:
        continue
    env = dict(e.decode(errors="replace").split("=", 1) for e in raw if b"=" in e)
    # This run's Panes only (another suite on the machine reuses Pane ids).
    if env.get("RADE_PANE_ID") == pane and env.get("XDG_RUNTIME_DIR") == os.environ.get("XDG_RUNTIME_DIR"):
        print(env.get(var, "<unset>"))
        break
PY
}
# A screen line that is exactly `VAR=value` (scrollback included).
line_is() { rade read "$1" --scrollback | sed 's/[[:space:]]*$//' | grep -qxF -- "$2"; }
export -f penv line_is rpc

# ---- The scene ----------------------------------------------------------------
A=$HOME/workspaces/acme P=$HOME/workspaces/personal
for r in "$A/api" "$P/notes"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && echo base >"$r/README.md"
    G -C "$r" add -A && G -C "$r" commit -qm init
done
mkdir -p "$A/.rade" "$P/.rade" "$HOME/.claude-work" "$HOME/.claude-team" "$HOME/.codex" "$HOME/.claude"
cat >"$A/.rade/workspace.toml" <<'EOF'
name = "acme"

# Work logins.
[env]
GH_CONFIG_DIR = "~/.config/gh-acme"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-work"
default = true

[[account]]
agent = "codex"
name = "acme"
config_dir = "~/.codex"
default = true
EOF
cat >"$P/.rade/workspace.toml" <<'EOF'
name = "personal"

[[account]]
agent = "claude"
name = "personal"
config_dir = "~/.claude"

[[account]]
agent = "codex"
name = "personal"
config_dir = "~/.codex"
EOF

start_app
rade workspace add "$A" >/dev/null
rade workspace add "$P" >/dev/null
rade source add "$A/api" --workspace acme >/dev/null
rade source add "$P/notes" --workspace personal >/dev/null
rade workspace switch acme >/dev/null

echo "== R108-g rade account add / default / list"
cp "$A/.rade/workspace.toml" "$T/acme-0.toml"
rc=$(code rade account add --agent claude --name acme-team --config-dir '~/.claude-team')
check "R108-g account add prints the file, exit 0" \
    bash -c "[[ $rc == 0 && \$(cat '$T/out') == '$A/.rade/workspace.toml' ]]"
check "R108-g workspace.toml gains one [[account]] with every other byte kept" \
    bash -c "head -c \$(stat -c %s '$T/acme-0.toml') '$A/.rade/workspace.toml' | cmp -s - '$T/acme-0.toml' && [[ \$(tail -n +\$((\$(wc -l <'$T/acme-0.toml') + 1)) '$A/.rade/workspace.toml') == \$'\n[[account]]\nagent = \"claude\"\nname = \"acme-team\"\nconfig_dir = \"~/.claude-team\"' ]]"
cp "$A/.rade/workspace.toml" "$T/acme-1.toml"
rc=$(code rade account default acme-team)
check "R108-g account default acme-team: default = true moves from acme-work (one line out, one in)" \
    bash -c "[[ $rc == 0 ]] && [[ \$(diff '$T/acme-1.toml' '$A/.rade/workspace.toml' | grep -c '^[<>]') == 2 ]] && diff '$T/acme-1.toml' '$A/.rade/workspace.toml' | grep -qx '< default = true' && diff '$T/acme-1.toml' '$A/.rade/workspace.toml' | grep -qx '> default = true'"
L=$(rade account list --json)
check "R108-g list --json: 3 accounts, acme-team the claude default, acme the codex default" \
    test "$(jq -c '[.accounts[] | "\(.agent):\(.name):\(.default)"]' <<<"$L")" = '["claude:acme-work:false","codex:acme:true","claude:acme-team:true"]'
check "R108-g list prints 'claude  acme-team  ~/.claude-team  ✓'" \
    bash -c "rade account list | grep -qxF 'claude  acme-team  ~/.claude-team  ✓'"
# Back to acme-work as the claude default for the spawns below (R108-e).
rade account default acme-work >/dev/null

echo "== R108-h rade setup --dry-run covers every declared config_dir"
snap() { find "$HOME" -path "$HOME/workspaces" -prune -o -type f -print0 | sort -z | xargs -0 -r md5sum; }
snap >"$T/home-0"
rc=$(code rade setup --dry-run --agent claude --agent codex)
cp "$T/out" "$OUT/setup-dry-run.txt"
check "R108-h the diff installs hooks into ~/.claude-work, ~/.claude-team and ~/.codex" \
    bash -c "[[ $rc == 0 ]] && grep -qF '$HOME/.claude-work/settings.json' '$T/out' && grep -qF '$HOME/.claude-team/settings.json' '$T/out' && grep -qF '$HOME/.codex/config.toml' '$T/out' && grep -qF 'dry run: nothing written' '$T/out'"
check "R108-h nothing written under HOME" bash -c "snap() { find '$HOME' -path '$HOME/workspaces' -prune -o -type f -print0 | sort -z | xargs -0 -r md5sum; }; snap | cmp -s - '$T/home-0'"

echo "== R108-d the account's env, for that Pane only"
d1=$(rade spawn --agent claude --account acme-team --cmd env --source api --tab new --name d1)
d2=$(rade spawn --agent codex --cmd env --source api --tab new --name d2)
eventually 10 screen_has "$d1" RADE_ACCOUNT= || true
eventually 10 screen_has "$d2" RADE_ACCOUNT= || true
check "R108-d claude Pane: CLAUDE_CONFIG_DIR=\$HOME/.claude-team and RADE_ACCOUNT=acme-team, no CODEX_HOME" \
    bash -c "line_is $d1 'CLAUDE_CONFIG_DIR=$HOME/.claude-team' && line_is $d1 RADE_ACCOUNT=acme-team && ! rade read $d1 --scrollback | grep -q '^CODEX_HOME='"
check "R108-d codex Pane: CODEX_HOME=\$HOME/.codex and RADE_ACCOUNT=acme, no CLAUDE_CONFIG_DIR" \
    bash -c "line_is $d2 'CODEX_HOME=$HOME/.codex' && line_is $d2 RADE_ACCOUNT=acme && ! rade read $d2 --scrollback | grep -q '^CLAUDE_CONFIG_DIR='"
check "R108-d pane.list and the spawn result carry the account" \
    test "$(rade list --json | jq -r --arg a "$d1" --arg b "$d2" '[.panes[] | select(.pane == $a or .pane == $b) | .account] | join(",")')" = "acme-team,acme"

echo "== R108-e a child inherits its parent's account; a root spawn gets the default"
e1=$(rade spawn --agent claude --account acme-team --cmd 'bash' --source api --tab new --name e1)
eventually 10 screen_has "$e1" '$' || true
e2=$(as_pane "$e1" rade spawn --agent claude --cmd 'sleep 600' --name e2)
e3=$(rade spawn --agent claude --cmd 'sleep 600' --source api --tab new --name e3)
check "R108-e the child spawned from the acme-team Pane has RADE_ACCOUNT=acme-team" eventually 10 is "penv $e2 RADE_ACCOUNT" acme-team
check "R108-e the root spawn has RADE_ACCOUNT=acme-work" eventually 10 is "penv $e3 RADE_ACCOUNT" acme-work
check "R108-e the root spawn's CLAUDE_CONFIG_DIR is acme-work's" is "penv $e3 CLAUDE_CONFIG_DIR" "$HOME/.claude-work"

echo "== R108-c an account of another Workspace is FORBIDDEN"
n0=$(npanes)
rc=$(code as_pane "$e1" rade spawn --agent claude --account personal)
check "R108-c CLI exits 7 naming personal and acme" \
    bash -c "[[ $rc == 7 ]] && grep -qF \"account 'personal' belongs to workspace 'personal', not 'acme'\" '$T/err'"
R=$(rpc pane.spawn "{\"agent\": \"claude\", \"account\": \"personal\", \"caller\": \"$e1\"}")
check "R108-c pane.spawn fails with kind FORBIDDEN" test "$(jq -r '.data.kind' <<<"$R")" = FORBIDDEN
check "R108-c no Pane was created" test "$(npanes)" = "$n0"

echo "== R108-l a name a claude and a codex account share needs --agent"
cp "$P/.rade/workspace.toml" "$T/personal-0.toml"
rc1=$(code rade account usage personal --workspace personal); cp "$T/err" "$T/err1"
rc2=$(code rade account default personal --workspace personal); cp "$T/err" "$T/err2"
check "R108-l account usage / default personal exit 4 naming claude and codex" \
    bash -c "[[ $rc1 == 4 && $rc2 == 4 ]] && grep -q claude '$T/err1' && grep -q codex '$T/err1' && grep -q claude '$T/err2' && grep -q codex '$T/err2'"
check "R108-l the socket answers AMBIGUOUS" \
    test "$(rpc usage.get '{"account": "personal", "workspace": "personal"}' | jq -r .data.kind),$(rpc account.set_default '{"name": "personal", "workspace": "personal"}' | jq -r .data.kind)" = AMBIGUOUS,AMBIGUOUS
check "R108-l workspace.toml unchanged" cmp -s "$T/personal-0.toml" "$P/.rade/workspace.toml"
U=$(rade account usage personal --agent codex --workspace personal --json)
check "R108-l usage --agent codex acts on the codex account (no data yet)" \
    test "$(jq -r '"\(.agent) \(.config_dir) \(.state)"' <<<"$U")" = "codex $HOME/.codex no-data"
rc=$(code rade account default personal --agent codex --workspace personal)
check "R108-l default --agent codex sets default on the codex table only" \
    bash -c "[[ $rc == 0 ]] && [[ \$(diff '$T/personal-0.toml' '$P/.rade/workspace.toml' | grep -c '^[<>]') == 1 ]] && tail -2 '$P/.rade/workspace.toml' | grep -qx 'default = true'"
l1=$(rade spawn --agent claude --account personal --cmd env --source notes --tab new --name l1)
eventually 10 screen_has "$l1" RADE_ACCOUNT= || true
check "R108-l spawn --agent claude --account personal --cmd env → CLAUDE_CONFIG_DIR=\$HOME/.claude" \
    bash -c "line_is $l1 'CLAUDE_CONFIG_DIR=$HOME/.claude' && line_is $l1 RADE_ACCOUNT=personal"
rade workspace switch acme >/dev/null

echo "== R108-j a Preset prefix that sets another config dir is FORBIDDEN"
preset_cmd "CLAUDE_CONFIG_DIR=~/.claude $FAKE"
n0=$(npanes)
rc=$(code rade spawn --agent claude --account acme-team --source api --tab new)
check "R108-j CLI exits 7 naming the Preset claude, ~/.claude and acme-team" \
    bash -c "[[ $rc == 7 ]] && grep -qF \"preset 'claude' sets CLAUDE_CONFIG_DIR=~/.claude but account 'acme-team' uses ~/.claude-team\" '$T/err'"
R=$(rpc pane.spawn '{"agent": "claude", "account": "acme-team", "source": "api", "tab": "new"}')
check "R108-j pane.spawn fails with kind FORBIDDEN naming the Preset" \
    test "$(jq -r .data.kind <<<"$R"),$(jq -r .message <<<"$R" | grep -cF "preset 'claude' sets CLAUDE_CONFIG_DIR=~/.claude but account 'acme-team'")" = FORBIDDEN,1
check "R108-j no Pane was created" test "$(npanes)" = "$n0"
preset_cmd "CLAUDE_CONFIG_DIR=~/.claude-team $FAKE"
j1=$(rade spawn --agent claude --account acme-team --source api --tab new --name j1)
R=$(rpc pane.spawn '{"agent": "claude", "account": "acme-team", "source": "api", "tab": "new", "name": "j2"}')
j2=$(jq -r .pane <<<"$R")
for p in "$j1" "$j2"; do
    check "R108-j with ~/.claude-team the Pane $p runs: CLAUDE_CONFIG_DIR=\$HOME/.claude-team, RADE_ACCOUNT=acme-team" \
        eventually 10 is "echo \$(penv $p CLAUDE_CONFIG_DIR) \$(penv $p RADE_ACCOUNT)" "$HOME/.claude-team acme-team"
done
preset_cmd "$FAKE"
rade close --force "$j1" >/dev/null; rade close --force "$j2" >/dev/null

echo "== R108-e / R108-j Fan-out Members and the Judge run under the Workspace's accounts"
preset_cmd "CLAUDE_CONFIG_DIR=~/.claude $FAKE"
n0=$(npanes); w0=$(G -C "$A/api" worktree list | wc -l)
rc=$(code rade fanout --source api --agents claude,codex 'add a line')
echo "  (rade fanout: exit $rc: $(cat "$T/err"))"
check "R108-j rade fanout with the ~/.claude prefix exits 7 naming the Preset and acme-work, no Pane, no Worktree" \
    bash -c "[[ $rc == 7 ]] && grep -qF \"preset 'claude' sets CLAUDE_CONFIG_DIR=~/.claude but account 'acme-work'\" '$T/err' && [[ \$(rade list --all --json | jq '.panes | length') == $n0 ]] && [[ \$(git -C '$A/api' worktree list | wc -l) == $w0 ]]"
preset_cmd "$FAKE"
FJ=$(as_pane "$e1" rade fanout --agents claude,codex 'add a line' --json)
FO=$(jq -r .fanout <<<"$FJ"); fc=$(jq -r '.panes[0]' <<<"$FJ"); fx=$(jq -r '.panes[1]' <<<"$FJ")
check "R108-e the claude Member started from the acme-team Pane: CLAUDE_CONFIG_DIR=\$HOME/.claude-team, RADE_ACCOUNT=acme-team" \
    eventually 10 is "echo \$(penv $fc CLAUDE_CONFIG_DIR) \$(penv $fc RADE_ACCOUNT)" "$HOME/.claude-team acme-team"
check "R108-e the codex Member: CODEX_HOME=\$HOME/.codex, RADE_ACCOUNT=acme" \
    eventually 10 is "echo \$(penv $fx CODEX_HOME) \$(penv $fx RADE_ACCOUNT)" "$HOME/.codex acme"
check "R108-e pane.list carries the Members' accounts" \
    test "$(rade list --json | jq -r --arg a "$fc" --arg b "$fx" '[.panes[] | select(.pane == $a or .pane == $b) | .account] | join(",")')" = "acme-team,acme"
rade fanout wait "$FO" --source api --timeout 30 >/dev/null || true
fj=$(rade fanout judge "$FO" --source api --agent claude --json | jq -r .pane)
check "R108-e the Judge gets the Workspace default: RADE_ACCOUNT=acme-work, CLAUDE_CONFIG_DIR=\$HOME/.claude-work" \
    eventually 10 is "echo \$(penv $fj CLAUDE_CONFIG_DIR) \$(penv $fj RADE_ACCOUNT)" "$HOME/.claude-work acme-work"
rade fanout discard "$FO" --source api >/dev/null

echo "== R108-f / R108-k Session Restore after SIGKILL"
f1=$(rade spawn --agent claude --account acme-team --source api --tab new --name f1)
eventually 10 screen_has "$f1" 'fake-agent session' || true
# k: a Pane on acme-team where the Agent was started by hand under ~/.claude.
k1=$(rade spawn --agent claude --account acme-team --cmd bash --source api --tab new --name k1)
eventually 10 screen_has "$k1" '$' || true
rade send "$k1" "CLAUDE_CONFIG_DIR=\$HOME/.claude $FAKE" --enter >/dev/null
inv() { jq -c --arg p "$1" '.. | objects | select(.id? == $p) | .invocation' "$STATE"; }
export -f inv
check "R108-k the session-start hook recorded CLAUDE_CONFIG_DIR=\$HOME/.claude claude" \
    eventually 10 is "inv $k1" "{\"argv\":[\"$FAKE\"],\"env\":{\"CLAUDE_CONFIG_DIR\":\"$HOME/.claude\"}}"
check "R108-f state.json records account acme-team (f1, k1)" eventually 10 is "echo \$(jq -r --arg a $f1 --arg b $k1 '[.. | objects | select(.id? == \$a or .id? == \$b) | .account] | join(\",\")' '$STATE')" "acme-team,acme-team"
# The env Panes are Agent Panes of their Preset; they'd restart as it.
for p in "$d1" "$d2" "$l1" "$e2" "$e3"; do rade close --force "$p" >/dev/null 2>&1 || true; done
sleep 2 # saved (≤ 5 s, R11.1)
argv0=$(grep -c "^$k1 " "$FAKE_ARGV" || true)
kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=
start_app
check "R108-f the restored Pane has RADE_ACCOUNT=acme-team" eventually 10 is "penv $f1 RADE_ACCOUNT" acme-team
check "R108-f state.json still records account acme-team for it" is "acct_of $f1" acme-team
check "R108-k state.json still records account acme-team for the refused Pane" is "acct_of $k1" acme-team
check "R108-k the Pane is a plain shell that printed the message naming the Preset and acme-team" \
    eventually 10 bash -c "rade read $k1 --scrollback | grep -F 'not resumed' | grep -F \"preset 'claude'\" | grep -qF \"account 'acme-team'\""
check "R108-k the recorded invocation was not run (fake Agent argv log unchanged: $argv0 start)" \
    test "$(grep -c "^$k1 " "$FAKE_ARGV" || true)" = "$argv0" -a "$argv0" = 1
check "R108-k it is no longer an Agent Pane (plain shell)" \
    test "$(rade list --json | jq -r --arg p "$k1" '.panes[] | select(.pane == $p) | "\(.agent)"')" = null
rade focus "$k1" >/dev/null
sleep 0.5
shot m38-restored
rade send "$k1" env --enter >/dev/null
check "R108-k env in the restored Pane: CLAUDE_CONFIG_DIR=\$HOME/.claude-team, RADE_ACCOUNT=acme-team" \
    eventually 10 bash -c "line_is $k1 'CLAUDE_CONFIG_DIR=$HOME/.claude-team' && line_is $k1 RADE_ACCOUNT=acme-team"

echo "== R108-i the + menu (spec F)"
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
TOKENS=$ROOT/crates/rade-app/src/tokens.toml
nm() { rpc debug.ui "{\"op\":\"new_menu\"${1:+, $1}}"; }
export -f nm
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
WEB=$A/shop-web
mkdir -p "$WEB" && G -C "$WEB" init -q -b develop && echo web >"$WEB/README.md" && G -C "$WEB" add -A && G -C "$WEB" commit -qm init
rade source add "$WEB" --workspace acme >/dev/null
rade project add Shop --source shop-web:develop >/dev/null
rade worktree add shop-web checkout-v2 --project shop --json >/dev/null
s1=$(rade spawn --source shop-web --worktree checkout-v2 --tab new --name sh1)
rade focus "$s1" >/dev/null
sleep 1
prim0=$(rpc debug.ui '{"op":"geometry","widget":"suggested-action","all":true}' | jq length)
nm '"arg":"open"' >/dev/null
check "R108-i the menu opens under the titlebar + with the header 'New in Shop / shop-web / checkout-v2'" \
    test "$(nm | jq -r '"\(.new_menu.open) \(.new_menu.header)"')" = "true New in Shop / shop-web / checkout-v2"
check "R108-i rows: kind name detail default locked sensitive" \
    test "$(nm | jq -c '[.new_menu.rows[] | "\(.kind)|\(.name)|\(.detail)|\(.default)|\(.locked)|\(.sensitive)"]')" = \
    '["shell|Shell|null|false|false|true","account|acme-work|no data yet|true|false|true","account|acme-team|no data yet|false|false|true","account|personal|other workspace|false|true|false","account|acme|no data yet|true|false|true","account|personal|other workspace|false|true|false","fan_out|Fan-out…|one prompt, several agents|false|false|true","browser|Browser|null|false|false|true"]'
check "R108-i the keyboard starts on the default acme-work; ↓ skips nothing sensitive, ↓↓ skips the locked personal" \
    bash -c "[[ \$(nm | jq .new_menu.selected) == 1 ]] && nm '\"arg\":\"key\",\"key\":\"down\"' >/dev/null && [[ \$(nm | jq .new_menu.selected) == 2 ]] && nm '\"arg\":\"key\",\"key\":\"down\"' >/dev/null && [[ \$(nm | jq .new_menu.selected) == 4 ]] && nm '\"arg\":\"key\",\"key\":\"up\"' >/dev/null && [[ \$(nm | jq .new_menu.selected) == 2 ]]"
nm '"arg":"key","key":"up"' >/dev/null
spec_f() { python3 "$HERE/spec_f.py" "$SOCK" "$TOKENS" "$1" "New in Shop / shop-web / checkout-v2" "$prim0" >"$OUT/spec-f-$1.txt" 2>&1; }
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    nm '"arg":"open"' >/dev/null; sleep 0.3
    check "R108-i $th: spec F rows F.1–F.7, F.11–F.14, F.21 within tolerance" spec_f "$th"
    sed 's/^/    /' "$OUT/spec-f-$th.txt"
    shot "m38-new-menu-$th"
done
rade debug action color_scheme_dark >/dev/null; sleep 0.5
check "R108-i Esc closes the menu" bash -c "nm '\"arg\":\"key\",\"key\":\"escape\"' >/dev/null && [[ \$(nm | jq .new_menu.open) == false ]]"
nm '"arg":"open_tab"' >/dev/null
check "R108-i the tab strip's + opens the same menu" test "$(nm | jq -r '.new_menu.open')" = true
before=$(rade list --all --json | jq -r '[.panes[].pane] | sort | join(" ")')
n0=$(npanes)
nm '"arg":"activate","name":"personal","agent":"claude"' | jq -r '"\(.activated) \(.new_menu.open)"' >"$T/act1"
check "R108-i activating the locked personal row does nothing (menu stays open) and creates no Pane" \
    test "$(cat "$T/act1") $(npanes)" = "false true $n0"
nm '"arg":"activate","name":"acme-team","agent":"claude"' | jq -r '"\(.activated) \(.new_menu.open)"' >"$T/act2"
check "R108-i activating acme-team closes the menu and adds one Pane" \
    test "$(cat "$T/act2") $(npanes)" = "true false $((n0 + 1))"
new=$(rade list --all --json | jq -r --arg b " $before " '[.panes[].pane | . as $id | select(($b | contains(" " + $id + " ")) | not)][0]')
check "R108-i the new Claude Pane ($new) has RADE_ACCOUNT=acme-team and acme-team's CLAUDE_CONFIG_DIR" \
    eventually 10 is "echo \$(penv $new RADE_ACCOUNT) \$(penv $new CLAUDE_CONFIG_DIR)" "acme-team $HOME/.claude-team"
CV=$(rade worktree list shop-web --json 2>/dev/null | jq -r '.. | objects | select(.name? == "checkout-v2" or .worktree? == "checkout-v2") | .path' | head -1)
got=$(rade list --all --json | jq -r --arg p "$new" '.panes[] | select(.pane == $p) | "\(.agent != null) \(.account) \(.cwd)"')
echo "    new Pane: $got"
check "R108-i it is an Agent Pane on account acme-team in checkout-v2" test "$got" = "true acme-team $CV"
shot m38-new-menu-spawned

echo
echo "m38: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
