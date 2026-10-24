#!/usr/bin/env bash
# Demo scene: the tour — three Workspaces (acme, a client globex, personal),
# each with its own Projects and Agents at work. Switch between them, see
# the `!` of a hidden Agent that needs you, open Agent Overview over all of
# them with its Preview, jump to that Agent, answer it, then read a file from
# the Inspector's Files. Recordings (each also a part of tour.mp4/.gif):
# workspaces, overview, files. Stills: tour, files-viewer, overview-preview.
# Usage: scripts/demo/tour.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=tour
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
plays_first 95-busy 96-tenant 97-short
more_world

key() { rpc debug.ui "{\"op\": \"key\", \"keys\": \"$1\"}" >/dev/null; }
ov() { rpc debug.ui "{\"op\": \"overview\", \"arg\": \"${1:-state}\"}"; }
spawn() { rade spawn "$@" --json | id; }
shown() { rade debug state | jq -r '.active_workspace // .workspace // empty'; }

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
more_register

echo "== Agents in three Workspaces"
rade workspace switch acme >/dev/null
rade worktree add shop-web checkout-v2 --project shop >/dev/null
rade worktree add billing invoices --project billing >/dev/null
plan=$(spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new "Plan the guest checkout flow")
tests=$(spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab current --split right "Snapshot the cart totals")
recon=$(spawn --source billing --worktree invoices --agent claude --name reconcile --tab new "Reconcile the September invoice totals")
rade workspace switch globex >/dev/null
rade worktree add portal-web sso --project portal >/dev/null
sso=$(spawn --source portal-web --worktree sso --agent claude --name sso --tab new "Add SSO to the portal")
rel=$(spawn --source mobile --agent codex --name notes --tab new "Write the 3.2 release notes")
rade workspace switch personal >/dev/null
post=$(spawn --source blog --agent claude --name post --tab new "Draft the launch post")
nvim=$(spawn --source dotfiles --agent codex --name nvim --tab new "Clean up the nvim config")
rade workspace switch acme >/dev/null
rade focus "$plan" >/dev/null
eventually 30 is "pane_status $plan" done
eventually 30 screen_has "$nvim" "written up"
eventually 30 screen_has "$rel" "written up"
sleep 2
notoasts
still tour

echo "== recording: workspaces"
rec_start workspaces
beat intro 1.5
rade workspace switch globex >/dev/null; rade focus "$sso" >/dev/null
# The client's API audit starts; it will ask while globex is hidden.
audit=$(spawn --source portal-api --agent codex --name audit --tab new "Check every route for the tenant guard")
rade focus "$sso" >/dev/null
beat globex 3
rade workspace switch personal >/dev/null; rade focus "$post" >/dev/null
beat personal 3
rade workspace switch acme >/dev/null; rade focus "$plan" >/dev/null
eventually 30 screen_has "$audit" "Patch both"; sleep 0.5
beat needs 3                              # the Toast and the switcher's `!`
rec_stop
echo "shown: $(rade debug state | jq -c '[.active_source]')"

echo "== still: the Overview with the waiting Agent's Preview"
notoasts
key ctrl+shift+space; sleep 1.5
ov | jq -c '[.preview.shown, .preview.text[-1]]'
still overview-preview
key escape; sleep 1

echo "== recording: overview"
notoasts
rec_start overview
sleep 0.8
key ctrl+shift+space; beat open 1.5   # open: audit (Needs you) is selected
beat preview 1.5                       # its Preview
ov | jq -c '[.preview, [.sections[] | [.title, [.cards[].name]]]]'
key ctrl+shift+space; beat next 2.5    # next Agent, its Preview
key ctrl+shift+space; sleep 2.5        # and the next
key alt+g; beat group 2.5              # by Workspace
key alt+g; sleep 1
for i in 1 2 3 4 5 6 7 8; do
    [[ $(ov | jq -r '[.sections[].cards[] | select(.selected)][0].name') == audit ]] && break
    key ctrl+shift+space; sleep 0.7
done
sleep 1.5
key enter; beat jump 2             # → globex, the audit Agent
mark answer
typein "$audit" "yes" --enter
eventually 30 is "pane_status $audit" done
sleep 1.5
rec_stop

echo "== recording: files"
notoasts
rec_start files
sleep 0.8
rade debug click "$audit" "src/routes/exports.ts:3" >/dev/null   # Ctrl+click in the Agent's output
beat click 2.5
rade debug action toggle_file_tree >/dev/null; sleep 0.3
ui right_page files >/dev/null
API=$G/portal-api
rade debug tree expand "$API/src" >/dev/null; sleep 0.5
rade debug tree expand "$API/src/routes" >/dev/null
beat tree 2.5
rade debug tree activate "$API/README.md" >/dev/null
beat readme 4
rec_stop
notoasts
still files-viewer


echo "== tour.mp4 = workspaces + overview + files"
printf "file '%s'\n" "$OUT/workspaces.mp4" "$OUT/overview.mp4" "$OUT/files.mp4" >"$OUT/tour.txt"
ffmpeg -loglevel error -y -f concat -safe 0 -i "$OUT/tour.txt" -c copy "$OUT/tour.mp4"
ffmpeg -loglevel error -y -i "$OUT/tour.mp4" \
    -vf "fps=8,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff:max_colors=128[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
    "$OUT/tour.gif"
ls -la "$OUT"/*.mp4 "$OUT"/*.gif | awk '{print "  " $5, $9}'
