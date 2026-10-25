#!/usr/bin/env bash
# E2E (M35, R102 / R103): the terminal-native visual system and window chrome
# on scene A's data (docs/internals/design/spec/a-workspace.md): bundled fonts under a
# fontconfig without them, `debug.ui geometry` and test mode, live theme
# switch with the Terminal's tokens, breadcrumb + Review count, the command
# field / Ctrl+Shift+P / chrome-only Ctrl+K, flat tab labels and LEDs, Pane
# headers with the focus glow, the Statusline; then scene A at 1440 × 900
# measured against spec A (tests/e2e/spec_a.py), the one primary button, the
# Toast lift, empty states, the Inspector toggles, and the design-diff page
# (tests/e2e/design-diff.sh; golden A moved to tests/e2e/m37.sh in M37).
# Screenshots light + dark → target/e2e-m35/.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m35}
# Scene A's window is 1440 × 900 (spec A.1).
export WESTON_WIDTH=1440
source "$HERE/lib.sh"
FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "$FAKE --model opus"
[agents.codex]
command = "$FAKE --model gpt-5"
C
# R102-d: a fontconfig that knows only DejaVu (no Inter, no JetBrains Mono).
mkdir -p "$T/fc-cache"
cat > "$T/fonts.conf" <<F
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig><dir>/usr/share/fonts/truetype/dejavu</dir><cachedir>$T/fc-cache</cachedir></fontconfig>
F
export FONTCONFIG_FILE=$T/fonts.conf

# Scene A's fixture: Workspace acme, Project Shop (base develop), Source
# shop-web, Worktree checkout-v2 3 commits ahead with 4 changed files.
G() { git -c user.name=dev -c user.email=d@e -c init.defaultBranch=main "$@"; }
ACME=$T/acme
WEB=$ACME/shop-web
mkdir -p "$WEB/src"
(cd "$WEB" && G init -q && printf '# shop-web\n' > README.md && printf 'x\n' > src/app.ts \
    && G add -A && G commit -qm init && G branch develop \
    && git config user.email richard@acme.io)
API=$ACME/shop-api UI=$ACME/shared-ui EMPTY=$T/empty-folder
for r in "$API" "$UI"; do
    mkdir -p "$r/src"
    (cd "$r" && G init -q && printf '# %s\n' "${r##*/}" > README.md && G add -A && G commit -qm init \
        && G branch develop && git config user.email richard@acme.io)
done
mkdir -p "$EMPTY"
ui() { rpc debug.ui "${1:-{\}}"; }
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+,$2}}"; }
chrome() { ui | jq -c .chrome; }
tabs() { rpc debug.tabs; }

start_app
ui '{"op":"layout","width":1440,"height":900}' >/dev/null
rade workspace add "$ACME" --name acme >/dev/null || echo "workspace add failed"
rade source add "$WEB" --name shop-web >/dev/null || echo "source add failed"
rade source add "$API" --name shop-api >/dev/null || echo "source add failed"
rade source add "$UI" --name shared-ui >/dev/null || echo "source add failed"
rade project add Shop --workspace acme --source shop-web:develop --source shop-api:develop \
    --source shared-ui:develop >/dev/null || echo "project add failed"
wt=$(rade worktree add shop-web checkout-v2 --project shop --base develop --json | jq -r .path)
for f in a b c; do
    printf '%s\n' "$f" > "$wt/src/$f.ts"; (cd "$wt" && G add -A && G commit -qm "add $f")
done
printf 'changed\n' >> "$wt/README.md"
rade workspace switch acme >/dev/null
shell=$(rade spawn --source shop-web --worktree checkout-v2 --tab new --json | jq -r .pane)
rade focus "$shell" >/dev/null
sleep 1

echo "== R102 visual system"
fam() { geo "$1" | jq -r '[.font.family, (.font.features | join(","))] | join(" ")'; }
check "R102-d Navigator row title family Inter (no system copy)" eventually 5 is "geo nav-row-title | jq -r .font.family" Inter
check "R102-d breadcrumb branch JetBrains Mono with tnum" eventually 5 is "fam crumb-branch" "JetBrains Mono tnum"

sl=$(geo statusline)
echo "$sl" > "$OUT/statusline-geometry.json"
check "R102-f statusline geometry: height 26 ± 1" bash -c "h=\$(jq .height <<<'$sl'); (( h >= 25 && h <= 27 ))"
# A token's #rrggbbaa: tok <dark|light> <name>.
tok() { python3 -c 'import sys, tomllib; v = tomllib.load(open(sys.argv[1], "rb"))[sys.argv[2]][sys.argv[3]]; print(v if len(v) == 9 else v + "ff")' "$ROOT/crates/rade-app/src/tokens.toml" "$1" "$2"; }
check "R102-f statusline geometry: margin, padding, font, color fg2, background bg1, classes" test "$(jq -r '[(.margin|type), (.padding|type), .font.family, .color, .background, (.classes|index("statusline")|type)] | join(" ")' <<<"$sl")" = "object object JetBrains Mono $(tok dark fg2) $(tok dark bg1) number"
# A second app on another state and socket, started without test mode.
plain() {
    local d=$T/plain; mkdir -p "$d/data"
    env -u RADE_DEBUG_SOCKET XDG_DATA_HOME=$d/data RADE_SOCKET=$d/p.sock \
        "$BIN/rade-app" >>"$OUT/plain-app.log" 2>&1 &
    PLAIN_PID=$!
    for _ in $(seq 100); do [[ -S $d/p.sock ]] && break; sleep 0.1; done
    sleep 0.5
    RADE_SOCKET=$d/p.sock rade debug ui geometry '{"widget":"statusline"}' >"$OUT/plain.out" 2>"$OUT/plain.err"
    echo $? >"$OUT/plain.code"
    kill "$PLAIN_PID" 2>/dev/null; wait "$PLAIN_PID" 2>/dev/null
}
plain
check "R102-f without RADE_DEBUG_SOCKET=1: exit 2 with the ADR-0021 hint" bash -c "[[ \$(cat '$OUT/plain.code') == 2 ]] && grep -q 'RADE_DEBUG_SOCKET=1 (ADR-0021)' '$OUT/plain.err'"

echo "== R102-g live theme switch"
# `background` = the color GTK draws for the widget's own box (its computed
# style rendered alone); `drawn.background` = the window's pixel where no
# child covers the widget (R102.5).
bgof() { geo "$1" | jq -r .background; }
drawnbg() { geo "$1" | jq -r .drawn.background; }
rade debug action color_scheme_light >/dev/null
check "R102-g light: window background = light bg1" eventually 5 is "bgof window" "$(tok light bg1)"
check "R102-g light: …and drawn: the Statusline's pixels = light bg1" eventually 5 is "drawnbg statusline" "$(tok light bg1)"
check "R102-g light: Terminal background = light bg0 (term-bg)" eventually 5 is "bgof terminal" "$(tok light bg0)"
rade debug action color_scheme_dark >/dev/null
check "R102-g dark again: window background = dark bg1" eventually 5 is "bgof window" "$(tok dark bg1)"
check "R102-g dark again: …and drawn: the Statusline's pixels = dark bg1" eventually 5 is "drawnbg statusline" "$(tok dark bg1)"
check "R102-g dark again: Terminal background = dark bg0, same process" eventually 5 is "bgof terminal" "$(tok dark bg0)"
check "…the same app process" kill -0 "$APP_PID"

echo "== R103-b breadcrumb and Review count"
check "R103-b breadcrumb 'Shop / shop-web / checkout-v2 ← develop ↑3'" eventually 10 is "chrome | jq -r .breadcrumb" "Shop / shop-web / checkout-v2 ← develop ↑3"
check "R103-b Review button counts checkout-v2's 4 changed files" eventually 10 is "chrome | jq -r .review_count" 4
check "R103-b …and shows it (review-count label)" test "$(geo review-count | jq -r .label)" = 4

echo "== R103-c command field, Ctrl+Shift+P, chrome-only Ctrl+K"
key() { rpc debug.ui "{\"op\":\"key\",\"keys\":\"$1\"}"; }
popen() { rade debug palette --ui info | jq -r .open; }
rade focus "$shell" >/dev/null
rade send "$shell" 'cat -v' --enter
sleep 0.5
rpc debug.ui '{"op":"click","widget":"command-field"}' >/dev/null
check "R103-c a click on the command field opens the palette" eventually 3 is popen true
key escape >/dev/null
check "R103-c …Esc closes it" eventually 3 is popen false
check "R103-c …and typing reaches the same Terminal" test "$(key a | jq -r '[.handled, .pane] | join(" ")')" = "terminal $shell"
key ctrl+shift+p >/dev/null
check "R103-c Ctrl+Shift+P from the Terminal opens the palette" eventually 3 is popen true
key escape >/dev/null
sleep 0.3
check "R103-c …Esc, typing reaches the same Terminal again" test "$(key b | jq -r '[.handled, .pane] | join(" ")')" = "terminal $shell"
check "R103-c Ctrl+K in the Terminal goes to the program" test "$(key ctrl+k | jq -r .handled)" = terminal
check "R103-c …the palette stays closed" is popen false
check "R103-c …cat -v prints ^K" eventually 3 screen_has "$shell" "ab^K"
rpc debug.ui '{"op":"focus","widget":"navigator"}' >/dev/null
check "R103-c Ctrl+K from the Navigator opens the palette" test "$(key ctrl+k | jq -r .handled)" = rade
check "R103-c …palette open" eventually 3 is popen true
key escape >/dev/null
rade focus "$shell" >/dev/null

echo "== R103-d flat tabs"
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new "hello" --json | jq -r .pane)
sleep 0.5
rade focus "$planner" >/dev/null
tests=$(rade spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab current --split right "ask which fixtures" --json | jq -r .pane)
eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$tests\") | .status'" waiting
eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$planner\") | .status'" done
# fake-agent's own hook marks it `fake`: give each Pane its Preset back.
rade status --pane "$planner" --agent claude >/dev/null
rade status --pane "$tests" --agent codex >/dev/null
agents_tab=$(rade tab list --json | jq -r --arg p "$planner" '.tabs[] | select(.panes | index($p)) | .tab')
rade tab rename "$agents_tab" agents >/dev/null
rade focus "$tests" >/dev/null
sleep 1
tl() { tabs | jq -r '[.tabs[] | .label] | join(",")'; }
check "R103-d labels: the shell Tab is 'checkout-v2', the renamed one 'agents'" eventually 5 is tl "checkout-v2,agents"
check "R103-d 'agents' has count 2 and the waiting LED" test "$(tabs | jq -r '.tabs[] | select(.label=="agents") | [.count, .led] | join(" ")')" = "2 waiting"
labels=$(tabs | jq -r '.tabs[].label')
check "R103-d no label matches 'Tab [0-9]+' ($(echo $labels))" bash -c "[[ -n '$labels' ]] && ! grep -Eq '^Tab [0-9]+' <<<'$labels'"
check "R103-d the open Tab has the 2 px acc bar over its full width" test "$(geo 'tab-activebar' '"all":true' | jq -r --arg a "$(tok dark acc)" '[.[] | select(.background==$a) | .height] | join(",")')" = 2
geo 'tab led' '"all":true' > "$OUT/tab-leds.json"
check "R103-d / R115 the lead Agent's glyph in the waiting colour: claude, 14 × 14" test "$(geo 'tab-glyph' '"all":true' | jq -r '[.[] | select(.classes | index("glyph-waiting")) | "\(.classes | map(select(startswith("mark-"))) | join(""))/\(.width)x\(.height)"] | join(",")')" = mark-claude/14x14

echo "== R103-e Pane headers and the focus glow"
panes=$(geo pane '"all":true')
echo "$panes" > "$OUT/panes-geometry.json"
check "R103-e two Panes shown, only 'tests' has class focused" test "$(jq -r '[.[] | select(.classes | index("focused")) | .label] | join(",")' <<<"$panes")" = tests
check "R103-e focused Pane: 1 px acc-line border and the 18 px glow" test "$(jq -r '.[] | select(.classes | index("focused")) | [.border.top, .border_color.top, .shadow] | join(" ")' <<<"$panes")" = "1 $(tok dark acc-line) 0 0 18px $(tok dark glow)"
check "R103-e unfocused Pane: 1 px line border, no glow" test "$(jq -r --arg l "$(tok dark line)" '.[] | select(.classes | index("focused") | not) | [.border.top, .border_color.top == $l, (.shadow // "none")] | join(" ")' <<<"$panes")" = "1 true none"
# As drawn (the window's pixels): no amber Attention Ring (retired,
# ADR-0036), so the waiting focused `tests` shows the focus ring too.
# over <fg #rrggbbaa> <bg #rrggbb(aa)>: fg composited over an opaque bg.
over() { python3 -c 'import sys
f, b = sys.argv[1], sys.argv[2]
c = lambda h, i: int(h[1 + 2 * i:3 + 2 * i], 16)
a = c(f, 3) / 255
print("#" + "".join("%02x" % round(c(f, i) * a + c(b, i) * (1 - a)) for i in range(3)) + "ff")' "$1" "$2"; }
# near <a> <b>: two #rrggbbaa within ±2 per channel.
near() { python3 -c 'import sys; a, b = sys.argv[1:3]; sys.exit(0 if all(abs(int(a[i:i + 2], 16) - int(b[i:i + 2], 16)) <= 2 for i in (1, 3, 5, 7)) else 1)' "$1" "$2"; }
edges() { geo pane '"all":true' | jq -r --arg p "$1" '.[] | select(.label == $p) | .drawn.edge | [.top, .right, .bottom, .left] | join(" ")'; }
edges_are() { local e; for e in $(edges "$1"); do near "$e" "$2" || return 1; done; [[ -n $(edges "$1") ]]; }
ring=$(over "$(tok dark acc-line)" "$(tok dark bg0)")
check "R103-e drawn: the waiting focused 'tests' shows the acc-line ring, no amber ($(edges tests))" eventually 5 edges_are tests "$ring"
rade focus "$planner" >/dev/null
check "R103-e drawn: focused 'planner' shows the 1 px acc-line ring on all four edges = $ring" eventually 5 edges_are planner "$ring"
glow_beside() { local g; g=$(geo pane '"all":true' | jq -r '.[] | select(.label == "planner") | .drawn.outside.right'); [[ -n $g && $g != null && $g != "$(tok dark bg1)" ]]; }
check "R103-e drawn: …and its glow in the gap beside it (the pixel right of it ≠ bg1)" glow_beside
rade focus "$tests" >/dev/null
hdr() { ui "{\"op\":\"pane\",\"pane\":\"$1\"}" | jq -r .pane.header; }
check "R103-e header 'tests · codex · gpt-5 · needs input'" eventually 5 is "hdr $tests" "tests · codex · gpt-5 · needs input"
check "R103-e header 'planner · claude · opus · done'" is "hdr $planner" "planner · claude · opus · done"
check "R103-e Pane header 26 px, JetBrains Mono" test "$(geo pane-header | jq -r '[(.height >= 25 and .height <= 27), .font.family] | join(" ")')" = "true JetBrains Mono"
check "R103-e no colored top edge on the header" test "$(geo pane-header | jq -r '.shadow // "none"')" = none

echo "== R103-k a click into a Pane moves the focus ring and the dimming"
lit() { ui '{"op":"pane_frames"}' | jq -r '"\(.lit | join(",")) \(.focused)"'; }
check "R103-k before: 'tests' focused and lit" eventually 3 is lit "$tests $tests"
ui "{\"op\":\"pane_focus\",\"pane\":\"$planner\"}" >/dev/null
check "R103-k focus into 'planner' (as a click): only 'planner' is lit" eventually 3 is lit "$planner $planner"
shot r103-k-click-planner
ui "{\"op\":\"pane_focus\",\"pane\":\"$tests\"}" >/dev/null
check "R103-k …and back into 'tests': only 'tests' is lit" eventually 3 is lit "$tests $tests"

echo "== R103-j Statusline"
ui_spin=$(rade spawn --source shop-web --agent claude --name ui --tab new "spin" --json | jq -r .pane)
dev=$(rade spawn --source shop-web --agent claude --name dev-server --tab new "hello" --json | jq -r .pane)
sleep 1
rade status running --pane "$ui_spin" >/dev/null
eventually 10 is "rade list --json | jq -r '.panes[] | select(.pane==\"$dev\") | .status'" done
rade status running --pane "$ui_spin" >/dev/null
rade send "$shell" $'\x03' >/dev/null 2>&1 || true
rade send "$shell" "python3 -m http.server 5173 --bind 127.0.0.1 >/dev/null 2>&1 & echo \$! > $T/http.pid" --enter
rade focus "$tests" >/dev/null
cells() { chrome | jq -r '.statusline.cells | join("|")'; }
want="ACME|Shop|checkout-v2 ↑3 ↓0|1 waiting · 1 running · 2 done|:5173|richard@acme.io|Ctrl ⇧ P commands|Ctrl ⇧ G next waiting"
eventually 15 is cells "$want"; chrome > "$OUT/chrome-statusline.json"; rade read "$shell" > "$OUT/shell.txt"
check "R103-j Statusline cells in order: $want" eventually 15 is cells "$want"
check "R103-j Workspace cell: acc fill, acc-fg text" test "$(geo status-cell-ws | jq -r '[.background, .color, .label] | join(" ")')" = "$(tok dark acc) $(tok dark acc-fg) ACME"
cat > "$XDG_CONFIG_HOME/rade/config.toml.new" <<C
idle_seconds = 30
status_bar = false
[agents.claude]
command = "$FAKE --model opus"
[agents.codex]
command = "$FAKE --model gpt-5"
C
mv "$XDG_CONFIG_HOME/rade/config.toml.new" "$XDG_CONFIG_HOME/rade/config.toml"
sleep 1; rade focus "$planner" >/dev/null
check "R103-j status_bar = false hides it" eventually 5 is "chrome | jq -r .statusline.visible" false
sed -i '/^status_bar = false$/d' "$XDG_CONFIG_HOME/rade/config.toml"
sleep 1; rade focus "$tests" >/dev/null
check "…and back" eventually 5 is "chrome | jq -r .statusline.visible" true

echo "== Scene A at 1440 × 900 (golden, R103-a, R103-f)"
# checkout-v2's 4 files also changed in the tree: Changes 4, Review still 4.
for f in a b c; do printf 'more\n' >> "$wt/src/$f.ts"; done
rade focus "$tests" >/dev/null
[[ $(chrome | jq -r .right_panel.toggle) == true ]] || rade debug action toggle_file_tree >/dev/null
ui '{"op":"right_page","arg":"changes"}' >/dev/null
check "scene A window 1440 × 900" eventually 10 is "rade debug window --json | jq -r '\"\(.width)x\(.height)\"'" 1440x900
# 1.0.0 (R107.1, M35 decision): the tab counts the Project's changed files.
ptotal() { rpc debug.scm | jq -r '[.cards[].groups[].count] | add // 0'; }
labels_total() { [[ "$(chrome | jq -r '.inspector.labels | join(",")')" == "Changes $(ptotal),Files,Search" ]]; }
check "scene A: Inspector shows Changes n = the Project's changed files" eventually 10 labels_total
eventually 10 is "geo commit-button | jq -r .visible" true
sleep 1
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
spec() { python3 "$HERE/spec_a.py" "$SOCK" "$ROOT/crates/rade-app/src/tokens.toml" "$1" "$planner" "$tests" >"$OUT/spec-a-$1.txt" 2>&1; }
spec dark
check "R103-a dark: spec A rows A.2–A.16, A.39–A.57, A.70–A.73, A.75–A.77 within tolerance ($(grep -c ^PASS "$OUT/spec-a-dark.txt") rows pass)" spec dark
grep ^FAIL "$OUT/spec-a-dark.txt"
check "R103-a A.78 / A.79: contrast.py (text ≥ 4.5, glyphs ≥ 3, both themes)" bash -c "python3 '$ROOT/docs/internals/design/contrast.py' > '$OUT/contrast.txt'"
# R103-f: the visible .suggested-action widgets of the top surface.
primaries() { geo .suggested-action '"all":true' | jq -r '[.[] | .label] | join(",")'; }
check "R103-f scene A: exactly one primary button, Commit" eventually 5 is primaries Commit
check "R103-f …Commit is the accent fill with acc-fg 600" test "$(geo commit-button | jq -r --arg a "$(tok dark acc)" '[.font.weight, (.classes | index("suggested-action") != null)] | join(" ")')" = "600 true"
key ctrl+shift+p >/dev/null
check "R103-f with the palette open the screen below has 0 (decision M33)" eventually 3 is primaries ""
key escape >/dev/null
check "R103-f …1 again when it closes" eventually 3 is primaries Commit
rade focus "$tests" >/dev/null
sleep 0.5
# Golden masks: what changes from run to run (Terminal contents and cursor,
# the running spinner, the pulsing running LEDs); small time texts ("7s")
# stay under the compare threshold.
masks() {
    python3 - "$(geo terminal '"all":true')" "$(geo glyph-running '"all":true')" "$(geo led-running '"all":true')" <<'PY' > "$OUT/masks-a.json"
import json, sys
out = []
for why, arg in zip(["terminal contents", "running spinner", "running LED pulse"], sys.argv[1:]):
    for g in json.loads(arg):
        out.append({"x": g["x"] - 2, "y": g["y"] - 2, "w": g["width"] + 4, "h": g["height"] + 4, "why": why})
print(json.dumps(out, indent=1))
PY
}
masks
shot a-dark
rade debug action color_scheme_light >/dev/null; sleep 1
spec light
check "R103-a light: spec A rows within tolerance ($(grep -c ^PASS "$OUT/spec-a-light.txt") rows pass)" spec light
grep ^FAIL "$OUT/spec-a-light.txt"
shot a-light
rade debug action color_scheme_dark >/dev/null; sleep 0.5

echo "== Golden A: moved to tests/e2e/m37.sh (tests/e2e/golden/m37/, M37: the Project-total data, M35 decision)"
check "design-diff page generated (this scene A beside the mockup and golden A of m37)" bash -c "OUT='$OUT' SCENES=a '$HERE/design-diff.sh' >/dev/null && [[ -s '$ROOT/target/design-diff/index.html' ]]"

echo "== R103-g a Toast never covers a button"
rv=$(rade review open --source shop-web --worktree checkout-v2 2>"$OUT/review-open.err")
echo "review pane: $rv"
sleep 1
geo button '"all":true' | jq -c '[.[] | .label]' > "$OUT/buttons.json"
review_buttons() { geo button '"all":true' | jq '[.[] | select(.label == "Comment" or ((.label // "") | startswith("Approve")))] | length'; }
check "R103-g the Review Pane's Comment / Approve are on screen" eventually 5 is review_buttons 2
rade status running --pane "$ui_spin" >/dev/null
sleep 0.5
rade status waiting --pane "$ui_spin" >/dev/null
check "R103-g the hidden Agent's Toast shows" eventually 5 is "geo toast | jq -r .visible" true
sleep 1.2  # the Toast's reveal and one lift tick
overlaps() { python3 - "$(geo toast)" "$(geo button '"all":true')" <<'PY'
import json, sys
t = json.loads(sys.argv[1]); bs = json.loads(sys.argv[2])
def inside(b): return b["x"] >= t["x"] and b["y"] >= t["y"] and b["x"] + b["width"] <= t["x"] + t["width"] and b["y"] + b["height"] <= t["y"] + t["height"]
def hit(b): return b["x"] < t["x"] + t["width"] and t["x"] < b["x"] + b["width"] and b["y"] < t["y"] + t["height"] and t["y"] < b["y"] + b["height"]
print(sum(1 for b in bs if hit(b) and not inside(b)))
PY
}
geo toast > "$OUT/toast.json"
check "R103-g the Toast's bounds intersect 0 visible buttons ($(overlaps))" eventually 3 is overlaps 0
shot toast-lift
rade status done --pane "$ui_spin" >/dev/null
rade close "$rv" >/dev/null 2>&1 || true

echo "== R103-h empty states"
noempty() { # one mono line, key caps, no StatusPage, no warning icon in the Inspector
    local g; g=$(geo 'inspector empty-line' '"all":true')
    [[ $(jq length <<<"$g") == 1 ]] && [[ $(jq -r '.[0].font.family' <<<"$g") == "JetBrains Mono" ]] \
        && [[ $(geo 'inspector empty-state kbd' '"all":true' | jq length) -ge 1 ]] \
        && [[ $(geo 'inspector statuspage' '"all":true' | jq length) == 0 ]] \
        && [[ $(geo 'inspector image' '"all":true' | jq '[.[] | select((.icon // "") | test("warning"))] | length') == 0 ]]
}
line() { geo 'inspector empty-line' | jq -r .label; }
clean=$(rade worktree add shop-web clean-wt --project shop --base develop --json | jq -r .path)
cshell=$(rade spawn --source shop-web --worktree clean-wt --tab new --json | jq -r .pane)
rade focus "$cshell" >/dev/null
ui '{"op":"right_page","arg":"changes"}' >/dev/null
# 1.0.0 (R107.1, M45): clean-wt is Shop's, so Changes shows the Project's cards.
clean_group() { rpc debug.scm | jq -e '.cards[] | select(.label=="clean-wt") | .clean' >/dev/null; }
check "R107.1 the clean Worktree's card is folded to 'clean'" eventually 10 clean_group
check "R103-h Changes of a clean Worktree (a Project's link, M45: one page): '$(line)'" eventually 10 is line "No changes against each link's base"
check "R103-h …one mono line + key hints, no StatusPage / dialog-warning" eventually 3 noempty
check "R103-f a clean Worktree: 0 primary buttons" eventually 5 is primaries ""
shot empty-changes
rade source add "$EMPTY" --name empty-folder >/dev/null
eshell=$(rade spawn --source empty-folder --tab new --json | jq -r .pane)
rade focus "$eshell" >/dev/null
ui '{"op":"right_page","arg":"files"}' >/dev/null
check "R103-h Files of an empty folder: '$(line)'" eventually 10 is line "Empty folder"
check "R103-h …one mono line + key hints, no StatusPage / dialog-warning" eventually 3 noempty
shot empty-files
ui '{"op":"right_page","arg":"search"}' >/dev/null
check "R103-h Search with no query: '$(line)'" eventually 5 is line "Type to search this checkout"
check "R103-h …one mono line + key hints, no StatusPage / dialog-warning" eventually 3 noempty
shot empty-search

echo "== R103-i Inspector toggles"
# shop-api's main checkout has 7 changed files (Changes n = the focused checkout's, decision M35).
for i in 1 2 3 4 5 6 7; do printf '%s\n' "$i" > "$API/src/f$i.rs"; done
ashell=$(rade spawn --source shop-api --tab new --json | jq -r .pane)
rade focus "$ashell" >/dev/null
rade send "$ashell" 'cat -v' --enter
ui '{"op":"right_page","arg":"changes"}' >/dev/null
shown() { chrome | jq -r .inspector.shown; }
cw() { geo center-column | jq -r .width; }
check "R103-i tabs read Changes 7, Files, Search + the Activity icon tab" eventually 10 is "chrome | jq -r '[(.inspector.labels | join(\",\")), .inspector.activity] | join(\" \")'" "Changes 7,Files,Search true"
sleep 1  # cat -v's title change lands first (a model update focuses the Pane)
w0=$(cw)
rpc debug.ui '{"op":"focus","widget":"navigator"}' >/dev/null
check "R103-i Ctrl+B from the Navigator is RADE's" test "$(key ctrl+b | jq -r .handled)" = rade
check "R103-i …hides the Inspector" eventually 5 is shown false
sleep 0.6
w1=$(cw)
check "R103-i …the center widens by 340 ± 1 ($w0 → $w1)" bash -c "d=\$(( $w1 - $w0 )); (( d >= 339 && d <= 341 ))"
key ctrl+b > "$OUT/second-ctrl-b.json"
check "R103-i …a second Ctrl+B shows it" eventually 5 is shown true
rade focus "$ashell" >/dev/null
sleep 0.3
check "R103-i Ctrl+Shift+E from a cat -v Terminal hides it" test "$(key ctrl+shift+e | jq -r .handled)" = rade
check "R103-i …hidden" eventually 5 is shown false
key ctrl+shift+e >/dev/null
check "R103-i …a second Ctrl+Shift+E shows it" eventually 5 is shown true
check "R103-i Ctrl+B in the Terminal goes to the program" test "$(key ctrl+b | jq -r .handled)" = terminal
check "R103-i …cat -v prints ^B" eventually 3 screen_has "$ashell" "^B"
check "R103-i …the Inspector is still shown" is shown true
shot inspector-toggle

[[ -s $T/http.pid ]] && kill "$(cat "$T/http.pid")" 2>/dev/null

summary() { echo "m35: $PASS passed, $FAIL failed"; ((FAIL == 0)); }
summary
