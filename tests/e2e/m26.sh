#!/usr/bin/env bash
# M26 end-to-end tests (ADR-0006, ADR-0028), as 1.0.0 keeps them (R101):
# the Navigator by recorded Project (1.0.0, R104: Project → link → Worktree,
# Scratch with suggestions; it replaced R30's Main checkouts / No Project),
# the one-time freeze of the 0.5.1 branch rule (R30.5),
# `rade project set` with the keys left and Project settings without the
# Tasks / Run defaults / Verify sections (R82). The owner's shape:
# Workspace globex, Projects BEACON / ATLAS / Globex Web Main / Forecast, main
# checkouts on unrelated branches, Worktrees with no Project.
# Usage: tests/e2e/m26.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m26
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$CONF" <<EOF
idle_seconds = 1
[agents.claude]
command = "$FAKE"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
uij() { rpc debug.ui "$1"; }
export -f ui uij rpc
# Navigator rows (1.0.0, R104): kind:title@depth, its suggestion (`→ BEACON?`),
# + when folded; a section as «Scratch».
rows() { ui sidebar | jq -r '[.sidebar[] | select(.kind != "agent") | if .kind == "section" then "«\(.title)»" else "\(.kind):\(.title)@\(.depth)\(.suggest // "")\(if .collapsed then "+" else "" end)" end] | join(" ")'; }
# The depth-0 row above a row (its Project, or a Scratch row).
parent_of() { ui sidebar | jq -r --arg k "$1" --arg t "$2" '.sidebar as $r | ($r | map(.kind == $k and .title == $t) | index(true)) as $i | if $i == null then "none" else ([$r[:$i][] | select(.depth == 0)] | last | "\(.kind):\(.title)") end'; }
export -f rows parent_of

# ---- The owner's shape ------------------------------------------------------
V=$HOME/workspaces/globex
APP=$V/globex-web BK=$V/globex-api REV=$V/globex-billing BOT=$V/support-bot
for r in "$APP" "$BK"; do
    mkdir -p "$r" && G -C "$r" init -q -b master && echo base >"$r/README.md" && G -C "$r" add -A && G -C "$r" commit -qm init
    for b in feat/dev/beacon feat/beacon feat/atlas feat/atlas-dev; do G -C "$r" branch "$b"; done
done
G -C "$APP" checkout -q feat/atlas
G -C "$BK" checkout -qb fix/labels-order
for r in "$APP" "$BK"; do G -C "$r" config user.name "Globex Dev" && G -C "$r" config user.email dev@globex.test; done
for r in "$REV" "$BOT"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && echo base >"$r/README.md" && G -C "$r" add -A && G -C "$r" commit -qm init
done
mkdir -p "$V/.rade"
echo 'name = "globex"' >"$V/.rade/workspace.toml"
# The owner's 0.7.1 (v1, Named Bases) files: migrated on the first read (R97.2),
# each Source's base = its branch for the first Named Base.
projects() {
mkdir -p "$V/.rade/projects"/{beacon,atlas,globex-web-main,forecast}
two() { printf '[[source]]\nid = "globex-web"\nbase = { %s }\n\n[[source]]\nid = "globex-api" # backend\nbase = { %s }\n' "$1" "$1"; }
{ printf '# BEACON: app + backend\nname = "BEACON"\nbases = ["dev", "prod"]\n\n'; two 'dev = "feat/dev/beacon", prod = "feat/beacon"'; } >"$V/.rade/projects/beacon/project.toml"
{ printf 'name = "ATLAS"\nbases = ["prod"]\n\n'; two 'prod = "feat/atlas"'; } >"$V/.rade/projects/atlas/project.toml"
{ printf 'name = "Globex Web Main"\nbases = ["main"]\n\n'; two 'main = "master"'; } >"$V/.rade/projects/globex-web-main/project.toml"
printf 'name = "Forecast"\nbases = ["main"]\n\n[[source]]\nid = "globex-billing"\nbase = { main = "main" }\n' >"$V/.rade/projects/forecast/project.toml"
}
PF=$V/.rade/projects/beacon/project.toml

start_app
rade workspace add "$V" >/dev/null
for r in "$APP" "$BK" "$REV" "$BOT"; do rade source add "$r" --workspace globex >/dev/null; done
rade workspace switch globex >/dev/null
rade source pause support-bot >/dev/null
# 1.0 (R99.1): a Source in several Projects needs --project, so the Worktrees
# with no Project are made before the Projects exist.
chat=$(rade worktree add globex-web agent-chat)
G -C "$chat" checkout -qb feat/beacon-agent-chat
ocr=$(rade worktree add globex-api ocr)
G -C "$ocr" checkout -qb fix/ocr-llm-reasoning-effort-400
projects
for s in globex-web globex-api; do
    rade worktree add "$s" beacon-dev --project beacon >/dev/null
    w=$(rade worktree add "$s" atlas-dev --project atlas --base feat/atlas-dev)
done
rade worktree add globex-api app-main --project globex-web-main >/dev/null
ui toggle_project globex-web-main >/dev/null # the owner folded it

echo "== R30-a the owner's shape: Projects by recorded project, Main checkouts, No Project"
EXPECT="project:ATLAS@0 link:globex-web@1 worktree:atlas-dev@2 link:globex-api@1 worktree:atlas-dev@2 project:BEACON@0 link:globex-web@1 worktree:beacon-dev@2 link:globex-api@1 worktree:beacon-dev@2 project:Forecast@0 link:globex-billing@1 project:Globex Web Main@0+ «Scratch» scratch:~/workspaces/globex@0 scratch:support-bot@0 worktree:agent-chat@0→ BEACON? worktree:ocr@0"
check "rows" eventually 8 is rows "$EXPECT"
check "the owner's fold hides Globex Web Main's links and its Worktree app-main" bash -c "! rows | grep -q 'app-main'"
check "no 'also in' notes, no Source header rows under Projects" test "$(ui sidebar | jq '[.sidebar[] | select(.kind=="source" and .nested)] | length')" = 0
check "Alt+1 = ATLAS's Project home (Projects' order, R104-i)" bash -c "rade debug action source_1 >/dev/null; sleep 0.3; [[ \$(ui home | jq -r .home.name) == ATLAS ]]"
check "… its row is selected" eventually 3 is "ui sidebar | jq -r '[.sidebar[] | select(.selected) | \"\(.kind):\(.title)\"] | join(\",\")'" "project:ATLAS"
rade debug action color_scheme_light >/dev/null
sleep 0.5
shot sidebar-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot sidebar-dark
rade debug action color_scheme_light >/dev/null

echo "== R30-f a branch switch moves nothing; a suggestion click moves; No Project moves back"
G -C "$APP" checkout -q feat/dev/beacon
G -C "$ocr" checkout -q feat/atlas
check "a branch switch moves no row (ocr only gains a suggestion)" eventually 8 is rows "${EXPECT/worktree:ocr@0/worktree:ocr@0→ ATLAS?}"
G -C "$ocr" checkout -q fix/ocr-llm-reasoning-effort-400
G -C "$APP" checkout -q feat/atlas
check "a main checkout's menu (the Source's) has no Move to Project" test "$(rade debug menu source globex-web --json | jq -r '[.items[].label | select(startswith("Move to Project"))] | length')" = 0
check "agent-chat's menu: the Projects listing globex-web, Scratch ✓ (M51)" test "$(rade debug menu worktree globex-web/agent-chat --json | jq -r '[.items[].label | select(startswith("Move to Project"))] | join("|")')" = "Move to Project ▸ ATLAS|Move to Project ▸ BEACON|Move to Project ▸ Globex Web Main|Move to Project ▸ Scratch ✓"
CHAT_ID=$(rade debug state | jq -r '.sources[] | select(.id=="globex-web") | .worktrees[] | select(.name=="agent-chat") | .id')
ui suggest "$CHAT_ID" >/dev/null
# M51 (R132.1): BEACON's base differs from the one agent-chat compares against: keep it.
if eventually 3 bash -c "rade debug dialog --json | jq -e .open >/dev/null"; then rade debug dialog respond keep >/dev/null; fi
check "→ BEACON? records project beacon" eventually 5 is "rade worktree list globex-web --json | jq -r '.worktrees[] | select(.worktree==\"agent-chat\") | .project'" beacon
# agent-chat was made before beacon-dev (it predates the Projects), so it comes first.
check "… the row moves under BEACON's globex-web (before beacon-dev)" eventually 5 bash -c "rows | grep -qF 'project:BEACON@0 link:globex-web@1 worktree:agent-chat@2 worktree:beacon-dev@2' && [[ \$(parent_of worktree agent-chat) == project:BEACON ]]"
G -C "$chat" checkout -q -b tmp-elsewhere
check "… and stays there on another branch" eventually 5 is "parent_of worktree agent-chat" project:BEACON
rade debug menu worktree globex-web/agent-chat activate "Move to Project ▸ Scratch" >/dev/null
G -C "$chat" checkout -q feat/beacon-agent-chat
check "Scratch moves it back" eventually 8 is rows "$EXPECT"

echo "== R30-b fold and unfold survive a restart"
ui toggle_project globex-web-main >/dev/null # unfold the owner's fold
ui toggle_project beacon >/dev/null
stop_app
start_app
check "after restart: BEACON folded, Globex Web Main open" eventually 8 is "ui sidebar | jq -r '[.sidebar[] | select(.kind==\"project\") | \"\(.title)=\(.collapsed)\"] | join(\",\")'" "ATLAS=false,BEACON=true,Forecast=false,Globex Web Main=false"
ui toggle_project beacon >/dev/null
ui toggle_project globex-web-main >/dev/null

echo "== R30-g / R30.5 the one-time freeze of a 0.5.1 state.json"
# 0.5.1 shape: Worktrees made by hand (no project) on base branches.
# (1.0: made with --project, R99.1; the records are stripped below. 1.0 keeps
# one base per link, so legacy-dev sits on BEACON's base feat/dev/beacon.)
leg=$(rade worktree add globex-web legacy-dev --project beacon)
G -C "$leg" checkout -q --detach && G -C "$leg" checkout -q feat/dev/beacon
two=$(rade worktree add globex-api legacy-both --project beacon)
G -C "$two" checkout -q --detach && G -C "$two" branch -q shared-x && G -C "$two" checkout -q shared-x
stop_app
cp "$STATE" "$T/state.m26.json"
# Back to what 0.5.1 wrote: no sidebar_rule, no expanded/dismissed keys,
# Projects recorded only where 0.5.1 recorded them (beacon-dev, atlas-dev, app-main).
jq 'del(.sidebar_rule, .expanded_projects)
    | (.sources[].worktrees[] | select(.name=="legacy-dev" or .name=="legacy-both" or .name=="agent-chat" or .name=="ocr")) |= del(.project, .base)' \
    "$T/state.m26.json" >"$STATE"
# A second Project naming shared-x makes legacy-both ambiguous (under two).
for p in atlas globex-web-main; do printf '\n# temp\n' >>"$V/.rade/projects/$p/project.toml"; done
mkdir -p "$V/.rade/projects/x1" "$V/.rade/projects/x2"
for p in x1 x2; do printf '[[source]]\nid = "globex-api"\nbase = "shared-x"\n' >"$V/.rade/projects/$p/project.toml"; done
check "0.5.1 state has no sidebar_rule" test "$(jq -r '.sidebar_rule // "none"' "$STATE")" = none
start_app
check "legacy-dev (on BEACON's base branch) is recorded under BEACON" eventually 5 is "rade worktree list globex-web --json | jq -r '.worktrees[] | select(.worktree==\"legacy-dev\") | .project'" beacon
check "… logged" grep -q 'globex-web/legacy-dev recorded under Project beacon' "$OUT/app.log"
check "legacy-both (under two Projects by branch) stays in Scratch" test "$(rade worktree list globex-api --json | jq -r '.worktrees[] | select(.worktree=="legacy-both") | .project // "none"')" = none
# M51 (R132.2): x1 and x2 both match shared-x: no guess.
check "… with no suggestion (two Projects match, R132.2)" eventually 5 is "ui sidebar | jq -r '.sidebar[] | select(.kind==\"worktree\" and .title==\"legacy-both\") | .suggest'" "null"
check "main checkouts never get one" test "$(rade debug state | jq '[.sources[] | .worktrees[0].project // empty] | length')" = 0
check "agent-chat / ocr (no base branch) stay unassigned" test "$(rade debug state | jq -r '[.sources[].worktrees[] | select(.name=="agent-chat" or .name=="ocr") | .project // "none"] | join(",")')" = "none,none"
check "sidebar_rule: 2 saved" eventually 5 is "jq -r .sidebar_rule '$STATE'" 2
G -C "$leg" checkout -q master
stop_app
start_app
check "a second start changes nothing (legacy-dev stays BEACON's off its branch)" test "$(rade worktree list globex-web --json | jq -r '.worktrees[] | select(.worktree=="legacy-dev") | .project')" = beacon
rm -r "$V/.rade/projects/x1" "$V/.rade/projects/x2"
rade worktree remove globex-api legacy-both --force >/dev/null
rade worktree remove globex-web legacy-dev --force >/dev/null

echo "== R82 / R97.3 rade project set: one line per edit, the v2 keys"
check "the owner's file was migrated: one base per link, comments kept, .v1.bak" bash -c "grep -qx 'base = \"feat/dev/beacon\"' '$PF' && ! grep -q '^bases' '$PF' && grep -q '^# BEACON: app + backend' '$PF' && test -f '$PF.v1.bak'"
cp "$PF" "$T/pf"
rade project set beacon source.globex-web.base feat/dev2/beacon >/dev/null
check "a link's base changes only that line" test "$(diff "$T/pf" "$PF" | grep -c '^[<>]')" = 2
check "… and keeps the backend's comment" grep -q 'id = "globex-api" # backend' "$PF"
rade project set beacon source.globex-web.base feat/dev/beacon >/dev/null
check "… set back: byte-identical" cmp -s "$T/pf" "$PF"
rade project set beacon source.globex-web.checkout main >/dev/null
check "source.<id>.checkout adds only its line" bash -c "[[ \$(diff '$T/pf' '$PF' | grep -c '^[<>]') == 1 ]] && grep -qx 'checkout = \"main\"' '$PF'"
rade project set beacon source.globex-web.checkout >/dev/null
check "… no value removes it: byte-identical" cmp -s "$T/pf" "$PF"
rade project set beacon name "BEACON 2" >/dev/null
check "name changes only its line" test "$(diff "$T/pf" "$PF" | grep -c '^[<>]')" = 2
rade project set beacon name BEACON >/dev/null
check "… set back: byte-identical" cmp -s "$T/pf" "$PF"
check "the Named Base key source.<id>.base.<name> exits 2" test "$(code rade project set beacon source.globex-web.base.dev x)" = 2
for k in task.profile task.verify batch.parallel batch.review; do
    check "removed key $k exits 2" test "$(code rade project set beacon "$k" x)" = 2
    check "… unknown key" grep -q 'unknown key' "$T/err"
done
check "a bad key exits 2" test "$(code rade project set beacon nope x)" = 2
check "… the file unchanged" cmp -s "$T/pf" "$PF"

echo "== R82 / R106.3 Project settings: Name and one row per link, no Tasks / Run defaults / Verify"
rpc debug.ui '{"op":"settings_open","arg":"beacon"}' >/dev/null
S=$(rpc debug.ui '{"op":"settings"}')
check "the form shows one row per link with its base" test "$(jq -r '[.settings.links[] | "\(.source)@\(.base)"] | join(",")' <<<"$S")" = "globex-web@feat/dev/beacon,globex-api@feat/dev/beacon"
check "… no task, run, batch, profile or verify field" test "$(jq '[.settings.fields[].key | select(test("task|run|batch|profile|verify"))] | length' <<<"$S")" = 0
check "… no such section titles" test "$(jq -r '[.. | strings | select(test("^(Tasks|Run defaults|Verify)$"))] | length' <<<"$S")" = 0
sleep 0.6
shot settings-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot settings-dark
rade debug action color_scheme_light >/dev/null
uij '{"op":"settings_set","arg":"beacon","key":"source.globex-web.base","value":"feat/dev3/beacon"}' >/dev/null
uij '{"op":"settings_close"}' >/dev/null
check "a form edit writes that one line" eventually 5 bash -c "grep -q 'feat/dev3/beacon' '$PF' && [[ \$(diff '$T/pf' '$PF' | grep -c '^[<>]') == 2 ]]"
rade project set beacon source.globex-web.base feat/dev/beacon >/dev/null
check "… set back: byte-identical" cmp -s "$T/pf" "$PF"

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
