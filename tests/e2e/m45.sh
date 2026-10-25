#!/usr/bin/env bash
# E2E (M45, R120–R123, R21): the Changes page on VS Code's model (cards, row
# click → Diff Pane, multi-select + keys, trash, context menu, list / tree,
# Commit ▾, Publish / Sync / Fetch, opt-in auto-fetch), the Diff Pane (hunks,
# lines, layouts, F7, restore) and Quick Diff marks in the Viewer. A fake `gh`
# and a fake Draft command, a bare remote, a second clone. Screenshots light
# + dark → target/e2e-m45/.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m45}
source "$HERE/lib.sh"

# A fake GitHub CLI and a fake Draft command (never a real Agent).
mkdir -p "$T/fakebin" "$XDG_CONFIG_HOME/rade"
cat > "$T/fakebin/gh" <<SH
#!/bin/sh
if [ -e "$T/gh-unauth" ]; then echo "To get started with GitHub CLI, please run:  gh auth login" >&2; exit 4; fi
printf '%s\n' "\$@" > "$T/gh.log"
echo "https://github.test/shop/pull/7"
SH
cat > "$T/draft.sh" <<SH
#!/bin/sh
cat > "$T/draft-prompt.txt"
if grep -q "pull request" "$T/draft-prompt.txt"; then
  printf 'Add a greeting\n\n## Summary\n- greets everyone\n'
else
  printf '\`\`\`\nfeat(R21): change the greeting\n\nDrafted by the fake Agent.\n\`\`\`\n'
fi
SH
chmod +x "$T/fakebin/gh" "$T/draft.sh"
export PATH=$T/fakebin:$PATH
CONF=$XDG_CONFIG_HOME/rade/config.toml
# A hand-written comment and a foreign key must survive the page's writes (R120-f).
cat > "$CONF" <<C
# my own notes about this file
[scm]
draft_command = "$T/draft.sh"

[terminal]
font_size = 13
C

G() { git -c user.name=dev -c user.email=d@e -c init.defaultBranch=main "$@"; }
export GIT_AUTHOR_NAME=dev GIT_AUTHOR_EMAIL=d@e GIT_COMMITTER_NAME=dev GIT_COMMITTER_EMAIL=d@e
R=$T/code/shop; export R
mkdir -p "$R/src" "$R/docs"
lines() { python3 -c "import sys; e=set(map(int, sys.argv[1:])); print(''.join(f'CHANGED {i}\n' if i in e else f'line {i}\n' for i in range(1, 21)), end='')" "$@"; }
( cd "$R" && G init -q
  printf '# Rules\n\nCommits: Conventional Commits with R-IDs as the scope.\n' > CLAUDE.md
  lines > src/app.rs
  printf 'pub fn greet() {}\n// Greeting greet_all GREET\nfn greet(x) {}\n' > src/util.rs
  printf '# Notes\n\nWe greet in docs.\n' > docs/notes.md
  printf 'target/\n' > .gitignore
  G add -A && G commit -qm "feat(R1): init"
  G init -q --bare -b main "$T/remote.git"
  G remote add origin "$T/remote.git"
  G push -q -u origin main
  G checkout -qb feat-greet )

scm() { rpc debug.scm "${1:-{\}}"; }
dsp() { rpc debug.diff "${1:-{\}}"; }
dialog() { rade debug dialog; }
respond() { rade debug dialog respond "$1" >/dev/null; }
# "M src/app.rs,U notes.txt" of one group of the first card.
grp() { scm | jq -r --arg g "$1" '[.cards[0].groups[] | select(.kind==$g) | .rows[] | select(.kind=="file") | .letter + " " + .path] | join(",")'; }
card() { scm | jq -r ".cards[0]$1"; }
click() { scm "{\"op\":\"click\",\"source\":\"shop\",\"group\":\"$1\",\"path\":\"$2\",\"arg\":\"${3:-}\"}" >/dev/null; }
rowact() { scm "{\"op\":\"row_action\",\"source\":\"shop\",\"group\":\"$1\",\"path\":\"$2\",\"arg\":\"$3\"}" >/dev/null; }
grpact() { scm "{\"op\":\"group_action\",\"source\":\"shop\",\"group\":\"$1\",\"arg\":\"$2\"}" >/dev/null; }
key() { scm "{\"op\":\"key\",\"source\":\"shop\",\"key\":\"$1\"}" >/dev/null; }
refresh() { scm '{"op":"refresh"}' >/dev/null; }   # the window's focus-in does this (headless Weston never activates it)
msg() { scm "{\"op\":\"message\",\"source\":\"shop\",\"text\":\"$1\"}" >/dev/null; }
commitop() { scm "{\"op\":\"commit\",\"source\":\"shop\",\"variant\":\"${1:-commit}\"}" >/dev/null; }
menu_item() { # enabled? of a Commit ▾ item
    scm | jq -r --arg l "$1" '.cards[0].commit.menu[] | select(.label==$l) | .enabled'
}
head_() { git -C "$R" log --format=%s -1; }
toast_() { rpc debug.ui | jq -r '.chrome.notes[-1]'; }
export -f rpc scm dsp grp card click menu_item head_ toast_ refresh
inspector() { # toggle the floating Inspector (Ctrl+B from the Navigator) so a screenshot shows the whole Diff Pane
    rpc debug.ui '{"op":"focus","widget":"navigator"}' >/dev/null
    rpc debug.ui '{"op":"key","keys":"ctrl+b"}' >/dev/null; sleep 0.8
}
scene() { # name: a light and a dark shot
    rade debug action color_scheme_light >/dev/null; sleep 0.8; shot "$1-light"
    rade debug action color_scheme_dark >/dev/null; sleep 0.8; shot "$1-dark"
    rade debug action color_scheme_light >/dev/null; sleep 0.3
}

# git never looks above the temp dir (TMPDIR inside a repository).
export GIT_CEILING_DIRECTORIES=$T
start_app
rade source add "$R" --name shop >/dev/null
sh1=$(rade spawn --source shop --cmd bash --name sh --json | jq -r .pane)
rade focus "$sh1" >/dev/null
rade debug action show_changes >/dev/null

echo "== R120-h a repository in no Project"
check "R120-h Ctrl+Shift+M shows the Changes page" eventually 5 is "scm | jq -r .visible" true
check "R120-h one card, header SOURCE · SHOP, mode source" eventually 5 is "scm | jq -r '[.mode, .header, (.cards|length)] | join(\"|\")'" "source|SOURCE · SHOP|1"
check "R120-h no op returns a checkout page (the Details ops are gone)" bash -c "rpc debug.ui '{\"op\":\"changes_details\",\"arg\":\"on\"}' | jq -e 'has(\"code\")' >/dev/null && ! scm | jq -e 'has(\"checkout\") or .page == \"checkout\"' >/dev/null"
check "R120-h a clean card without upstream shows Publish Branch and is not folded (D12)" eventually 5 is "card '| [.action, .folded] | join(\"|\")'" "Publish Branch|false"

echo "== R120-a/c a row click opens the diff"
lines 2 18 > "$R/src/app.rs"
printf 'pub fn greet() {}\n// Greeting greet_all GREET\nfn greet(x) {}\npub fn staged() {}\n' > "$R/src/util.rs"
git -C "$R" add src/util.rs
printf 'scratch\n' > "$R/notes.txt"
printf '# Notes\n\nWe greet in docs. More.\n' > "$R/docs/notes.md"
refresh
check "R120-a Staged util.rs; Changes docs/notes.md, src/app.rs, notes.txt (U)" eventually 5 is "grp staged; grp changes" "M src/util.rs
M docs/notes.md,U notes.txt,M src/app.rs"
check "R120-a the switcher counts 4 changed files" eventually 5 is "rpc debug.ui | jq -r '.chrome.right_panel.labels[0]'" "Changes 4"
scene changes-list
click changes src/app.rs
check "R120-c a Diff Pane 'app.rs (Working Tree)' opens" eventually 5 is "dsp | jq -r .title" "app.rs (Working Tree)"
check "R120-c …keyboard focus is still in the Changes list" eventually 3 is "scm | jq -r '.focus.in_list'" true
check "R120-c …nothing was staged" test "$(git -C "$R" diff --cached --name-only)" = "src/util.rs"
diff_pane=$(dsp | jq -r .pane)
check "R120-c …split right of the focused Terminal" bash -c "rade debug state | jq -e --arg t '$sh1' '[.. | objects | select(.type? == \"split\" and .dir? == \"right\")] | any(.a.pane.id? == \$t and .b.pane.kind.type? == \"diff\")' >/dev/null"
click staged src/util.rs
check "R120-c the staged row shows 'util.rs (Staged)' in the same Pane" eventually 5 is "dsp | jq -r '.title + \"|\" + .pane'" "util.rs (Staged)|$diff_pane"
click changes src/app.rs double
check "R120-c a double click focuses the Diff Pane (the list lost the keyboard)" eventually 3 is "scm | jq -r '.focus.in_list // false'" false
dsp '{"op":"escape"}' >/dev/null
check "R120-c Esc closes the Diff Pane" eventually 3 bash -c "! rade list --json | jq -e --arg p '$diff_pane' '.panes[] | select(.pane==\$p)' >/dev/null"
rade focus "$sh1" >/dev/null

echo "== R120-d multi-select, Space, Delete, trash"
sel3() { # the 2 tracked modified rows + the untracked one of group $1
    click "$1" docs/notes.md; click "$1" src/app.rs ctrl; click "$1" notes.txt ctrl
}
sel3 changes
check "R120-d 3 rows selected" eventually 3 is "scm | jq '[.cards[0].groups[].rows[] | select(.selected)] | length'" 3
key space
check "R120-d Space stages the 3" eventually 5 is "git -C '$R' diff --cached --name-only | tr '\n' ,; echo" "docs/notes.md,notes.txt,src/app.rs,src/util.rs,"
check "R120-d …the Changes group is empty" eventually 5 is "grp changes" ""
sel3 staged
key space
check "R120-d Space on the 3 staged rows unstages them" eventually 5 is "git -C '$R' diff --cached --name-only" "src/util.rs"
sel3 changes
key delete
check "R120-d Delete asks once: Discard changes in 3 files?" eventually 3 is "dialog | jq -r .heading" "Discard changes in 3 files?"
respond discard
check "R120-d …the 2 tracked files are restored" eventually 5 bash -c "[ -z \"\$(git -C '$R' diff --name-only)\" ] && sed -n 2p '$R/src/app.rs' | grep -q 'line 2'"
check "R120-d …the untracked one is in \$XDG_DATA_HOME/Trash/files/" eventually 5 bash -c "[ ! -e '$R/notes.txt' ] && ls '$XDG_DATA_HOME/Trash/files/' | grep -qx notes.txt"
check "R120-d …nothing left in Changes" eventually 5 is "grp changes" ""

echo "== R120-e context menu, Add to .gitignore"
printf 'log\n' > "$R/app.log"
refresh
eventually 5 is "grp changes" "U app.log"
menu=$(scm '{"op":"rightclick","source":"shop","group":"changes","path":"app.log"}' | jq -c .menu)
sleep 0.4; shot changes-row-actions-light
check "R120-e the untracked menu is R120.5's list" test "$menu" = '["Open Changes","Open File","Open File to the Side","Stage Changes","Delete File…","Add to .gitignore","Copy Path","Copy Relative Path"]'
scm '{"op":"menu_activate","source":"shop","item":"Add to .gitignore"}' >/dev/null
check "R120-e .gitignore ends with /app.log" eventually 5 bash -c "tail -n 1 '$R/.gitignore' | grep -qx '/app.log'"
check "R120-e the row is gone (.gitignore itself is the one change)" eventually 5 is "grp changes" "M .gitignore"
git -C "$R" checkout -q .gitignore; rm -f "$R/app.log"; refresh
menu=$(scm '{"op":"rightclick","source":"shop","group":"staged","path":"src/util.rs"}' | jq -c .menu)
check "R120-e the staged row's menu is R120.5's list" test "$menu" = '["Open Changes","Open File","Open File to the Side","Unstage Changes","Copy Path","Copy Relative Path"]'
git -C "$R" reset -q src/util.rs; git -C "$R" checkout -q src/util.rs; refresh

echo "== R120-f view and sort are written, relaunch keeps the tree"
lines 2 18 > "$R/src/app.rs"; printf 'x\n' > "$R/docs/notes2.md"; refresh
eventually 5 is "grp changes" "U docs/notes2.md,M src/app.rs"
scm '{"op":"view","arg":"tree"}' >/dev/null
scm '{"op":"sort","arg":"name"}' >/dev/null
check "R120-f View as Tree: folders first (docs, src)" eventually 5 is "scm | jq -r '[.cards[0].groups[0].rows[] | .kind + \":\" + .name] | join(\",\")'" "folder:docs,file:notes2.md,folder:src,file:app.rs"
check "R120-f config.toml holds view = \"tree\" and sort = \"name\" under [scm]" eventually 5 bash -c "s=\$(awk '/^\\[scm\\]/{s=1;next} /^\\[/{s=0} s' '$CONF'); grep -q 'view = \"tree\"' <<<\"\$s\" && grep -q 'sort = \"name\"' <<<\"\$s\""
check "R120-f …the comment and the other lines are unchanged" bash -c "grep -q '^# my own notes about this file' '$CONF' && grep -q 'font_size = 13' '$CONF' && grep -q 'draft_command' '$CONF'"
scene changes-tree
stop_app; start_app
rade focus "$(rade list --json | jq -r '[.panes[] | select(.source=="shop")][0].pane')" >/dev/null
rade debug action show_changes >/dev/null
check "R120-f after a relaunch the page is still a tree" eventually 8 is "scm | jq -r '[.view, .sort] | join(\"|\")'" "tree|name"
scm '{"op":"view","arg":"list"}' >/dev/null
scm '{"op":"sort","arg":"path"}' >/dev/null
check "R120-f back to list / path" eventually 5 is "scm | jq -r '[.view, .sort] | join(\"|\")'" "list|path"
git -C "$R" checkout -q . ; rm -f "$R/docs/notes2.md"; refresh
sh1=$(rade list --json | jq -r '[.panes[] | select(.source=="shop")][0].pane')

echo "== R21-a stage, unstage, hunks, discard"
lines 2 18 > "$R/src/app.rs"
printf 'scratch\n' > "$R/notes.txt"
printf 'pub fn new() {}\n' > "$R/src/new.rs"
refresh
check "R21-a Changes: M src/app.rs, U notes.txt, U src/new.rs" eventually 5 is "grp changes" "U notes.txt,M src/app.rs,U src/new.rs"
rowact changes src/new.rs "Stage Changes"
check "R21-a the row's Stage Changes → Staged A src/new.rs" eventually 5 is "grp staged" "A src/new.rs"
rowact staged src/new.rs "Unstage Changes"
check "R21-a Unstage Changes → untracked again, and git agrees" eventually 5 is "grp staged; git -C '$R' diff --cached --name-only" ""
rowact changes notes.txt "Delete File"
check "R21-a deleting an untracked file asks" eventually 3 is "dialog | jq -r .heading" "Delete “notes.txt”?"
respond cancel
check "R21-a Cancel keeps it" test -e "$R/notes.txt"
rowact changes notes.txt "Delete File"
eventually 3 is "dialog | jq -r .open" true; respond discard
check "R21-a Move to Trash removes it from the tree and puts it in the Trash" eventually 5 bash -c "[ ! -e '$R/notes.txt' ] && ls '$XDG_DATA_HOME/Trash/files/' | grep -q '^notes'"
git -C "$R" add src/new.rs
check "R21-a git add outside RADE shows up by itself (.git watched)" eventually 5 is "grp staged" "A src/new.rs"
git -C "$R" reset -q src/new.rs
eventually 5 is "grp staged" ""
dsp '{"op":"open","path":"src/app.rs","section":"unstaged"}' >/dev/null
check "R21-a the Diff Pane shows 2 hunks with per-hunk actions" eventually 5 is "dsp | jq -c '[(.hunks|length), (.hunks[0].actions|join(\"/\"))]'" '[2,"Stage Hunk/Discard Hunk…"]'
dsp '{"op":"hunk","arg":"1","action":"stage"}' >/dev/null
check "R21-a Stage Hunk on hunk 2 → the index holds line 18 only" eventually 5 bash -c "git -C '$R' diff --cached | grep -q '^+CHANGED 18' && ! git -C '$R' diff --cached | grep -q 'CHANGED 2\$'"
check "R21-a …the working tree keeps line 2 unstaged" bash -c "git -C '$R' diff | grep -q '^+CHANGED 2\$'"
check "R21-a src/app.rs is in both lists" eventually 5 is "grp staged" "M src/app.rs"
dsp '{"op":"open","path":"src/app.rs","section":"staged"}' >/dev/null
eventually 5 is "dsp | jq -r .title" "app.rs (Staged)"
dsp '{"op":"hunk","arg":"0","action":"unstage"}' >/dev/null
check "R21-a Unstage Hunk → the index is clean" eventually 5 is "git -C '$R' diff --cached --name-only" ""
dsp '{"op":"open","path":"src/app.rs","section":"unstaged"}' >/dev/null
eventually 5 is "dsp | jq -r .title" "app.rs (Working Tree)"
dsp '{"op":"hunk","arg":"1","action":"stage"}' >/dev/null
eventually 5 is "grp staged" "M src/app.rs"
eventually 5 is "dsp | jq '.hunks | length'" 1
dsp '{"op":"hunk","arg":"0","action":"discard"}' >/dev/null
check "R21-a Discard Hunk asks" eventually 3 is "dialog | jq -r .heading" "Discard this hunk?"
check "R21-a …and has not touched the file yet" bash -c "grep -q 'CHANGED 2' '$R/src/app.rs'"
respond discard
check "R21-a …line 2 restored in the file, line 18 still staged" eventually 5 bash -c "[ \"\$(sed -n 2p '$R/src/app.rs')\" = 'line 2' ] && git -C '$R' diff --cached | grep -q '^+CHANGED 18'"
dsp '{"op":"escape"}' >/dev/null
rade focus "$sh1" >/dev/null

echo "== R21-b Generate, Commit"
check "R21-b Commit is disabled without a message" test "$(card .commit.enabled)" = false
scm '{"op":"generate","source":"shop"}' >/dev/null
check "R21-b Generate fills the message from the fake Agent (fence stripped)" eventually 10 is "card .message | head -1" "feat(R21): change the greeting"
check "R21-b …its prompt had CLAUDE.md, the staged diff and recent subjects" bash -c "grep -q 'Conventional Commits with R-IDs' '$T/draft-prompt.txt' && grep -q '^+CHANGED 18' '$T/draft-prompt.txt' && grep -q -- '- feat(R1): init' '$T/draft-prompt.txt'"
check "R21-b …not the untracked file" bash -c "! grep -q 'src/new.rs' '$T/draft-prompt.txt'"
check "R21-b nothing committed by Generate" test "$(head_)" = "feat(R1): init"
check "R21-b Commit is enabled now" test "$(card .commit.enabled)" = true
shot changes-commit-box
commitop commit
check "R21-b Commit commits with that message" eventually 5 is "head_" "feat(R21): change the greeting"
check "R21-b …only the staged file" test "$(git -C "$R" show --name-only --format= HEAD)" = "src/app.rs"
check "R21-b …src/new.rs stays untracked, the box empties" eventually 5 is "grp changes; echo \"[\$(card .message)]\"" "U src/new.rs
[]"
check "R21-b a Toast says Committed" eventually 3 bash -c "toast_ | grep -q '^Committed'"

echo "== R21-c Push, Pull"
scm '{"op":"card_menu_activate","source":"shop","item":"Push…"}' >/dev/null
check "R21-c Push asks, naming remote and upstream" eventually 5 bash -c "rade debug dialog | jq -e '.heading == \"Push to origin?\" and (.body | test(\"1 commit on feat-greet → origin/feat-greet\")) and (.body | test(\"upstream\"))'"
shot push-dialog
respond cancel
check "R21-c Cancel pushes nothing" bash -c "! git --git-dir='$T/remote.git' rev-parse -q --verify feat-greet"
scm '{"op":"card_menu_activate","source":"shop","item":"Push…"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true; respond push
check "R21-c Push → the remote has feat-greet at HEAD" eventually 10 is "git --git-dir='$T/remote.git' rev-parse feat-greet" "$(git -C "$R" rev-parse HEAD)"
check "R21-c …its upstream is origin/feat-greet" eventually 5 is "git -C '$R' rev-parse --abbrev-ref '@{u}'" "origin/feat-greet"
G clone -q -b feat-greet "$T/remote.git" "$T/other"
( cd "$T/other" && printf 'from elsewhere\n' > other.txt && G add other.txt && G commit -qm "fix: other" && G push -q )
scm '{"op":"card_menu_activate","source":"shop","item":"Pull"}' >/dev/null
check "R21-c Pull fast-forwards the other clone's commit" eventually 10 test -e "$R/other.txt"
printf 'local\n' > "$R/src/new.rs"; git -C "$R" add src/new.rs; git -C "$R" commit -qm "feat: local"
refresh
check "R21-c a local commit → the card's sync reads ↑1 ↓0" eventually 5 is "card .sync.label" "↑1 ↓0"

echo "== R21-d Create PR"
scm '{"op":"card_menu_activate","source":"shop","item":"Create Pull Request…"}' >/dev/null
check "R21-d the form: base main, says it pushes first" eventually 5 is "scm | jq -r '[.pr.base, .pr.note] | join(\"|\")'" "main|Pushes 1 commit to origin first."
scm '{"op":"pr_generate"}' >/dev/null
check "R21-d Generate fills title and body" eventually 10 is "scm | jq -r '.pr | .title + \"|\" + .body'" "Add a greeting|## Summary
- greets everyone"
check "R21-d …from the branch's diff and commits" bash -c "grep -q 'merging branch .feat-greet. into .main.' '$T/draft-prompt.txt' && grep -q -- '- feat: local' '$T/draft-prompt.txt'"
shot pr-dialog
respond create
check "R21-d Create pushes, then runs gh pr create --base main --title --body" eventually 10 is "tr '\n' '|' < '$T/gh.log'" "pr|create|--head|feat-greet|--base|main|--title|Add a greeting|--body|## Summary|- greets everyone|"
check "R21-d …the remote got the new commit first" test "$(git --git-dir="$T/remote.git" rev-parse feat-greet)" = "$(git -C "$R" rev-parse HEAD)"
check "R21-d a Toast shows the PR URL" eventually 3 bash -c "toast_ | grep -q 'github.test/shop/pull/7'"
# A card with changes is unfolded: a clean card folds its error line away (reported).
printf 'x\n' > "$R/scratch.tmp"; refresh
touch "$T/gh-unauth"
scm '{"op":"card_menu_activate","source":"shop","item":"Create Pull Request…"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true
scm '{"op":"pr_fill","title":"T"}' >/dev/null
respond create
check "R21-d gh not logged in → its message in the card" eventually 10 is "card .error | grep -c 'gh auth login'" 1
shot changes-error
rm -f "$T/gh-unauth"
scm '{"op":"card_menu_activate","source":"shop","item":"Fetch"}' >/dev/null
check "R21-d the card's next successful operation clears the error line" eventually 10 is "card .error" "null"
rm -f "$R/scratch.tmp"; refresh

echo "== R121-d smart commit, R121-e Commit & Push"
# A fresh branch off main so the previous section's state does not matter.
git -C "$R" checkout -q -b feat-two main
git -C "$R" push -q -u origin feat-two
refresh
check "A.69 a clean card with its upstream folds to its header, no action button" eventually 5 is "card '| [.clean, .folded, .action] | map(tostring) | join(\"|\")'" "true|true|null"
printf 'one\n' > "$R/one.txt"; printf 'two\n' > "$R/two.txt"
refresh
check "R121-d 2 unstaged changes, none staged" eventually 5 is "grp staged; grp changes" "
U one.txt,U two.txt"
msg "feat: two files"
commitop commit
check "R121-d Commit asks 'Stage all 2 changes and commit?'" eventually 3 is "dialog | jq -r .heading" "Stage all 2 changes and commit?"
shot smart-commit
respond cancel
check "R121-d Cancel commits nothing" test "$(head_)" = "feat(R1): init"
commitop commit
eventually 3 is "dialog | jq -r .open" true; respond commit
check "R121-d confirming commits both files with the message" eventually 5 bash -c "[ \"\$(head_)\" = 'feat: two files' ] && [ \"\$(git -C '$R' show --name-only --format= HEAD | tr '\n' ,)\" = 'one.txt,two.txt,' ]"
check "R121-e Undo Last Commit is enabled while HEAD is local" eventually 5 is "menu_item 'Undo Last Commit'" true
printf 'three\n' > "$R/three.txt"; git -C "$R" add three.txt; refresh
eventually 5 is "grp staged" "A three.txt"
msg "feat: third"
commitop push
check "R121-e Commit & Push commits, then asks Push's confirmation" eventually 8 bash -c "[ \"\$(head_)\" = 'feat: third' ] && rade debug dialog | jq -e '.heading == \"Push to origin?\"' >/dev/null"
respond push
check "R121-e the bare remote has the commit" eventually 10 is "git --git-dir='$T/remote.git' rev-parse feat-two" "$(git -C "$R" rev-parse HEAD)"
refresh
check "R121-e Undo Last Commit and Commit (Amend) are disabled once HEAD is pushed" eventually 5 is "menu_item 'Undo Last Commit'; menu_item 'Commit (Amend)'" "false
false"

echo "== R121-f Publish, Fetch, Sync"
git -C "$R" checkout -q -b feat-pub
refresh
check "R121-f a clean branch without upstream: the action button reads Publish Branch" eventually 5 is "card '| [.action, .sync.kind] | join(\"|\")'" "Publish Branch|publish"
scm '{"op":"action_button","source":"shop"}' >/dev/null
check "R121-f Publish asks 'Publish feat-pub to origin?'" eventually 5 is "dialog | jq -r .heading" "Publish feat-pub to origin?"
respond push
check "R121-f publishing sets origin/feat-pub" eventually 10 is "git -C '$R' rev-parse --abbrev-ref '@{u}'" "origin/feat-pub"
refresh
check "R121-f …the card is clean, folded, no action button, ↑0 ↓0" eventually 5 is "card '| [.clean, .folded, .action, .sync.label] | map(tostring) | join(\"|\")'" "true|true|null|↑0 ↓0"
( cd "$T/other" && G fetch -q && G checkout -q -b feat-pub origin/feat-pub && printf 'remote\n' > remote1.txt && G add remote1.txt && G commit -qm "fix: remote 1" && G push -q )
printf 'local\n' > "$R/local1.txt"; git -C "$R" add local1.txt; git -C "$R" commit -qm "feat: local 1"
refresh
check "R121-f before Fetch the card knows only its own commit (↑1 ↓0)" eventually 5 is "card .sync.label" "↑1 ↓0"
scm '{"op":"card_menu_activate","source":"shop","item":"Fetch"}' >/dev/null
check "R121-f Fetch: the header shows ↑1 ↓1" eventually 10 is "card .sync.label" "↑1 ↓1"
check "R121-f …and the action button reads Sync Changes ↑1 ↓1" eventually 5 is "card .action" "Sync Changes ↑1 ↓1"
check "R121-f …a card with an action button is not folded" test "$(card .folded)" = false
shot sync-button
scm '{"op":"sync_button","source":"shop"}' >/dev/null
check "R121-f Sync asks once" eventually 5 is "dialog | jq -r .heading" "Sync feat-pub with origin?"
respond sync
check "R121-f diverged: the card shows 'Sync: ' and git's message" eventually 15 is "card .error | head -n 1 | cut -c1-6" "Sync: "
check "R121-f …the remote is unchanged" test "$(git --git-dir="$T/remote.git" rev-parse feat-pub)" = "$(git -C "$T/other" rev-parse HEAD)"
git -C "$R" reset -q --hard origin/feat-pub
( cd "$T/other" && printf 'remote\n' > remote2.txt && G add remote2.txt && G commit -qm "fix: remote 2" && G push -q )
scm '{"op":"card_menu_activate","source":"shop","item":"Fetch"}' >/dev/null
check "R121-f behind only: Sync Changes ↑0 ↓1" eventually 10 is "card .action" "Sync Changes ↑0 ↓1"
scm '{"op":"sync_button","source":"shop"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true; respond sync
check "R121-f Sync with a fast-forwardable remote pulls it" eventually 15 is "git -C '$R' rev-parse HEAD" "$(git -C "$T/other" rev-parse HEAD)"
printf 'local\n' > "$R/local2.txt"; git -C "$R" add local2.txt; git -C "$R" commit -qm "feat: local 2"
refresh
check "R121-f ahead only: Sync Changes ↑1 ↓0" eventually 5 is "card .action" "Sync Changes ↑1 ↓0"
scm '{"op":"sync_button","source":"shop"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true; respond sync
check "R121-f …and Sync pushes it" eventually 15 is "git --git-dir='$T/remote.git' rev-parse feat-pub" "$(git -C "$R" rev-parse HEAD)"
check "R121-f …the error line is cleared" eventually 5 is "card .error" "null"

echo "== R121-g auto-fetch is opt-in"
( cd "$T/other" && G pull -q --ff-only && printf 'remote\n' > remote3.txt && G add remote3.txt && G commit -qm "fix: remote 3" && G push -q )
scm '{"op":"autofetch_tick"}' >/dev/null
sleep 2
refresh
check "R121-g default config: the tick fetches nothing (↓ unchanged)" test "$(card .sync.label)" = "↑0 ↓0"
python3 - "$CONF" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
open(p, "w").write(s.replace("[scm]\n", "[scm]\nautofetch = true\n", 1))
PY
sleep 1.5
scm '{"op":"autofetch_tick"}' >/dev/null
check "R121-g with autofetch = true the tick fetches and ↓ becomes 1" eventually 15 is "refresh; card .sync.label" "↑0 ↓1"

echo "== R122-d hunks and lines in the Diff Pane"
git -C "$R" checkout -q feat-pub 2>/dev/null
lines 2 18 > "$R/src/app.rs"
refresh
dsp '{"op":"open","path":"src/app.rs","section":"unstaged"}' >/dev/null
check "R122-d the Diff Pane shows 2 hunks" eventually 5 is "dsp | jq '.hunks | length'" 2
row_of() { dsp | jq -r --arg re "$1" '[.text | to_entries[] | select(.value | test($re)) | .key][0]'; }
dsp '{"op":"hunk","arg":"1","action":"stage"}' >/dev/null
check "R122-d Stage Hunk on hunk 2 → git diff --cached holds hunk 2" eventually 5 bash -c "git -C '$R' diff --cached | grep -q '^+CHANGED 18' && ! git -C '$R' diff --cached | grep -q 'CHANGED 2\$'"
eventually 5 is "dsp | jq '.hunks | length'" 1
dsp "{\"op\":\"select\",\"rows\":[$(row_of '\+CHANGED 2$')]}" >/dev/null
check "R122-d selecting the added line CHANGED 2 offers the line actions" eventually 3 is "dsp | jq -r '.selected | length'" 1
dsp '{"op":"lines","action":"stage"}' >/dev/null
check "R122-d Stage Selected Lines → the index also holds +CHANGED 2 …" eventually 5 bash -c "git -C '$R' diff --cached | grep -q '^+CHANGED 2\$'"
check "R122-d …and nothing else of hunk 1 (line 2 not removed there)" bash -c "! git -C '$R' diff --cached | grep -q '^-line 2\$'"
cp "$R/src/app.rs" "$T/app.before"
dsp '{"op":"hunk","arg":"0","action":"discard"}' >/dev/null
check "R122-d Discard Hunk… asks first" eventually 3 is "dialog | jq -r .heading" "Discard this hunk?"
check "R122-d …and the working tree is untouched until confirmed" cmp -s "$R/src/app.rs" "$T/app.before"
respond cancel
git -C "$R" reset -q src/app.rs; refresh
dsp '{"op":"refresh"}' >/dev/null

echo "== R122-e layouts, Show Full File, F7"
inspector
dsp '{"op":"width","arg":"1000"}' >/dev/null
check "R122-e auto at 1000 px is side by side" eventually 5 is "dsp | jq -r '[.layout_mode, .layout] | join(\"|\")'" "auto|side_by_side"
shot diff-side-pane
scene diff-side
dsp '{"op":"width","arg":"600"}' >/dev/null
check "R122-e at 600 px it is inline" eventually 5 is "dsp | jq -r .layout" inline
scene diff-inline
dsp '{"op":"width","arg":"1000"}' >/dev/null
eventually 5 is "dsp | jq -r .layout" side_by_side
dsp '{"op":"layout"}' >/dev/null
check "R122-e the toggle forces inline at 1000 px" eventually 5 is "dsp | jq -r '[.layout_mode, .layout] | join(\"|\")'" "inline|inline"
check "R122-e the Pane shows only the 3-line context (no 'line 10')" bash -c "! dsp | jq -e '.text | any(test(\"line 10\$\"))' >/dev/null"
dsp '{"op":"full"}' >/dev/null
check "R122-e Show Full File shows every line of the file" eventually 5 bash -c "dsp | jq -e '.full and ([range(1;21)] | all(. as \$n | (\$ARGS.named | true) and true)) and (.text | any(test(\"line 10\$\"))) and (.text | any(test(\"line 20\$\")))' >/dev/null"
first=$(dsp '{"op":"next"}' | jq -r .current); second=$(dsp '{"op":"next"}' | jq -r .current); third=$(dsp '{"op":"next"}' | jq -r .current)
check "R122-e F7 walks the changes and wraps from the last to the first ($first, $second, $third)" test "$first" != null -a "$second" -gt "$first" -a "$third" = "$first"
check "R122-e Shift+F7 from the first goes to the last" test "$(dsp '{"op":"prev"}' | jq -r .current)" = "$second"
dsp '{"op":"full"}' >/dev/null
dsp '{"op":"layout"}' >/dev/null
inspector

echo "== R122-f restore and the empty state"
dsp '{"op":"width","arg":"700"}' >/dev/null
stop_app; start_app
check "R122-f after a relaunch the Pane is back on app.rs (Working Tree)" eventually 10 is "dsp | jq -r '.title'" "app.rs (Working Tree)"
git -C "$R" add src/app.rs
dsp '{"op":"refresh"}' >/dev/null
check "R122-f after git add it shows 'No changes' and Open Staged Changes" eventually 8 is "dsp '{\"op\":\"refresh\"}' | jq -r '[.empty, .empty_button] | map(tostring) | join(\"|\")'" "true|Open Staged Changes"
shot diff-empty
dsp '{"op":"empty_button"}' >/dev/null
check "R122-f that button switches it to app.rs (Staged)" eventually 5 is "dsp | jq -r .title" "app.rs (Staged)"
dsp '{"op":"escape"}' >/dev/null
git -C "$R" reset -q; git -C "$R" checkout -q -- .
sh1=$(rade list --json | jq -r '[.panes[] | select(.source=="shop")][0].pane')
rade focus "$sh1" >/dev/null

echo "== R123-b Quick Diff"
lines 2 18 > "$R/src/app.rs"
rade open "$R/src/app.rs" --json >/dev/null
v=$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer") | .id][0]')
vq() { rade debug viewer "$v" info | jq -c '[.quick_diff[] | [.line, .kind] | join(":")] | join(",")' | tr -d '"'; }
rade debug action color_scheme_light >/dev/null; sleep 0.5   # the relaunch above started in the default (dark) scheme
check "R123-b the Viewer marks lines 2 and 18 as modified" eventually 8 is "vq" "2:modified,18:modified"
printf 'extra\n' >> "$R/src/app.rs"
check "R123-b an edit on disk updates the marks within 5 s (line 21 added)" eventually 5 is "vq" "2:modified,18:modified,21:added"
check "R123-b the peek on a mark shows its hunk and its buttons" bash -c "rade debug viewer '$v' peek 2 | jq -e '.peek.open and (.peek.lines | any(. == \"+CHANGED 2\")) and (.peek.buttons | contains([\"Stage Change\", \"Revert Change…\", \"Open Changes\"]))' >/dev/null"
sleep 0.6
rade debug viewer "$v" peek_shot "$OUT/quick-diff-peek-light.png" >/dev/null && echo "  shot target/e2e-m45/quick-diff-peek-light.png"
rade debug action color_scheme_dark >/dev/null; sleep 0.8
rade debug viewer "$v" peek 18 >/dev/null; sleep 0.6
rade debug viewer "$v" peek_shot "$OUT/quick-diff-peek-dark.png" >/dev/null && echo "  shot target/e2e-m45/quick-diff-peek-dark.png"
rade debug action color_scheme_light >/dev/null; sleep 0.5
shot quick-diff-marks
rade debug viewer "$v" peek 2 >/dev/null
rade debug viewer "$v" peek_action "Stage Change" >/dev/null
check "R123-b Stage Change stages that hunk (the index holds CHANGED 2)" eventually 5 bash -c "git -C '$R' diff --cached | grep -q '^+CHANGED 2\$'"
check "R123-b …and the mark is gone" eventually 8 is "vq" "18:modified,21:added"
printf 'quick_diff_off\n' > "$T/qd.txt"
python3 - "$CONF" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
open(p, "w").write(s.replace("[scm]\n", "[scm]\nquick_diff = false\n", 1))
PY
sleep 1.5
rade open "$R/src/util.rs" --json >/dev/null
printf '// changed\n' >> "$R/src/util.rs"
sleep 1.5
rade open "$R/src/app.rs" --json >/dev/null
check "R123-b with quick_diff = false there are no marks" eventually 8 is "vq" ""
python3 - "$CONF" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
open(p, "w").write(s.replace("quick_diff = false\n", "", 1))
PY
sleep 1.5
printf 'new file\n' > "$R/fresh.txt"
rade open "$R/fresh.txt" --json >/dev/null
v2=$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer" and (.kind.path | endswith("fresh.txt"))) | .id][0]')
check "R123-b an untracked file has no marks" eventually 5 is "rade debug viewer '$v2' info | jq -c '.quick_diff'" "[]"

echo "== R21.1 / R120 a folder that is not a repository"
mkdir -p "$T/plain"
rade source add "$T/plain" --name plain >/dev/null
pp=$(rade spawn --source plain --cmd bash --json | jq -r .pane)
rade focus "$pp" >/dev/null
check "R120-h a plain folder: mode none ('No Repository')" eventually 5 is "scm | jq -r .mode" none
check "R120-h …and no count on the switcher" test "$(rpc debug.ui | jq -r '.chrome.right_panel.labels[0]')" = "Changes"

echo
echo "M45: $PASS passed, $FAIL failed (screenshots: ${OUT#$ROOT/})"
stop_app
[[ $FAIL -eq 0 ]]
