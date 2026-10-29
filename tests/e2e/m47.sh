#!/usr/bin/env bash
# M47 end-to-end tests (ADR-0047): Agent Accounts with env, user accounts,
# detection, Settings → Accounts (PRD R125, R126).
# A temp HOME with user accounts in $XDG_CONFIG_HOME/rade/accounts.toml:
# `cl` (claude ~/.claude, default) and `clw` (claude ~/.claude-work, env_file
# ~/.claude-5-token.env holding CLAUDE_CODE_OAUTH_TOKEN); Workspace `acme`
# (own account acme-work, default) and `corp` (accounts = "own", own
# corp-work); Source `notes` in the default Workspace. The fake Agent
# (tests/e2e/fake-agent.sh) is the `claude` Preset's command and names it in
# its session-start hook. Screenshots → target/e2e-m47/.
# Usage: tests/e2e/m47.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m47
export WESTON_WIDTH=1440
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
export FAKE_AGENT=claude
mkdir -p "$XDG_CONFIG_HOME/rade"
printf 'idle_seconds = 1\n\n[agents.claude]\ncommand = "%s"\n\n[agents.codex]\ncommand = "%s"\n' "$FAKE" "$FAKE" \
    >"$XDG_CONFIG_HOME/rade/config.toml"
ACC=$XDG_CONFIG_HOME/rade/accounts.toml
TOKEN=tok-e2e-5f1c0ffee
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
npanes() { rade list --all --json | jq '.panes | length'; }
line_is() { rade read "$1" --scrollback | sed 's/[[:space:]]*$//' | grep -qxF -- "$2"; }
pane_field() { rade list --all --json | jq -r --arg p "$1" --arg f "$2" '.panes[] | select(.pane == $p) | .[$f]'; }
acct() { rpc debug.ui '{"op":"app_settings_accounts"'"${1:+, $1}"'}'; }
export -f line_is pane_field rpc acct npanes

# ---- The scene ----------------------------------------------------------------
A=$HOME/workspaces/acme C=$HOME/workspaces/corp N=$HOME/code/notes
for r in "$A/api" "$C/svc" "$N"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && echo base >"$r/README.md"
    G -C "$r" add -A && G -C "$r" commit -qm init
done
mkdir -p "$A/.rade" "$C/.rade" "$HOME/.claude" "$HOME/.claude-work" "$HOME/.claude-acme" "$HOME/.claude-corp"
printf '# the second login (claude setup-token)\nexport CLAUDE_CODE_OAUTH_TOKEN=%s\n' "$TOKEN" >"$HOME/.claude-5-token.env"
cat >"$ACC" <<'EOF'
# My logins, for every Workspace.
[[account]]
agent = "claude"
name = "cl"
config_dir = "~/.claude"
default = true

[[account]]
agent = "claude"
name = "clw"
config_dir = "~/.claude-work"
env_file = "~/.claude-5-token.env"
EOF
cat >"$A/.rade/workspace.toml" <<'EOF'
name = "acme"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-acme"
default = true
EOF
cat >"$C/.rade/workspace.toml" <<'EOF'
name = "corp"
accounts = "own"

[[account]]
agent = "claude"
name = "corp-work"
config_dir = "~/.claude-corp"
EOF

start_app
rade workspace add "$A" >/dev/null
rade workspace add "$C" >/dev/null
rade source add "$A/api" --workspace acme >/dev/null
rade source add "$C/svc" --workspace corp >/dev/null
rade source add "$N" --workspace default >/dev/null

echo "== R125-e user accounts in every Workspace, accounts = \"own\", defaults"
row() { jq -c '[.accounts[] | "\(.name):\(.default):\(.scope)"]'; }
check "R125-e acme lists acme-work (default), then the user's cl and clw" \
    test "$(rade account list --workspace acme --json | row)" = '["acme-work:true:workspace","cl:false:user","clw:false:user"]'
check "R125-e default lists cl (default) and clw, both user accounts" \
    test "$(rade account list --workspace default --json | row)" = '["cl:true:user","clw:false:user"]'
check "R125-e corp (accounts = \"own\") lists only corp-work" \
    test "$(rade account list --workspace corp --json | row)" = '["corp-work:true:workspace"]'
check "R125-e the list prints '(user)' after a user account" \
    bash -c "rade account list --workspace default | grep -qxF 'claude  clw  ~/.claude-work  (user)'"
check "R125-e account list --json shows clw's env file and its key, never the token" \
    bash -c "L=\$(rade account list --workspace default --json); [[ \$(jq -r '.accounts[1].env_file' <<<\"\$L\") == '$HOME/.claude-5-token.env' && \$(jq -c '.accounts[1].env_keys' <<<\"\$L\") == '[\"CLAUDE_CODE_OAUTH_TOKEN\"]' ]] && ! grep -qF '$TOKEN' <<<\"\$L\""
n0=$(npanes)
rc=$(code rade spawn --agent claude --account clw --cmd env --source svc --tab new)
check "R125-e --account clw in corp exits 7 naming clw and corp, no Pane" \
    bash -c "[[ $rc == 7 && \$(npanes) == $n0 ]] && grep -qF \"'clw'\" '$T/err' && grep -qF \"'corp'\" '$T/err'"
c1=$(rade spawn --agent claude --account clw --cmd env --source api --tab new --name c1)
check "R125-e --account clw in acme is allowed" eventually 10 line_is "$c1" RADE_ACCOUNT=clw
printf '[[account]]\nagent = "claude"\nname = "clw"\nconfig_dir = "~/.x"\n' >>"$A/.rade/workspace.toml"
rc=$(code rade account list --workspace acme --json)
check "R125-e one name in both files: an error naming both files" \
    bash -c "jq -r '.errors[0]' '$T/out' | grep -qF '$A/.rade/workspace.toml' && jq -r '.errors[0]' '$T/out' | grep -qF '$ACC' && jq -r '.errors[0]' '$T/out' | grep -qF clw"
sed -i '/^\[\[account\]\]$/{N;N;N;/name = "clw"/d}' "$A/.rade/workspace.toml"
check "R125-e back to one clw (acme lists 3 again)" \
    test "$(rade account list --workspace acme --json | jq '.accounts | length')" = 3

cp "$ACC" "$T/acc-pre"
rc=$(code rade account add --user --agent claude --name acme-work --config-dir '~/.q')
check "R125-e add --user of a name acme declares exits 2 naming acme's file, accounts.toml unchanged" \
    bash -c "[[ $rc == 2 ]] && grep -qF '$A/.rade/workspace.toml' '$T/err' && cmp -s '$T/acc-pre' '$ACC'"
rc=$(code rade account add --workspace acme --agent claude --name cl --config-dir '~/.q')
check "R125-e add of a user account's name to acme exits 2 naming accounts.toml" \
    bash -c "[[ $rc == 2 ]] && grep -qF '$ACC' '$T/err' && ! grep -q '~/.q' '$A/.rade/workspace.toml'"
rc=$(code rade account set clw --user --workspace corp --env BAR=1)
check "R125-e set --user edits clw from corp (accounts = \"own\") in accounts.toml" \
    bash -c "[[ $rc == 0 && \$(cat '$T/out') == '$ACC' ]] && grep -qxF 'BAR = \"1\"' '$ACC'"
rade account set clw --user --unset-env BAR >/dev/null
check "R125-e --unset-env BAR puts accounts.toml back byte for byte" cmp -s "$T/acc-pre" "$ACC"

echo "== R125-j a Workspace's [env] login stays its default"
V=$HOME/workspaces/viact
mkdir -p "$V/svc2" "$V/.rade" "$HOME/.claude-vm25" && G -C "$V/svc2" init -q -b main && echo b >"$V/svc2/README.md" && G -C "$V/svc2" add -A && G -C "$V/svc2" commit -qm init
printf 'name = "viact"\n\n[env]\nCLAUDE_CONFIG_DIR = "~/.claude-work"\n' >"$V/.rade/workspace.toml"
printf '\n[[account]]\nagent = "claude"\nname = "clv"\nconfig_dir = "~/.claude-vm25"\n' >>"$ACC"
rade workspace add "$V" >/dev/null
rade source add "$V/svc2" --workspace viact >/dev/null
check "R125-j viact (no account, [env] ~/.claude-work): clw is its default, not the user default cl" \
    test "$(rade account list --workspace viact --json | jq -r '[.accounts[] | select(.default) | .name] | join(",")')" = clw
j1=$(rade spawn --agent claude --cmd env --source svc2 --tab new --name j1)
check "R125-j an Agent spawned there without --account runs on clw" \
    eventually 10 bash -c "line_is $j1 RADE_ACCOUNT=clw && line_is $j1 'CLAUDE_CONFIG_DIR=$HOME/.claude-work'"
printf 'name = "viact"\n\n[env]\nCLAUDE_CONFIG_DIR = "~/.claude-other"\n' >"$V/.rade/workspace.toml"
j2=$(rade spawn --agent claude --cmd env --source svc2 --tab new --name j2)
check "R125-j [env] on a dir no account has: no account, the Pane keeps [env]" \
    eventually 10 bash -c "line_is $j2 'CLAUDE_CONFIG_DIR=$HOME/.claude-other' && ! rade read $j2 --scrollback | grep -q '^RADE_ACCOUNT='"
j3=$(rade spawn --agent claude --account clv --cmd env --source svc2 --tab new --name j3)
check "R125-j --account clv still works there" \
    eventually 10 bash -c "line_is $j3 RADE_ACCOUNT=clv && line_is $j3 'CLAUDE_CONFIG_DIR=$HOME/.claude-vm25'"
# The rest of the suite runs with the three user accounts of the scene.
sed -i '/^$/{N;/\n\[\[account\]\]$/{N;N;N;/name = "clv"/d}}' "$ACC"
for p in "$j1" "$j2" "$j3"; do rade close --force "$p" >/dev/null 2>&1 || true; done

echo "== R125-c the account's env file reaches the Pane, never state or answers"
p1=$(rade spawn --agent claude --account clw --cmd env --source notes --tab new --name p1)
eventually 10 screen_has "$p1" RADE_ACCOUNT= || true
check "R125-c the Pane has the token, CLAUDE_CONFIG_DIR=\$HOME/.claude-work, RADE_ACCOUNT=clw" \
    bash -c "line_is $p1 CLAUDE_CODE_OAUTH_TOKEN=$TOKEN && line_is $p1 'CLAUDE_CONFIG_DIR=$HOME/.claude-work' && line_is $p1 RADE_ACCOUNT=clw"
sleep 2 # saved (≤ 5 s, R11.1)
check "R125-c the token is in none of state.json, account list --json, debug state" \
    bash -c "! grep -qF '$TOKEN' '$STATE' && ! rade account list --workspace default --json | grep -qF '$TOKEN' && ! rade debug state | grep -qF '$TOKEN'"
rade close --force "$p1" >/dev/null

echo "== R125-d a missing env file refuses the spawn and the restore"
mv "$HOME/.claude-5-token.env" "$T/token.env"
n0=$(npanes)
rc=$(code rade spawn --agent claude --account clw --cmd env --source notes --tab new)
check "R125-d spawn exits 2 naming the file, no Pane" \
    bash -c "[[ $rc == 2 && \$(npanes) == $n0 ]] && grep -qF '$HOME/.claude-5-token.env' '$T/err'"
mv "$T/token.env" "$HOME/.claude-5-token.env"
d1=$(rade spawn --agent claude --account clw --source notes --tab new --name d1)
eventually 10 screen_has "$d1" 'fake-agent session' || true
sleep 2
mv "$HOME/.claude-5-token.env" "$T/token.env"
kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=
start_app
check "R125-d the restored Pane is a plain shell that printed 'rade: not resumed:' naming the file" \
    eventually 10 bash -c "rade read $d1 --scrollback | grep -F 'rade: not resumed:' | grep -qF '$HOME/.claude-5-token.env'"
check "R125-d it keeps its account" test "$(pane_field "$d1" account)" = clw
mv "$T/token.env" "$HOME/.claude-5-token.env"
d2=$(rade spawn --agent claude --account clw --cmd env --source notes --tab new --name d2)
check "R125-d with the file back, the next Pane has the token" \
    eventually 10 line_is "$d2" "CLAUDE_CODE_OAUTH_TOKEN=$TOKEN"

echo "== R125-f rade setup covers user accounts' config dirs"
rc=$(code rade setup --dry-run --agent claude)
check "R125-f the diff installs the hooks into ~/.claude-work" \
    bash -c "[[ $rc == 0 ]] && grep -qF '$HOME/.claude-work/settings.json' '$T/out' && grep -qF 'dry run: nothing written' '$T/out'"

echo "== R125-g a hand-started Agent's account is detected"
g1=$(rade spawn --source notes --tab new --name g1)
eventually 10 screen_has "$g1" '$' || true
check "R125-g a shell Pane starts without an account" test "$(pane_field "$g1" account)" = null
rade send "$g1" "CLAUDE_CONFIG_DIR=\$HOME/.claude-work $FAKE" --enter >/dev/null
check "R125-g the Agent reporting CLAUDE_CONFIG_DIR=~/.claude-work gives the Pane clw, detected" \
    eventually 10 is "echo \$(pane_field $g1 account) \$(pane_field $g1 account_detected)" "clw true"
check "R125-g state.json records account_detected for it" \
    eventually 10 is "jq -r --arg p $g1 '.. | objects | select(.id? == \$p) | \"\\(.account) \\(.account_detected)\"' '$STATE'" "clw true"
sub=$(RADE_PANE_ID=$g1 rade spawn --agent claude --cmd env --tab new --name g1-sub)
check "R125-g its Subagent inherits RADE_ACCOUNT=clw" eventually 10 line_is "$sub" RADE_ACCOUNT=clw
rade send "$g1" quit --enter >/dev/null
check "R125-g the Agent ends: the detected account is cleared" \
    eventually 10 is "pane_field $g1 account" null
rade send "$g1" "$FAKE" --enter >/dev/null
check "R125-g an Agent with no CLAUDE_CONFIG_DIR gets cl (~/.claude)" \
    eventually 10 is "echo \$(pane_field $g1 account) \$(pane_field $g1 account_detected)" "cl true"

echo "== R126-b Settings → Accounts (list + detail)"
rade workspace switch acme >/dev/null
rpc debug.ui '{"op":"app_settings_open"}' >/dev/null
sleep 0.5
ac() { acct "$1" >"$T/ac.json"; cat "$T/ac.json"; }
export -f ac
list() { acct | jq -c '[.list[] | "\(.name):\(.scope):\(.default):\(.badge)"]'; }
check "R126-b the list: cl, clw with .env · 1 var, then the locked acme-work ✓ (acme's own default wins)" \
    test "$(list)" = '["cl:user:false:","clw:user:false:.env · 1 var","acme-work:workspace:true:"]'
acct '"do":"select","agent":"claude","name":"clw"' >/dev/null
acct '"do":"scroll","y":40' >/dev/null
sleep 0.3
y0=$(acct | jq .scroll)
check "R126-b the page is scrolled to the section (scroll > 0: $y0)" bash -c "(( \$(printf '%.0f' $y0) > 0 ))"
acct '"do":"add_var","key":"FOO","value":"bar"' >/dev/null
sleep 0.3
check "R126-b clw's Environment: login, the file's token (from .env, no value), FOO" \
    test "$(acct | jq -c '[.env[] | "\(.key):\(.source):\(.overridden)"]')" = '["CLAUDE_CONFIG_DIR:login:false","CLAUDE_CODE_OAUTH_TOKEN:file:false","FOO:own:false"]'
check "R126-b adding FOO keeps the page's scroll position ($y0)" test "$(acct | jq .scroll)" = "$y0"
check "R126-b after a save the cursor is on + Add variable" \
    bash -c "acct | jq -r '.focus | join(\" \")' | grep -qF settings-account-add-var"
check "R126-b accounts.toml gains [account.env] FOO under clw, other bytes kept" \
    bash -c "grep -qx 'FOO = \"bar\"' '$ACC' && grep -qx '# My logins, for every Workspace.' '$ACC'"
check "R126-b the state never holds the token" bash -c "! acct | grep -qF '$TOKEN'"
cp "$ACC" "$T/acc-1"
acct '"do":"add_var","key":"RADE_X","value":"1"' >"$T/rade-x.json"
check "R126-b RADE_X shows an error, keeps the typed row, writes nothing" \
    bash -c "jq -r .error '$T/rade-x.json' | grep -qF RADE_X && [[ \$(jq -r .new_var '$T/rade-x.json') == RADE_X ]] && cmp -s '$T/acc-1' '$ACC'"
acct '"do":"escape"' >/dev/null
check "R126-b Esc drops the typed row (Settings stays open)" \
    bash -c "[[ \$(acct | jq -r .new_var) == null && \$(rpc debug.ui '{\"op\":\"app_settings\"}' | jq .open) == true ]]"
acct '"do":"paste","text":"# two\nA_ONE=1\nexport B_TWO=\"x y\"\n"' >/dev/null
check "R126-b pasting two KEY=VALUE lines writes both" \
    bash -c "grep -qx 'A_ONE = \"1\"' '$ACC' && grep -qx 'B_TWO = \"x y\"' '$ACC'"
check "R126-b the list badge counts them: .env · 4 vars" \
    test "$(acct | jq -r '.list[] | select(.name == "clw") | .badge')" = ".env · 4 vars"
cp "$ACC" "$T/acc-2"
acct '"do":"new","agent":"codex","name":"cx"' >/dev/null
check "R126-b + New account: a draft row, nothing written, cursor in its name" \
    bash -c "A=\$(acct); [[ \$(jq -r .draft.name <<<\"\$A\") == cx ]] && jq -r '.focus | join(\" \")' <<<\"\$A\" | grep -qF settings-account-draft-name && cmp -s '$T/acc-2' '$ACC'"
acct '"do":"create"' >/dev/null
check "R126-b Create writes cx (codex, default login: no config_dir) and selects it" \
    bash -c "A=\$(acct); [[ \$(jq -r .selected.name <<<\"\$A\") == cx && \$(jq -r '.list[] | select(.name == \"cx\") | .dir' <<<\"\$A\") == 'default login' ]] && tail -n 3 '$ACC' | tr '\n' '|' | grep -qF 'agent = \"codex\"|name = \"cx\"|'"
check "R126-b cx's login row is CODEX_HOME, empty" \
    test "$(acct | jq -c '[.env[] | "\(.key):\(.source)"]')" = '["CODEX_HOME:login"]'
w1=$(rade spawn --agent claude --account clw --cmd env --source api --tab new --name w1)
check "R126-b a new Pane on clw has FOO=bar and A_ONE=1" \
    eventually 10 bash -c "line_is $w1 FOO=bar && line_is $w1 A_ONE=1"
w2=$(rade spawn --agent codex --account cx --cmd env --source api --tab new --name w2)
check "R125-b a Pane on cx (default login) has no CODEX_HOME, RADE_ACCOUNT=cx" \
    eventually 10 bash -c "line_is $w2 RADE_ACCOUNT=cx && ! rade read $w2 --scrollback | grep -q '^CODEX_HOME='"
rpc debug.ui '{"op":"app_settings_open"}' >/dev/null
acct '"do":"select","agent":"claude","name":"clw"' >/dev/null
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    acct '"do":"scroll"' >/dev/null; sleep 0.4
    shot "m47-settings-accounts-$th"
done
rade debug action color_scheme_dark >/dev/null
printf 'nope\n' >"$HOME/bad.env"
acct '"do":"edit","agent":"claude","name":"cl","edit":{"env_file":"~/bad.env"}' >/dev/null
acct '"do":"select","agent":"claude","name":"cl"' >/dev/null
check "R126-b a .env with a bad line shows the file and line, not the line's text" \
    bash -c "A=\$(acct); jq -r .env_file.error <<<\"\$A\" | grep -qF '$HOME/bad.env:1' && ! jq -r .env_file.error <<<\"\$A\" | grep -q nope"
acct '"do":"select","agent":"claude","name":"acme-work"' >/dev/null
sleep 0.4
shot m47-settings-accounts-workspace
rpc debug.ui '{"op":"app_settings_close"}' >/dev/null

echo
echo "m47: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
