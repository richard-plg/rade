# Live (R111-f, M40): scenes A, D and F on the real GNOME desktop at the
# monitor's scale, screenshots of the Phosphor icons at 16 px, the focused
# Pane's glow and the waiting LED, and an end-to-end Navigator scroll whose
# frame interval (`debug.ui perf` → `nav_scroll:interval`) is ≤ 16.7 ms at p95.
# Run it twice before tagging 1.0.0, once with Settings → Displays → Scale at
# 100 % and once at 200 % (decision 2026-10-23: the run never changes the
# owner's display settings):
#   tests/live/run.sh m40     → target/live/r111-f-<scale>x-*.png, results.txt
# Everything lives in the run's temp dir: Workspaces under $T/files, Agent
# config dirs under $T/files/home, the Usage store under $XDG_STATE_HOME = $T/state.
FAKE=$ROOT/tests/e2e/fake-agent.sh
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
ui() { rpc debug.ui "{\"op\":\"$1\"${2:+,\"arg\":\"$2\"}}"; }
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+,$2}}"; }
pf() { ui perf | jq -r --arg n "$1" --arg f "$2" '.samples[$n][$f] // "none"'; }
le() { python3 -c 'import sys; a, b = sys.argv[1:]; sys.exit(0 if a != "none" and float(a) <= float(b) else 1)' "$1" "$2"; }
tok() { python3 -c 'import sys, tomllib; v = tomllib.load(open(sys.argv[1], "rb"))[sys.argv[2]][sys.argv[3]]; print(v if len(v) == 9 else v + "ff")' "$ROOT/crates/rade-app/src/tokens.toml" "$1" "$2"; }
pane_status() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }

# The Usage store is the run's own ($XDG_STATE_HOME = $T/state, lib.sh).
USAGE=$XDG_STATE_HOME/rade-dev/usage
FH=$T/files/home
A=$T/files/acme
WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui
for r in "$WEB" "$API" "$UI"; do
    mkdir -p "$r/src" && G -C "$r" init -q -b main
    for f in a b c; do printf '%s\n' "$f" >"$r/src/$f.ts"; done
    G -C "$r" add -A && G -C "$r" commit -qm init && G -C "$r" branch develop
done
for d in .claude-work .claude-team .codex .codex-biz .claude .codex-personal .codex-oss; do mkdir -p "$FH/$d"; done
mkdir -p "$A/.rade"
cat >"$A/.rade/workspace.toml" <<EOF
name = "acme"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "$FH/.claude-work"
default = true

[[account]]
agent = "claude"
name = "acme-team"
config_dir = "$FH/.claude-team"

[[account]]
agent = "codex"
name = "acme"
config_dir = "$FH/.codex"
default = true

[[account]]
agent = "codex"
name = "acme-biz"
config_dir = "$FH/.codex-biz"
EOF
scene() { python3 "$ROOT/tests/e2e/usage-scene.py" "$USAGE" "$FH" >/dev/null; }
scene
S=$(rade debug window --json | jq -r .scale)
echo "  monitor scale: ${S}× (window $(rade debug window --json | jq -r '"\(.width) × \(.height)"'))"
P=r111-f-${S}x

rade workspace add "$A" >/dev/null
for r in "$WEB" "$API" "$UI"; do rade source add "$r" --workspace acme >/dev/null; done
rade workspace switch acme >/dev/null
rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null || echo "project add Shop failed"
CV=$(rade worktree add shop-web checkout-v2 --project shop --json | jq -r .path)
echo v2 >>"$CV/src/a.ts"
planner=$(rade spawn --source shop-web --worktree checkout-v2 --name planner --tab new --json \
    --cmd "$FAKE --model opus 'osc 24 tests passed'" | jq -r .pane)
eventually 10 is "pane_status $planner" done || true
rade focus "$planner" >/dev/null
tests=$(rade spawn --source shop-web --worktree checkout-v2 --name tests --tab current --split right --json \
    --cmd "$FAKE --model gpt-5 'oscfirst Which fixtures?'" | jq -r .pane)
eventually 10 is "pane_status $tests" waiting || true
rpc usage.refresh '{"workspace":"acme"}' >/dev/null
rade focus "$tests" >/dev/null
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null
sleep 1

echo "-- R111-f scene A at ${S}×"
check "R111-f ${S}× the window is on the real desktop at an integer scale (1 or 2)" bash -c "[[ '$S' == 1 || '$S' == 2 ]]"
want "R111-f ${S}× A: tests waits, planner is done" 5 "echo \$(pane_status $tests) \$(pane_status $planner)" "waiting done"
panes=$(geo pane '"all":true')
check "R111-f ${S}× A: the focused Pane (tests) has the 18 px glow" \
    test "$(jq -r '.[] | select(.classes | index("focused")) | .shadow' <<<"$panes")" = "0 0 18px $(tok dark glow)"
check "R111-f ${S}× A: the Tab shows the waiting LED (7 × 7)" \
    test "$(geo 'tab led' '"all":true' | jq -r '[.[] | select(.classes | index("led-waiting")) | "\(.width)x\(.height)"] | first')" = 7x7
check "R111-f ${S}× A: the + icon is a 16 px Phosphor image drawn at the monitor scale" \
    bash -c "[[ \$(geo 'tab-add image' | jq -r '\"\(.width)x\(.height)\"') == 16x16 && \$(rade debug window --json | jq -r .scale) == '$S' ]]"
shot "$P-a-dark"
rade debug action color_scheme_light >/dev/null; sleep 1
shot "$P-a-light"
rade debug action color_scheme_dark >/dev/null; sleep 1

echo "-- R111-f scene D at ${S}×"
rpc debug.ui '{"op":"key","keys":"ctrl+shift+p"}' >/dev/null
want "R111-f ${S}× D: the palette opens" 3 "rade --json debug palette --ui info | jq -r .open" true
rade --json debug palette --ui type shared >/dev/null
want "R111-f ${S}× D: results for 'shared' are ranked" 5 "rade --json debug palette --ui info | jq -r .busy" false
shot "$P-d-dark"
rade --json debug palette --ui key escape >/dev/null || true
sleep 0.3

echo "-- R111-f scene F at ${S}×"
scene
rpc debug.ui '{"op":"forecast_check","arg":"reset"}' >/dev/null
want "R111-f ${S}× F: the forecast Toast is up" 10 "geo toast | jq -r .visible" true
rpc debug.ui '{"op":"new_menu","arg":"open"}' >/dev/null
want "R111-f ${S}× F: the + popover is open, 340 px" 3 "geo new-menu | jq -r .width" 340
shot "$P-f-dark"
rpc debug.ui '{"op":"new_menu","arg":"close"}' >/dev/null
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null

echo "-- R111-f an end-to-end Navigator scroll at ${S}× (10 Projects × 3 links × 2 Worktrees)"
NAV=$T/files/nav
TPL=$T/files/tpl
mkdir -p "$NAV/.rade" && printf 'name = "nav"\n' >"$NAV/.rade/workspace.toml"
mkdir -p "$TPL" && G -C "$TPL" init -q -b main && echo base >"$TPL/README.md" && G -C "$TPL" add -A && G -C "$TPL" commit -qm init
rade workspace add "$NAV" >/dev/null
for p in $(seq -w 1 10); do
    for l in 1 2 3; do cp -a "$TPL" "$NAV/s$p$l" && rade source add "$NAV/s$p$l" --workspace nav >/dev/null; done
done
rade workspace switch nav >/dev/null
for p in $(seq -w 1 10); do
    rade project add "P$p" --source "s${p}1:main" --source "s${p}2:main" --source "s${p}3:main" >/dev/null || echo "project add P$p failed"
    for l in 1 2 3; do
        for w in a b; do rade worktree add "s$p$l" "w$p$l$w" --project "p$p" >/dev/null || echo "worktree w$p$l$w failed"; done
    done
done
navrows() { ui navigator | jq -r '[.sidebar[] | select(.kind == "project" or .kind == "link" or .kind == "worktree")] | length'; }
want "R111-f ${S}× the Navigator shows 100 rows (10 + 30 + 60)" 20 navrows 100
ui perf "reset nav_scroll" >/dev/null
ui perf "scroll 240" >/dev/null
eventually 30 is "ui perf | jq -r .scrolling" false || true
iv=$(pf nav_scroll:interval p95) work=$(pf nav_scroll:work p95) frames=$(pf nav_scroll:interval n)
echo "  Navigator scroll: $frames frames, interval p95 $iv ms (max $(pf nav_scroll:interval max)), work p95 $work ms"
check "R111-f ${S}× the frame interval of an end-to-end Navigator scroll is ≤ 16.7 ms at p95 ($iv ms over $frames frames)" \
    bash -c "[[ $frames != none ]] && (( $frames >= 200 )) && python3 -c 'import sys; sys.exit(0 if $iv <= 16.7 else 1)'"
check "R111-f ${S}× …and each frame's own work stays ≤ 16.7 ms at p95 ($work ms)" le "$work" 16.7
shot "$P-navigator-scrolled"
rade workspace switch acme >/dev/null
echo "  look at target/live/$P-*.png: icons crisp at 16 px (no blur at ${S}×), the glow around 'tests', the amber LED on its Tab"
