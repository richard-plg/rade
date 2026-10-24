#!/usr/bin/env bash
# E2E (M37, PRD R100 UI, R104, R105, R107): the Navigator, the Inbox popover,
# Project home, the drift UI and Activity on the mockup's data set
# (docs/design/spec/a-workspace.md / b-project-home.md "Content"): Workspace
# `acme` with real git repositories shop-web (main checkout on develop, 2
# commits behind origin/develop on a bare remote), shop-api (drifts to
# feat/checkout-v2), shared-ui (main checkout on main; develop and
# release/2.4 only on a bare remote), billing; Projects Shop (shop-web,
# shop-api, shared-ui at develop), Billing (billing at main, shared-ui at
# release/2.4), Docs (no Sources); Worktrees checkout-v2 (Shop) and tokens
# (Billing); fake Agents dev-server, planner, tests, ui (tests/e2e/fake-agent.sh);
# 2 shells at the Workspace root. Scene B adds Worktree perf-img and a PR
# from it (tests/e2e/fake-gh.sh). The Devel build ⇒ `.worktrees-dev`.
# Part 1 (Group 2): R104-a, c, f, g, h, i; R105-a, b, d, e, f; R100-g
# (Navigator + Home), R100-h, R100-i; R107-d.
# Part 2 (Group 3): scene A again; R107-a, R100-g (Changes card), R107-e
# (palette), R106-a…f (dialogs), R107-g (Search scope), R107-h (identity);
# geometry R104-b, R107-b (tests/e2e/spec_a.py --rows), R105-c, R106-c,
# R107-f (tests/e2e/spec_bcd.py); goldens A–D light + dark against
# tests/e2e/golden/m37/ (tests/e2e/golden-diff.py; APPROVE=1 writes missing
# goldens from this run's shots), tests/e2e/design-diff.sh side by side
# with the mockup. Screenshots light + dark → target/e2e-m37/.
# Usage: tests/e2e/m37.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m37}
export WESTON_WIDTH=1440
source "$HERE/lib.sh"
FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/fakebin"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "$FAKE --model opus"
[agents.codex]
command = "$FAKE --model gpt-5"
[agents.opencode]
icon = "$T/opencode.svg"
C
# R115-e/f: hand-started agents (`agy`, `opencode`) are found by their
# process; opencode has the user's own icon.
printf '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" fill="#e5484d"/></svg>' > "$T/opencode.svg"
for a in agy opencode; do printf '#!/bin/bash\nwhile :; do sleep 1; done\n' > "$T/fakebin/$a"; chmod +x "$T/fakebin/$a"; done
ln -s "$HERE/fake-gh.sh" "$T/fakebin/gh"
export PATH=$T/fakebin:$PATH FAKE_GH_LOG=$T/gh.log FAKE_GH_PR_URL=https://example.test/acme/shop-web/pull/482

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false -c init.defaultBranch=main "$@"; }
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
nav() { ui sidebar | jq -c '.sidebar'; }
# Navigator rows as `kind:text@depth` (+ ! drift, ~ also in).
rows() { ui sidebar | jq -r '[.sidebar[] | "\(.kind):\(.text)@\(.depth)\(if .drift then "!" else "" end)\(if .also_in then "~" else "" end)"] | join(" | ")'; }
# The row of `kind` titled `title` (the first, or the `n`-th).
row() { ui sidebar | jq -c --arg k "$1" --arg t "$2" "[.sidebar[] | select(.kind == \$k and .title == \$t)][${3:-0}]"; }
home() { ui home "${1:-}" | jq -c '.home'; }
chrome() { ui chrome | jq -c .chrome; }
pane_status() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
panes() { rade list --json | jq -r '[.panes[].pane] | sort | join(" ")'; }
export -f ui rpc nav rows row home chrome pane_status panes

# Geometry, masks and goldens (part 2 and scene B).
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
TOKENS=$ROOT/crates/rade-app/src/tokens.toml
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+, $2}}"; }
changes() { ui changes | jq -c .changes; }
export -f geo changes
# Golden masks: Terminal contents, running LEDs / spinner, times ("2m", "now").
masks() { # file, extra selectors…
    local f=$1; shift
    local args=()
    for w in terminal glyph-running led-running nav-row-meta activity-time "$@"; do args+=("$(geo "$w" '"all":true')"); done
    python3 - "${args[@]}" <<'PY' >"$f"
import json, sys
out = []
for arg in sys.argv[1:]:
    for g in json.loads(arg):
        if not g.get("visible", True):
            continue
        out.append({"x": g["x"] - 2, "y": g["y"] - 2, "w": g["width"] + 4, "h": g["height"] + 4,
                    "why": ",".join(c for c in g["classes"] if not c.startswith("st-"))[:40]})
print(json.dumps(out, indent=1))
PY
}
spec_a() { python3 "$HERE/spec_a.py" "$SOCK" "$TOKENS" "$1" "$planner" "$tests" --rows "$2" >"$OUT/spec-a-$3-$1.txt" 2>&1; }
spec_bcd() { python3 "$HERE/spec_bcd.py" "$SOCK" "$TOKENS" "$1" "$2" >"$OUT/spec-$2-$1.txt" 2>&1; }
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
# No Toast in a golden scene (they carry temp paths and times).
notoasts() { rpc debug.ui '{"op":"toasts_clear"}' >/dev/null; sleep 0.6; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
primaries() { geo .suggested-action '"all":true' | jq -r '[.[] | .label] | join(",")'; }
export -f primaries


# ---- The repositories ---------------------------------------------------------
A=$HOME/workspaces/acme
WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui BILL=$A/billing
WT=$A/.worktrees-dev
REM=$T/remotes
mkdir -p "$REM"
for r in "$WEB" "$API" "$UI" "$BILL"; do
    mkdir -p "$r/src" && G -C "$r" init -q
    printf '# %s\n' "${r##*/}" >"$r/README.md"
    for f in a b c; do printf '%s\n' "$f" >"$r/src/$f.ts"; done
    G -C "$r" add -A && G -C "$r" commit -qm init
done
# shop-web: develop on a bare remote, which is 2 commits ahead of the local
# develop (the main checkout's `↓2`).
G -C "$WEB" checkout -qb develop
G init -q --bare "$REM/shop-web.git"
G -C "$WEB" remote add origin "$REM/shop-web.git"
G -C "$WEB" push -qu origin main develop 2>/dev/null
G clone -q -b develop "$REM/shop-web.git" "$T/web-clone"
for i in 1 2; do echo "up $i" >"$T/web-clone/up$i.txt"; G -C "$T/web-clone" add -A; G -C "$T/web-clone" commit -qm "up $i"; done
G -C "$T/web-clone" push -q origin develop
G -C "$WEB" fetch -q origin
# shop-api: main checkout on develop; feat/checkout-v2 4 commits ahead.
G -C "$API" checkout -qb develop
G -C "$API" checkout -qb feat/checkout-v2
for i in 1 2 3 4; do echo "v2 $i" >"$API/src/v2-$i.rs"; G -C "$API" add -A; G -C "$API" commit -qm "v2 $i"; done
G -C "$API" checkout -q develop
# shared-ui: develop and release/2.4 only on the bare remote.
G init -q --bare "$REM/shared-ui.git"
G -C "$UI" branch develop && G -C "$UI" branch release/2.4
G -C "$UI" remote add origin "$REM/shared-ui.git"
G -C "$UI" push -q origin main develop release/2.4 2>/dev/null
G -C "$UI" branch -qD develop release/2.4
G -C "$UI" fetch -q origin
# shared-ui: release/2.5 too (R106-a, R106-d), only on the remote.
G -C "$UI" branch release/2.5 origin/release/2.4 && G -C "$UI" push -q origin release/2.5 2>/dev/null
G -C "$UI" branch -qD release/2.5 && G -C "$UI" fetch -q origin
# The Workspace's identity and each repository's (R107-h, spec A.75).
for r in "$WEB" "$API" "$UI" "$BILL"; do git -C "$r" config user.email richard@acme.io; done
mkdir -p "$A/.rade"
printf 'name = "acme"\n[identity]\ngit_email = "richard@acme.io"\n' >"$A/.rade/workspace.toml"

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
rade workspace add "$A" >/dev/null
for r in "$WEB" "$API" "$UI" "$BILL"; do rade source add "$r" --workspace acme >/dev/null; done
rade workspace switch acme >/dev/null
rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null || echo "project add Shop failed"
rade project add Billing --source billing:main --source shared-ui:release/2.4 >/dev/null || echo "project add Billing failed"
rade project add Docs >/dev/null || echo "project add Docs failed"

# Worktrees: checkout-v2 (Shop, ↑3 with 4 changed files), tokens (Billing, ↑1).
CV=$(rade worktree add shop-web checkout-v2 --project shop --json | jq -r .path)
for f in a b c; do echo "v2 $f" >>"$CV/src/$f.ts"; G -C "$CV" add -A; G -C "$CV" commit -qm "checkout $f"; done
for f in src/a.ts src/b.ts src/c.ts README.md; do echo wip >>"$CV/$f"; done
TK=$(rade worktree add shared-ui tokens --project billing --json | jq -r .path)
echo tok >"$TK/src/tokens.ts" && G -C "$TK" add -A && G -C "$TK" commit -qm tokens

# Agents and shells.
id() { jq -r .pane; }
# The Workspace root's 2 shells: its first Terminal (if it has one) and one more.
r1=$(rade list --json | jq -r '[.panes[] | select(.source=="general@acme") | .pane][0] // empty')
[[ -n $r1 ]] || r1=$(rade spawn --source general@acme --tab new --json | id)
rade focus "$r1" >/dev/null
r2=$(rade spawn --source general@acme --tab current --split right --json | id)
dev=$(rade spawn --source shop-web --agent claude --name dev-server --tab new "hello" --json | id)
eventually 10 is "pane_status $dev" done
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new "osc 24 tests passed" --json | id)
eventually 10 is "pane_status $planner" done
sleep 1.5
rade focus "$planner" >/dev/null
tests=$(rade spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab current --split right "oscfirst Which fixtures?" --json | id)
eventually 10 is "pane_status $tests" waiting
uip=$(rade spawn --source shared-ui --worktree tokens --agent claude --name ui --tab new "spin" --json | id)
# fake-agent's own hook marks it `fake`: wait for it (else it can land
# after, and `ui` shows the `other` glyph, R115), then give each Pane its
# Preset back.
agent_of() { rade list --json | jq -r --arg p "$1" '(.panes // .)[] | select(.pane == $p) | .agent'; }
for p in "$dev" "$planner" "$uip" "$tests"; do eventually 10 is "agent_of $p" fake; done
for p in "$dev" "$planner" "$uip"; do rade status --pane "$p" --agent claude >/dev/null; done
rade status --pane "$tests" --agent codex >/dev/null
rade status running --pane "$uip" >/dev/null
sleep 1.5
# The Worktree checkout-v2 shown, the `tests` Pane focused (scene A).
rade focus "$tests" >/dev/null
AGENTS_TAB=$(rade list --json | jq -r --arg p "$tests" '.panes[] | select(.pane==$p) | .tab')
chrome() { ui chrome | jq -c .chrome; }
check "R20.9 tests went waiting on screen: no Toast for it" \
    bash -c "[[ \$(chrome | jq '[.toasts[] | select(.pane==\"$tests\")] | length') == 0 ]]"

echo "== R107-d Activity: tests waiting, then planner done, newest first"
act() { ui activity | jq -r --argjson n "${1:-2}" '[.activity[:$n][] | "\(.line) (\(.ago))"] | join(" | ")'; }
export -f act
check "R107-d first two Activity rows: tests asked which fixtures, planner finished · 24 tests passed (with ages)" \
    eventually 5 is "act 2" "tests asked which fixtures (now) | planner finished · 24 tests passed (now)"
check "R107-d …the Inspector's Activity tab shows them" \
    bash -c "ui right_page activity >/dev/null; [[ \$(ui chrome | jq -r .chrome.inspector.page) == activity ]]"

echo "== R104-h drift marker follows HEAD within 3 s"
drift() { row link shop-api | jq -r .drift; }
export -f drift
check "R104-h shop-api on develop: no marker" eventually 8 is drift false
G -C "$API" switch -q feat/checkout-v2
check "R104-h git switch feat/checkout-v2 → marker within 3 s" eventually 3 is drift true
G -C "$API" switch -q develop
check "R104-h git switch develop → gone within 3 s" eventually 3 is drift false
# Scene A: shop-api drifted with 3 uncommitted changes.
G -C "$API" switch -q feat/checkout-v2
echo dirty >>"$API/src/a.ts"; echo dirty >>"$API/src/b.ts"; echo new >"$API/src/new.rs"
eventually 5 is drift true

echo "== R104-a Navigator rows (scene A)"
SHOP_ROWS="project:Shop@0 | link:shop-web develop@1 | agent:dev-server@2 | worktree:checkout-v2@2 | agent:planner@3 | agent:tests@3 | link:shop-api@1! | link:shared-ui develop@1~"
check "R104-a Shop's rows: Shop, shop-web develop, dev-server, checkout-v2, planner, tests, shop-api (drift), shared-ui develop (also in)" \
    eventually 5 bash -c "rows | grep -qF '$SHOP_ROWS | project:Billing@0'"
check "R104-a the last section is Scratch with ~/workspaces/acme" \
    bash -c "ui sidebar | jq -e '[.sidebar[] | select(.kind==\"section\")] | last | .title == \"Scratch\"' >/dev/null && [[ \$(ui sidebar | jq -r '.sidebar as \$r | (\$r | map(.kind) | rindex(\"section\")) as \$i | \$r[\$i+1].title') == '~/workspaces/acme' ]]"
check "R104-a Billing, Docs (dim) in order; Scratch row meta 2 (the root's 2 shells)" \
    bash -c "rows | grep -qF 'project:Billing@0 | link:billing main@1 | link:shared-ui release/2.4@1~ | worktree:tokens@2 | agent:ui@3 | project:Docs@0 | section:Scratch@0 | scratch:~/workspaces/acme@0' && [[ \$(row project Docs | jq -r .dim) == true && \$(row scratch '~/workspaces/acme' | jq -r .meta) == 2 ]]"
nav >"$OUT/navigator-a.json"
check "R100-g Navigator: shop-api row has the warning-diamond marker, meta feat/checkout-v2" \
    test "$(row link shop-api | jq -r '[.drift, .meta] | join(" ")')" = "true feat/checkout-v2"

echo "== R104-c also-in tooltips"
check "R104-c Shop's shared-ui: 'Also in Billing @ release/2.4'" test "$(row link shared-ui 0 | jq -r .tooltip)" = "Also in Billing @ release/2.4"
check "R104-c Billing's shared-ui: 'Also in Shop @ develop'" test "$(row link shared-ui 1 | jq -r .tooltip)" = "Also in Shop @ develop"
check "R104-c shop-web (only Shop links it): no link-simple marker" test "$(row link shop-web | jq -r .also_in)" = false

echo "== screenshots: Navigator, scene A (dark, light)"
sleep 1
shot navigator-a-dark
rade debug action color_scheme_light >/dev/null; sleep 0.8
shot navigator-a-light
rade debug action color_scheme_dark >/dev/null

echo "== R104-f Scratch: the root's Terminal and a Worktree with no Project"
G -C "$WEB" worktree add -q "$WT/shop-web/spike" -b spike develop
rade worktree list shop-web >/dev/null
check "R104-f spike under Scratch with → Shop?" \
    eventually 8 bash -c "ui sidebar | jq -e '.sidebar as \$r | (\$r | map(.kind) | index(\"section\")) as \$i | [\$r[\$i:][] | select(.title==\"spike\" and .suggest==\"→ Shop?\")] | length == 1' >/dev/null"
check "R104-f the root's Terminals (~/workspaces/acme) under Scratch too" \
    bash -c "ui sidebar | jq -e '.sidebar as \$r | (\$r | map(.kind) | index(\"section\")) as \$i | [\$r[\$i:][] | select(.title==\"~/workspaces/acme\")] | length == 1' >/dev/null"
check "R104-f no row titled General or No Project" \
    bash -c "! ui sidebar | jq -r '.sidebar[].title' | grep -qE '^(General|No Project)'"
rade worktree remove shop-web spike --force >/dev/null 2>&1 || G -C "$WEB" worktree remove --force "$WT/shop-web/spike"

echo "== R104-g Inbox: 3 Notifications, pill, popover, Ctrl+Shift+G"
rade debug panel clear >/dev/null
rade notify --pane "$r1" "build finished" >/dev/null
sleep 1.1
rade notify --pane "$dev" "dev server ready on 5173" >/dev/null
sleep 1.1
rade focus "$r1" >/dev/null
sleep 0.3
rade send "$tests" "oscfirst Which fixtures?" --enter >/dev/null
eventually 10 bash -c "[[ \$(pane_status $tests) == running ]]"
eventually 10 is "pane_status $tests" waiting
sleep 1.5
inbox() { ui inbox "${1:-}" | jq -c .inbox; }
export -f inbox
check "R104-g the pill reads 'tests needs input'" eventually 5 is "inbox | jq -r .pill" "tests needs input"
rpc debug.ui '{"op":"inbox","arg":"pill"}' >"$OUT/inbox-open.json"
check "R104-g a click on the pill opens the popover" test "$(jq -r .inbox.open "$OUT/inbox-open.json")" = true
check "R104-g it lists 3 entries, newest (tests) first" \
    bash -c "[[ \$(jq '.inbox.rows | length' '$OUT/inbox-open.json') == 3 ]] && jq -r '.inbox.rows[0].text' '$OUT/inbox-open.json' | grep -q 'Which fixtures' && jq -r '.inbox.rows[2].text' '$OUT/inbox-open.json' | grep -q 'build finished'"
shot inbox-popover
echo "== R115-b each Agent's glyph in its status colour"
check "R115-b the pill shows tests's glyph (codex) waiting" \
    test "$(jq -r '.inbox.pill_glyph | "\(.glyph)/\(.status)"' "$OUT/inbox-open.json")" = codex/waiting
check "R115-b the newest Inbox row (tests) shows the codex glyph" test "$(jq -r '.inbox.rows[0].mark' "$OUT/inbox-open.json")" = codex
check "R115-b Navigator: planner = claude, tests = codex waiting" \
    bash -c "ui sidebar | jq -e '[.sidebar[] | select(.kind == \"agent\") | {(.title): \"\(.glyph)/\(.led)\"}] | add | .planner == \"claude/done\" and .tests == \"codex/waiting\"' >/dev/null"
check "R115-b tests's Pane header shows the codex glyph, waiting" \
    test "$(rpc debug.ui "{\"op\":\"pane\",\"pane\":\"$tests\"}" | jq -r '.pane.glyph | "\(.glyph)/\(.status)"')" = codex/waiting
ui inbox close >/dev/null
check "R104-g Esc closes it" eventually 3 is "inbox | jq -r .open" false
rade focus "$r1" >/dev/null
sleep 0.5
key() { rpc debug.ui "{\"op\":\"key\",\"keys\":\"$1\"}"; }
check "R104-g keyboard in a Terminal: Ctrl+G reaches the program (not RADE)" \
    test "$(key ctrl+g | jq -r .handled)" = terminal
key ctrl+shift+g >/dev/null
check "R104-g Ctrl+Shift+G from the Terminal focuses the tests Pane" \
    eventually 3 is "rade debug window --json | jq -r .focus.pane" "$tests"

echo "== R115-e/f a hand-started agent is found by its process; the user's icon"
glyph_of() { ui sidebar | jq -r --arg k "$1" '.sidebar[] | select(.key == $k) | "\(.glyph)/\(.led)"'; }
export -f glyph_of
hs=$(rade spawn --source shared-ui --tab new --json | id)
eventually 5 screen_has "$hs" '$'
check "R115-e a plain shell has no Agent row" test -z "$(glyph_of "$hs")"
rade send "$hs" "printf '\\033]2;Refactor auth\\007'; $T/fakebin/agy" --enter >/dev/null
check "R115-e 'agy' typed in the shell: an Agent row with the antigravity glyph within 4 s" \
    eventually 5 is "glyph_of $hs" antigravity/null
check "R104-p the row shows the session's title, not the Preset" \
    eventually 3 is "ui sidebar | jq -r --arg k $hs '.sidebar[] | select(.key == \$k) | .title'" "Refactor auth"
check "R115-e …its Pane header too" \
    test "$(rpc debug.ui "{\"op\":\"pane\",\"pane\":\"$hs\"}" | jq -r .pane.glyph.glyph)" = antigravity
shot r115-detected-agy
rade send "$hs" $'\x03' >/dev/null
check "R115-e Ctrl+C ends it: the row goes" eventually 5 is "glyph_of $hs" ""
rade send "$hs" "opencode" --enter >/dev/null
check "R115-f opencode (on PATH) is found, drawn with the user's icon" \
    eventually 5 bash -c "rpc debug.ui '{\"op\":\"pane\",\"pane\":\"$hs\"}' | jq -e '.pane.glyph | .glyph == \"opencode\" and .custom == true and .status == \"idle\"' >/dev/null"
shot r115-custom-icon
rade send "$hs" $'\x03' >/dev/null
rade close "$hs" --force >/dev/null 2>&1 || true
eventually 5 bash -c "[[ -z \$(glyph_of $hs) ]]"

echo "== R104-o the focused Agent's row is the selected one"
rade focus "$planner" >/dev/null
check "R104-o planner focused: its Agent row is the only selected row" \
    eventually 3 bash -c "nav | jq -e '[.[] | select(.selected) | .title] == [\"planner\"]' >/dev/null"
shot r104-o-agent-selected

echo "== R104-i Alt+2 → Billing's Project home"
rade focus "$tests" >/dev/null
key alt+2 >/dev/null
check "R104-i Projects in order Shop, Billing, Docs" \
    test "$(ui sidebar | jq -r '[.sidebar[] | select(.kind=="project") | .title] | join(",")')" = "Shop,Billing,Docs"
check "R104-i Alt+2 shows Billing's home and selects the Billing row" \
    eventually 3 bash -c "[[ \$(home | jq -r '[.shown, .name] | join(\" \")') == 'true Billing' && \$(row project Billing | jq -r .selected) == true ]]"
check "…breadcrumb Billing / Home" eventually 3 is "chrome | jq -r .breadcrumb" "Billing / Home"

echo "== R105-d Open terminal on Shop's shared-ui creates its Project checkout"
PC=$WT/.projects/shop/shared-ui
check "R105-d before: no Project checkout, main checkout on main" \
    bash -c "[[ ! -e '$PC' && \$(git -C '$UI' branch --show-current) == main ]]"
before=$(panes)
home "open Shop" >/dev/null
sleep 0.5
home "terminal shared-ui" >/dev/null
newpane() { comm -13 <(tr ' ' '\n' <<<"$before") <(panes | tr ' ' '\n') | head -1; }
eventually 10 bash -c "[[ -n \$(comm -13 <(tr ' ' '\n' <<<'$before') <(panes | tr ' ' '\n')) ]]"
np=$(newpane)
sleep 1
rade send "$np" 'pwd' --enter >/dev/null
check "R105-d pwd prints <root>/.worktrees-dev/.projects/shop/shared-ui" eventually 8 screen_has "$np" "$PC"
check "R105-d HEAD there is develop" test "$(git -C "$PC" branch --show-current 2>/dev/null)" = develop
check "R105-d a notice names the new folder" \
    eventually 3 bash -c "chrome | jq -r '.notes[]' | grep -qF 'Created the Project checkout $PC'"
check "R105-d the main checkout is still on main" test "$(git -C "$UI" branch --show-current)" = main

echo "== R104-j/l a Terminal on a Project checkout selects its link; Remove from Scratch"
rade focus "$np" >/dev/null
check "R104-j focused Terminal in Shop's shared-ui Project checkout: that link row is selected, no Scratch row is" \
    eventually 4 bash -c "[[ \$(row link shared-ui 0 | jq -r .selected) == true && \$(nav | jq '[.[] | select(.selected and .kind == \"scratch\")] | length') == 0 ]]"
before2=$(panes)
rade debug action split_right >/dev/null
rade debug action new_tab >/dev/null
eventually 5 bash -c "[[ \$(comm -13 <(tr ' ' '\n' <<<'$before2') <(panes | tr ' ' '\n') | wc -l) -ge 2 ]]"
sleep 1
for x in $(comm -13 <(tr ' ' '\n' <<<"$before2") <(panes | tr ' ' '\n')); do rade send "$x" 'pwd' --enter >/dev/null; done
for x in $(comm -13 <(tr ' ' '\n' <<<"$before2") <(panes | tr ' ' '\n')); do
    check "R104-n split / New Tab beside it starts in the Project checkout ($x)" eventually 8 screen_has "$x" "$PC"
done
check "R104-n …and adds no shared-ui Scratch row; the link stays selected" \
    eventually 4 bash -c "[[ \$(row scratch shared-ui) == null && \$(row link shared-ui 0 | jq -r .selected) == true ]]"
shot link-new-tab
for x in $(comm -13 <(tr ' ' '\n' <<<"$before2") <(panes | tr ' ' '\n')); do rade close "$x" --force >/dev/null 2>&1 || true; done
rade focus "$np" >/dev/null
check "R104-j the breadcrumb and Statusline name Shop (the selected link's Project), not Billing" \
    eventually 4 bash -c "[[ \$(chrome | jq -r .breadcrumb) == 'Shop / shared-ui' && \$(chrome | jq -r '.statusline.cells[1]') == Shop ]]"
sh=$(rade spawn --source shared-ui --tab new --json | id)
check "R104-l a shell in shared-ui's main checkout (no link uses it) adds a Scratch row" \
    eventually 4 bash -c "[[ \$(row scratch shared-ui | jq -r .meta) == 1 ]]"
check "R104-l its menu starts with Remove from Scratch…" \
    test "$(rade debug menu scratch shared-ui --json | jq -r '.items[0].label')" = "Remove from Scratch…"
check "R104-l the root's Scratch row offers no Remove from Scratch…" \
    bash -c "! rade debug menu scratch general@acme --json | jq -r '.items[].label' | grep -qx 'Remove from Scratch…'"
rade debug menu scratch shared-ui activate "Remove from Scratch" >/dev/null
eventually 3 bash -c "rade debug dialog --json | jq -e '.open' >/dev/null"
rade debug dialog --json >"$OUT/scratch-remove-dialog.json"
shot scratch-remove-dialog
check "R104-l the dialog names the Source and the 1 Pane it closes" \
    bash -c "jq -r '.heading' '$OUT/scratch-remove-dialog.json' | grep -qF 'Remove “shared-ui” from Scratch?' && jq -r '.body' '$OUT/scratch-remove-dialog.json' | grep -qF 'Closes its 1 Pane in $UI'"
rade debug dialog respond remove >/dev/null
check "R104-l after Close Panes: the shell is gone, the link's Terminal stays, no shared-ui Scratch row" \
    eventually 5 bash -c "! panes | tr ' ' '\n' | grep -qx '$sh' && panes | tr ' ' '\n' | grep -qx '$np' && [[ \$(row scratch shared-ui) == null ]]"
check "R104-l the Source is still linked by Shop" test "$(row link shared-ui 0 | jq -r .title)" = shared-ui

echo "== scene B: Worktree perf-img, a PR from it, planner done, tests waiting"
PI=$(rade worktree add shop-web perf-img --project shop --json | jq -r .path)
ps=$(rade spawn --source shop-web --worktree perf-img --tab new --json | id)
rade focus "$ps" >/dev/null
sleep 1
rpc debug.scm '{"op":"refresh"}' >/dev/null
sleep 1
rpc debug.scm '{"op":"pr"}' >/dev/null
eventually 8 bash -c "rpc debug.scm | jq -e '.pr != null' >/dev/null"
rpc debug.scm '{"op":"pr_fill","title":"Lazy-load product images"}' >/dev/null
rade debug dialog respond create >/dev/null
check "scene B: gh pr create ran from perf-img" eventually 10 bash -c "grep -q 'pr create --head perf-img' '$T/gh.log'"
eventually 5 bash -c "ui activity | jq -r '.activity[0].line' | grep -q '#482'"
rade close "$ps" --force >/dev/null 2>&1 || true
rade send "$planner" "osc 24 tests passed" --enter >/dev/null
eventually 10 bash -c "[[ \$(pane_status $planner) == running ]]"
eventually 10 is "pane_status $planner" done
sleep 1.5
rade send "$tests" "oscfirst Which fixtures?" --enter >/dev/null
eventually 10 bash -c "[[ \$(pane_status $tests) == running ]]"
eventually 10 is "pane_status $tests" waiting
rade status running --pane "$dev" >/dev/null
sleep 1.5
rade focus "$tests" >/dev/null

echo "== R105-a Project home from the Shop row"
rpc debug.ui '{"op":"nav_click","kind":"project","title":"Shop"}' >/dev/null
check "R105-a breadcrumb Shop / Home" eventually 3 is "chrome | jq -r .breadcrumb" "Shop / Home"
kpis() { home | jq -r '[.kpis[] | "\(.value) \(.detail)"] | join(" | ")'; }
export -f kpis
check "R105-a KPIs: 3 1 waiting · 1 running | 2 checkout-v2, perf-img | 7 files in 2 sources" \
    eventually 8 is kpis "3 1 waiting · 1 running | 2 checkout-v2, perf-img | 7 files in 2 sources"
check "R105-a the Shop row is selected" test "$(row project Shop | jq -r .selected)" = true
check "R105 header buttons Settings, Open terminal; sub-line" \
    test "$(home | jq -r '[(.buttons | join(",")), .sub] | join(" | ")')" = "Settings,Open terminal | acme · 3 sources · .rade/projects/shop/project.toml"

echo "== R105-b link table"
links() { home | jq -r '[.links[] | "\(.name) \(.checkout)\(if .path then " " + .path else "" end) \(.head) \(.vs_base) \(.chip)"] | join(" | ")'; }
export -f links
check "R105-b rows: shop-web main checkout develop ↓2 clean; shop-api … ↑4 ↓0 off base · 3 changes; shared-ui project checkout … = also in · Billing @ release/2.4" \
    eventually 8 is links "shop-web main checkout develop ↓2 clean | shop-api main checkout feat/checkout-v2 ↑4 ↓0 off base · 3 changes | shared-ui project checkout .worktrees-dev/.projects/shop/shared-ui develop = also in · Billing @ release/2.4"
check "R100-g Home: shop-api chip 'off base · 3 changes' (chip-warn)" \
    test "$(home | jq -r '.links[] | select(.name=="shop-api") | "\(.chip) \(.tone)"')" = "off base · 3 changes chip-warn"
check "R100-g …its menu has the dirty drift actions only: Keep, show in Shop and Move to a Worktree…" \
    test "$(home | jq -r '[.links[] | select(.name=="shop-api") | .actions[][0]] | join(",")')" = "Keep, show in Shop,Move to a Worktree…"

echo "== R105-e Recent activity"
recent() { home | jq -r '[.activity[] | "\(.actor) \(.text)"] | join(" | ")'; }
export -f recent
check "R105-e tests asked which fixtures | planner finished · 24 tests passed | #482 opened from perf-img" \
    eventually 5 is recent "tests asked which fixtures | planner finished · 24 tests passed | #482 opened from perf-img"
check "R105 Worktrees card rows" \
    test "$(home | jq -r '[.worktrees[] | "\(.name) \(.sub) \(.meta)"] | join(" | ")')" = "checkout-v2 shop-web ← develop ↑3 · 4 changed · 2 agents | perf-img shop-web ← develop clean"
home >"$OUT/home-b.json"
echo "== R105-c Project home geometry (scene B, no Inspector), golden B"
[[ $(chrome | jq -r .right_panel.toggle) == true ]] && rade debug action toggle_file_tree >/dev/null
# Spec B.3: Shop's links collapsed (folded here, unfolded after the shots).
shopkey=$(ui sidebar | jq -r '.sidebar[] | select(.kind=="project" and .title=="Shop") | .key')
shoplinks=$(ui sidebar | jq -r --arg k "$shopkey/" '.sidebar[] | select(.kind=="link" and (.key | startswith($k))) | .key')
for k in $shoplinks; do rpc debug.ui "{\"op\":\"nav_toggle\",\"key\":\"$k\"}" >/dev/null; done
check "B.3 the collapsed shop-web link carries the waiting badge (R104.2)" \
    test "$(ui sidebar | jq -r --arg k "$shopkey/" '[.sidebar[] | select(.kind=="link" and (.key | startswith($k))) | "\(.title) \(.collapsed) \(.badge)"] | join(",")')" \
    = "shop-web true 1,shop-api true 0,shared-ui true 0"
sleep 1.5
for th in dark light; do
    [[ $th == light ]] && light
    check "R105-c $th: spec B rows within tolerance" spec_bcd "$th" b
    grep ^FAIL "$OUT/spec-b-$th.txt"
    notoasts
    masks "$OUT/masks-b-$th.json" kpi-detail
    shot "b-$th"
done
dark
for k in $shoplinks; do rpc debug.ui "{\"op\":\"nav_toggle\",\"key\":\"$k\"}" >/dev/null; done
rade debug action toggle_file_tree >/dev/null
home "open Docs" >/dev/null
check "R105-e Docs (no Activity): one empty-state line" \
    eventually 3 is "home | jq -r '[.name, (.activity | length), .activity_empty] | join(\"|\")'" "Docs|0|No activity since RADE started"
home "open Shop" >/dev/null

echo "== R105-f a Worktree row opens it with its last Tab"
check "R105-f before: checkout-v2 reads shop-web ← develop ↑3 · 4 changed · 2 agents" \
    test "$(home | jq -r '.worktrees[] | select(.name=="checkout-v2") | "\(.sub) \(.meta)"')" = "shop-web ← develop ↑3 · 4 changed · 2 agents"
home "worktree checkout-v2" >/dev/null
check "R105-f the window shows Worktree checkout-v2 with its last Tab" \
    eventually 3 bash -c "[[ \$(rade debug state | jq -r --arg t '$AGENTS_TAB' '[.sources[].worktrees[] | select(.name==\"checkout-v2\") | .active_tab][0] == \$t') == true && \$(home | jq -r .shown) == false && \$(chrome | jq -r .breadcrumb) == 'Shop / shop-web / checkout-v2 ← develop ↑3' ]]"

echo "== R100-h Keep hides the markers until HEAD changes"
home "open Shop" >/dev/null
home "action shop-api keep" >/dev/null
check "R100-h after Keep: no Navigator marker, Home chip no longer off base" \
    eventually 3 bash -c "[[ \$(drift) == false && \$(home | jq -r '.links[] | select(.name==\"shop-api\") | .chip') != off* ]]"
G -C "$API" switch -qc other
check "R100-h git switch -c other → the markers show again within 3 s" \
    eventually 3 bash -c "[[ \$(drift) == true && \$(home | jq -r '.links[] | select(.name==\"shop-api\") | .chip') == 'off base · 3 changes' ]]"

echo "== R100-i Move to a Worktree… on a clean drift"
G -C "$API" switch -q feat/checkout-v2
G -C "$API" stash -qu
eventually 5 bash -c "[[ \$(home | jq -r '.links[] | select(.name==\"shop-api\") | .chip') == 'off base' ]]"
home "action shop-api move_to_worktree" >/dev/null
eventually 3 bash -c "rade debug dialog --json | jq -e '.open' >/dev/null"
rade debug dialog --json >"$OUT/move-dialog.json"
check "R100-i one confirmation naming both steps" \
    bash -c "jq -r .body '$OUT/move-dialog.json' | grep -q 'back to develop, then adds a Worktree of branch feat/checkout-v2 under Shop'"
rade debug dialog respond move >/dev/null
check "R100-i the checkout is on develop" eventually 10 is "git -C '$API' branch --show-current" develop
check "R100-i a Worktree of branch feat/checkout-v2 under Shop's shop-api link" \
    eventually 10 bash -c "[[ \$(rade worktree list shop-api --json | jq -r '.worktrees[] | select(.worktree==\"feat/checkout-v2\") | .project') == shop ]] && rows | grep -qF 'link:shop-api develop@1 | worktree:feat/checkout-v2@2'"
check "R100-i the drift markers are gone" \
    eventually 5 bash -c "[[ \$(drift) == false && \$(home | jq -r '.links[] | select(.name==\"shop-api\") | .drift') == false ]]"

# ============================ Part 2 (Group 3) ============================
echo "== scene A again: shop-api drifted and dirty, Worktrees checkout-v2 and tokens"
rade worktree remove shop-api feat/checkout-v2 --force >/dev/null 2>&1 || echo "remove feat/checkout-v2 failed"
rade worktree remove shop-web perf-img --force >/dev/null 2>&1 || echo "remove perf-img failed"
rade close "$np" --force >/dev/null 2>&1 || true
G -C "$API" switch -q feat/checkout-v2
echo dirty >>"$API/src/a.ts"; echo dirty >>"$API/src/b.ts"; echo new >"$API/src/new.rs"
rade status done --pane "$dev" >/dev/null
rade focus "$tests" >/dev/null
# Leave Project home: the tests row (scene A selects checkout-v2's row).
rpc debug.ui '{"op":"nav_click","kind":"agent","title":"tests"}' >/dev/null
[[ $(chrome | jq -r .right_panel.toggle) == true ]] || rade debug action toggle_file_tree >/dev/null
ui right_page changes >/dev/null
A_GROUPS="shop-web checkout-v2 4 files | shop-api feat/checkout-v2 3 files | shared-ui develop clean"
groups_text() { changes | jq -r '[.groups[] | "\(.name) \(.label) \(if .clean then "clean" else "\(.rows) files" end)"] | join(" | ")'; }
export -f groups_text
eventually 15 is groups_text "$A_GROUPS"
eventually 5 bash -c "[[ \$(changes | jq -r '.groups[1].drift != null') == true ]]"
sleep 1
changes >"$OUT/changes-a.json"

echo "== R107-a Changes by Project: header, 3 link groups"
check "R107-a the Inspector shows Changes by Project" test "$(jq -r .mode "$OUT/changes-a.json")" = project
check "R107-a header PROJECT · SHOP" test "$(jq -r .header "$OUT/changes-a.json")" = "PROJECT · SHOP"
check "R107-a groups: shop-web checkout-v2 4 files | shop-api 3 files | shared-ui develop clean" \
    test "$(jq -r '[.groups[] | "\(.name) \(.label) \(if .clean then "clean" else "\(.rows) files" end)"] | join(" | ")' "$OUT/changes-a.json")" \
    = "shop-web checkout-v2 4 files | shop-api feat/checkout-v2 3 files | shared-ui develop clean"
check "R107-a shop-web meta ↑3 · 4, its own Commit (the one primary)" \
    test "$(jq -r '.groups[0] | "\(.meta) \(.commit.shown) \(.commit.primary)"' "$OUT/changes-a.json")" = "↑3 · 4 true true"
check "R107-a every changed group has its own commit box; only one primary" \
    test "$(jq -r '[.groups[] | select(.clean | not) | .commit.shown] | all' "$OUT/changes-a.json")/$(jq '[.groups[] | select(.commit.primary)] | length' "$OUT/changes-a.json")" = "true/1"
check "R107-a header totals +a −r are the sum of the file rows" \
    test "$(jq -r '"+\([.groups[].files[].add] | add) −\([.groups[].files[].del] | add)"' "$OUT/changes-a.json")" = "$(jq -r '"\(.add) \(.del)"' "$OUT/changes-a.json")"
check "R107-a the Inspector tab counts the Project's 7 files" eventually 5 is "chrome | jq -r '.inspector.labels[0]'" "Changes 7"

echo "== R100-g Changes: the drift card's text and exactly its 2 buttons"
check "R100-g card: shop-api is on feat/checkout-v2, Shop's base is develop. 3 uncommitted changes, so RADE won't switch it." \
    test "$(jq -r '.groups[1].drift.full' "$OUT/changes-a.json")" = "shop-api is on feat/checkout-v2, Shop's base is develop. 3 uncommitted changes, so RADE won't switch it."
check "R100-g …exactly the buttons Keep, show in Shop and Move to a Worktree… (no Back to base: dirty)" \
    test "$(jq -r '.groups[1].drift.buttons | join("|")' "$OUT/changes-a.json")" = "Keep, show in Shop|Move to a Worktree…"
check "R100-g …and on screen: 2 drift-card buttons" \
    eventually 5 is "geo 'drift-card btn' '\"all\":true' | jq -r '[.[].label] | join(\"|\")'" "Keep, show in Shop|Move to a Worktree…"

echo "== R104-b / R107-b geometry of scene A (spec_a.py --rows), golden A"
rade focus "$tests" >/dev/null
sleep 1.5
for th in dark light; do
    [[ $th == light ]] && light
    check "R104-b $th: spec A rows A.17–A.38 within tolerance" spec_a "$th" A.17-A.38 nav
    grep ^FAIL "$OUT/spec-a-nav-$th.txt"
    check "R107-b $th: spec A rows A.58–A.69 within tolerance" spec_a "$th" A.58-A.69 changes
    grep ^FAIL "$OUT/spec-a-changes-$th.txt"
    notoasts
    masks "$OUT/masks-a-$th.json"
    shot "a-$th"
done
dark
check "scene A: exactly one primary button (Commit)" eventually 3 is primaries Commit

echo "== R107-e / R107-f palette 'shared' (scene D)"
# A file of the focused Worktree that matches (spec D.19's FILES row).
mkdir -p "$CV/src/shared" && printf 'export const theme = 0\n' >"$CV/src/shared/theme.ts"
pal() { rade --json debug palette --ui "$@"; }
q() { rade --json debug palette --limit "${2:-30}" -- "$1"; }
q shared >"$OUT/palette-shared.json"
check "R107-e first two results kind link: shared-ui · Shop · develop, shared-ui · Billing · release/2.4 · 1 agent" \
    test "$(jq -r '[.results[:2][] | "\(.kind):\(.title) · \(.detail)"] | join(" | ")' "$OUT/palette-shared.json")" \
    = "link:shared-ui · Shop · develop | link:shared-ui · Billing · release/2.4 · 1 agent"
check "R107-e actions Add shared-ui to Project…, Change base of shared-ui in Shop…, Fan-out on shared-ui… listed" \
    bash -c "jq -r '.results[] | select(.kind==\"action\") | .title' '$OUT/palette-shared.json' | grep -qxF 'Add shared-ui to Project…' && jq -r '.results[] | select(.kind==\"action\") | .title' '$OUT/palette-shared.json' | grep -qxF 'Change base of shared-ui in Shop…' && jq -r '.results[] | select(.kind==\"action\") | .title' '$OUT/palette-shared.json' | grep -qxF 'Fan-out on shared-ui…'"
check "R107-e the Worktree tokens · Billing · shared-ui ← release/2.4 · ↑1" \
    test "$(jq -r '[.results[] | select(.kind=="worktree") | "\(.title) · \(.detail)"][0]' "$OUT/palette-shared.json")" = "tokens · Billing · shared-ui ← release/2.4 · ↑1"
q "" 200 >"$OUT/palette-all.json"
check "R107-e no title contains /home, p_ or General (every item of the empty query too)" \
    bash -c "! jq -r '.results[].title' '$OUT/palette-shared.json' '$OUT/palette-all.json' | grep -qE '/home|p_|General'"
rade focus "$tests" >/dev/null
key ctrl+shift+p >/dev/null
eventually 3 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .open) == true ]]"
pal type shared >/dev/null
eventually 5 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .busy) == false ]]"
pal info >"$OUT/palette-ui.json"
check "R107-f sections SOURCES, WORKTREES, ACTIONS … and the scope chips" \
    test "$(jq -r '[.sections[:3][]] | join(",")' "$OUT/palette-ui.json")|$(jq -r '.scopes | join(",")' "$OUT/palette-ui.json")" \
    = "SOURCES,WORKTREES,ACTIONS|All,> Actions,@ Agents,/ Sources,# Files,\$ Symbols"
check "R107-f no primary button while the palette is open (D.22)" eventually 3 is primaries ""
for th in dark light; do
    [[ $th == light ]] && light
    check "R107-f $th: spec D rows within tolerance" spec_bcd "$th" d
    grep ^FAIL "$OUT/spec-d-$th.txt"
    notoasts
    masks "$OUT/masks-d-$th.json"
    shot "d-$th"
done
dark
key escape >/dev/null
rm -rf "$CV/src/shared"

echo "== R106-a … R106-c New Project dialog (scene C)"
snap_files() { (cd "$A" && find . -path ./.git -prune -o -newer "$T/mark" -print 2>/dev/null | grep -v '^\.$' | grep -vE '\.git/|/\.git$|state' | sort); }
wtl() { for r in "$WEB" "$API" "$UI"; do git -C "$r" worktree list --porcelain; done; }
wtl >"$T/wt-before"
touch "$T/mark"; sleep 1
dlg() { ui "$@" | jq -c .dialog; }
dlgop() { rpc debug.ui "$1" | jq -c .dialog; }
export -f dlg dlgop
dlgop '{"op":"dialog_open","kind":"new_project"}' >/dev/null
dlgop '{"op":"dialog_name","text":"Checkout revamp"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shop-web","base":"develop"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shop-api","base":"develop"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shared-ui","base":"release/2.5"}' >/dev/null
plans() { dlgop '{"op":"dialog"}' | jq -r '[.rows[] | "\(.name): \(.plan)"] | join(" | ")'; }
export -f plans
C19="shop-web: main checkout · on develop · shared with Shop | shop-api: new project checkout · main checkout is on feat/checkout-v2 | shared-ui: new project checkout · Shop uses develop, Billing release/2.4"
check "R106-a plan texts within 200 ms of the change = spec C.19" eventually 3 is plans "$C19"
rade project plan "Checkout revamp" --source shop-web:develop --source shop-api:develop --source shared-ui:release/2.5 --json >"$OUT/plan-c.json"
check "R106-a …= project.plan's plan for the same input" \
    test "$(jq -r '[.links[] | "\(.source): \(.plan)"] | join(" | ")' "$OUT/plan-c.json")" = "$C19"
dlgop '{"op":"dialog"}' >"$OUT/dialog-c.json"
check "R106-a the Name input has the keyboard; buttons Cancel, Create Project" \
    test "$(jq -r '[.focus, (.buttons | join(","))] | join(" ")' "$OUT/dialog-c.json")" = "project-name-entry Cancel,Create Project"
check "R106-a nothing written yet (no file or folder under the Workspace)" \
    bash -c "[[ ! -e '$A/.rade/projects/checkout-revamp' && ! -e '$WT/.projects/checkout-revamp' && -z \$(snap_files) ]]"
check "R106-c exactly one primary button (Create Project)" eventually 3 is primaries "Create Project"
for th in dark light; do
    [[ $th == light ]] && light
    check "R106-c $th: spec C rows within tolerance" spec_bcd "$th" c
    grep ^FAIL "$OUT/spec-c-$th.txt"
    notoasts
    masks "$OUT/masks-c-$th.json"
    shot "c-$th"
done
dark
[[ -n ${PAUSE_C:-} ]] && { echo "paused at C: RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock"; sleep "$PAUSE_C"; }
dlgop '{"op":"dialog_primary"}' >/dev/null
PF=$A/.rade/projects/checkout-revamp/project.toml
check "R106-b Create → project.toml written" eventually 5 test -f "$PF"
cp "$PF" "$OUT/checkout-revamp.toml" 2>/dev/null
check "R106-b 3 [[source]] with those bases and checkout = \"auto\"" \
    bash -c "[[ \$(grep -c '^\[\[source\]\]' '$PF') == 3 && \$(grep -c '^checkout = \"auto\"' '$PF') == 3 ]] && grep -A2 'id = \"shared-ui\"' '$PF' | grep -q 'base = \"release/2.5\"' && grep -A2 'id = \"shop-api\"' '$PF' | grep -q 'base = \"develop\"'"
wtl >"$T/wt-after"
check "R106-b git worktree list of each Source unchanged" cmp -s "$T/wt-after" "$T/wt-before"
check "R106-b the dialog closed" eventually 3 is "dlgop '{\"op\":\"dialog\"}' | jq -r .open" false
rm -rf "$A/.rade/projects/checkout-revamp"
eventually 8 bash -c "! ui sidebar | jq -r '.sidebar[].title' | grep -qx 'Checkout revamp'"

echo "== R107-a Commit in shop-web commits only shop-web's staged files"
apihead=$(git -C "$API" rev-parse HEAD)
rpc debug.ui '{"op":"changes_stage","source":"shop-web","path":"src/a.ts"}' >/dev/null
eventually 5 bash -c "[[ \$(changes | jq -r '.groups[0].staged') == 1 ]]"
rpc debug.ui '{"op":"changes_message","source":"shop-web","text":"feat: checkout form"}' >/dev/null
check "R107-a shop-web's Commit is enabled (1 staged, a message)" eventually 3 is "changes | jq -r .groups[0].commit.enabled" true
check "R107-a shop-api's Commit is not (nothing staged)" test "$(changes | jq -r .groups[1].commit.enabled)" = false
rpc debug.ui '{"op":"changes_commit","source":"shop-web"}' >/dev/null
check "R107-a the commit holds only src/a.ts of checkout-v2" eventually 5 is "git -C '$CV' show --name-only --format=%s HEAD | tr '\n' ' '" "feat: checkout form  src/a.ts "
check "R107-a …shop-api untouched (HEAD and its 3 changes)" \
    bash -c "[[ \$(git -C '$API' rev-parse HEAD) == $apihead && \$(git -C '$API' status --porcelain | wc -l) == 3 ]]"
check "R107-a shop-web's group now 3 files, ↑4" eventually 5 is "changes | jq -r '.groups[0] | \"\(.rows) \(.meta)\"'" "3 ↑4 · 3"

echo "== R106-d Change base… shows the plan first; only the base line changes"
SF=$A/.rade/projects/shop/project.toml
TS=$(rade worktree add shared-ui tokens-shop --project shop --json | jq -r .path)
cp "$SF" "$T/shop-before.toml"
dlgop '{"op":"dialog_open","kind":"change_base","project":"shop","source":"shared-ui"}' >/dev/null
check "R106-d the dialog shows the link's plan before anything is written" \
    eventually 3 bash -c "[[ -n \$(dlgop '{\"op\":\"dialog\"}' | jq -r '.rows[0].plan') ]] && cmp -s '$SF' '$T/shop-before.toml'"
dlgop '{"op":"dialog_row","source":"shared-ui","base":"release/2.5"}' >/dev/null
eventually 3 bash -c "[[ \$(dlgop '{\"op\":\"dialog\"}' | jq -r '.rows[0].base') == release/2.5 ]]"
dlgop '{"op":"dialog"}' >"$OUT/dialog-change-base.json"
dlgop '{"op":"dialog_primary"}' >/dev/null
eventually 5 bash -c "! cmp -s '$SF' '$T/shop-before.toml'"
diff "$T/shop-before.toml" "$SF" >"$OUT/change-base.diff"
check "R106-d project.toml differs only in shared-ui's base line" \
    test "$(grep -E '^[<>]' "$OUT/change-base.diff" | tr '\n' '|')" = '< base = "develop"|> base = "release/2.5"|'
check "R106-d Worktree tokens-shop still records base develop" \
    test "$(rade worktree list shared-ui --json | jq -r '.worktrees[] | select(.worktree=="tokens-shop") | "\(.project)@\(.base)"')" = "shop@develop"
check "R106-d Billing's tokens keeps release/2.4" \
    test "$(rade worktree list shared-ui --json | jq -r '.worktrees[] | select(.worktree=="tokens") | .base')" = "release/2.4"
rade project link set-base shop shared-ui develop >/dev/null
rade worktree remove shared-ui tokens-shop --force >/dev/null 2>&1

echo "== R106-e Add source… appends a 4th [[source]], every other byte unchanged"
cp "$SF" "$T/shop-before.toml"
dlgop '{"op":"dialog_open","kind":"add_source","project":"shop"}' >/dev/null
dlgop '{"op":"dialog_add","source":"billing","base":"main"}' >/dev/null
eventually 3 bash -c "[[ -n \$(dlgop '{\"op\":\"dialog\"}' | jq -r '.rows[] | select(.source==\"billing\") | .plan') ]]"
check "R106-e the 3 existing links are shown read-only, billing added" \
    test "$(dlgop '{"op":"dialog"}' | jq -r '[.rows[] | "\(.source):\(.existing)"] | join(",")')" = "shop-web:true,shop-api:true,shared-ui:true,billing:false"
dlgop '{"op":"dialog"}' >"$OUT/dialog-add.json"
# Spec C.14 needs a one-line plan, which scene C's texts never are (decision
# M37): Add Source shows Shop's links with short plans.
c14() { python3 - "$(geo link-editor-row '"all":true')" "$(geo link-plan '"all":true')" <<'PY'
import json, sys
rows, plans = json.loads(sys.argv[1]), json.loads(sys.argv[2])
one = [r["height"] for r, p in zip(rows, plans) if p["height"] < 20]
print(one)
sys.exit(0 if one and all(abs(h - 43) <= 1 for h in one) else 1)
PY
}
check "R106-c C.14 a link row whose plan is one line is 43 px ($(c14 2>/dev/null))" c14
rpc debug.ui '{"op":"dialog_primary"}' >"$OUT/dialog-add-primary.json" 2>&1
eventually 5 bash -c "! cmp -s '$SF' '$T/shop-before.toml'"
cp "$SF" "$OUT/shop-added.toml"
check "R106-e a 4th [[source]] billing / main appended" \
    bash -c "[[ \$(grep -c '^\[\[source\]\]' '$SF') == 4 ]] && grep -A2 'id = \"billing\"' '$SF' | grep -q 'base = \"main\"'"
check "R106-e every other byte unchanged (the old file is a prefix + the new entry)" \
    python3 -c "
import sys
old = open('$T/shop-before.toml').read(); new = open('$SF').read()
i = new.find('\n[[source]]\nid = \"billing\"')
add = new[i:new.index('\n', new.index('base = \"main\"', i)) + 1] if i >= 0 else ''
sys.exit(0 if i >= 0 and new.replace(add, '\n', 1).replace('\n\n\n', '\n\n') in (old, old + '\n') or new.replace(add, '', 1) == old else 1)"
rade project link remove shop billing >/dev/null

echo "== R106-f Project settings: Name and the link table"
ui settings_open shop >/dev/null
rpc debug.ui '{"op":"settings"}' | jq -c .settings >"$OUT/settings.json"
check "R106-f fields: name; one row per link (source, base, checkout, Remove)" \
    test "$(jq -r '[(.fields | map(.key) | join(",")), (.links | map("\(.source) \(.base) \(.checkout) \(.remove)") | join(","))] | join(" | ")' "$OUT/settings.json")" \
    = "name | shop-web develop auto true,shop-api develop auto true,shared-ui develop auto true"
check "R106-f no field bases, task.verify, task.profile, batch.parallel or batch.review" \
    bash -c "! jq -r '.. | .key? // empty' '$OUT/settings.json' | grep -qE '^(bases|task\.verify|task\.profile|batch\.parallel|batch\.review)\$'"
check "R106-f its menu: Open project.toml, Customize with Claude…" test "$(jq -r '.menu | join(",")' "$OUT/settings.json")" = "Open project.toml,Customize with Claude…"
shot settings-dark
ui settings_close >/dev/null

echo "== R107-g Search scope link / Project, grouped by link"
printf 'export const theme = 1\n' >"$CV/src/theme.ts"
printf 'export const theme = 2\n' >"$PC/src/theme.ts"
rade focus "$tests" >/dev/null
find_() { rpc debug.find "${1:-{\}}"; }
export -f find_
find_ '{"op":"show"}' >/dev/null
find_ '{"op":"set","query":"theme","scope":"link"}' >/dev/null
eventually 5 bash -c "[[ \$(find_ | jq -r .running) == false && \$(find_ | jq -r .searched) == theme ]]"
check "R107-g the scope toggle shows in a Project" test "$(find_ | jq -r .scope_shown)" = true
check "R107-g scope link on shop-web: only shop-web's file" \
    eventually 5 is "find_ | jq -r '[.rows[] | select(.kind==\"file\") | \"\(.link)/\(.path)\"] | join(\",\")'" "shop-web/src/theme.ts"
find_ '{"op":"set","scope":"project"}' >/dev/null
check "R107-g scope Project: both, grouped under shop-web and shared-ui" \
    eventually 8 is "find_ | jq -r '[.rows[] | select(.kind!=\"hit\") | if .kind==\"group\" then \"[\(.link)]\" else .path end] | join(\",\")'" "[shop-web],src/theme.ts,[shared-ui],src/theme.ts"
find_ >"$OUT/search-project.json"
find_ '{"op":"set","scope":"link","query":""}' >/dev/null
# A Terminal on Shop's shared-ui Project checkout (on develop) belongs to
# the Source's main Worktree (on main): scope link searches the Project
# checkout, not the main checkout.
printf 'export const theme = "main"\n' >"$UI/src/theme.ts"
before=$(panes)
home "open Shop" >/dev/null
sleep 0.5
home "terminal shared-ui" >/dev/null
eventually 10 bash -c "[[ -n \$(comm -13 <(tr ' ' '\n' <<<'$before') <(panes | tr ' ' '\n')) ]]"
uip=$(comm -13 <(tr ' ' '\n' <<<"$before") <(panes | tr ' ' '\n') | head -1)
rade focus "$uip" >/dev/null
find_ '{"op":"show"}' >/dev/null
find_ '{"op":"set","query":"theme","scope":"link"}' >/dev/null
check "R107-g scope link on a Project checkout's Terminal: that checkout's file, not the main checkout's" \
    eventually 8 is "find_ | jq -r '[.rows[] | select(.kind==\"hit\") | \"\(.link)/\(.path): \(.preview)\"] | join(\",\")'" \
    "shared-ui/src/theme.ts: export const theme = 2"
find_ '{"op":"set","scope":"link","query":""}' >/dev/null
rade close "$uip" --force >/dev/null 2>&1
rade focus "$tests" >/dev/null
rm -f "$CV/src/theme.ts" "$PC/src/theme.ts" "$UI/src/theme.ts"

echo "== R107-h identity warning"
apish=$(rade spawn --source shop-api --tab new --json | id)
git -C "$API" config user.email other@example.com
rade focus "$apish" >/dev/null
idw() { chrome | jq -r '.statusline.identity_warning // ""'; }
export -f idw
check "R107-h a Pane in shop-api: the identity cell warns, naming both emails" \
    eventually 8 bash -c "idw | grep -q other@example.com && idw | grep -q richard@acme.io"
check "R107-h …the cell has the wait style" eventually 3 is "geo status-cell-identity | jq -r '.classes | index(\"wait\") != null'" true
gid() { changes | jq -r --arg s "$1" '[.groups[] | select(.source==$s) | .identity // empty] | join("")'; }
export -f gid
check "R107-h …and Changes warns on shop-api's group only" \
    eventually 8 bash -c "gid shop-api | grep -q other@example.com && [[ -z \$(gid shop-web) ]]"
rade focus "$dev" >/dev/null
check "R107-h a Pane in shop-web: no cell warning, and shop-web's group has none" \
    eventually 8 bash -c "[[ -z \$(idw) && -z \$(gid shop-web) ]]"
check "R107-h …but shop-api's group still warns (its Commit commits as other@example.com)" \
    eventually 5 bash -c "gid shop-api | grep -q other@example.com"
git -C "$API" config user.email richard@acme.io
check "R107-h same emails again: no group warns" \
    eventually 8 bash -c "[[ \$(changes | jq '[.groups[] | select(.identity != null)] | length') == 0 ]]"
# The single-checkout page (Details) warns too, for the focused checkout
# (its facts are read again when the focus moves to another checkout).
git -C "$WEB" config user.email other@example.com
rade focus "$apish" >/dev/null && rade focus "$dev" >/dev/null
ui changes_details on >/dev/null
scmi() { rpc debug.scm | jq -r '.identity // ""'; }
export -f scmi
check "R107-h …the single-checkout page shows the warning above its Commit" \
    eventually 8 bash -c "scmi | grep -q other@example.com"
git -C "$WEB" config user.email richard@acme.io
ui changes_details off >/dev/null
rade focus "$apish" >/dev/null && rade focus "$dev" >/dev/null
rade close "$apish" --force >/dev/null 2>&1

echo "== Goldens A–D (tests/e2e/golden/m37/)"
GOLD=$HERE/golden/m37
mkdir -p "$GOLD"
for s in a b c d; do
    for th in dark light; do
        if [[ ! -s $GOLD/$s-$th.png && ${APPROVE:-} == 1 ]]; then
            cp "$OUT/$s-$th.png" "$GOLD/$s-$th.png" && echo "  approved (first run): $s-$th"
        fi
        check "R115-c golden ${s^^} $th (with the Agent glyphs) within threshold" python3 "$HERE/golden-diff.py" "$GOLD/$s-$th.png" "$OUT/$s-$th.png" --masks "$OUT/masks-$s-$th.json" --out "$OUT/$s-$th-diff.png"
    done
done
check "design-diff page (mockup · GTK · golden) generated" \
    bash -c "OUT='$OUT' SCENES='a b c d' GOLDEN='$GOLD' '$HERE/design-diff.sh' >/dev/null && [[ -s '$ROOT/target/design-diff/index.html' ]]"

# Debugging: PAUSE=<s> keeps the app up (socket in $XDG_RUNTIME_DIR/rade).
[[ -n ${PAUSE:-} ]] && { echo "paused: RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock"; sleep "$PAUSE"; }
stop_app
echo "M37 E2E: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
