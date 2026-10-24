#!/usr/bin/env bash
# Demo scene: Agent Accounts and Usage — the Usage page, the `+` menu with
# each account's figures, the limit forecast Toast. Stills only.
# Usage: scripts/demo/usage.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=usage
source "$(dirname "$0")/lib.sh"
export XDG_STATE_HOME=$T/state
mkdir -p "$XDG_STATE_HOME"
source "$DEMO/world.sh"
USAGE=$XDG_STATE_HOME/rade-dev/usage
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
P=$HOME/workspaces/personal
nm() { rpc debug.ui "{\"op\":\"new_menu\"${1:+, $1}}"; }
chrome() { ui chrome | jq -c .chrome; }
loaded() { [[ $(ui usage | jq -r '.usage | "\(.shown) \(.loading)"') == "true false" ]]; }
export -f nm chrome loaded
# Scene E's usage data, re-centred on now (5-hour limit forecast in ~40m).
scene() { python3 "$ROOT/tests/e2e/usage-scene.py" "$USAGE" "$HOME" >/dev/null; }

# Real agent names, no "(demo)".
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' $AGENT"
[agents.codex]
command = "env DEMO_AGENT=codex DEMO_TITLE='Codex (demo)' $AGENT"
C

# ---- Accounts: acme (2 claude + codex), personal (claude + codex) ---------------
for d in .claude-work .claude-team .codex .claude .codex-personal; do mkdir -p "$HOME/$d"; done
cat >>"$A/.rade/workspace.toml" <<'TOML'

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-work"
default = true

[[account]]
agent = "claude"
name = "acme-team"
config_dir = "~/.claude-team"

[[account]]
agent = "codex"
name = "acme"
config_dir = "~/.codex"
default = true
TOML
repo "$P/notes"
commit "$P/notes" "docs: reading list" README.md='# notes\n\nReading list and scratch ideas.\n'
mkdir -p "$P/.rade"
cat >"$P/.rade/workspace.toml" <<'TOML'
name = "personal"

[[account]]
agent = "claude"
name = "personal"
config_dir = "~/.claude"

[[account]]
agent = "codex"
name = "personal"
config_dir = "~/.codex-personal"
TOML
scene

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
rade workspace add "$P" >/dev/null
rade source add "$P/notes" --workspace personal >/dev/null
acme_register
WT=$(rade worktree add shop-api retry --project shop --json | jq -r .path)
dev=$(rade spawn --source shop-api --worktree retry --cmd "$DEMO/devserver.sh 3001" --name api-dev --tab new --json | id)
pl=$(rade spawn --source shop-api --worktree retry --agent claude --name payments --tab new "Add retry with backoff to charge()" --json | id)
eventually 40 is "pane_status $pl" done || true
sleep 1.5
rade focus "$pl" >/dev/null
[[ $(chrome | jq -r .inspector.shown) == true ]] && rpc debug.ui '{"op":"key","keys":"ctrl+shift+e"}' >/dev/null
rpc usage.refresh '{"workspace":"acme"}' >/dev/null
eventually 10 bash -c "[[ \$(rpc debug.ui '{\"op\":\"chrome\"}' | jq '.chrome.forecast_toasts | length') -ge 1 ]]" || true
notoasts

# ---- usage: the Usage page --------------------------------------------------------
rpc debug.ui '{"op":"click","widget":"tb-button-usage"}' >/dev/null
eventually 10 loaded || true
ui usage "filter all" >/dev/null
eventually 10 loaded || true
ui usage "select claude acme-work acme" >/dev/null
ui usage "period 7d" >/dev/null
sleep 1.5
notoasts
still usage
ui usage close >/dev/null
sleep 0.5

# ---- new-menu: the + menu with accounts and figures ----------------------------
rade focus "$pl" >/dev/null
scene; rpc usage.refresh '{"workspace":"acme"}' >/dev/null
nm '"arg":"open"' >/dev/null
sleep 1
notoasts
still new-menu
nm '"arg":"close"' >/dev/null
sleep 0.5

# ---- forecast: the limit forecast Toast -------------------------------------------
scene
rpc debug.ui '{"op":"forecast_check","arg":"reset"}' >/dev/null
eventually 10 bash -c "[[ \$(rpc debug.ui '{\"op\":\"chrome\"}' | jq '.chrome.forecast_toasts | length') -ge 1 ]]" || true
sleep 1.5
still forecast
