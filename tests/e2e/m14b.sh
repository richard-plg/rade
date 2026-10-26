#!/usr/bin/env bash
# E2E (M14 phase 2, R22): the Right Panel's Search page (regex, case, word,
# globs, .gitignore, click → Viewer at the line, 50k-file timing).
# Screenshots light + dark → target/e2e-m14b/. The Changes checks (R21) moved
# to tests/e2e/m45.sh with the Changes page of M45.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m14b}
source "$HERE/lib.sh"

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
  G add -A && G commit -qm "feat(R1): init" )

find_() { rpc debug.find "${1:-{\}}"; }
export -f rpc find_
scene() { # name: a light and a dark shot
    rade debug action color_scheme_light >/dev/null; sleep 0.8; shot "$1-light"
    rade debug action color_scheme_dark >/dev/null; sleep 0.8; shot "$1-dark"
    rade debug action color_scheme_light >/dev/null; sleep 0.3
}

# git never looks above the temp dir: with TMPDIR inside a repository (the
# gate's target/tmp) the 50k-file folder would otherwise be inside one.
export GIT_CEILING_DIRECTORIES=$T
start_app
rade source add "$R" --name shop >/dev/null
sh1=$(rade spawn --source shop --cmd bash --name sh --json | jq -r .pane)
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
echo "M14b (Search): $PASS passed, $FAIL failed (screenshots: ${OUT#$ROOT/})"
stop_app
[[ $FAIL -eq 0 ]]
