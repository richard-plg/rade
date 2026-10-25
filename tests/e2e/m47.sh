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

echo "== R126-b Settings → Accounts"
rade workspace switch acme >/dev/null
rpc debug.ui '{"op":"app_settings_open"}' >/dev/null
rpc debug.ui '{"op":"app_settings_search","arg":"accounts"}' >/dev/null
cards() { acct | jq -c '[.cards[] | "\(.name):\(.scope):\(.editable)"]'; }
check "R126-b cards: cl and clw editable (user), acme-work read-only (workspace)" \
    test "$(cards)" = '["cl:user:true","clw:user:true","acme-work:workspace:false"]'
check "R126-b clw's card shows 1 variable from its .env with its key and no value" \
    bash -c "C=\$(acct); [[ \$(jq -r '.cards[1].env_file_text' <<<\"\$C\") == '1 variable' && \$(jq -c '.cards[1].env_file_keys' <<<\"\$C\") == '[\"CLAUDE_CODE_OAUTH_TOKEN\"]' ]] && ! grep -qF '$TOKEN' <<<\"\$C\""
cp "$ACC" "$T/acc-0"
acct '"do":"edit","agent":"claude","name":"clw","edit":{"set_env":["FOO","bar"]}' >/dev/null
check "R126-b adding FOO=bar to clw writes [account.env] and keeps the rest of accounts.toml" \
    bash -c "head -c \$(stat -c %s '$T/acc-0') '$ACC' | cmp -s - '$T/acc-0' && tail -n 3 '$ACC' | tr '\n' '|' | grep -qF '[account.env]|FOO = \"bar\"|'"
printf 'TOK=x\n' >"$HOME/tok.env"
acct '"do":"edit","agent":"claude","name":"cl","edit":{"env_file":"~/tok.env"}' >/dev/null
check "R126-b loading ~/tok.env into cl: the card says 1 variable, key TOK, no value" \
    bash -c "C=\$(acct); [[ \$(jq -r '.cards[0].env_file_text' <<<\"\$C\") == '1 variable' && \$(jq -c '.cards[0].env_file_keys' <<<\"\$C\") == '[\"TOK\"]' ]] && ! jq -c '.cards' <<<\"\$C\" | grep -q '\"x\"'"
cp "$ACC" "$T/acc-1"
acct '"do":"edit","agent":"claude","name":"clw","edit":{"set_env":["RADE_X","1"]}' >"$T/rade-x.json"
check "R126-b a key RADE_X shows an error and writes nothing" \
    bash -c "jq -r .error '$T/rade-x.json' | grep -qF RADE_X && cmp -s '$T/acc-1' '$ACC'"
check "R126-b the page shows that error" \
    bash -c "rpc debug.ui '{\"op\":\"app_settings\"}' | jq -r .error | grep -qF RADE_X"
w1=$(rade spawn --agent claude --account clw --cmd env --source api --tab new --name w1)
w2=$(rade spawn --agent claude --account cl --cmd env --source api --tab new --name w2)
check "R126-b a new Pane on clw has FOO=bar; one on cl has TOK=x" \
    eventually 10 bash -c "line_is $w1 FOO=bar && line_is $w2 TOK=x"
rpc debug.ui '{"op":"app_settings_open"}' >/dev/null
rpc debug.ui '{"op":"app_settings_search","arg":"accounts"}' >/dev/null
acct >/dev/null
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    rpc debug.ui '{"op":"app_settings_search","arg":"accounts"}' >/dev/null; sleep 0.5
    shot "m47-settings-accounts-$th"
done
rade debug action color_scheme_dark >/dev/null
printf 'nope\n' >"$HOME/bad.env"
acct '"do":"edit","agent":"claude","name":"cl","edit":{"env_file":"~/bad.env"}' >/dev/null
check "R126-b a .env with a bad line shows the file and line on the card" \
    bash -c "acct | jq -r '.cards[0].env_file_text' | grep -qF '$HOME/bad.env:1'"
sleep 0.5
shot m47-settings-accounts-bad-env
rpc debug.ui '{"op":"app_settings_close"}' >/dev/null

echo
echo "m47: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
