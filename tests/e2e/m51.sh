#!/usr/bin/env bash
# M51 end-to-end tests (ADR-0006): Projects and Scratch fixes, the Project
# lifecycle, Ctrl+click across wrapped rows, select and copy in the Viewer
# (PLAN-m51.md). Real git repositories in a temp Workspace `acme`.
# R131 (git): a folder without git in New Project — Cancel forgets it,
# Initialize runs `git init` only, an unborn base links on its main checkout,
# a Worktree waits for the first commit, the CLI's hint.
# (Tab labels, R103.2 amendment: tests/e2e/titles.sh.)
# Usage: tests/e2e/m51.sh   (PARTS="git …", NO_BUILD=1, KEEP=1)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m51
source "$HERE/lib.sh"
PARTS=${PARTS:-git}
part() { [[ " $PARTS " == *" $1 "* ]]; }

mkdir -p "$XDG_CONFIG_HOME/rade"
printf 'idle_seconds = 1\n' >"$XDG_CONFIG_HOME/rade/config.toml"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
dlgop() { rpc debug.ui "$1" | jq -c .dialog; }
dialog() { rade debug dialog; }
respond() { rade debug dialog respond "$1" >/dev/null; }
sources() { rade source list --json | jq -r '[.sources[].name] | sort | join(",")'; }
export -f rpc dlgop sources

# ---- The scene ----------------------------------------------------------------
A=$HOME/workspaces/acme
WEB=$A/shop-web
mkdir -p "$WEB" && G -C "$WEB" init -q -b main
echo base >"$WEB/README.md" && G -C "$WEB" add -A && G -C "$WEB" commit -qm init
mkdir -p "$A/.rade"
echo 'name = "acme"' >"$A/.rade/workspace.toml"

start_app
rade workspace add "$A" >/dev/null
rade source add "$WEB" --workspace acme >/dev/null
rade workspace switch acme >/dev/null
PROJ=$A/.rade/projects

if part git; then
    echo "== R131 a folder without git"
    PLAIN=$A/notes
    mkdir -p "$PLAIN" && echo hi >"$PLAIN/a.txt"
    before=$(sources)
    cp "$STATE" "$T/state-0.json" 2>/dev/null || true

    echo "-- R131-b Cancel forgets the folder"
    dlgop '{"op":"dialog_open","kind":"new_project"}' >/dev/null
    dlgop '{"op":"dialog_name","text":"Notes"}' >/dev/null
    dlgop "$(jq -nc --arg p "$PLAIN" '{op: "dialog_folder", path: $p}')" >/dev/null
    row() { dlgop '{"op":"dialog"}' | jq -c '.rows[] | select(.name == "notes")'; }
    export -f row
    check "R131 the row says Not a git repository and offers Initialize" \
        eventually 3 bash -c "row | jq -e '.init == true and (.refused | test(\"not a git repository\"))' >/dev/null"
    check "R131 Create is disabled" eventually 3 bash -c "dlgop '{\"op\":\"dialog\"}' | jq -e '.primary_enabled == false' >/dev/null"
    dlgop '{"op":"dialog"}' >"$OUT/dialog-plain.json"
    shot git-plain
    dlgop '{"op":"dialog_cancel"}' >/dev/null
    check "R131-b after Cancel the folder is no Source (got '$(sources)')" eventually 3 is sources "$before"
    check "R131-b …and no Project file" test ! -e "$PROJ/notes"

    echo "-- R131-c Initialize runs git init only"
    dlgop '{"op":"dialog_open","kind":"new_project"}' >/dev/null
    dlgop '{"op":"dialog_name","text":"Notes"}' >/dev/null
    dlgop "$(jq -nc --arg p "$PLAIN" '{op: "dialog_folder", path: $p}')" >/dev/null
    eventually 3 bash -c "row | jq -e '.init == true' >/dev/null" || true
    dlgop '{"op":"dialog_init","source":"notes"}' >/dev/null
    check "R131-c Initialize asks first, naming the command" \
        eventually 3 bash -c "rade debug dialog | jq -e '(.heading == \"Initialize a git repository?\") and (.body | test(\"git init -b main\")) and (.body | test(\"No file is added or committed\"))' >/dev/null"
    respond init
    check "R131-c HEAD is refs/heads/main" eventually 5 is "git -C '$PLAIN' symbolic-ref HEAD 2>/dev/null" "refs/heads/main"
    check "R131-c no commit, a.txt untracked" \
        bash -c "! git -C '$PLAIN' rev-parse -q --verify HEAD >/dev/null && git -C '$PLAIN' status --porcelain | grep -qx '?? a.txt'"
    check "R131-c the row now plans the main checkout on main" \
        eventually 5 bash -c "row | jq -e '.refused == null and .init == false and (.plan | startswith(\"main checkout\"))' >/dev/null"
    check "R131-c Create is enabled" eventually 3 bash -c "dlgop '{\"op\":\"dialog\"}' | jq -e '.primary_enabled' >/dev/null"
    shot git-initialized
    dlgop '{"op":"dialog_primary"}' >/dev/null
    check "R131-c Create wrote the link" eventually 5 bash -c "grep -q 'id = \"notes\"' '$PROJ/notes/project.toml'"
    check "R131-c …and the Source stays registered" eventually 3 bash -c "sources | grep -q notes"

    echo "-- R131-d an unborn base: no Worktree until the first commit"
    rade project show notes --json >"$OUT/show-unborn.json" || true
    check "R131-d project show: main checkout, HEAD main" \
        bash -c "jq -e '.links[0].checkout_kind == \"main\" or .links[0].kind == \"main\"' '$OUT/show-unborn.json' >/dev/null"
    rc=$(code rade worktree add notes feature --project notes)
    check "R131-d New Worktree refused, saying why (exit $rc)" \
        bash -c "[[ $rc != 0 ]] && grep -q 'has no commits yet: commit once, then try again' '$T/err'"
    check "R131-d nothing created" test ! -e "$A/.worktrees-dev/notes/feature"
    G -C "$PLAIN" add -A && G -C "$PLAIN" commit -qm first
    rc=$(code rade worktree add notes feature --project notes)
    check "R131-d after one commit New Worktree works (exit $rc)" test "$rc" = 0

    echo "-- R131-e the CLI names the fix"
    BARE=$A/scratchpad && mkdir -p "$BARE"
    rade source add "$BARE" --workspace acme >/dev/null
    rc=$(code rade project plan Pad --workspace acme --source scratchpad:main)
    cat "$T/out" "$T/err" >"$OUT/plan-plain.txt"
    check "R131-e project plan on a plain folder names the fix (exit $rc)" \
        grep -q 'not a git repository (run: git init -b main)' "$OUT/plan-plain.txt"
    check "R131-e …no file written" test ! -e "$PROJ/pad"
fi

echo
echo "m51: $PASS passed, $FAIL failed"
((FAIL == 0))
