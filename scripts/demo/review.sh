#!/usr/bin/env bash
# Demo scene: the Review Loop — an Agent finishes the coupon work in a
# Worktree, you comment on two lines, Request changes (Round 1), it answers,
# you read Since round 1, Approve ▾. Stills: review, review-round2,
# review-approve; recording review.mp4/.gif of the round trip.
# Two passes over the same story: stills (worktree `coupons`), then film
# (worktree `promo-codes`), so the film never flips the color scheme.
# Usage: scripts/demo/review.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=review
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

# The Agent's plays: the Review round's answer must win over 20-tests / 40-coupon
# (a round prompt quotes code), so it is copied in first.
PLAYS=$T/plays
mkdir -p "$PLAYS" && cp "$DEMO"/plays/*.play "$PLAYS/" && cp "$DEMO/plays/50-review.play" "$PLAYS/00-review.play"
# "Approved (review round n). Commit these changes…" — sorts before 00-review, which would match it too.
cat >"$PLAYS/00-approve.play" <<'P'
match: Approved
think 1
tool Bash git status --short
tool Bash git log --oneline develop..HEAD
out 3f9c2d1 refactor(cart): coupon table
out a81e07b feat(cart): coupon codes
say Everything is already committed in two commits. Not pushed.
done
P
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' DEMO_PLAYS=$PLAYS $AGENT"
C

spawn() { rade spawn "$@" --json | id; }
rv_do() { rade debug review "$rv" "$@" >/dev/null; }
rinfo() { rade debug review "$rv" info --json; }
agent_status() { rade list --json | jq -r --arg p "$ag" '.panes[] | select(.pane==$p) | .status'; }
FIRST='const pct = code === "WELCOME10" ? 10 : 0;'
SECOND='placeholder="Coupon code"'

# prep <worktree>: the Agent adds the coupon codes in a new Worktree.
prep() {
    rade worktree add shop-web "$1" --project shop >/dev/null
    ag=$(spawn --source shop-web --worktree "$1" --agent claude --name coupons --tab new "Add coupon codes to the cart")
    eventually 60 is agent_status done
    sleep 1
}

# round_trip <worktree> still|film: Review, two comments, Request changes,
# the Agent answers, Since round 1, Approve.
round_trip() {
    local wt=$1 mode=$2 f=0
    [[ $mode == film ]] && f=1
    rv=$(rade review open --source shop-web --worktree "$wt")
    eventually 30 is "rinfo | jq -r '.files | length'" 3
    notoasts
    sleep $((f * 1))
    rv_do select src/cart/useCart.ts
    sleep 0.8
    rv_do comment "$FIRST|$FIRST|Codes will multiply. Move them into a table."
    sleep 1.5
    rv_do select src/cart/CouponField.tsx
    sleep 1
    rv_do comment "$SECOND|$SECOND|Screen readers need an aria-label here."
    sleep 1.5
    rv_do select src/cart/useCart.ts
    sleep 1
    if [[ $mode == still ]]; then notoasts; still review; fi
    sleep $((f * 1))
    sleep 1.1   # the Agent's turn ended in an earlier second than the Round
    rv_do send
    eventually 20 is "rinfo | jq -r .head.round" "Round 1 · changes requested"
    [[ $mode == film ]] && rade focus "$ag" >/dev/null   # watch it work
    eventually 60 is "agent_status" running
    eventually 60 is "agent_status" done
    # The Review Toast of the finished Agent → Review, Since round 1.
    sleep $((1 + f))
    rpc debug.ui '{"op":"toast_review"}' >/dev/null
    eventually 20 is "rinfo | jq -r '.head.scope_since | startswith(\"Since round 1\")'" true
    rv_do select src/cart/useCart.ts
    rv_do refresh
    sleep 1.5
    if [[ $mode == still ]]; then notoasts; still review-round2; fi
    sleep $((f * 2))
    # Approve ▾ → Ask Agent to commit (its popover is a grab-less popup surface the
    # headless compositor dismisses at once, so the still shows the state after it).
    sleep 1.1
    rv_do approve ask_commit
    eventually 20 is "rinfo | jq -r .head.round" "Round 2 · approved"
    if [[ $mode == still ]]; then
        eventually 60 is "agent_status" running
        eventually 60 is "agent_status" done
        sleep 1
        notoasts
        rv_do refresh
        sleep 1.5
        still review-approve
    else
        eventually 60 is "agent_status" running
        eventually 60 is "agent_status" done
        notoasts
        sleep 1.5
    fi
}

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register

echo "== pass 1: stills"
prep coupons
round_trip coupons still
rade worktree remove shop-web coupons --force --delete-branch >/dev/null

echo "== pass 2: recording"
prep promo-codes
rade focus "$ag" >/dev/null
sleep 1
rec_start review
round_trip promo-codes film
rec_stop
