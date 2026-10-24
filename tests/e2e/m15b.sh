#!/usr/bin/env bash
# M15b end-to-end tests (ADR-0006, ADR-0016): Workspace setup (R25),
# doctor (R26), the rade-workspace skill (R27), all in a temp HOME with
# fake `gh`, `ssh-keygen` and `ssh` first on PATH (no network, no real
# keys). Every `rade workspace` call has stdin </dev/null and a 60 s cap.
# Usage: tests/e2e/m15b.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m15b
source "$HERE/lib.sh"

mkdir -p "$T/bin" "$T/remotes"
ln -s "$HERE/fake-gh.sh" "$T/bin/gh"
ln -s "$HERE/fake-ssh-keygen.sh" "$T/bin/ssh-keygen"
ln -s "$HERE/fake-ssh.sh" "$T/bin/ssh"
export PATH=$T/bin:$PATH FAKE_GH_LOG=$T/gh.log FAKE_GH_REMOTES=$T/remotes \
    FAKE_SSH_KEYGEN_LOG=$T/keygen.log FAKE_SSH_LOG=$T/ssh.log
unset GH_CONFIG_DIR GH_TOKEN GIT_CONFIG_GLOBAL
: >"$FAKE_GH_LOG"; : >"$FAKE_SSH_KEYGEN_LOG"; : >"$FAKE_SSH_LOG"
# A login shell reads .bash_profile; it sources .bashrc. No history file:
# the doctor/import shell probe must leave HOME as it was.
printf 'source ~/.bashrc\n' >"$HOME/.bash_profile"
printf 'PS1="$ "\nunset HISTFILE\n' >"$HOME/.bashrc"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err" </dev/null; local c=$?; set -e; echo $c; }
ws() { timeout 60 rade workspace "$@" </dev/null; }
# Every file and symlink under HOME with its mode and content hash.
snap() { (cd "$HOME" && find . \( -type f -o -type l \) -printf '%p %m %l\n' | sort && find . -type f -exec sha256sum {} + | sort); }
bare() { # name: a bare remote with one commit on main
    G init -q --bare "$T/remotes/$1.git"
    G clone -q "$T/remotes/$1.git" "$T/seed-$1" 2>/dev/null
    G -C "$T/seed-$1" commit -q --allow-empty -m first
    G -C "$T/seed-$1" push -q origin HEAD:main 2>/dev/null
    G --git-dir="$T/remotes/$1.git" symbolic-ref HEAD refs/heads/main
}
bare api; bare web
export FAKE_GH_LOGIN=ann-acme   # also for the palette's setup Pane (R25-i)
start_app

A=$HOME/workspaces/acme
SETUP=(setup "$A" --git-name "Ann Acme" --email ann@acme.test --github ann-acme --claude separate --default-owner acme-org)
export FAKE_GH_LOGIN=ann-acme
# The user's ~/.gitconfig is a symlink into dotfiles, mode 640 (R25-d).
mkdir -p "$HOME/dotfiles"
printf '[user]\n\tname = Global\n' >"$HOME/dotfiles/gitconfig"
chmod 640 "$HOME/dotfiles/gitconfig"
ln -s dotfiles/gitconfig "$HOME/.gitconfig"

echo "== R25-c --dry-run changes nothing"
snap >"$T/snap0"
check "dry run exits 0" test "$(code ws "${SETUP[@]}" --dry-run)" = 0
cp "$T/out" "$OUT/setup-dry-run.txt"
check "prints the workspace.toml diff" grep -q "== create $A/.rade/workspace.toml" "$T/out"
check "prints the ~/.gitconfig block" grep -q "# >>> rade workspace acme >>>" "$T/out"
check "prints the planned ssh-keygen" grep -q "^would run: ssh-keygen .* -f $HOME/.ssh/id_ed25519_acme" "$T/out"
check "prints the planned gh login" grep -q "^would run: GH_CONFIG_DIR=$HOME/.config/gh-acme gh auth login .*--insecure-storage" "$T/out"
check "says nothing was done" grep -q "(dry run: nothing written, nothing run)" "$T/out"
snap >"$T/snap1"
check "HOME byte-identical (sha256 snapshot)" cmp -s "$T/snap0" "$T/snap1"
check "no ssh-keygen ran" test ! -s "$FAKE_SSH_KEYGEN_LOG"
check "no mutating gh call" bash -c "! grep -qE 'insecure-storage|ssh-key add' '$FAKE_GH_LOG'"

echo "== R25-a setup on an empty HOME"
check "setup exits 0" test "$(code ws "${SETUP[@]}")" = 0
cp "$T/out" "$OUT/setup.txt"
TOML=$A/.rade/workspace.toml
check "workspace.toml: [identity]" bash -c "grep -qx 'git_email = \"ann@acme.test\"' '$TOML' && grep -qx 'github = \"ann-acme\"' '$TOML' && grep -qx 'ssh_key = \"id_ed25519_acme\"' '$TOML' && grep -qx 'sign = \"ssh\"' '$TOML'"
check "workspace.toml: [env] dirs, portable ~" bash -c "grep -qx 'GH_CONFIG_DIR = \"~/.config/gh-acme\"' '$TOML' && grep -qx 'CLAUDE_CONFIG_DIR = \"~/.claude-acme\"' '$TOML'"
check ".rade and .worktrees created" test -d "$A/.worktrees"
gc() { git config --file "$A/.gitconfig" --get "$1"; }
check "<root>/.gitconfig: name, email" test "$(gc user.name)/$(gc user.email)" = "Ann Acme/ann@acme.test"
check "<root>/.gitconfig: sshCommand" test "$(gc core.sshCommand)" = "ssh -i $HOME/.ssh/id_ed25519_acme -o IdentitiesOnly=yes"
check "<root>/.gitconfig: ssh signing" test "$(gc gpg.format) $(gc user.signingkey) $(gc commit.gpgsign)" = "ssh $HOME/.ssh/id_ed25519_acme.pub true"
check "one marked block in ~/.gitconfig" test "$(grep -c '^# >>> rade workspace acme >>>$' "$HOME/.gitconfig")" = 1
check "… with includeIf for the root" grep -qF '[includeIf "gitdir:~/workspaces/acme/"]' "$HOME/.gitconfig"
check "key pair, private 600" test "$(stat -c %a "$HOME/.ssh/id_ed25519_acme")" = 600 -a -f "$HOME/.ssh/id_ed25519_acme.pub"
check "gh login in the Workspace's GH_CONFIG_DIR, --insecure-storage" grep -q "^GH_CONFIG_DIR=$HOME/.config/gh-acme auth login --hostname github.com --git-protocol ssh --web --insecure-storage --scopes admin:public_key,admin:ssh_signing_key" "$FAKE_GH_LOG"
PUB=$(cut -d' ' -f1,2 "$HOME/.ssh/id_ed25519_acme.pub")
check "key added for authentication" grep -qxF "$PUB" "$HOME/.config/gh-acme/keys-authentication"
check "key added for signing" grep -qxF "$PUB" "$HOME/.config/gh-acme/keys-signing"
check "registered with the app" test "$(rade workspace list --json | jq -r '.workspaces[] | select(.workspace=="acme") | .identity.github')" = ann-acme
check "clone a Source" test "$(code ws clone --workspace acme api)" = 0
check "git user.email inside it = the Workspace's" test "$(git -C "$A/api" config user.email)" = ann@acme.test
check "… and it signs with the Workspace key" test "$(git -C "$A/api" config user.signingkey)" = "$HOME/.ssh/id_ed25519_acme.pub"

echo "== R25-d backups, symlink, mode"
check "~/.gitconfig still a symlink" test -L "$HOME/.gitconfig"
check "its target got the block" grep -q "rade workspace acme" "$HOME/dotfiles/gitconfig"
check "target mode kept (640)" test "$(stat -c %a "$HOME/dotfiles/gitconfig")" = 640
check "backup of the old content" bash -c "f=\$(ls $HOME/.gitconfig.rade-bak-*) && grep -q 'name = Global' \$f && ! grep -q rade \$f"

echo "== R25-b a second run has nothing to do"
baks=$(find "$HOME" -name '*.rade-bak-*' | wc -l)
logins=$(grep -c "auth login --hostname" "$FAKE_GH_LOG")
adds=$(grep -c "ssh-key add" "$FAKE_GH_LOG")
check "second run exits 0" test "$(code ws "${SETUP[@]}")" = 0
check "prints 'nothing to do'" grep -qx "nothing to do" "$T/out"
check "no new backups" test "$(find "$HOME" -name '*.rade-bak-*' | wc -l)" = "$baks"
check "no second login" test "$(grep -c "auth login --hostname" "$FAKE_GH_LOG")" = "$logins"
check "no second key add" test "$(grep -c "ssh-key add" "$FAKE_GH_LOG")" = "$adds"
check "one ssh-keygen in all" test "$(wc -l <"$FAKE_SSH_KEYGEN_LOG")" = 1
check "a conflicting flag → USAGE" test "$(code ws setup "$A" --email other@acme.test)" = 2
check "… naming the key" grep -q "git_email" "$T/err"
check "a git checkout as root → USAGE" test "$(code ws setup "$A/api" --git-name x --email x@y --github x)" = 2
check "… that's a Source" grep -q "that's a Source" "$T/err"

echo "== R25-e an existing includeIf is respected; a moved root replaces the block"
B=$HOME/workspaces/bravo
printf '[includeIf "gitdir/i:%s/"]\n\tpath = ~/.gitconfig-bravo\n' "$B" >>"$HOME/.gitconfig"
check "bravo setup" test "$(code ws setup "$B" --git-name Bo --email bo@bravo.test --github bo-bravo --env "CLAUDE_CONFIG_DIR=$T/elsewhere/claude")" = 0
check "no marked block for bravo" bash -c "! grep -q 'rade workspace bravo' '$HOME/.gitconfig'"
D1=$HOME/workspaces/delta-old D2=$HOME/workspaces/delta
check "delta setup at its first root" test "$(code ws setup "$D1" --name delta --git-name D --email d@delta.test --github d-delta)" = 0
mv "$D1" "$D2"
check "delta setup at the moved root" test "$(code ws setup "$D2" --git-name D --email d@delta.test --github d-delta)" = 0
check "still one delta block" test "$(grep -c '^# >>> rade workspace delta >>>$' "$HOME/.gitconfig")" = 1
check "… now for the new root only" bash -c "grep -qF 'gitdir:~/workspaces/delta/\"' '$HOME/.gitconfig' && ! grep -q delta-old '$HOME/.gitconfig'"
check "acme's block untouched" test "$(grep -c '^# >>> rade workspace acme >>>$' "$HOME/.gitconfig")" = 1
Fx=$HOME/workspaces/foxtrot
printf '[includeIf "gitdir:%s"]\n\tpath = ~/.gitconfig-foxtrot\n' "$Fx" >>"$HOME/.gitconfig"
check "foxtrot setup (its includeIf lacks the trailing /: no repo under the root matches)" test "$(code ws setup "$Fx" --git-name F --email f@fox.test --github f-fox)" = 0
check "… so it gets a marked block" grep -q '^# >>> rade workspace foxtrot >>>$' "$HOME/.gitconfig"

echo "== R25-a the key goes only to the Workspace's own account; a hung gh stops setup"
mkdir -p "$HOME/.config/gh-golf" && echo '{"login": "someone"}' >"$HOME/.config/gh-golf/login"
: >"$FAKE_GH_LOG"
check "setup with gh logged in as another account exits 0" test "$(code ws setup "$HOME/workspaces/golf" --git-name Gi --email g@golf.test --github g-golf)" = 0
check "… warns" grep -q "logged in as 'someone'.*not added" "$T/out"
check "… adds no key to it" bash -c "! grep -q 'ssh-key add' '$FAKE_GH_LOG'"
mkdir -p "$T/hung" && printf '#!/bin/sh\nexec sleep 300\n' >"$T/hung/gh" && chmod +x "$T/hung/gh"
t0=$(date +%s)
check "a gh that never answers: setup exits 2" test "$(PATH=$T/hung:$PATH code ws setup "$HOME/workspaces/hotel" --git-name H --email h@hotel.test --github h-hotel)" = 2
check "… within the 20 s probe limit" test $(( $(date +%s) - t0 )) -lt 40
check "… saying gh timed out" grep -q 'gh api user.*timed out' "$T/err"

echo "== R25-h hooks into a Workspace's own Claude dir, from both setups"
check "workspace setup installed them" grep -q "rade status" "$T/elsewhere/claude/settings.json"
rm -rf "$T/elsewhere"
rade setup --agent claude >/dev/null </dev/null
check "rade setup installs them again (registered Workspace's [env])" grep -q "rade status" "$T/elsewhere/claude/settings.json"
check "… and the skills" test -f "$T/elsewhere/claude/skills/rade-workspace/SKILL.md"

echo "== R25-f import the owner's layout"
V=$HOME/workspaces/globex
mkdir -p "$V" "$HOME/.ssh" "$HOME/.config/gh-globex" "$HOME/.claude-work"
printf 'Host github-globex\n\tHostName github.com\n\tIdentityFile ~/.ssh/id_ed25519_globex\n' >"$HOME/.ssh/config"
FAKE_SSH_KEYGEN_LOG=/dev/null ssh-keygen -f "$HOME/.ssh/id_ed25519_globex" -C rich@globex.test
printf '[includeIf "gitdir:~/workspaces/globex/"]\n\tpath = ~/.gitconfig-globex\n' >>"$HOME/.gitconfig"
cat >"$HOME/.gitconfig-globex" <<'EOF'
[user]
	name = Rich V
	email = rich@globex.test
	signingkey = ~/.ssh/id_ed25519_globex.pub
[gpg]
	format = ssh
[commit]
	gpgsign = true
[url "github-globex:"]
	insteadOf = git@github.com:
EOF
echo '{"login": "rich-globex"}' >"$HOME/.config/gh-globex/login"
cat >>"$HOME/.bashrc" <<'EOF'
cd() { builtin cd "$@" || return; case $PWD in "$HOME"/workspaces/globex*) export GH_CONFIG_DIR=$HOME/.config/gh-globex CLAUDE_CONFIG_DIR=$HOME/.claude-work ;; esac; }
EOF
G init -q -b main "$V/app" && G -C "$V/app" remote add origin github-globex:globex-ai/app.git
G init -q -b main "$V/lib" && G -C "$V/lib" remote add origin git@github.com:globex-ai/lib.git
snap >"$T/snap0"
check "import --dry-run exits 0" test "$(code ws import "$V" --dry-run)" = 0
snap >"$T/snap1"
check "… changes nothing" cmp -s "$T/snap0" "$T/snap1"
check "import exits 0" test "$(code ws import "$V")" = 0
cp "$T/out" "$OUT/import.txt"
VT=$V/.rade/workspace.toml
for line in 'name = "globex"' 'git_name = "Rich V"' 'git_email = "rich@globex.test"' 'github = "rich-globex"' \
    'ssh_key = "id_ed25519_globex"' 'sign = "ssh"' 'GH_CONFIG_DIR = "~/.config/gh-globex"' \
    'CLAUDE_CONFIG_DIR = "~/.claude-work"' 'default_owner = "globex-ai"'; do
    check "toml: $line" grep -qxF "$line" "$VT"
done
snap >"$T/snap2"
check "only workspace.toml changed in HOME" test "$(diff "$T/snap1" "$T/snap2" | grep '^>' | grep -v 'workspaces/globex/.rade' | wc -l)" = 0
check "registered" test "$(rade workspace list --json | jq -r '.workspaces[] | select(.workspace=="globex") | .identity.ssh_key')" = id_ed25519_globex
check "second import: nothing to do" test "$(code ws import "$V")" = 0 -a "$(cat "$T/out")" = "nothing to do"
snap >"$T/snap3"
check "… HOME unchanged" cmp -s "$T/snap2" "$T/snap3"
check "a Source as root → USAGE" test "$(code ws import "$V/app")" = 2
check "ssh -G ran for the alias" grep -qx -- "-G github-globex" "$FAKE_SSH_LOG"

echo "== R25-g clone: one new, one there, one failing"
: >"$FAKE_GH_LOG"
check "exit 2" test "$(code ws clone --workspace acme api web nope)" = 2
cp "$T/out" "$OUT/clone.txt"
check "summary" grep -qx "cloned 1, skipped 1, failed 1" "$T/out"
check "web cloned" test -d "$A/web/.git"
check "nope reported" grep -q "nope" "$T/err"
check "gh ran with the Workspace's GH_CONFIG_DIR and GIT_SSH_COMMAND" grep -qxF "GH_CONFIG_DIR=$HOME/.config/gh-acme GIT_SSH_COMMAND=ssh -i $HOME/.ssh/id_ed25519_acme -o IdentitiesOnly=yes repo clone acme-org/web $A/web" "$FAKE_GH_LOG"
check "no clone for api (already there)" bash -c "! grep -q 'clone acme-org/api' '$FAKE_GH_LOG'"
check "web registered in acme" test "$(rade source list --workspace acme --json | jq -r '[.sources[].source] | sort | join(",")')" = api,general@acme,web

echo "== R26-a doctor: healthy exits 0; each check fails with its fix"
mkdir -p "$HOME/.claude-acme"
echo '{"oauthAccount": {"emailAddress": "ann@acme.test"}}' >"$HOME/.claude-acme/.claude.json"
check "healthy acme: exit 0" test "$(code ws doctor acme)" = 0
cp "$T/out" "$OUT/doctor-healthy.txt"
check "12 checks ok" test "$(grep -c '^ok ' "$T/out")" = 12
check "--json ok" test "$(timeout 60 rade --json workspace doctor "$A" </dev/null | jq -c '[.ok, (.workspaces[0].checks | length)]')" = '[true,12]'
broken() { # check-name, break-cmd, restore-cmd, [level]
    local name=$1 level=${4:-FAIL} want=1
    [[ $level == warn ]] && want=0
    eval "$2"
    local c; c=$(code ws doctor "$A")
    cp "$T/out" "$OUT/doctor-$(tr ' ' - <<<"$name").txt"
    check "$name: $level, exit $want" bash -c "[[ $c == $want ]] && grep -q '^$level $name: ' '$T/out'"
    check "$name: fix line" bash -c "grep -A1 '^$level $name: ' '$T/out' | grep -q '^  fix: .'"
    eval "$3"
}
broken root "mv '$A/.rade/workspace.toml' '$T/wt' && echo 'bad = [' >'$A/.rade/workspace.toml'" "mv '$T/wt' '$A/.rade/workspace.toml'"
broken tools "export FAKE_GH_VERSION=2.30.0" "unset FAKE_GH_VERSION"
broken "git identity" "git config --file '$A/.gitconfig' user.email wrong@x.test" "git config --file '$A/.gitconfig' user.email ann@acme.test"
broken signing "git config --file '$A/.gitconfig' commit.gpgsign false" "git config --file '$A/.gitconfig' commit.gpgsign true"
broken "ssh key" "chmod 644 '$HOME/.ssh/id_ed25519_acme'" "chmod 600 '$HOME/.ssh/id_ed25519_acme'"
broken "gh account" "cp '$HOME/.config/gh-acme/login' '$T/login' && echo '{\"login\": \"someone\"}' >'$HOME/.config/gh-acme/login'" "cp '$T/login' '$HOME/.config/gh-acme/login'"
broken "gh token" "cp '$HOME/.config/gh-acme/hosts.yml' '$T/hosts' && sed -i /oauth_token/d '$HOME/.config/gh-acme/hosts.yml'" "cp '$T/hosts' '$HOME/.config/gh-acme/hosts.yml'" warn
broken "keys on GitHub" "cp '$HOME/.config/gh-acme/keys-signing' '$T/ks' && : >'$HOME/.config/gh-acme/keys-signing'" "cp '$T/ks' '$HOME/.config/gh-acme/keys-signing'"
check "… its fix adds the signing key" grep -q "fix: GH_CONFIG_DIR=.* gh ssh-key add .*--type signing" "$OUT/doctor-keys-on-GitHub.txt"
broken ssh "cp '$HOME/.config/gh-acme/keys-authentication' '$T/ka' && : >'$HOME/.config/gh-acme/keys-authentication'" "cp '$T/ka' '$HOME/.config/gh-acme/keys-authentication'"
broken claude "mv '$HOME/.claude-acme/.claude.json' '$T/cj'" "mv '$T/cj' '$HOME/.claude-acme/.claude.json'"
broken registered "export RADE_SOCKET=$T/nope.sock" "unset RADE_SOCKET" warn
check "a Workspace without [identity]: one warning" bash -c "mkdir -p '$HOME/workspaces/bare/.rade' && echo 'name = \"bare\"' >'$HOME/workspaces/bare/.rade/workspace.toml' && rade workspace doctor '$HOME/workspaces/bare' </dev/null | grep -q '^warn identity: no \[identity\]'"
check "healthy again" test "$(code ws doctor acme)" = 0

echo "== R26-b an rc file exporting another GH_CONFIG_DIR → warning naming it"
cp "$HOME/.bashrc" "$T/bashrc"
echo 'export GH_CONFIG_DIR=$HOME/.config/gh' >>"$HOME/.bashrc"
n=$(wc -l <"$HOME/.bashrc")
check "exit 0 (a warning)" test "$(code ws doctor acme)" = 0
cp "$T/out" "$OUT/doctor-shell.txt"
check "warns on the variable and the rc line" grep -q "^warn shell: GH_CONFIG_DIR becomes $HOME/.config/gh (set in .*$HOME/.bashrc:$n)" "$T/out"
check "… with a grep hint" grep -q "^  fix: grep -n GH_CONFIG_DIR .*\.bashrc" "$T/out"
cp "$T/bashrc" "$HOME/.bashrc"
echo 'read -r x; sleep 30' >>"$HOME/.bashrc"
t0=$(date +%s)
check "a hanging rc file: warning, no hang" bash -c "timeout 60 rade workspace doctor acme </dev/null | grep -q '^warn shell: .*timed out'"
check "… within the 5 s probe limit" test $(( $(date +%s) - t0 )) -lt 20
cp "$T/bashrc" "$HOME/.bashrc"

echo "== R27-a the skill: installed, rade commands only"
SK=$HOME/.claude/skills/rade-workspace/SKILL.md
check "installed by rade setup" test -f "$SK"
check "names setup --dry-run, doctor, spawn for the login" bash -c "grep -q 'rade workspace setup' '$SK' && grep -q -- '--dry-run' '$SK' && grep -q 'rade workspace doctor' '$SK' && grep -q 'rade spawn --cmd' '$SK'"
check "no hand edits in its commands" bash -c "! grep -E '^(git config --global [^-]|ssh-keygen|gh auth login|gh ssh-key|echo .*>>)' '$SK'"

echo "== R25-i Create Workspace… palette form opens a setup Pane"
E=$HOME/workspaces/echo
rade debug action command_palette >/dev/null
rade debug palette --ui type ">create workspace" >/dev/null
F=$(rade debug palette --ui key enter)
check "form opened" test "$(jq -r .form.title <<<"$F")" = "Create Workspace"
rade debug palette --ui field "root=$E" >/dev/null
rade debug palette --ui field "git_name=Eve E" >/dev/null
rade debug palette --ui field "email=eve@echo.test" >/dev/null
rade debug palette --ui field "github=ann-acme" >/dev/null
rade debug palette --ui field "claude=Separate (~/.claude-<name>)" >/dev/null
shot palette-create-workspace
before=$(rade list --all --json | jq '.panes | length')
rade debug palette --ui key enter >/dev/null
check "a Pane named 'workspace setup' opened" eventually 5 is "rade list --all --json | jq -r '.panes[-1].name'" "workspace setup"
P=$(rade list --all --json | jq -r '.panes[-1].pane')
check "… one more Pane" test "$(rade list --all --json | jq '.panes | length')" = $((before + 1))
check "… in General" test "$(rade list --all --json | jq -r --arg p "$P" '.panes[] | select(.pane==$p) | .source')" = general
WANT="rade workspace setup '$E' --git-name 'Eve E' --email 'eve@echo.test' --github 'ann-acme' --claude separate"
check "its command line is the setup command" eventually 10 is "rade debug state | jq -r --arg p $P '.. | objects | select(.id? == \$p) | .kind.command // empty'" "$WANT"
check "setup ran there: Workspace created" eventually 30 test -f "$E/.rade/workspace.toml"
check "… with the form's email" eventually 5 grep -qx 'git_email = "eve@echo.test"' "$E/.rade/workspace.toml"
check "… to the end: registered" eventually 30 screen_has "$P" "registered: workspace 'echo'"
shot palette-setup-pane

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
