#!/usr/bin/env bash
# M36 end-to-end tests (ADR-0006, ADR-0034): Source links and Project
# checkouts (PRD R97–R100). Real git repositories in a temp Workspace
# `acme`: shop-web (main checkout on develop, develop 2 commits ahead of
# main), shop-api (main checkout on develop), shared-ui (main checkout on
# main; develop and release/2.4 only on a bare remote), billing. Shop links
# shop-web, shop-api, shared-ui at develop; Billing links billing and
# shared-ui at release/2.4. The Devel build puts Project checkouts under
# `.worktrees-dev/.projects/<project>/<source>`.
# Group 2: the N1–N5 reproduction (design §1): R99-a, R99-e (N2), R99-c (N3),
# R98-k (N1), R100-f, R100-d (N5), R98-n (N4).
# Group 3: R98-h (plan), R97-f / R97-g (add), R97-h (link edits), R99-b,
# R99-i (shared-ui Worktrees; a link without base), R99-f / R99-g (hotfix off
# main), R100-e.
# Usage: tests/e2e/m36.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m36
source "$HERE/lib.sh"

mkdir -p "$XDG_CONFIG_HOME/rade"
printf 'idle_seconds = 1\n' >"$XDG_CONFIG_HOME/rade/config.toml"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
# Navigator rows (1.0.0, R104): kind:title@depth; a section as «Scratch».
rows() { ui sidebar | jq -r '[.sidebar[] | select(.kind != "agent") | if .kind == "section" then "«\(.title)»" else "\(.kind):\(.title)@\(.depth)" end] | join(" ")'; }
# The depth-0 row a row sits under (its Project, or Scratch's rows).
parent_of() { ui sidebar | jq -r --arg k "$1" --arg t "$2" '.sidebar as $r | ($r | map(.kind == $k and .title == $t) | index(true)) as $i | if $i == null then "none" else ([$r[:$i][] | select(.depth == 0)] | last | "\(.kind):\(.title)") end'; }
link() { rade project show "$1" --json | jq -c --arg s "$2" '.links[] | select(.source == $s)'; }
export -f ui rpc rows link parent_of

# ---- The scene ----------------------------------------------------------------
A=$HOME/workspaces/acme
WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui BILL=$A/billing REMOTE=$T/shared-ui.git
WT=$A/.worktrees-dev
for r in "$WEB" "$API" "$UI" "$BILL"; do
    mkdir -p "$r" && G -C "$r" init -q -b main
    printf 'base\n' >"$r/README.md"
    for f in x y z; do printf '%s\n' "$f" >"$r/$f.txt"; done
    G -C "$r" add -A && G -C "$r" commit -qm init
done
# shop-web: develop is 2 commits ahead of main, its main checkout on develop.
G -C "$WEB" checkout -qb develop
for i in 1 2; do echo "dev $i" >"$WEB/dev$i.txt" && G -C "$WEB" add -A && G -C "$WEB" commit -qm "dev $i"; done
# shop-api: main checkout on develop.
G -C "$API" checkout -qb develop
# shared-ui: develop and release/2.4 only on the bare remote.
G init -q --bare "$REMOTE"
G -C "$UI" branch develop && G -C "$UI" branch release/2.4
# release/2.5 has a commit of its own (rel25.txt) for R99-i.
G -C "$UI" checkout -qb release/2.5 && echo r25 >"$UI/rel25.txt" && G -C "$UI" add -A && G -C "$UI" commit -qm "release 2.5"
G -C "$UI" checkout -q main
G -C "$UI" remote add origin "$REMOTE"
G -C "$UI" push -q origin main develop release/2.4 release/2.5
G -C "$UI" branch -qD develop release/2.4 release/2.5
G -C "$UI" fetch -q origin
mkdir -p "$A/.rade"
echo 'name = "acme"' >"$A/.rade/workspace.toml"

start_app
rade workspace add "$A" >/dev/null
for r in "$WEB" "$API" "$UI" "$BILL"; do rade source add "$r" --workspace acme >/dev/null; done
rade workspace switch acme >/dev/null
PROJ=$A/.rade/projects
wtl() { for r in "$WEB" "$UI"; do git -C "$r" worktree list --porcelain; done; }
dirs() { { find "$WT" -maxdepth 4 -type d 2>/dev/null || true; } | sort; }
export -f dirs

echo "== R98-h rade project plan: printed, nothing written"
wtl >"$T/wtl-0"
ls -laR "$A/.rade" >"$T/rade-ls-0" 2>&1 || true
rc=$(code rade project plan Shop --source shop-web:develop --source shared-ui:develop)
check "R98-h plan prints the main checkout and the new Project checkout, exit 0" \
    bash -c "[[ $rc == 0 ]] && grep -qE '^ *shop-web  main checkout' '$T/out' && grep -qE '^ *shared-ui  new project checkout \.worktrees-dev/\.projects/shop/shared-ui' '$T/out'"
wtl >"$T/wtl-1"
check "R98-h git worktree list and .rade/projects unchanged" \
    bash -c "[[ ! -e '$WT/.projects' && ! -e '$PROJ/shop' ]] && cmp -s '$T/wtl-0' '$T/wtl-1' && diff <(ls -laR '$A/.rade' 2>&1) '$T/rade-ls-0' >/dev/null"

echo "== R97-f rade project add twice: written once, then existing"
rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null
cp "$PROJ/shop/project.toml" "$T/shop-1.toml"
rc=$(code rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop --json)
check "R97-f first run: 3 [[source]] entries; second: exit 0, existing true, bytes equal" \
    bash -c "[[ \$(grep -c '^\[\[source\]\]' '$T/shop-1.toml') == 3 && $rc == 0 && \$(jq -r .existing '$T/out') == true ]] && cmp -s '$T/shop-1.toml' '$PROJ/shop/project.toml'"

echo "== R97-g the old Named Base form is refused"
ls -R "$PROJ" >"$T/proj-ls-1"
rc=$(code rade project add Shop --source shop-web:dev=develop)
check "R97-g --source shop-web:dev=develop: exit 2, names --source <src>[:<branch>], nothing written" \
    bash -c "[[ $rc == 2 ]] && grep -qF -- '--source <src>[:<branch>]' '$T/err' && cmp -s '$T/shop-1.toml' '$PROJ/shop/project.toml' && [[ \"\$(ls -R '$PROJ')\" == \"\$(cat '$T/proj-ls-1')\" ]]"
rade project add Billing --source billing --source shared-ui:release/2.4 >/dev/null

echo "== R97-h link add / set-base / remove: only that link's lines change"
F=$PROJ/shop/project.toml
d0=$(dirs)
cp "$F" "$T/h0"
rade project link add shop billing --base main >/dev/null && cp "$F" "$T/h1"
rade project link set-base shop shared-ui release/2.5 >/dev/null && cp "$F" "$T/h2"
rade project link remove shop billing >/dev/null && cp "$F" "$T/h3"
rc=$(code rade project link add shop nope)
# Changed lines of a step: diff's +/- lines without the ---/+++ header.
changed() { diff -U0 "$1" "$2" | grep -E '^[-+][^-+]' || true; }
export -f changed
check "R97-h link add appends only billing's lines" \
    bash -c "c=\$(changed '$T/h0' '$T/h1'); ! grep -q '^-' <<<\"\$c\" && grep -q 'billing' <<<\"\$c\" && ! grep -qE 'shop-web|shop-api|shared-ui' <<<\"\$c\""
check "R97-h set-base changes only shared-ui's base line" \
    is "changed '$T/h1' '$T/h2' | tr '\n' '|'" '-base = "develop"|+base = "release/2.5"|'
check "R97-h remove takes only billing's lines; final = first except shared-ui's base" \
    bash -c "c=\$(changed '$T/h2' '$T/h3'); ! grep -q '^+' <<<\"\$c\" && grep -q billing <<<\"\$c\" && [[ \$(changed '$T/h0' '$T/h3' | tr '\n' '|') == '-base = \"develop\"|+base = \"release/2.5\"|' ]] && grep -A2 'id = \"shared-ui\"' '$T/h3' | grep -q 'release/2.5'"
check "R97-h link add shop nope: exit 2, file unchanged; no folder under .worktrees-dev created or removed" \
    bash -c "[[ $rc == 2 ]] && cmp -s '$T/h3' '$F' && [[ \"\$(dirs)\" == '$d0' ]]"
rade project link set-base shop shared-ui develop >/dev/null

echo "== N2 R99-a a Worktree on the only link: cut from its base, recorded"
code rade worktree add shop-web checkout-v2 >/dev/null
CV=$WT/shop-web/checkout-v2
check "R99-a checkout-v2 starts at develop's commit, project shop, base develop" \
    bash -c "[[ -d '$CV' && \$(git -C '$WEB' rev-parse checkout-v2) == \$(git -C '$WEB' rev-parse develop) && \$(rade worktree list shop-web --json | jq -r '.worktrees[] | select(.worktree==\"checkout-v2\") | \"\(.project)@\(.base)\"') == shop@develop ]]"

echo "== N2 R99-e its row is under Shop"
# The row follows the Shop header before the next Project / section heading.
check "R99-e checkout-v2 is under Shop, not in No Project / Scratch" \
    eventually 8 bash -c "[[ \$(parent_of worktree checkout-v2) == project:Shop ]] && ! rows | grep -qE '«Scratch».*worktree:checkout-v2@0'"

echo "== N3 R99-c Review against the Worktree's base: only its own 4 files"
if [[ -d $CV ]]; then
    for f in a b; do echo "$f" >"$CV/$f.txt"; done
    G -C "$CV" add -A && G -C "$CV" commit -qm "own work"
    echo changed >>"$CV/x.txt"
    echo new >"$CV/c.txt"
    rv=$(rade review open --source shop-web --worktree checkout-v2 2>/dev/null || true)
fi
check "R99-c Review lists exactly a, b, c, x (none of develop's dev1 / dev2, which the Worktree has)" \
    eventually 10 is "test -f '$CV/dev1.txt' && rade debug review '${rv:-none}' info --json | jq -r '[.files[].path] | sort | join(\",\")'" "a.txt,b.txt,c.txt,x.txt"

echo "== R99-f / R99-g hotfix off main: recorded base main, Review against main"
code rade worktree add shop-web hotfix --base main >/dev/null
HF=$WT/shop-web/hotfix
check "R99-f hotfix starts at main, records base main with project shop" \
    bash -c "[[ -d '$HF' && \$(git -C '$WEB' rev-parse hotfix) == \$(git -C '$WEB' rev-parse main) && \$(rade worktree list shop-web --json | jq -r '.worktrees[] | select(.worktree==\"hotfix\") | \"\(.project)@\(.base)\"') == shop@main ]]"
if [[ -d $HF ]]; then
    echo fix >>"$HF/y.txt" && echo fix >"$HF/h.txt"
    G -C "$HF" add -A && G -C "$HF" commit -qm "hotfix"
    rh=$(rade review open --source shop-web --worktree hotfix 2>/dev/null || true)
fi
check "R99-g Review of hotfix lists exactly h, y (none of develop's dev1 / dev2)" \
    eventually 10 is "rade debug review '${rh:-none}' info --json | jq -r '[.files[].path] | sort | join(\",\")'" "h.txt,y.txt"

echo "== R99-b shared-ui is linked twice: --project needed"
rc=$(code rade worktree add shared-ui tokens-billing)
check "R99-b without --project: exit 2 listing shop and billing, nothing created" \
    bash -c "[[ $rc == 2 ]] && grep -q shop '$T/err' && grep -q billing '$T/err' && [[ ! -e '$WT/shared-ui/tokens-billing' ]] && ! git -C '$UI' rev-parse -q --verify refs/heads/tokens-billing >/dev/null"
rc=$(code rade worktree add shared-ui tokens-billing --project billing)
check "R99-b --project billing starts tokens-billing at release/2.4" \
    bash -c "[[ $rc == 0 && \$(git -C '$UI' rev-parse tokens-billing) == \$(git -C '$UI' rev-parse origin/release/2.4) && \$(rade worktree list shared-ui --json | jq -r '.worktrees[] | select(.worktree==\"tokens-billing\") | \"\(.project)@\(.base)\"') == billing@release/2.4 ]]"

echo "== R99-i set-base keeps a Worktree's recorded base and Review"
code rade worktree add shared-ui tokens --project shop >/dev/null
TK=$WT/shared-ui/tokens
if [[ -d $TK ]]; then
    for f in t1 t2 t3; do echo "$f" >"$TK/$f.txt"; done
    G -C "$TK" add -A && G -C "$TK" commit -qm "tokens"
fi
rade project link set-base shop shared-ui release/2.5 >/dev/null
rt=$(rade review open --source shared-ui --worktree tokens 2>/dev/null || true)
check "R99-i tokens still records base develop after set-base release/2.5" \
    is "rade worktree list shared-ui --json | jq -r '.worktrees[] | select(.worktree==\"tokens\") | \"\(.project)@\(.base)\"'" "shop@develop"
check "R99-i Review of tokens lists exactly t1, t2, t3 (not release/2.5's rel25.txt)" \
    eventually 10 is "rade debug review '${rt:-none}' info --json | jq -r '[.files[].path] | sort | join(\",\")'" "t1.txt,t2.txt,t3.txt"
rade project link set-base shop shared-ui develop >/dev/null

echo "== N1 R98-k the main checkout recorded on develop, then switched"
G -C "$API" switch -qc feat/checkout-v2
stop_app
start_app
check "R98-k shop-api: kind main, path = main checkout, drift.head feat/checkout-v2, no .projects/shop folder" \
    bash -c "l=\$(link shop shop-api); [[ \$(jq -r .checkout_kind <<<\"\$l\") == main && \$(jq -r .path <<<\"\$l\") == \$(realpath '$API') && \$(jq -r .drift.head <<<\"\$l\") == feat/checkout-v2 && ! -e '$WT/.projects/shop' ]]"

echo "== N5 R100-f / R100-d drifted and dirty: shown, never switched"
for f in x y z; do echo dirty >>"$API/$f.txt"; done
check "R100-f show --json: drift {head feat/checkout-v2, base develop, dirty 3}" \
    eventually 5 is "link shop shop-api | jq -c .drift" '{"head":"feat/checkout-v2","base":"develop","dirty":3}'
check "R100-f text line ends 'drift feat/checkout-v2 (base develop)'" \
    bash -c "rade project show shop | grep -E '^ *shop-api ' | grep -q 'drift feat/checkout-v2 (base develop)\$'"
rc=$(code rade project link to-base shop shop-api)
check "R100-d to-base on a dirty checkout: exit 2 naming 3 changes, HEAD kept, no stash" \
    bash -c "[[ $rc == 2 ]] && grep -q '3 uncommitted changes' '$T/err' && [[ \$(git -C '$API' branch --show-current) == feat/checkout-v2 && -z \$(git -C '$API' stash list) ]]"

echo "== R100-e clean again: to-base switches to develop, the branch is kept"
G -C "$API" checkout -q -- .
old_head=$(git -C "$API" rev-parse feat/checkout-v2)
rc=$(code rade project link to-base shop shop-api)
check "R100-e to-base on a clean checkout: exit 0, HEAD develop, feat/checkout-v2 unchanged" \
    bash -c "[[ $rc == 0 && \$(git -C '$API' branch --show-current) == develop && \$(git -C '$API' rev-parse feat/checkout-v2) == $old_head ]]"

echo "== N4 R98-n a Terminal on Billing's shared-ui link, then a Worktree of billing"
lt=$(rpc debug.action '{"action": "link_terminal", "project": "billing", "source": "shared-ui"}')
echo "  link_terminal → $lt"
PC=$WT/.projects/billing/shared-ui
rc=$(code rade worktree add billing shared-ui)
paths() { rade worktree list "$1" --json | jq -r '.worktrees[].path'; }
export -f paths
LT_PANE=$(jq -r '.pane // empty' <<<"$lt")
sleep 1
[[ -n $LT_PANE ]] && rade send "$LT_PANE" "test \"\$PWD\" -ef '$PC' && echo LINK-CWD-\$((1+1))" --enter >/dev/null 2>&1 || true
check "R98-n the Terminal on the link starts in the Project checkout" \
    eventually 8 screen_has "${LT_PANE:-none}" "LINK-CWD-2"
check "R98-n Project checkout on release/2.4 and Worktree billing/shared-ui both exist; no list shows the Project checkout" \
    bash -c "[[ -d '$PC' && \$(git -C '$PC' branch --show-current) == release/2.4 && -d '$WT/billing/shared-ui' && $rc == 0 ]] && [[ \$({ paths shared-ui; paths billing; } | wc -l) -ge 3 ]] && ! { paths shared-ui; paths billing; } | xargs -r -n1 realpath -m | grep -qx \"\$(realpath -m '$PC')\""

echo "== R99-i a link without base: the Worktree records the main branch it was cut from"
# Billing links billing with no base; R98-n's Worktree billing/shared-ui was cut from main.
wlb() { rade worktree list billing --json | jq -r '.worktrees[] | select(.worktree=="shared-ui") | "\(.project)@\(.base)"'; }
export -f wlb
check "R99-i billing/shared-ui records billing@main (link has no base)" is wlb "billing@main"
rade project link set-base billing billing release/9 >/dev/null
check "R99-i after set-base billing billing release/9 it still records billing@main" is wlb "billing@main"
shot m36-sidebar

echo
echo "m36: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
