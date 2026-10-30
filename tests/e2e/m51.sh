#!/usr/bin/env bash
# M51 end-to-end tests (ADR-0006): Projects and Scratch fixes, the Project
# lifecycle, Ctrl+click across wrapped rows, select and copy in the Viewer
# (PLAN-m51.md). Real git repositories in a temp Workspace `acme`.
# R131 (git): a folder without git in New Project — Cancel forgets it,
# Initialize runs `git init` only, an unborn base links on its main checkout,
# a Worktree waits for the first commit, the CLI's hint.
# R132 (move): Move to Project asks about the base (Keep / Compare against),
# `rade worktree move`, Scratch in the submenu, suggestions only on a real
# match. (Close Panes in Scratch…, R132.4: tests/e2e/m37.sh R104-l.)
# R132.5 (home): Project home's Open terminal with several links offers
# them, the last used first; with one it opens at once.
# R133 (remove): Remove Project… / `rade project remove` (folder deleted,
# unused clean Project checkouts removed, a shared or dirty one kept, Panes
# there closed, Worktrees to Scratch), `rade project prune`, the
# "no longer used" line, the to-base hint.
# R134 (links): Ctrl+click on a path or URL wrapped over rows — a shell's
# soft wrap, a TUI's hard wrap at its own width with an indent — opens the
# whole token.
# (Tab labels, R103.2 amendment: tests/e2e/titles.sh.)
# Usage: tests/e2e/m51.sh   (PARTS="git …", NO_BUILD=1, KEEP=1)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m51
source "$HERE/lib.sh"
PARTS=${PARTS:-git move home remove links}
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
G -C "$WEB" branch develop && G -C "$WEB" branch release/2.4
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

if part move; then
    echo "== R132 Worktree ↔ Project moves"
    rade project add Shop --workspace acme --source shop-web:develop >/dev/null
    rade project add Shop24 --workspace acme --source shop-web:release/2.4 >/dev/null
    rade worktree add shop-web checkout-v2 --project shop >/dev/null
    wt() { rade worktree list shop-web --json | jq -r --arg w "$1" '.worktrees[] | select(.worktree == $w) | "\(.project)@\(.base)"'; }
    export -f wt
    check "R132 checkout-v2 is Shop's, cut from develop" eventually 5 is "wt checkout-v2" "shop@develop"
    rade debug menu worktree shop-web/checkout-v2 --json >"$OUT/move-menu.json"
    check "R132-c the submenu lists Shop ✓, Shop24 and Scratch, no No Project" \
        bash -c "jq -r '[.items[].label | select(startswith(\"Move to Project\"))] | join(\"|\")' '$OUT/move-menu.json' | grep -qx 'Move to Project ▸ Shop ✓|Move to Project ▸ Shop24|Move to Project ▸ Scratch'"

    echo "-- R132-b the base differs: one question, Keep or Compare against"
    rade debug menu worktree shop-web/checkout-v2 activate "Move to Project ▸ Shop24" >/dev/null
    check "R132-b the dialog names both bases" \
        eventually 3 bash -c "rade debug dialog | jq -e '(.heading == \"Move “checkout-v2” to Shop24?\") and (.body | test(\"links this Source at release/2.4; the Worktree compares against develop\"))' >/dev/null"
    rade debug dialog >"$OUT/move-dialog.json"
    check "R132-b buttons: Cancel, Keep comparing against develop, Compare against release/2.4" \
        test "$(jq -r '.responses | join("|")' "$OUT/move-dialog.json")" = "cancel|keep|rebase"
    shot move-dialog
    respond cancel
    check "R132-b Cancel moves nothing" eventually 3 is "wt checkout-v2" "shop@develop"
    rade debug menu worktree shop-web/checkout-v2 activate "Move to Project ▸ Shop24" >/dev/null
    eventually 3 bash -c "rade debug dialog | jq -e .open >/dev/null" || true
    respond rebase
    check "R132-b Compare against → Shop24 at release/2.4" eventually 3 is "wt checkout-v2" "shop24@release/2.4"
    rc=$(code rade worktree move shop-web checkout-v2 --project shop)
    check "R132-b CLI move back without --rebase-to-link keeps release/2.4 (exit $rc)" eventually 3 is "wt checkout-v2" "shop@release/2.4"
    rc=$(code rade worktree move shop-web checkout-v2 --project shop --rebase-to-link)
    check "R132-b --rebase-to-link → develop" eventually 3 is "wt checkout-v2" "shop@develop"
    rade debug menu worktree shop-web/checkout-v2 activate "Move to Project ▸ Scratch" >/dev/null
    check "R132-b to Scratch: no question, base kept" eventually 3 is "wt checkout-v2" "null@develop"
    rc=$(code rade worktree move shop-web checkout-v2 --project billing)
    check "R132-b a Project not linking the Source is refused (exit $rc)" \
        bash -c "[[ $rc == 2 ]] && grep -q \"project 'billing' doesn't link source\" '$T/err'"
    rc=$(code rade worktree move shop-web main --project shop)
    check "R132-b the main checkout never moves (exit $rc)" bash -c "[[ $rc == 2 ]]"

    echo "-- R132-a suggestions only on a real match"
    G -C "$WEB" worktree add -q "$A/.worktrees-dev/shop-web/shopping-cart" -b shopping-cart develop
    G -C "$WEB" worktree add -q "$A/.worktrees-dev/shop-web/rel-fix" -b shop24/fix release/2.4
    rade worktree list shop-web >/dev/null
    sug() { rpc debug.ui '{"op":"sidebar"}' | jq -r --arg t "$1" '[.sidebar[] | select(.title == $t) | .suggest // "none"] | first'; }
    export -f sug
    check "R132-a shop24/fix → Shop24?" eventually 8 is "sug shop24/fix" "→ Shop24?"
    check "R132-a shopping-cart: no guess (not 'shop')" is "sug shopping-cart" "none"
    check "R132-a checkout-v2 on develop's line but named nothing: no guess" is "sug checkout-v2" "none"
    rpc debug.ui '{"op":"sidebar"}' >"$OUT/move-sidebar.json"
    shot move-scratch
fi

if part home; then
    echo "== R132-e Project home: Open terminal on which link"
    for r in shop-api shared-ui; do
        mkdir -p "$A/$r" && G -C "$A/$r" init -q -b develop
        echo x >"$A/$r/x" && G -C "$A/$r" add -A && G -C "$A/$r" commit -qm init
        rade source add "$A/$r" --workspace acme >/dev/null
    done
    rade project add Trio --workspace acme --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null
    rade project add Solo --workspace acme --source shared-ui:develop >/dev/null
    home() { rpc debug.ui "$(jq -nc --arg a "${1:-}" '{op: "home", arg: $a}')" | jq -c .home; }
    menu() { home | jq -r '.open_terminal_menu | join("|")'; }
    panes() { rade list --json | jq -r '[.panes[].pane] | sort | join(" ")'; }
    export -f home menu panes
    home "open Trio" >/dev/null
    check "R132-e three links in table order" eventually 5 is menu "shop-web · develop|shop-api · develop|shared-ui · develop"
    before=$(panes)
    home "terminal shop-api" >/dev/null
    check "R132-e a Terminal opened on shop-api" eventually 8 bash -c "[[ \$(panes) != '$before' ]]"
    np=$(comm -13 <(tr ' ' '\n' <<<"$before") <(panes | tr ' ' '\n') | head -1)
    sleep 1
    rade send "$np" 'pwd' --enter >/dev/null
    check "R132-e …in shop-api's checkout" eventually 8 screen_has "$np" "$A/shop-api"
    home "open Trio" >/dev/null
    check "R132-e the link used last comes first" eventually 5 is menu "shop-api · develop|shop-web · develop|shared-ui · develop"
    home "click_open" >/dev/null
    check "R132-e with three links the click shows the menu, no Terminal" \
        eventually 3 bash -c "home | jq -e '.open_terminal_menu_up' >/dev/null && [[ \$(panes) == '$(panes)' ]]"
    before=$(panes)
    home "open Solo" >/dev/null
    sleep 0.5
    home "click_open" >/dev/null
    check "R132-e with one link Open terminal opens at once" eventually 8 bash -c "[[ \$(panes) != '$before' ]]"
fi

if part remove; then
    echo "== R133 remove a Project, prune Project checkouts"
    LIB=$A/lib
    mkdir -p "$LIB" && G -C "$LIB" init -q -b main
    echo x >"$LIB/x" && G -C "$LIB" add -A && G -C "$LIB" commit -qm init
    for b in develop release/2.4 release/2.5; do G -C "$LIB" branch "$b"; done
    rade source add "$LIB" --workspace acme >/dev/null
    PC=$A/.worktrees-dev/.projects
    # Store's shop-web base is its own (other parts' Projects link develop).
    G -C "$WEB" branch staging
    rade project add Store --workspace acme --source shop-web:staging --source lib:develop >/dev/null
    rade project add Billing --workspace acme --source lib:develop >/dev/null
    rade project add Rel --workspace acme --source lib:release/2.4 >/dev/null
    home() { rpc debug.ui "$(jq -nc --arg a "${1:-}" '{op: "home", arg: $a}')" | jq -c .home; }
    panes() { rade list --json | jq -r '[.panes[].pane] | sort | join(" ")'; }
    export -f home panes
    # Use each link once: their Project checkouts are made on first use.
    for pl in "Store shop-web" "Store lib" "Billing lib" "Rel lib"; do
        set -- $pl
        home "open $1" >/dev/null; sleep 0.3
        home "terminal $2" >/dev/null; sleep 1.5
    done
    # Store and Billing share lib's develop checkout (made under the first
    # Project by id: billing).
    check "R133 Store's shop-web checkout and the shared lib one exist" \
        eventually 10 bash -c "[[ -d '$PC/store/shop-web' && -d '$PC/billing/lib' && ! -e '$PC/store/lib' ]]"
    check "R133 Rel's checkout exists" eventually 10 test -d "$PC/rel/lib"
    rade worktree add shop-web feat --project store >/dev/null
    echo dirty >"$PC/rel/lib/dirty.txt"
    find "$PC" -maxdepth 2 >"$OUT/projects-dirs.txt"

    echo "-- R133-a / R133-d the plan and the dialog"
    rade project remove store --dry-run --json >"$OUT/remove-plan.json"
    check "R133-a dry run: removes shop-web's checkout only, closes 1 Pane, 1 Worktree to Scratch" \
        bash -c "jq -e '([.remove[].path] == [\"$PC/store/shop-web\"]) and (.keep == []) and (.panes | length == 1) and (.worktrees == [{source: \"shop-web\", worktree: \"feat\"}])' '$OUT/remove-plan.json' >/dev/null"
    check "R133-a dry run wrote nothing" test -f "$A/.rade/projects/store/project.toml"
    rade debug menu project store activate "Remove Project" >/dev/null
    check "R133-d Remove Project… asks with the 4 groups" \
        eventually 5 bash -c "rade debug dialog | jq -e '(.heading == \"Remove Project “Store”?\") and (.body | test(\"This can.t be undone\")) and (.body | test(\".worktrees-dev/.projects/store/shop-web\")) and (.body | test(\"Closes the Panes running there: \")) and (.body | test(\"Its Worktree moves to Scratch\"))' >/dev/null"
    rade debug dialog >"$OUT/remove-dialog.json"
    sleep 0.5
    shot remove-dialog
    before=$(panes)
    respond delete

    echo "-- R133-b removed"
    check "R133-b the Project's folder is gone" eventually 8 test ! -e "$A/.rade/projects/store"
    check "R133-b shop-web's checkout removed, not in git worktree list" \
        bash -c "[[ ! -e '$PC/store/shop-web' ]] && ! git -C '$WEB' worktree list | grep -q '.projects/store/shop-web'"
    check "R133-b lib's shared checkout kept: Billing uses it" test -d "$PC/billing/lib"
    check "R133-b the Pane in shop-web's checkout closed" bash -c "[[ \$(panes | wc -w) -lt \$(wc -w <<<'$before') ]]"
    check "R133-b feat is in Scratch with base staging" \
        eventually 3 is "rade worktree list shop-web --json | jq -r '.worktrees[] | select(.worktree==\"feat\") | \"\\(.project)@\\(.base)\"'" "null@staging"
    check "R133-b the Navigator has no Store" \
        eventually 5 bash -c "! rpc debug.ui '{\"op\":\"sidebar\"}' | jq -r '.sidebar[] | select(.kind==\"project\") | .title' | grep -qx Store"

    echo "-- R133-c no longer used, prune"
    rc=$(code rade project link set-base billing lib release/2.5)
    check "R133-c set-base reports the unused checkout (exit $rc)" \
        grep -qF "1 Project checkout no longer used: $PC/billing/lib (rade project prune)" "$T/out"
    rade project remove rel --json >"$OUT/remove-rel.json"
    check "R133-c a dirty checkout is kept by remove, with why" \
        bash -c "jq -e '.kept[0].path == \"$PC/rel/lib\" and (.kept[0].why | test(\"uncommitted\"))' '$OUT/remove-rel.json' >/dev/null && [[ -f '$PC/rel/lib/dirty.txt' ]]"
    # The palette's Prune Unused Project Checkouts… asks first.
    rpc debug.ui '{"op":"key","keys":"ctrl+shift+p"}' >/dev/null
    eventually 3 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .open) == true ]]" || true
    rade --json debug palette --ui type "> Prune Unused" >/dev/null
    eventually 5 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .busy) == false ]]" || true
    rade --json debug palette --ui key enter >/dev/null
    check "R133-c the palette's Prune… lists the clean one and the kept one" \
        eventually 5 bash -c "rade debug dialog | jq -e '(.heading == \"Prune Project checkouts?\") and (.body | test(\"$PC/billing/lib\")) and (.body | test(\"Keeps, with uncommitted changes:\\n$PC/rel/lib\"))' >/dev/null"
    respond cancel
    rade project prune --workspace acme --dry-run >"$OUT/prune-dry.txt"
    check "R133-c prune --dry-run lists the clean one and keeps the dirty one" \
        bash -c "grep -qx 'would remove $PC/billing/lib' '$OUT/prune-dry.txt' && grep -q '^kept $PC/rel/lib' '$OUT/prune-dry.txt' && [[ -d '$PC/billing/lib' ]]"
    rade project prune --workspace acme >"$OUT/prune.txt"
    check "R133-c prune removed it, the dirty one stays" \
        bash -c "[[ ! -e '$PC/billing/lib' && -f '$PC/rel/lib/dirty.txt' ]] && grep -qx 'removed $PC/billing/lib' '$OUT/prune.txt'"
fi

if part links; then
    echo "== R134 Ctrl+click across wrapped rows"
    # A stand-in browser: records the URLs GIO hands to the http handler.
    BROWSER_LOG=$T/browser.log
    mkdir -p "$XDG_DATA_HOME/applications"
    printf '[Desktop Entry]\nType=Application\nName=Fake Browser\nExec=sh -c '"'"'echo "$1" >> %s'"'"' sh %%u\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\n' "$BROWSER_LOG" \
        >"$XDG_DATA_HOME/applications/fakebrowser.desktop"
    printf '[Default Applications]\nx-scheme-handler/http=fakebrowser.desktop\nx-scheme-handler/https=fakebrowser.desktop\n' \
        >"$XDG_CONFIG_HOME/mimeapps.list"
    X=$A/linky
    mkdir -p "$X"
    rade source add "$X" --workspace acme >/dev/null
    a=$(rade spawn --cmd bash --source linky --name a --json | jq -r .pane)
    rade focus "$a" >/dev/null
    eventually 10 screen_has "$a" '$' || true
    viewer_path() { rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer")][0].kind.path // ""'; }
    export -f viewer_path
    rade send "$a" "tput cols > $T/cols" --enter
    eventually 5 test -s "$T/cols"
    cols=$(cat "$T/cols")
    # A path longer than the Terminal is wide: a soft wrap.
    long=docs
    while (( ${#long} < cols + 30 )); do long="$long/segment-$((${#long} % 97))-abcdefghij"; done
    long="$long/LONGTAIL.md"
    mkdir -p "$X/$(dirname "$long")" && echo '# long' >"$X/$long"
    rade send "$a" "clear; echo $long" --enter
    eventually 5 screen_has "$a" "LONGTAIL.md"
    rade debug click "$a" "LONGTAIL.md" >/dev/null
    check "R134-b soft wrap: a click on the 2nd row opens the whole path" eventually 5 is viewer_path "$X/$long"
    rade close "$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer")][0].id')" >/dev/null 2>&1 || true

    # A TUI that wraps at 60 columns itself, the next row indented.
    mkdir -p "$X/src/components/checkout" && echo x >"$X/src/components/checkout/ShippingAddressForm.tsx"
    url="https://example.com/orders/12345/items?expand=shipping&lang=vi"
    cat >"$T/tui.txt" <<TUI
⏺ I changed the checkout form and the order page so that a
  guest can pay. The form is in src/components/checkout/Shipp
  ingAddressForm.tsx and the API answer is at https://example.
  com/orders/12345/items?expand=shipping&lang=vi for now.
TUI
    rade send "$a" "clear; cat $T/tui.txt" --enter
    eventually 5 screen_has "$a" "ingAddressForm.tsx"
    sleep 0.3
    rade debug click "$a" "https://example." >/dev/null
    check "R134-b a hard-wrapped URL opens whole (not Ghostty's first-row part)" \
        eventually 5 bash -c "[[ \$(tail -1 '$BROWSER_LOG' 2>/dev/null) == '$url' ]]"
    : >"$BROWSER_LOG"
    rade debug click "$a" "com/orders" >/dev/null
    check "R134-b …also from its 2nd row" eventually 5 bash -c "[[ \$(tail -1 '$BROWSER_LOG' 2>/dev/null) == '$url' ]]"
    rade debug click "$a" "ingAddressForm" >/dev/null
    check "R134-b hard wrap with an indent: the 2nd row's part opens the whole path" \
        eventually 5 is viewer_path "$X/src/components/checkout/ShippingAddressForm.tsx"
    shot links-tui
fi

echo
echo "m51: $PASS passed, $FAIL failed"
((FAIL == 0))
