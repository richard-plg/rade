#!/usr/bin/env bash
# Demo scene: the main window — Shop's Navigator, three Agents at work (one
# needs you), the Inspector's Changes by Project. Stills + a recording of the
# Agents starting. Usage: scripts/demo/workspace.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=workspace
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
CV=$(rade worktree add shop-web checkout-v2 --project shop --json | jq -r .path)
TK=$(rade worktree add shared-ui tokens --project billing --json | jq -r .path)
r1=$(rade list --json | jq -r '[.panes[] | select(.source=="general@acme") | .pane][0] // empty')

dev=$(rade spawn --source shop-web --name dev-server --tab new --cmd "$DEMO/devserver.sh 5173" --json | id)
sleep 1
rec_start agents
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new "Plan the guest checkout flow" --json | id)
sleep 2.5
tests=$(rade spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab current --split right "Write tests for the cart totals" --json | id)
eventually 30 is "pane_status $tests" waiting
eventually 30 is "pane_status $planner" done
sleep 2
rec_stop
uip=$(rade spawn --source shared-ui --worktree tokens --agent claude --name ui --tab new "Add focus ring tokens" --json | id)
sleep 3
# Uncommitted work in checkout-v2 for the Changes page.
printf 'export const shipping = 499;\n' >"$CV/src/cart/shipping.ts"
echo "// guest checkout" >>"$CV/src/App.tsx"
rade focus "$tests" >/dev/null
rade debug action show_changes >/dev/null
sleep 3
notoasts
still workspace
