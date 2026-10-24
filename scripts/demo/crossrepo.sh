#!/usr/bin/env bash
# Demo scene: one feature across two repositories of Project Shop — a
# Worktree `guest-checkout` in shop-web and one in shop-api, an Agent in
# each, then Changes by Project with one group per link. Stills.
# Usage: scripts/demo/crossrepo.sh (NO_BUILD=1)
set -uo pipefail
NAME=crossrepo
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade worktree add shop-web guest-checkout --project shop >/dev/null
rade worktree add shop-api guest-checkout --project shop >/dev/null
web=$(rade spawn --source shop-web --worktree guest-checkout --agent claude --name web --tab new "Build the guest checkout form" --json | id)
api=$(rade spawn --source shop-api --worktree guest-checkout --agent codex --name api --tab new "Add the guest endpoint for orders" --json | id)
eventually 60 is "pane_status $web" done
eventually 60 is "pane_status $api" done
rade focus "$web" >/dev/null
rade debug action show_changes >/dev/null
sleep 3.5
notoasts
still crossrepo
rade focus "$api" >/dev/null
sleep 2
notoasts
still crossrepo-api
