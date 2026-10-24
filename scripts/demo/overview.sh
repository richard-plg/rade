#!/usr/bin/env bash
# Demo scene: Agent Overview (Ctrl+Shift+Space) over two Workspaces — eight
# Agents needing you, finished, running and failed. Stills: overview,
# overview-by-workspace; recording overview.mp4/.gif: open, move the selection,
# Enter jumps to an Agent in the other Workspace.
# Usage: scripts/demo/overview.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=overview
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

key() { rpc debug.ui "{\"op\": \"key\", \"keys\": \"$1\"}" >/dev/null; }
ov() { rpc debug.ui "{\"op\": \"overview\", \"arg\": \"${1:-state}\"}"; }
spawn() { rade spawn "$@" --json | id; }

# A second Workspace with a repo of its own.
P=$HOME/workspaces/personal
mkdir -p "$P/.rade" && printf 'name = "personal"\ncolor = "purple"\n' >"$P/.rade/workspace.toml"
BLOG=$P/blog
mkdir -p "$BLOG" && git -C "$BLOG" init -q
git init -q --bare "$REM/blog.git" && git -C "$BLOG" remote add origin "$REM/blog.git"
commit "$BLOG" "chore: scaffold the blog" \
    package.json='{\n  "name": "blog",\n  "private": true,\n  "scripts": { "build": "eleventy" }\n}\n' \
    README.md='# blog\n\nNotes on Rust, GTK and tooling.\n' \
    src/feed.ts='export const feed = { title: "Sam Rivera", posts: [] as string[] };\n' \
    posts/2026-09-rust-notes.md='# Rust notes\n\nA few things I learned this month.\n' \
    posts/2026-10-gtk-tips.md='# GTK tips\n\nSmall things that save time.\n'
git -C "$BLOG" push -qu origin main 2>/dev/null

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade workspace add "$P" >/dev/null
rade source add "$BLOG" --workspace personal >/dev/null
mkdir -p "$API/db/migrations"
rade workspace switch acme >/dev/null
rade worktree add shop-web checkout-v2 --project shop >/dev/null
rade worktree add shared-ui tokens --project billing >/dev/null

echo "== Agents"
tests=$(spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab new "Write tests for the cart totals")
ui=$(spawn --source shared-ui --worktree tokens --agent claude --name ui --tab new "Wire the spacing scale into Button")
migrate=$(spawn --source shop-api --agent claude --name migrate --tab new "Run the orders migration on staging")
recon=$(spawn --source billing --agent claude --name reconcile --tab new "Reconcile the September invoice totals")
rade workspace switch personal >/dev/null
series=$(spawn --source blog --agent claude --name series --tab new "Outline the launch post series")
draft=$(spawn --source blog --agent claude --name launch-post --tab new "Draft the launch post")
rss=$(spawn --source blog --agent codex --name feed --tab new "Add an RSS feed")
links=$(spawn --source blog --agent codex --name links --tab new "Find broken links in the built site")
rade workspace switch acme >/dev/null
eventually 60 is "pane_status $series" done
eventually 60 is "pane_status $tests" waiting
eventually 60 is "pane_status $migrate" error
eventually 60 is "pane_status $recon" waiting
eventually 60 is "pane_status $rss" waiting
eventually 30 is "pane_status $draft" running
eventually 30 is "pane_status $links" running
eventually 30 is "pane_status $ui" running
# Leave a shell in front, like real work.
sleep 2
home=$(spawn --source shop-web --name shell --tab new)
sleep 1
notoasts
ov open >/dev/null; sleep 1; ov | jq -c '[.count, [.sections[] | [.title, [.cards[].name]]]]'
ov | jq -c '.sections[].cards[] | [.name, .tail]'
key escape; sleep 0.5

echo "== stills"
open_ov() { ov open >/dev/null; sleep 1; key alt+p; sleep 0.5; }
open_ov
ov | jq -c '[.preview.shown, [.sections[].cards[] | select(.selected) | [.name, .workspace]]]'
dark; shot overview-dark; light; shot overview-light; dark
key alt+g; sleep 0.6
dark; shot overview-by-workspace-dark; light; shot overview-by-workspace-light; dark
key alt+g; key escape; sleep 0.5

echo "== recording"
key alt+p   # Previews back on for the film
rec_start overview
sleep 1
key ctrl+shift+space          # open
sleep 3
key ctrl+shift+space; sleep 2.2   # next Agent
key alt+g; sleep 2.5              # by Workspace
key alt+g; sleep 1.5              # by status
# on to the Agent in the other Workspace that waits for you
for i in 1 2 3 4 5 6; do
    [[ $(ov | jq -r '[.sections[].cards[] | select(.selected)][0].name') == feed ]] && break
    key ctrl+shift+space; sleep 1.5
done
sleep 2.5
key enter
sleep 3
rec_stop
echo "shown: $(rade debug state | jq -r '.active_source')"
