#!/usr/bin/env bash
# Demo scene: the Agent Grid — one Claude Code Agent splits guest checkout
# across four Subagents (two Claude Code, two Codex) with `rade spawn`, they
# work side by side in its Tab (one asks a question), it waits for each and
# sums up. Still: grid; recording grid.mp4/.gif.
# Usage: scripts/demo/grid.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=grid
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
plays_first 90-team 91-form 92-validate 93-receipt 94-guesttests

# NARRATE=1: the Agents work slower, so the narration keeps up.
[[ -n ${NARRATE:-} ]] && export DEMO_SPEED=1.6
start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade worktree add shop-web guest-checkout --project shop >/dev/null
named() { rade list --json | jq -r --arg n "$1" '.panes[] | select(.name==$n) | .pane' | head -1; }
export -f named

rec_start grid
mark intro
lead=$(rade spawn --source shop-web --worktree guest-checkout --agent claude --name lead --tab new \
    "Split the guest checkout across four Subagents" --json | id)
rade focus "$lead" >/dev/null
eventually 60 bash -c "[[ -n \$(named form) ]]"
mark spawn
eventually 60 bash -c "[[ -n \$(named tests) ]]"
tests=$(named tests)
mark parallel
eventually 60 is "pane_status $tests" waiting
beat ask 3
mark answer
typein "$tests" "Playwright" --enter
rade focus "$lead" >/dev/null
mark waits
eventually 120 is "pane_status $lead" done
beat done 3
rec_stop
rade list --json | jq -r '.panes[] | "  \(.name) \(.status) parent=\(.parent // "-")"'
notoasts
still grid
