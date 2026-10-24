#!/usr/bin/env bash
# M8 end-to-end tests (ADR-0006): the command palette (R14) — opened by its
# key action, queried per category (prefixes, CLI ranking), Enter focuses a
# Source / Worktree / Tab / Pane, opens a file in the Viewer, runs actions
# (keybinding labels shown), the inline Spawn Agent / New Worktree forms,
# Esc and Ctrl+Shift+P give focus back to the Pane, recently used first, and
# timings on a 1k-file and a generated 50k-file Worktree. Screenshots light
# and dark. Temp HOME, driven only through the `rade` CLI.
# Usage: tests/e2e/m8.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m8
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
R=$T/repo
mkdir -p "$R/src" "$R/build"
printf 'fn main() {}\n' >"$R/src/main.rs"
printf 'pub fn palette_target() {}\n' >"$R/src/palette_target.rs"
printf '# Repo\n' >"$R/README.md"
printf 'build/\n' >"$R/.gitignore"
printf 'x\n' >"$R/build/palette_ignored.rs"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init

# Generated trees for R14.5: 1,000 and 50,000 files (+ an ignored folder).
gen_tree() { # dir files
    python3 - "$1" "$2" <<'PY'
import os, sys
root, n = sys.argv[1], int(sys.argv[2])
for i in range(n):
    d = os.path.join(root, f"pkg{i % 50:02d}", f"mod{i % 997:03d}")
    os.makedirs(d, exist_ok=True)
    open(os.path.join(d, f"file_{i}.rs"), "w").close()
os.makedirs(os.path.join(root, "node_modules/x"), exist_ok=True)
for i in range(500):
    open(os.path.join(root, "node_modules/x", f"dep_{i}.js"), "w").close()
open(os.path.join(root, ".gitignore"), "w").write("node_modules/\n")
PY
    git -C "$1" init -q -b main
}
gen_tree "$T/k1" 1000
gen_tree "$T/k50" 50000

pal() { rade --json debug palette --ui "$@"; }
info() { pal info | jq -r "$1"; }
q() { rade --json debug palette --limit "${2:-20}" -- "$1"; }
first() { q "$1" | jq -r ".results[0].$2"; }
open_pal() {
    [[ $(info .open) == true ]] || rade debug action command_palette >/dev/null
    eventually 3 is "info .open" true
}
typeq() { pal type "$1" >/dev/null; eventually 5 is "info '.busy'" false; }
settled() { eventually 5 is "info '.busy'" false; }
focus_pane() { rade debug window --json | jq -r '.focus.pane // "none"'; }
focus_widget() { rade debug window --json | jq -r '.focus.widget // "none"'; }
cur_tab_panes() { rade debug state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | [.. | objects | select(has("kind") and has("id")) | .id] | length'; }
active_source() { rade debug state | jq -r .active_source; }
focused() { rade debug state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | .focused'; }

echo "== start"
start_app
rade source add "$R" --name proj >/dev/null
rade source add "$T/k1" --name k1 >/dev/null
rade source add "$T/k50" --name k50 >/dev/null
P1=$(rade spawn --source proj --name editor --json | jq -r .pane)
REV=$(rade spawn --source proj --agent fake --name rev --split right "hello" --json | jq -r .pane)
eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$REV\") | .status'" done || true
G1=$(rade list --json | jq -r '[.panes[] | select(.source=="general")][0].pane')
rade focus "$P1" >/dev/null
sleep 0.5

echo "== R14.1 Ctrl+Shift+P opens the palette; the search entry takes the keys"
check "the command_palette key action (Ctrl+Shift+P, unit-tested default) runs" bash -c "rade debug action command_palette >/dev/null"
check "palette open" eventually 3 is "info .open" true
rade debug window --json | jq -c .focus
check "keyboard focus in its text entry (GtkText, not a Pane)" test "$(focus_widget)/$(focus_pane)" = "GtkText/none"
settled
check "empty query lists Panes first (nothing used yet)" test "$(info '.rows[0].kind')" = pane
check "…and never files (they need text or #)" test "$(info '[.rows[] | select(.kind=="file")] | length')" = 0
shot palette-open-light

echo "== R14.1 Esc closes and focus returns to the same Pane"
pal key escape >/dev/null
check "closed" test "$(info .open)" = false
check "focus back on the Pane (editor)" eventually 3 is focus_pane "$P1"
check "…in its Terminal (GtkGLArea), not an entry" test "$(focus_widget)" != GtkText
open_pal
typeq 'zzz'
rade debug action command_palette >/dev/null
check "Ctrl+Shift+P again closes it" test "$(info .open)" = false
check "…and focus returns to the Pane" eventually 3 is focus_pane "$P1"
rade focus "$REV" >/dev/null
open_pal
pal key escape >/dev/null
check "focus restores to whichever Pane was focused (rev)" eventually 3 is focus_pane "$REV"
rade focus "$P1" >/dev/null

echo "== R14.1/R14.3 categories, prefixes, ranking (debug palette = CLI)"
check "@rev → the Pane rev first" test "$(first @rev title)/$(first @rev kind)" = "rev/pane"
check "@ matches the Agent Preset (fake)" test "$(first '@fake' title)" = rev
check "@ matches the status (done)" test "$(first '@done' title)" = rev
check "/proj → Source proj (Alt+1 shown)" test "$(first /proj title)/$(first /proj keys)" = "proj/Alt+1"
check ">right panel → Toggle Right Panel with its key" test "$(first '>right panel' title)|$(first '>right panel' keys)" = "Toggle Right Panel|Ctrl+Shift+E"
check ">sr → Split Right (Ctrl+Shift+Enter)" test "$(first '>sr' title)|$(first '>sr' keys)" = "Split Right|Ctrl+Shift+Enter"
check "> lists non-bound actions (Mark All Notifications Read, no key)" test "$(q '>mark all' | jq -r '.results[0] | "\(.title)|\(.keys)"')" = "Mark All Notifications Read|null"
check "> lists 'Spawn fake' for the Preset" test "$(first '>spawn fake' title)" = "Spawn fake"
check "a filter keeps other kinds out (@ has no actions)" test "$(q '@split' | jq '[.results[] | select(.kind != "pane" and .kind != "tab")] | length')" = 0
check "no prefix searches every kind (files too)" test "$(q 'palette_target' | jq -r '[.results[].kind] | index("file") != null')" = true
check "match positions reported for highlighting" test "$(q '>sr' | jq -c '.results[0].positions')" = "[0,6]"

echo "== R14.1 files of the active Worktree (.gitignore respected)"
rade focus "$P1" >/dev/null
check "#palette_target → src/palette_target.rs" test "$(first '#palette_target' title)|$(first '#palette_target' detail)" = "palette_target.rs|src"
check "ignored build/ file not listed" test "$(q '#palette_ignored' | jq '.results | length')" = 0
check "4 files: .gitignore, README.md, src/main.rs, src/palette_target.rs" test "$(q '#' 50 | jq .files)" = 4

echo "== R14.4 Enter: open a file in the Viewer"
open_pal
typeq '#palette_target'
check "row 0 is the file" test "$(info '.rows[0].title')" = palette_target.rs
pal key enter >/dev/null
check "palette closed" test "$(info .open)" = false
check "Viewer shows src/palette_target.rs" eventually 5 is "rade debug state | jq -r '[.. | objects | select(.type? == \"viewer\") | .path][0]'" "$R/src/palette_target.rs"

echo "== R14.4 Enter: focus a Pane / Tab / Source / Worktree elsewhere"
rade focus "$G1" >/dev/null
open_pal
typeq '@rev'
pal key enter >/dev/null
check "Pane rev focused (Source proj on screen)" eventually 3 is focused "$REV"
check "…and keyboard focus is on rev" eventually 3 is focus_pane "$REV"
check "active Source proj" test "$(active_source)" = proj
GT=$(rade debug state | jq -r '.sources[] | select(.id=="general") | .worktrees[0].tabs[0].name')
open_pal
typeq "@$GT"
check "Tab row found by its name" test "$(info '[.rows[] | select(.kind=="tab")][0].title')" = "$GT"
# Every Tab here is "Tab 1": pick General's with the arrows.
for _ in 1 2 3 4 5; do
    [[ $(info '.rows[.selected] | "\(.kind) \(.detail)"') == "tab General"* ]] && break
    pal key down >/dev/null
done
pal key enter >/dev/null
check "Enter on a Tab shows it (General, its focused Pane)" eventually 3 is active_source general
check "…focus on that Tab's Pane" eventually 3 is focus_pane "$G1"
open_pal
typeq '/k1'
pal key enter >/dev/null
check "Enter on a Source switches to it" eventually 3 is active_source k1
DOOMED=$(rade spawn --source proj --name doomed --json | jq -r .pane)
open_pal
typeq '@doomed'
rade close "$DOOMED" >/dev/null
check "a Pane closed after the palette opened is still listed" test "$(info '.rows[0].title')" = doomed
pal key enter >/dev/null
check "…Enter on it keeps the palette open and says it is gone" test "$(info .open)|$(info .status)" = "true|That is gone (closed since the palette opened)"
pal key escape >/dev/null
rade worktree add proj feat-pal >/dev/null
open_pal
typeq '/feat-pal'
check "Worktree row (Source has 2 Worktrees)" test "$(info '.rows[0].kind')/$(info '.rows[0].title')" = "worktree/feat-pal"
pal key enter >/dev/null
check "Enter on a Worktree shows it" eventually 3 is "rade worktree list proj --json | jq -r '.worktrees[] | select(.active) | .worktree'" feat-pal
rade focus "$P1" >/dev/null

echo "== R14.4 Enter: run actions"
n=$(cur_tab_panes)
open_pal
typeq '>split right'
pal key enter >/dev/null
check "Split Right ran (one more Pane in the Tab)" eventually 3 is cur_tab_panes $((n + 1))
check "…focus on the new Pane (keyboard too)" eventually 3 is 'test "$(focus_pane)" = "$(focused)" && echo y' y
open_pal
typeq '>toggle right panel'
pal key enter >/dev/null
check "Toggle Right Panel ran" eventually 3 is "rade debug state | jq -r .file_tree" true
rade notify --pane "$P1" "palette-note" >/dev/null
check "a Notification is unread" test "$(rade debug panel counts --json | jq .total)" -ge 1
open_pal
typeq '>mark all read'
pal key enter >/dev/null
check "Mark All Notifications Read ran" eventually 3 is "rade debug panel counts --json | jq .total" 0
open_pal
typeq '>spawn fake'
pal key enter >/dev/null
check "'Spawn fake' opened an Agent Pane in a new Tab (focused)" eventually 5 is "rade list --json | jq -r --arg f \"\$(rade debug state | jq -r '. as \$s | \$s.sources[] | select(.id==\$s.active_source) | . as \$p | .worktrees[] | select(.id==\$p.active_worktree) | . as \$w | .tabs[] | select(.id==\$w.active_tab) | .focused')\" '.panes[] | select(.pane==\$f) | .agent'" fake

echo "== R14.3 recently used first on an empty query"
open_pal
typeq ''
check "most recent first: Spawn fake" test "$(info '.rows[0].title')" = "Spawn fake"
check "then Mark All Notifications Read" test "$(info '.rows[1].title')" = "Mark All Notifications Read"
check "recent list saved in state.json (palette_recent)" test "$(rade debug state | jq -r '.palette_recent[0]')" = "action:spawn:fake"

echo "== R14.4 keyboard: arrows, pages, Tab cycles categories"
typeq ''
pal key down >/dev/null
check "Down → row 1" test "$(info .selected)" = 1
pal key up >/dev/null; pal key up >/dev/null
check "Up from row 0 wraps to the last row" test "$(info .selected)" = "$(($(info .count) - 1))"
pal key page_up >/dev/null
check "Page Up moves 10 rows" test "$(info .selected)" = "$(($(info .count) - 11))"
pal key tab >/dev/null; settled
check "Tab → Actions (query '>')" test "$(info .query)|$(info .filter)" = ">|actions"
# 1.0.0 (spec D.8): All · > Actions · @ Agents · / Sources · # Files · $ Symbols.
pal key tab >/dev/null; pal key tab >/dev/null; settled
check "Tab, Tab → Sources (query '/')" test "$(info .filter)" = sources
check "only Sources/Worktrees listed" test "$(info '[.rows[] | select(.kind != "source" and .kind != "worktree" and .kind != "link" and .kind != "project")] | length')" = 0
pal key shift+tab >/dev/null; settled
check "Shift+Tab → back to Agents (query '@')" test "$(info .filter)|$(info .query)" = "panes|@"
check "only Panes/Tabs listed" test "$(info '[.rows[] | select(.kind != "pane" and .kind != "tab")] | length')" = 0
typeq '@pro'
pal key tab >/dev/null; settled
check "Tab keeps the typed text ('/pro')" test "$(info .query)" = "/pro"
pal key escape >/dev/null

echo "== R14.4 inline form: Spawn Agent… with the fake Agent"
rade focus "$P1" >/dev/null
open_pal
typeq '>spawn agent'
check "row 0 is 'Spawn Agent…'" test "$(info '.rows[0].title')" = "Spawn Agent…"
pal key enter >/dev/null
check "form shown (palette stays open)" test "$(info .open)|$(info .form.title)" = "true|Spawn Agent"
check "fields Agent, Prompt, Where" test "$(info '[.form.fields[].key] | join(",")')" = "agent,prompt,where"
check "focus in the first field" test "$(info .form.focus)" = agent
rade status running --pane "$REV" >/dev/null # a model update (hook) while the form is open
sleep 0.3
check "a model update keeps the form open and its focus" test "$(info .open)|$(info .form.title)|$(info .form.focus)" = "true|Spawn Agent|agent"
for _ in 1 2 3 4 5; do pal key tab >/dev/null; done
check "Tab ×5 cycles inside the form (wraps; never out of the palette)" test "$(info .open)|$(info '.form.focus // "button"')" = "true|prompt"
pal key shift+tab >/dev/null; pal key shift+tab >/dev/null
check "Shift+Tab ×2 wraps back to the submit button" test "$(info .open)|$(info '.form.focus // "button"')" = "true|button"
pal key tab >/dev/null
shot palette-form-light
pal key escape >/dev/null
check "Esc in a form → back to the list, query kept" test "$(info .open)|$(info .form)|$(info .query)" = "true|null|>spawn agent"
pal key enter >/dev/null
pal field agent=fake >/dev/null
pal field prompt="palette works" >/dev/null
pal field where="Split Right" >/dev/null
check "form values set" test "$(info '[.form.fields[].value] | join("|")')" = "fake|palette works|Split Right"
ids() { rade list --json | jq -r '[.panes[].pane] | sort | join(" ")'; }
before=$(ids)
pal key enter >/dev/null
check "submitted: palette closed" test "$(info .open)" = false
eventually 3 bash -c "[[ \"\$(rade list --json | jq -r '[.panes[].pane] | sort | join(\" \")')\" != '$before' ]]"
NEW=$(comm -13 <(tr ' ' '\n' <<<"$before") <(ids | tr ' ' '\n'))
check "one new Pane ($NEW)" test "$(wc -w <<<"$NEW")" = 1
check "…the fake Agent got the prompt" eventually 10 screen_has "$NEW" "REPLY: PALETTE WORKS"
check "…split right of editor in its Tab, focused" test "$(focused)/$(rade list --json | jq -r --arg p "$NEW" '.panes[] | select(.pane==$p) | .agent')" = "$NEW/fake"
check "…keyboard focus on it" eventually 3 is focus_pane "$NEW"

echo "== R14.4 inline form: New Worktree…"
rade focus "$P1" >/dev/null
open_pal
typeq '>new worktree'
pal key enter >/dev/null
check "fresh form: no error shown, can't submit yet" test "$(info .form.error)|$(info .form.can_submit)" = "|false"
pal key enter >/dev/null
check "Enter on it says why (and stays open)" test "$(info .open)|$(info .form.error)" = "true|Name the Worktree (it is also its branch)"
pal field name="a b" >/dev/null
check "invalid name → reason shown" bash -c "rade --json debug palette --ui info | jq -r .form.error | grep -q 'invalid worktree name'"
pal field name=feat-form >/dev/null
check "valid name → can submit" test "$(info .form.can_submit)" = true
pal key enter >/dev/null
check "Worktree feat-form created" eventually 15 is "rade worktree list proj --json | jq -r '.worktrees[] | select(.worktree==\"feat-form\") | .branch'" feat-form
check "…and shown" eventually 5 is "rade worktree list proj --json | jq -r '.worktrees[] | select(.active) | .worktree'" feat-form
rade focus "$P1" >/dev/null

echo "== R14.5 performance (release build)"
pk() { rade --json debug palette --limit 20 -- "$1"; }
rade focus "$(rade spawn --source k1 --json | jq -r .pane)" >/dev/null
r=$(pk '#file 42')
echo "  k1: $(jq -c '{items, files, walk_ms, rank_ms}' <<<"$r")"
check "k1: 1,000 files + .gitignore, node_modules ignored" test "$(jq .files <<<"$r")" = 1001
perf_check "k1: ranking ~1k items < 16 ms" test "$(jq '.rank_ms < 16' <<<"$r")" = true
open_pal
check "UI: files indexed" eventually 10 is "info .files" 1001
worst=0
for s in f fi fil file 'file 4' 'file 42' 'file 421'; do
    typeq "#$s"
    u=$(info .timing.update_ms)
    echo "    k1 '#$s' → $(info .count) rows, update $(printf %.2f "$u") ms (rank $(printf %.2f "$(info .timing.rank_ms)"), apply $(printf %.2f "$(info .timing.apply_ms)"))"
    worst=$(python3 -c "print(max($worst, $u))")
done
perf_check "k1: result list updates < 16 ms (worst $(printf %.2f "$worst") ms)" python3 -c "import sys; sys.exit(not $worst < 16)"
pal key escape >/dev/null
rade focus "$(rade spawn --source k50 --json | jq -r .pane)" >/dev/null
r=$(pk '#file_49999')
echo "  k50: $(jq -c '{items, files, walk_ms, rank_ms}' <<<"$r")"
check "k50: 50,000 files + .gitignore listed (ignored node_modules skipped)" test "$(jq .files <<<"$r")" = 50001
check "k50: best hit file_49999.rs" test "$(jq -r '.results[0].title' <<<"$r")" = file_49999.rs
open_pal
check "UI: the palette answers while the 50k walk runs" test "$(timeout 1 rade --json debug palette --ui info | jq -r '.files // "indexing"')" = indexing
check "UI: 50,000 files indexed in the background" eventually 20 is "info .files" 50001
echo "    walk $(printf %.0f "$(info .timing.walk_ms)") ms"
worst=0 worst_rank=0
for s in f fi fil file 'file 4' 'file 49' 'file 4999' 'mod12 file'; do
    typeq "#$s"
    u=$(info .timing.update_ms) rk=$(info .timing.rank_ms)
    echo "    k50 '#$s' → $(info .count) rows, update $(printf %.2f "$u") ms (rank $(printf %.2f "$rk"), apply $(printf %.2f "$(info .timing.apply_ms)"))"
    worst=$(python3 -c "print(max($worst, $u))") worst_rank=$(python3 -c "print(max($worst_rank, $rk))")
done
perf_check "k50: every update lands (worst $(printf %.2f "$worst") ms; ranking off the GTK thread, worst $(printf %.2f "$worst_rank") ms)" python3 -c "import sys; sys.exit(not $worst < 100)"
# Typing fast: every keystroke re-ranks; stale ones are cancelled. A probe
# asks the GTK thread for info every 50 ms meanwhile; its slowest answer
# shows the window never blocked.
( slow=0; while [[ ! -f $T/burst-done ]]; do
    a=$(date +%s%N); timeout 1 rade debug palette --ui info >/dev/null || echo 9999 >>"$T/probe"
    echo $((($(date +%s%N) - a) / 1000000)) >>"$T/probe"; sleep 0.05; done ) &
PROBE=$!
for s in m mo mod mod1 mod12 'mod12 f' 'mod12 fi' 'mod12 file_'; do pal type "#$s" >/dev/null; done
settled
touch "$T/burst-done"; wait $PROBE
check "typing a burst: the last query's results win" test "$(info .query)|$(info '.rows[0].title | startswith("file_")')" = "#mod12 file_|true"
check "the GTK thread answered all through the burst (slowest $(sort -n "$T/probe" | tail -1) ms incl. CLI start)" test "$(sort -n "$T/probe" | tail -1)" -lt 500
# Enter right after typing runs the row of THAT query, not the previous one.
pal type '#file_49999' >/dev/null
pal key enter >/dev/null
check "Enter while ranking ran waits for the new rows (opens file_49999.rs)" eventually 5 is "rade debug state | jq -r '[.. | objects | select(.type? == \"viewer\") | .path] | map(select(endswith(\"file_49999.rs\"))) | length'" 1
open_pal
pal key escape >/dev/null

echo "== R14.1 General: no file search"
rade focus "$G1" >/dev/null
open_pal
typeq '#x'
check "status says there is no file search in General" test "$(info .status)" = "No matches · no file search in General"
pal key escape >/dev/null

echo "== look: light and dark"
rade focus "$P1" >/dev/null
open_pal
typeq 'rev'
shot palette-results-light
typeq '>split r'
check "one row for one result" test "$(info .count)" = 1
shot palette-one-row-light
pal key escape >/dev/null
rade debug action new_worktree >/dev/null # a libadwaita dialog is open
sleep 0.8
rade debug action command_palette >/dev/null
check "the palette doesn't open over a dialog" test "$(info .open)" = false
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not restart"; exit 1; }
sleep 1
rade focus "$P1" >/dev/null
open_pal
typeq '>t'
shot palette-actions-dark
pal key escape >/dev/null
open_pal
typeq '>spawn agent'
pal key enter >/dev/null
pal field prompt="Tiếng Việt có dấu" >/dev/null
check "form keeps Vietnamese text" test "$(info '.form.fields[1].value')" = "Tiếng Việt có dấu"
shot palette-form-dark
pal key escape >/dev/null
pal key escape >/dev/null
check "recently used survives a restart" test "$(rade debug state | jq -r '.palette_recent | length')" -ge 3
stop_app

echo "== M8: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
