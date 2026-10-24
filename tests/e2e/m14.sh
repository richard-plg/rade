#!/usr/bin/env bash
# E2E (M14, R20): the redesign "Adwaita Pro" on the baseline scene of
# v0.7.1:docs/design/current/scene.sh — screenshots in light and dark (switched live)
# → target/e2e-m14/, plus checks of each design element through `rade debug`.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m14}
source "$HERE/lib.sh"
FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "$FAKE"
[agents.codex]
command = "$FAKE"
[agents.lead]
command = "bash"
[agents.opencode]
command = "$FAKE"
C
G() { git -c user.name=dev -c user.email=d@e -c init.defaultBranch=main "$@"; }
mk() { local d=$T/code/$1; mkdir -p "$d/src" "$d/docs"; cd "$d"; G init -q
  printf '# %s\n\nA source.\n' "$1" > README.md
  printf 'fn main() {\n    println!("hi");\n}\n' > src/main.rs
  printf 'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n' > src/lib.rs
  printf 'target/\n' > .gitignore; printf '[package]\nname="%s"\n' "$1" > Cargo.toml
  printf '# Design\n' > docs/design.md
  G add -A; G commit -qm init; cd - >/dev/null; }
mk payments-api; mk web-dashboard
scene() { # name: a light and a dark shot
    rade debug action color_scheme_light >/dev/null; sleep 1; shot "$1-light"
    rade debug action color_scheme_dark >/dev/null; sleep 1; shot "$1-dark"
    rade debug action color_scheme_light >/dev/null; sleep 0.3
}

echo "== baseline scene"
start_app
rade source add "$T/code/payments-api" --name payments-api >/dev/null
rade source add "$T/code/web-dashboard" --name web-dashboard >/dev/null
wt=$(rade worktree add payments-api feat-refunds --json | jq -r .path)
rade worktree add payments-api fix-idempotency >/dev/null
rade worktree add web-dashboard chart-polish >/dev/null
printf 'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n/// Refund a charge, partially or fully.\npub fn refund(amount: i64, captured: i64) -> Result<i64, String> {\n    if amount > captured {\n        return Err("refund exceeds capture".into());\n    }\n    Ok(captured - amount)\n}\n' > "$wt/src/lib.rs"
printf 'pub mod webhooks;\n' > "$wt/src/webhooks.rs"
lead=$(rade spawn --source payments-api --worktree feat-refunds --agent lead --name lead --json | jq -r .pane)
sleep 1
as() { RADE_PANE_ID=$lead RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock "$@"; }
as rade spawn --agent claude --name claude-refunds "implement partial refunds" >/dev/null
codex=$(as rade spawn --agent codex --name codex-tests "ask which fixtures" --json | jq -r .pane)
as rade spawn --agent opencode --name opencode-docs "spin" >/dev/null
rade spawn --source web-dashboard --worktree chart-polish --agent claude --name claude-charts "osc Charts restyled" >/dev/null
sleep 3
rade notify --pane "$lead" "cargo test: 42 passed" >/dev/null 2>&1 || true
rade focus "$lead" >/dev/null
rade send "$lead" 'git status --short; cargo --version 2>/dev/null; ls' --enter
rade debug action toggle_file_tree >/dev/null
sleep 2
scene main
rade debug action command_palette >/dev/null; sleep 0.5
rade debug palette --ui type "rev" >/dev/null 2>&1; sleep 1
scene palette
rade debug palette --ui key escape >/dev/null; sleep 0.3
ui() { rpc debug.ui "${1:-{\}}"; }
row() { ui '{"op":"sidebar"}' | jq -c --arg n "$1" '.sidebar[] | select(.title==$n)'; }
tabs() { rpc debug.tabs; }
# The focused Pane of the Tab on screen.
focused() { rade debug state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | .focused'; }
cl=$(rade list --json | jq -r '.panes[] | select(.name=="claude-refunds") | .pane')
op=$(rade list --json | jq -r '.panes[] | select(.name=="opencode-docs") | .pane')

echo "== R20.6 Agents in the sidebar, Worktree rows"
check "Agent rows under feat-refunds: lead, claude-refunds, codex-tests, opencode-docs" test "$(ui '{"op":"sidebar"}' | jq -r '[.sidebar[] | select(.kind=="agent") | .title] | join(",")')" = "lead,claude-refunds,codex-tests,opencode-docs,claude-charts"
# 1.0.0 (R104): Navigator rows carry an LED (`led`) instead of a glyph; the
# Sources of this scene belong to no Project, so they are Scratch rows.
check "codex-tests row: waiting LED, 'needs input'" test "$(row codex-tests | jq -r '[.led, .meta] | join(" ")')" = "waiting needs input"
check "claude-refunds row: done LED" test "$(row claude-refunds | jq -r .led)" = "done"
od=$(row opencode-docs | jq -r '.led + " " + .meta')
check "opencode-docs row: running LED and its time ($od)" bash -c "[[ '$od' =~ ^running\ [0-9]+[smh]\$ ]]"
check "feat-refunds row: a Worktree row of payments-api with no branch subtitle; its Agents under it" \
    test "$(ui '{"op":"sidebar"}' | jq -r '.sidebar as $r | ($r | map(.title) | index("feat-refunds")) as $i | "\($r[$i].kind) \($r[$i].meta) \($r[$i+1].title)@\($r[$i+1].depth)"')" = "worktree payments-api lead@1"
# R104.6: the focused Agent's row is the selected one (else its Worktree).
want=$(rade list --json | jq -r --arg p "$(focused)" '[.panes[] | select(.pane==$p and .agent != null) | .name][0] // "feat-refunds"')
check "the selected row is the focused Agent's (R104.6), else the active Worktree ($want)" test "$(ui '{"op":"sidebar"}' | jq -r '.sidebar[] | select(.selected) | .title')" = "$want"
check "Agent rows are compact (≤ 30 px), Source rows ≥ Worktree rows" test "$(row lead | jq -r .height)" -le 30
check "Unread lives in the Inbox (payments-api: 2)" test "$(rpc debug.panel '{"op":"counts"}' | jq -r '.sources["payments-api"]')" = 2
ui '{"op":"toggle_source","arg":"payments-api"}' >/dev/null
check "folded state is in state.json" eventually 3 is "jq -r '.sources[] | select(.id==\"payments-api\") | .collapsed' $STATE" true
ui '{"op":"toggle_source","arg":"payments-api"}' >/dev/null
rade focus "$lead" >/dev/null
ui "{\"op\":\"click_agent\",\"pane\":\"$cl\"}" >/dev/null
check "a click on an Agent row focuses its Pane (R20-c)" eventually 3 is focused "$cl"
rade focus "$lead" >/dev/null; sleep 0.3

echo "== R20.2–R20.5 glyphs, ring, captions, Tabs"
check "no Pane has an Attention Ring (retired, ADR-0036)" test "$(for p in $codex $lead $cl $op; do ui "{\"op\":\"pane\",\"pane\":\"$p\"}" | jq -r .pane.ring; done | sort -u)" = null
check "unfocused splits dimmed, the focused one not" test "$(ui "{\"op\":\"pane\",\"pane\":\"$cl\"}" | jq -r .pane.dimmed)|$(ui "{\"op\":\"pane\",\"pane\":\"$lead\"}" | jq -r .pane.dimmed)" = "true|false"
check "no Tab label is a path (labels: $(tabs | jq -c '[.tabs[].label]'))" bash -c "! echo '$(tabs | jq -r '.tabs[].label')' | grep -q /"
# 1.0.0 (R103.2): the label is the lead Agent's name; the count is its own widget.
check "the grid Tab: named, with its Agent count" test "$(tabs | jq -r '.tabs[0] | "\(.label) \(.count)"')" = "lead 4"
check "…marked as needing attention, waiting glyph as its icon" test "$(tabs | jq -r '.tabs[0] | [.needs_attention, .icon] | join(" ")')" = "true waiting"
check "…unread in the tooltip, not the label" bash -c "echo '$(tabs | jq -r '.tabs[0].tooltip')' | grep -q 'unread'"

echo "== R20.9 Status Bar, R20.11 header, R20.10 Right Panel"
# 1.0.0 (R103.1): the titlebar breadcrumb replaces the header title / subtitle.
crumb() { ui | jq -r .chrome.breadcrumb; }
contains() { [[ $1 == *"$2"* ]]; }
check "breadcrumb = Source / Worktree ($(crumb))" contains "$(crumb)" "payments-api / feat-refunds"
check "Status Bar shown by default with counts worst first" test "$(ui | jq -c '.chrome.status_bar | [.visible, .counts]')" = '[true,["1 needs input","2 running","2 done"]]'
rade debug action toggle_status_bar >/dev/null
check "palette Toggle Status Bar hides it" test "$(ui | jq -r .chrome.status_bar.visible)" = false
rade debug action toggle_status_bar >/dev/null
check "…and shows it again" test "$(ui | jq -r .chrome.status_bar.visible)" = true
check "Inspector pages Changes / Files / Search (the Code page went in 1.0.0), Files shown" test "$(ui | jq -c '.chrome.right_panel | [.page, .pages, .toggle]')" = '["files",["changes","files","search"],true]'
ui '{"op":"right_page","arg":"changes"}' >/dev/null; sleep 0.5
check "the switcher shows Changes" test "$(ui | jq -r .chrome.right_panel.page)" = changes
scene right-changes
ui '{"op":"right_page","arg":"files"}' >/dev/null

echo "== R3.2 Inbox"
check "Inbox header counts Unread" test "$(ui '{"op":"inbox"}' | jq -r .inbox.title)" = "Inbox · $(rpc debug.panel '{"op":"counts"}' | jq -r .total)"
check "rows: status glyph per event, Source › Worktree first" test "$(ui '{"op":"inbox"}' | jq -r '.inbox.rows[] | select(.text=="codex-tests needs input") | [.glyph, .path] | join("|")')" = "waiting|payments-api › feat-refunds"
check "a message has no glyph" test "$(ui '{"op":"inbox"}' | jq -r '.inbox.rows[] | select(.text=="cargo test: 42 passed") | .glyph')" = null

echo "== R13.1 Review in its own full-width Tab"
rv=$(rade review open --source payments-api --worktree feat-refunds)
check "Review Pane is alone in a new Tab" test "$(rade list --json | jq -r --arg p "$rv" '.panes[] | select(.pane==$p) | .tab')" != "$(rade list --json | jq -r --arg p "$lead" '.panes[] | select(.pane==$p) | .tab')"
check "its Tab: 'Review', selected; the Review button counts 2 files" eventually 8 is "tabs | jq -r '.tabs[] | select(.selected) | .label'; ui | jq -r .chrome.review_count" "Review
2"
sleep 1
scene review
rade focus "$lead" >/dev/null; sleep 0.3
rade review open --source payments-api --worktree feat-refunds >/dev/null
check "opening it again shows that Tab (no second Review)" test "$(tabs | jq '[.tabs[] | select(.label|startswith("Review"))] | length')|$(tabs | jq -r '.tabs[] | select(.selected) | .label')" = "1|Review"
rade focus "$lead" >/dev/null; sleep 0.3

echo "== R20.8 Terminals follow light / dark (R20-d)"
bg() { # mean luminance of a blank strip at the bottom of a Pane's Terminal
    local b; b=$(rpc debug.window "{\"pane\":\"$1\"}" | jq -c .pane)
    rade debug screenshot "$OUT/px.png" >/dev/null
    python3 - "$OUT/px.png" "$b" <<'PY'
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB"); b = json.loads(sys.argv[2])
x0, y = int(b["x"] + b["width"] * 0.5), int(b["y"] + b["height"] - 12)
px = [im.getpixel((x, y)) for x in range(x0, x0 + 40)]
print(round(sum(0.2126*r + 0.7152*g + 0.0722*b_ for r, g, b_ in px) / len(px) / 255, 2))
PY
}
rade debug action color_scheme_light >/dev/null; sleep 1
L=$(bg "$lead")
rade debug action color_scheme_dark >/dev/null; sleep 1
D=$(bg "$lead")
rade debug action color_scheme_light >/dev/null; sleep 0.5
check "light style → light Terminal ($L), dark → dark ($D), live" python3 -c "import sys; sys.exit(not ($L > 0.8 and $D < 0.2))"

echo "== R20-b Toast when an Agent starts waiting off screen"
rade focus "$lead" >/dev/null
hid=$(rade spawn --source payments-api --worktree fix-idempotency --agent codex --name fixer "ask about it" --json | jq -r .pane)
check "a Toast '<b>fixer</b> needs input'" eventually 15 is "ui | jq -r '.chrome.toasts[-1] | .pane + \" \" + .title'" "$hid <b>fixer</b> needs input"
check "no Toast for codex-tests (it waited on screen)" test "$(ui | jq '[.chrome.toasts[] | select(.pane=="'"$codex"'")] | length')" -le 1
sleep 0.5; shot toast-light
ui '{"op":"toast_show"}' >/dev/null
check "Show focuses that Pane (its Worktree and Tab)" eventually 3 is focused "$hid"
check "breadcrumb now on fix-idempotency" contains "$(crumb)" " / fix-idempotency"
rade focus "$lead" >/dev/null

echo "== M13 follow-up: worktree add racing worktree list imports"
( for _ in $(seq 60); do rade worktree list web-dashboard >/dev/null 2>&1; done ) &
LISTER=$!
fails=0
for i in 1 2 3 4 5; do rade worktree add web-dashboard race-$i >/dev/null 2>>"$OUT/race.err" || fails=$((fails + 1)); done
wait $LISTER
check "5 worktree adds during 60 imports: no 'already has a worktree' ($fails failed)" test "$fails" = 0
rade worktree list web-dashboard --json > "$OUT/race.json"
check "…each listed once" test "$(rade worktree list web-dashboard --json | jq '[.worktrees[] | select(.worktree|startswith("race-"))] | length')" = 5

echo "== R20.1 contrast tokens (docs/internals/design/contrast.py)"
check "every status token pair meets WCAG 2.2" python3 "$ROOT/docs/internals/design/contrast.py"

echo
echo "M14: $PASS passed, $FAIL failed (screenshots: ${OUT#$ROOT/})"
stop_app
[[ $FAIL -eq 0 ]]
