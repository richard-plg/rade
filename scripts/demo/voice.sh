#!/usr/bin/env bash
# Demo scene: a Voice Prompt (R124) on an Agent Pane — the Voice Composer
# with what was said folded to one line and the drafted prompt, ready to send
# (spec I's scene c, filled by `debug.voice scene`). Stills: voice.
# Usage: scripts/demo/voice.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=voice
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade worktree add shop-web checkout-v2 --project shop >/dev/null

echo "== Agents"
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new \
    "Look at the checkout form, the totals look off after a promo code" --json | id)
eventually 60 is "pane_status $planner" done
rade focus "$planner" >/dev/null
notoasts

echo "== stills"
rpc debug.voice '{"op":"scene","arg":"c"}' >/dev/null; sleep 1
rpc debug.voice '{"op":"info"}' | jq -c '{state, label, time}'
dark; shot voice-dark; light; shot voice-light; dark
