#!/usr/bin/env bash
# E2E (M14 phase 2, R21, R22): the Right Panel's Changes page (stage /
# unstage / discard per file and hunk, Generate with a fake draft command,
# commit, push / pull with a bare remote, Create PR with a fake `gh`) and
# Search page (regex, case, word, globs, .gitignore, click → Viewer at the
# line, 50k-file timing). Screenshots light + dark → target/e2e-m14b/.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m14b}
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
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<C
[scm]
draft_command = "$T/draft.sh"
C

G() { git -c user.name=dev -c user.email=d@e -c init.defaultBranch=main "$@"; }
export GIT_AUTHOR_NAME=dev GIT_AUTHOR_EMAIL=d@e GIT_COMMITTER_NAME=dev GIT_COMMITTER_EMAIL=d@e
R=$T/code/shop
mkdir -p "$R/src" "$R/docs" "$R/target"
lines() { python3 -c "import sys; e=set(map(int, sys.argv[1:])); print(''.join(f'CHANGED {i}\n' if i in e else f'line {i}\n' for i in range(1, 21)), end='')" "$@"; }
( cd "$R" && G init -q
  printf '# Rules\n\nCommits: Conventional Commits with R-IDs as the scope.\n' > CLAUDE.md
  lines > src/app.rs
  printf 'pub fn greet() {}\n// Greeting greet_all GREET\nfn greet(x) {}\n' > src/util.rs
  printf '# Notes\n\nWe greet in docs.\n' > docs/notes.md
  printf 'target/\n*.log\n' > .gitignore
  printf 'greet in build output\n' > target/out.rs
  printf 'greet logged\n' > app.log
  G add -A && G commit -qm "feat(R1): init"
  G init -q --bare -b main "$T/remote.git"
  G remote add origin "$T/remote.git"
  G push -q -u origin main
  G checkout -qb feat-greet )

scm() { rpc debug.scm "${1:-{\}}"; }
find_() { rpc debug.find "${1:-{\}}"; }
dialog() { rade debug dialog; }
respond() { rade debug dialog respond "$1" >/dev/null; }
export -f rpc scm find_
sect() { scm | jq -r --arg s "$1" '[.sections[$s][] | .letter + " " + .path] | join(",")'; }
scene() { # name: a light and a dark shot
    rade debug action color_scheme_light >/dev/null; sleep 0.8; shot "$1-light"
    rade debug action color_scheme_dark >/dev/null; sleep 0.8; shot "$1-dark"
    rade debug action color_scheme_light >/dev/null; sleep 0.3
}

# git never looks above the temp dir: with TMPDIR inside a repository (the
# gate's target/tmp) the plain folder would otherwise be in one (R21.1 check).
export GIT_CEILING_DIRECTORIES=$T
start_app
rade source add "$R" --name shop >/dev/null
sh1=$(rade spawn --source shop --cmd bash --name sh --json | jq -r .pane)
rade focus "$sh1" >/dev/null
rade debug action show_changes >/dev/null
echo "== R21.1 lists"
lines 2 18 > "$R/src/app.rs"
printf 'scratch\n' > "$R/notes.txt"
printf 'pub fn new() {}\n' > "$R/src/new.rs"
scm '{"op":"refresh"}' >/dev/null   # the window's focus-in does this (headless Weston never activates it)
check "Ctrl+Shift+M action shows the Changes page (panel open)" eventually 5 is "scm | jq -r '[.visible, .page] | join(\" \")'" "true repo"
check "branch feat-greet, not pushed yet" eventually 5 is "scm | jq -r '[.branch, .sync] | join(\"|\")'" "feat-greet|not pushed"
check "Changes: M src/app.rs" eventually 5 is "sect unstaged" "M src/app.rs"
check "Untracked: notes.txt, src/new.rs" test "$(sect untracked)" = "U notes.txt,U src/new.rs"
check "the switcher counts 3 changed files" test "$(rpc debug.ui | jq -r '.chrome.right_panel.labels[0]')" = "Changes 3"

echo "== R21.2 per file"
scm '{"op":"stage","section":"untracked","path":"src/new.rs"}' >/dev/null
check "stage src/new.rs → Staged A src/new.rs" eventually 5 is "sect staged" "A src/new.rs"
scm '{"op":"unstage","section":"staged","path":"src/new.rs"}' >/dev/null
check "unstage → Untracked again" eventually 5 is "sect untracked" "U notes.txt,U src/new.rs"
check "…and git agrees" test "$(git -C "$R" diff --cached --name-only)" = ""
scm '{"op":"discard","section":"untracked","path":"notes.txt"}' >/dev/null
check "discard of an untracked file asks (Delete)" eventually 3 is "dialog | jq -r '.heading'" "Delete “notes.txt”?"
respond cancel
check "Cancel keeps it" test -e "$R/notes.txt"
scm '{"op":"discard","section":"untracked","path":"notes.txt"}' >/dev/null
eventually 3 is "dialog | jq -r .open" true; respond discard
check "Delete removes it" eventually 5 bash -c "[ ! -e '$R/notes.txt' ]"
git -C "$R" add src/new.rs
check "git add outside RADE shows up by itself (.git watched)" eventually 5 is "sect staged" "A src/new.rs"
git -C "$R" reset -q src/new.rs
eventually 5 is "sect staged" ""

echo "== R21.3 per hunk"
scm '{"op":"click","section":"unstaged","path":"src/app.rs"}' >/dev/null
check "a click shows its diff: 2 hunks, per-hunk actions" eventually 5 is "scm | jq -c '.expanded[0] | [(.hunks|length), .per_hunk]'" "[2,true]"
scene changes-diff
scm '{"op":"hunk","section":"unstaged","path":"src/app.rs","hunk":1,"action":"stage"}' >/dev/null
check "stage hunk 2 → the index holds line 18 only" eventually 5 bash -c "git -C '$R' diff --cached | grep -q '^+CHANGED 18' && ! git -C '$R' diff --cached | grep -q 'CHANGED 2\$'"
check "…the working tree keeps line 2 unstaged" bash -c "git -C '$R' diff | grep -q '^+CHANGED 2\$'"
check "src/app.rs in both lists" eventually 5 is "sect staged" "M src/app.rs"
scm '{"op":"click","section":"staged","path":"src/app.rs"}' >/dev/null
eventually 5 is "scm | jq '[.expanded[] | select(.section==\"staged\")] | length'" 1
scm '{"op":"hunk","section":"staged","path":"src/app.rs","hunk":0,"action":"unstage"}' >/dev/null
check "unstage that hunk → index clean" eventually 5 is "git -C '$R' diff --cached --name-only" ""
scm '{"op":"hunk","section":"unstaged","path":"src/app.rs","hunk":1,"action":"stage"}' >/dev/null
eventually 5 is "sect staged" "M src/app.rs"
eventually 5 is "scm | jq '[.expanded[] | select(.section==\"unstaged\") | .hunks | length] | .[0]'" 1
scm '{"op":"hunk","section":"unstaged","path":"src/app.rs","hunk":0,"action":"discard"}' >/dev/null
check "discard hunk asks" eventually 3 is "dialog | jq -r .heading" "Discard this hunk?"
respond discard
check "…line 2 restored in the file, line 18 still staged" eventually 5 bash -c "diff <(cd '$R' && git show :src/app.rs) - <<<'$(lines 18)' >/dev/null && [ \"\$(sed -n 2p '$R/src/app.rs')\" = 'line 2' ]"

echo "== R21.7 Generate, R21.4 Commit"
check "Commit disabled without a message" test "$(scm | jq -r .commit_enabled)" = false
scm '{"op":"generate"}' >/dev/null
check "Generate fills the message from the fake Agent (fence stripped)" eventually 10 is "scm | jq -r .message | head -1" "feat(R21): change the greeting"
check "…its prompt had CLAUDE.md, the staged diff and recent subjects" bash -c "grep -q 'Conventional Commits with R-IDs' '$T/draft-prompt.txt' && grep -q '^+CHANGED 18' '$T/draft-prompt.txt' && grep -q -- '- feat(R1): init' '$T/draft-prompt.txt'"
check "…not the unstaged file" bash -c "! grep -q 'src/new.rs' '$T/draft-prompt.txt'"
check "nothing committed by Generate" test "$(git -C "$R" log --format=%s -1)" = "feat(R1): init"
check "Commit enabled now" test "$(scm | jq -r .commit_enabled)" = true
scene changes-commit
scm '{"op":"commit"}' >/dev/null
check "Commit commits with that message" eventually 5 is "git -C '$R' log --format=%s -1" "feat(R21): change the greeting"
check "…only the staged file" test "$(git -C "$R" show --name-only --format= HEAD)" = "src/app.rs"
check "…src/new.rs stays untracked, the box empties" eventually 5 is "sect untracked; scm | jq -r .message" "U src/new.rs"
check "a Toast says Committed" eventually 3 bash -c "rpc debug.ui | jq -r '.chrome.notes[-1]' | grep -q '^Committed'"

echo "== R21.5 Push / Pull"
scm '{"op":"push"}' >/dev/null
check "Push asks, naming remote and upstream" eventually 5 bash -c "rade debug dialog | jq -e '.heading == \"Push to origin?\" and (.body | test(\"1 commit on feat-greet → origin/feat-greet\")) and (.body | test(\"upstream\"))'"
scene push-dialog
respond cancel
check "Cancel pushes nothing" bash -c "! git --git-dir='$T/remote.git' rev-parse -q --verify feat-greet"
scm '{"op":"push"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true; respond push
check "Push → the remote has feat-greet at HEAD" eventually 10 is "git --git-dir='$T/remote.git' rev-parse feat-greet" "$(git -C "$R" rev-parse HEAD)"
check "…upstream set, ↑0 ↓0" eventually 5 is "scm | jq -r '[.upstream, .sync] | join(\"|\")'" "origin/feat-greet|↑0 ↓0"
G clone -q -b feat-greet "$T/remote.git" "$T/other"
( cd "$T/other" && printf 'from elsewhere\n' > other.txt && G add other.txt && G commit -qm "fix: other" && G push -q )
scm '{"op":"pull"}' >/dev/null
check "Pull fast-forwards the other clone's commit" eventually 10 test -e "$R/other.txt"
printf 'local\n' > "$R/src/new.rs"; git -C "$R" add src/new.rs; git -C "$R" commit -qm "feat: local"
scm '{"op":"refresh"}' >/dev/null
check "a local commit → ↑1" eventually 5 is "scm | jq -r .sync" "↑1 ↓0"

echo "== R21.6 Create PR"
scm '{"op":"pr"}' >/dev/null
check "Create PR form: base main, says it pushes first" eventually 5 is "scm | jq -r '[.pr.base, .pr.note] | join(\"|\")'" "main|Pushes 1 commit to origin first."
scm '{"op":"pr_generate"}' >/dev/null
check "Generate fills title and body" eventually 10 is "scm | jq -r '.pr | .title + \"|\" + .body'" "Add a greeting|## Summary
- greets everyone"
check "…from the branch's diff and commits" bash -c "grep -q 'merging branch .feat-greet. into .main.' '$T/draft-prompt.txt' && grep -q -- '- feat: local' '$T/draft-prompt.txt'"
scene pr-dialog
respond create
check "Create pushes, then runs gh pr create --base main --title --body" eventually 10 is "tr '\n' '|' < '$T/gh.log'" "pr|create|--head|feat-greet|--base|main|--title|Add a greeting|--body|## Summary|- greets everyone|"
check "…the remote got the new commit first" test "$(git --git-dir="$T/remote.git" rev-parse feat-greet)" = "$(git -C "$R" rev-parse HEAD)"
check "a Toast shows the PR URL" eventually 3 bash -c "rpc debug.ui | jq -r '.chrome.notes[-1]' | grep -q 'github.test/shop/pull/7'"
touch "$T/gh-unauth"
scm '{"op":"pr"}' >/dev/null
eventually 5 is "dialog | jq -r .open" true
scm '{"op":"pr_fill","title":"T"}' >/dev/null
respond create
check "gh not logged in → its message in the page" eventually 10 bash -c "rade debug state >/dev/null; rpc debug.scm | jq -r .error | grep -q 'gh auth login'"
shot changes-error-light

echo "== R21.1 outside a repository"
mkdir -p "$T/plain"
rade source add "$T/plain" --name plain >/dev/null
pp=$(rade spawn --source plain --cmd bash --json | jq -r .pane)
rade focus "$pp" >/dev/null
check "a plain folder: 'No Repository'" eventually 5 is "scm | jq -r .page" none
check "…and no count on the switcher" test "$(rpc debug.ui | jq -r '.chrome.right_panel.labels[0]')" = "Changes"
rade focus "$sh1" >/dev/null

echo "== R22 Search"
rade debug action search_worktree >/dev/null
check "Ctrl+Shift+S action: Search page, keyboard in its entry" eventually 3 is "find_ | jq -r '[.visible, .entry_focused] | join(\" \")'" "true true"
found() { find_ | jq -r '[.rows[] | select(.kind=="hit") | .path + ":" + (.line|tostring)] | join(",")'; }
done_() { find_ | jq -e --arg q "$1" '.running == false and .searched == $q' >/dev/null; }
q() { find_ "$1" >/dev/null; eventually 10 done_ "$(jq -r .query <<<"$1")"; }
q '{"op":"set","query":"greet"}'
check "greet: .gitignore'd target/ and *.log are not searched" test "$(found)" = "docs/notes.md:3,src/util.rs:1,src/util.rs:2,src/util.rs:3"
check "summary counts results and files" test "$(find_ | jq -r .summary)" = "6 results in 2 files"
check "matches highlighted in the preview" test "$(find_ | jq -c '.rows[] | select(.kind=="hit" and .line==2) | .ranges')" = "[[3,8],[12,17],[22,27]]"
scene search
q '{"op":"set","query":"GREET","case":true}'
check "Match Case: GREET only" test "$(found)" = "src/util.rs:2"
q '{"op":"set","query":"greet","case":false,"word":true}'
check "Whole Word: not greet_all / Greeting" test "$(find_ | jq -r .summary)" = "4 results in 2 files"
q '{"op":"set","query":"gre+t\\(","word":false,"regex":true}'
check "Regex: gre+t\\(" test "$(found)" = "src/util.rs:1,src/util.rs:3"
q '{"op":"set","query":"(","regex":true}'
check "an invalid regex is reported" bash -c "find_ | jq -e '.error and (.summary | test(\"unclosed\"))' >/dev/null"
q '{"op":"set","query":"greet","regex":false,"include":"*.md"}'
check "include *.md" test "$(found)" = "docs/notes.md:3"
q '{"op":"set","query":"greet","include":"","exclude":"docs/**"}'
check "exclude docs/**" test "$(found)" = "src/util.rs:1,src/util.rs:2,src/util.rs:3"
find_ '{"op":"activate","path":"src/util.rs"}' >/dev/null
check "a click on the file row folds it" test "$(found)|$(find_ | jq -r '.rows[0].folded')" = "|true"
find_ '{"op":"activate","path":"src/util.rs"}' >/dev/null
viewers() { rade debug state | jq -c '[.. | objects | select(.kind?.type? == "viewer") | {path: .kind.path, line: .kind.line}]'; }
find_ '{"op":"activate","path":"src/util.rs","line":3}' >/dev/null
check "a click on a line opens the Viewer at that line" eventually 5 is "viewers | jq -c '.[-1] | [.path, .line]'" "[\"$R/src/util.rs\",3]"

echo "== R22.4 50,000 files"
python3 - "$T/k50" <<'PY'
import os, sys
root = sys.argv[1]
for i in range(50000):
    d = os.path.join(root, f"pkg{i % 50:02d}", f"mod{i % 997:03d}")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, f"file_{i}.rs"), "w") as f:
        f.write(f"pub fn item_{i}() -> u32 {{ {i} }}\n" + ("// the needle\n" if i % 9973 == 0 else ""))
os.makedirs(os.path.join(root, "node_modules/x"), exist_ok=True)
for i in range(500):
    open(os.path.join(root, "node_modules/x", f"dep_{i}.js"), "w").write("needle\n")
open(os.path.join(root, ".gitignore"), "w").write("node_modules/\n")
PY
git -C "$T/k50" init -q
rade source add "$T/k50" --name k50 >/dev/null
kp=$(rade spawn --source k50 --cmd bash --json | jq -r .pane)
rade focus "$kp" >/dev/null
sleep 1
: > "$T/probe"
( while [ ! -e "$T/probe.stop" ]; do s=$(date +%s%N); find_ >/dev/null; echo $(( ($(date +%s%N) - s) / 1000000 )) >> "$T/probe"; sleep 0.05; done ) &
PROBE=$!
q '{"op":"set","query":"needle","include":"","exclude":""}'
touch "$T/probe.stop"; wait $PROBE
took=$(find_ | jq -r .took_ms)
echo "  k50 'needle': $(find_ | jq -r .summary) in ${took} ms; GTK probe slowest $(sort -n "$T/probe" | tail -1) ms over $(wc -l < "$T/probe") calls"
check "50k files: node_modules ignored, 6 results" test "$(find_ | jq -r .summary)" = "6 results in 6 files"
check "50k files searched in < 2 s ($took ms)" test "$took" -lt 2000
check "the GTK thread kept answering (slowest $(sort -n "$T/probe" | tail -1) ms incl. CLI start)" test "$(sort -n "$T/probe" | tail -1)" -lt 500
q '{"op":"set","query":"item_"}'
check "a common word stops at 2,000 results" test "$(find_ | jq -r '[.matches, .capped] | join(" ")')" = "2000 true"
check "…and says so" bash -c "find_ | jq -r .summary | grep -q 'showing the first 2000'"

echo
echo "M14b: $PASS passed, $FAIL failed (screenshots: ${OUT#$ROOT/})"
stop_app
[[ $FAIL -eq 0 ]]
