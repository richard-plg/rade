#!/usr/bin/env bash
# Demo scene: Workspaces — acme (work) and personal, each with its own git
# identity, gh login and Claude config; the Workspace switcher popover, the
# Create Workspace… form and `rade workspace doctor` in a Terminal. Everything
# runs in a temp HOME with a fake gh / ssh / ssh-keygen (tests/e2e/fake-*.sh):
# no network, nothing touches the real ~/.ssh or gh login.
# Stills: switcher, create-workspace, doctor.
# Usage: NO_BUILD=1 scripts/demo/workspaces.sh   (KEEP=1 keeps the temp dir)
set -uo pipefail
NAME=workspaces
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
T0=$(date +%s)

# ---- Stand-ins for gh / ssh-keygen / ssh -------------------------------------------
E2E=$ROOT/tests/e2e
mkdir -p "$T/bin" "$REM"
ln -s "$E2E/fake-gh.sh" "$T/bin/gh"
ln -s "$E2E/fake-ssh-keygen.sh" "$T/bin/ssh-keygen"
ln -s "$E2E/fake-ssh.sh" "$T/bin/ssh"
export PATH=$T/bin:$PATH FAKE_GH_LOG=$T/gh.log FAKE_GH_REMOTES=$REM \
    FAKE_SSH_KEYGEN_LOG=$T/keygen.log FAKE_SSH_LOG=$T/ssh.log
unset GH_CONFIG_DIR GH_TOKEN GIT_CONFIG_GLOBAL
: >"$FAKE_GH_LOG"; : >"$FAKE_SSH_KEYGEN_LOG"; : >"$FAKE_SSH_LOG"
printf 'source ~/.bashrc\n' >"$HOME/.bash_profile"
echo 'unset HISTFILE' >>"$HOME/.bashrc"
# Paths in `rade workspace` output read as a normal home, not the temp HOME.
cat >>"$HOME/.bashrc" <<RC
rade() { if [[ \$1 == workspace ]]; then command rade "\$@" | sed "s|$HOME|/home/sam|g"; else command rade "\$@"; fi; }
RC
ws() { timeout 60 rade workspace "$@" </dev/null; }

# ---- The personal Workspace's repositories -------------------------------------------
P=$HOME/workspaces/personal
SITE=$P/site DOT=$P/dotfiles
repo "$SITE"
commit "$SITE" "chore: scaffold site" README.md='# site\n\nMy blog and notes.\n' \
    content/posts/2026-09-notes.md='# September notes\n\nWhat I read, what I built.\n' \
    config.toml='title = "Sam Rivera"\n'
git -C "$SITE" push -qu origin main 2>/dev/null
repo "$DOT"
commit "$DOT" "chore: shell and git config" README.md='# dotfiles\n' .zshrc='export EDITOR=nvim\n'
git -C "$DOT" push -qu origin main 2>/dev/null

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null

echo "== identities: acme (work) and personal"
FAKE_GH_LOGIN=sam-acme ws setup "$A" --git-name "Sam Rivera" --email sam@acme.dev --github sam-acme --claude separate >"$OUT/setup-acme.txt" 2>&1 || { cat "$OUT/setup-acme.txt"; exit 1; }
FAKE_GH_LOGIN=samrivera ws setup "$P" --name personal --git-name "Sam Rivera" --email sam.rivera@example.org --github samrivera --claude separate >"$OUT/setup-personal.txt" 2>&1 || { cat "$OUT/setup-personal.txt"; exit 1; }
for d in acme personal; do
    mkdir -p "$HOME/.claude-$d"
    case $d in acme) m=sam@acme.dev ;; *) m=sam.rivera@example.org ;; esac
    echo "{\"oauthAccount\": {\"emailAddress\": \"$m\"}}" >"$HOME/.claude-$d/.claude.json"
done
acme_register
rade workspace add "$P" >/dev/null 2>&1
for r in "$SITE" "$DOT"; do rade source add "$r" --workspace personal >/dev/null; done
rade workspace list --json | jq -r '.workspaces[] | "  \(.workspace) \(.name) \(.key)"'

echo "== Agents in both Workspaces"
rade workspace switch personal >/dev/null
blog=$(rade spawn --source site --agent claude --name blog --tab new "Write up the October notes" --json | id)
eventually 20 is "pane_status $blog" running
rade workspace switch acme >/dev/null
planner=$(rade spawn --source shop-web --agent claude --name planner --tab new "Plan the guest checkout flow" --json | id)
tests=$(rade spawn --source shop-web --agent codex --name tests --tab current --split right "Write tests for the cart totals" --json | id)
eventually 40 is "pane_status $tests" waiting
eventually 40 is "pane_status $planner" done
rade focus "$planner" >/dev/null
# acme has a prompt waiting; show personal, so the switcher carries the badge.
rade workspace switch personal >/dev/null
rade focus "$blog" >/dev/null
sleep 2
notoasts

echo "== switcher"
popstill() { # name: the window (dark + light) with the popover composited over it
    local th
    for th in dark light; do
        $th; sleep 0.4
        notoasts
        rade debug screenshot "$OUT/$1-win.png" >/dev/null
        rpc debug.ui "{\"op\": \"switcher\", \"arg\": \"shot\", \"path\": \"$OUT/$1-pop.png\"}" >/dev/null
        python3 -I - "$OUT/$1-win.png" "$OUT/$1-pop.png" "$OUT/$1-$th.png" <<'PY'
import sys
from PIL import Image
win, pop, out = (Image.open(sys.argv[1]).convert("RGBA"), Image.open(sys.argv[2]).convert("RGBA"), sys.argv[3])
x, y = 10, 46
win.alpha_composite(pop, (x, y))
win.convert("RGB").save(out)
print("  shot", out)
PY
    done
    dark
}
rpc debug.ui '{"op": "switcher", "arg": "open"}' >/dev/null
eventually 3 is "ui workspaces | jq -r .workspaces.open" true
sleep 0.6
popstill switcher
rpc debug.ui '{"op": "switcher", "arg": "close"}' >/dev/null

echo "== Create Workspace… form"
rade debug action command_palette >/dev/null
rade debug palette --ui type ">create workspace" >/dev/null
sleep 0.4
F=$(rade debug palette --ui key enter)
[[ $(jq -r .form.title <<<"$F") == "Create Workspace" ]] || { echo "form did not open"; exit 1; }
rade debug palette --ui field "root=~/workspaces/studio" >/dev/null
rade debug palette --ui field "git_name=Sam Rivera" >/dev/null
rade debug palette --ui field "email=sam@studio.dev" >/dev/null
rade debug palette --ui field "github=samrivera" >/dev/null
rade debug palette --ui field "claude=Separate (~/.claude-<name>)" >/dev/null
sleep 0.6
still create-workspace
rade debug palette --ui key escape >/dev/null
rpc debug.ui '{"op":"key","keys":"escape"}' >/dev/null

echo "== doctor"
# One honest warning, with its fix: personal's gh token is not stored in hosts.yml.
sed -i /oauth_token/d "$HOME/.config/gh-personal/hosts.yml"
rade workspace switch acme >/dev/null
sh_=$(rade spawn --source general@acme --tab new --json | id)
eventually 10 screen_has "$sh_" '$'
rade focus "$sh_" >/dev/null
sleep 0.5
typein "$sh_" "rade workspace doctor" --enter
eventually 60 screen_has "$sh_" "ssh"
sleep 4
rade read "$sh_" >"$OUT/doctor.txt"
notoasts
still doctor
echo "total $(( $(date +%s) - T0 )) s"
