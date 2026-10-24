#!/usr/bin/env bash
# 0.7.1 end-to-end tests (code navigation as the owner uses it): Ctrl held
# over a name shows it as a link (underline, hand pointer) in the Viewer
# and on Review diff lines; Ctrl+click navigates without touching the
# selection; the results popover's call hierarchy tree (call sites per caller,
# callers of callers on demand, a recursion not expanded again, Callers ⇄
# Callees, "Other references (not calls)"); `rade code callers` per call
# site, `--depth`; files outside the Worktree (a std library) and in
# node_modules asked through the server they were reached from (no second
# server). FAKE language server (tests/e2e/fake-lsp.py), then a real
# typescript-language-server on a small TS project of the owner's
# momentWithTZ shape (skipped with the reason when not installed).
# Light + dark screenshots.
# Usage: tests/e2e/m31.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m31
# Before lib.sh swaps HOME: a real typescript-language-server and the
# typescript module it runs (read only).
export TSLS=${RADE_E2E_TSLS:-$(command -v typescript-language-server || true)}
export TSMOD=${RADE_E2E_TS:-$( (timeout 20 npm root -g </dev/null 2>/dev/null || true) | sed 's|$|/typescript|')}
source "$HERE/lib.sh"

FAKE=$HERE/fake-lsp.py
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade"
export FAKE_LSP_LOG=$T/lsp.log
: >"$FAKE_LSP_LOG"
cat >"$CONF" <<EOF
[code.server.rust]
command = ["python3", "$FAKE"]
[code.server.typescript]
command = ["python3", "$FAKE"]
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
q() { rade code "$@" --json; }
code_ui() { rpc debug.code "${1:-{\}}"; }
vinfo() { rpc debug.viewer "{\"pane\": \"$1\", \"op\": \"info\"}" | jq -r "$2"; }
vop() { rpc debug.viewer "{\"pane\": \"$1\", \"op\": \"$2\", \"arg\": \"${3:-}\"}"; }
vkey() { rade debug viewer "$1" key "$2" >/dev/null; }
rv() { rade debug review "$RV" "$@" --json; }
rows() { code_ui | jq -r "[.rows[] | $1] | join(\",\")"; }
servers() { rade code status --json | jq -r "[.servers[] | select(.language==\"$1\") | .root] | join(\" \")"; }
export -f code_ui vinfo rpc rows

W=$HOME/workspaces/acme
API=$W/api
WEB=$W/web
STD=$T/rustlib/library
mkdir -p "$API/src" "$WEB/src" "$WEB/node_modules/moment" "$STD/core/src"

# A std library outside every Worktree (as ~/.rustup/…/library).
printf '[workspace]\nmembers = ["core"]\n' >"$STD/Cargo.toml"
cat >"$STD/core/src/fmt.rs" <<'EOF'
pub fn core_fmt() -> u8 {
    inner()
}
pub fn inner() -> u8 { 1 }
EOF
cat >"$API/Cargo.toml" <<'EOF'
[package]
name = "api"
version = "0.1.0"
edition = "2021"
EOF
cat >"$API/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub fn helper() -> String {
    greet("x")
}

pub fn twice() -> String {
    greet("a") + &greet("b")
}
EOF
cat >"$API/src/main.rs" <<'EOF'
fn main() {
    let s = api::greet("you");
    println!("{s} {}", api::helper());
    let _ = core::fmt::core_fmt();
    let _ = web_link;
}
EOF
cat >"$API/.fake-lsp.json" <<EOF
{"greet": {"def": ["src/lib.rs:1:8", "src/lib.rs:9:8"]},
 "helper": {"def": ["src/lib.rs:5:8"]},
 "core_fmt": {"def": ["$STD/core/src/fmt.rs:1:8"]},
 "inner": {"def": ["$STD/core/src/fmt.rs:4:8"]},
 "web_link": {"def": ["$WEB/src/utils.ts:1:14"]}}
EOF
echo target >"$API/.gitignore"
G -C "$API" init -q -b main && G -C "$API" add -A && G -C "$API" commit -qm init

# The owner's momentWithTZ, in small: callers with several call sites, a
# caller of a caller, a recursion, a use that is not a call, imports.
cat >"$WEB/package.json" <<'EOF'
{"name": "web"}
EOF
cat >"$WEB/src/utils.ts" <<'EOF'
export const momentWithTZ = (date?: string) => new Date(date ?? 0)
EOF
cat >"$WEB/src/Events.ts" <<'EOF'
import { momentWithTZ } from './utils'
export function range() {
  return [momentWithTZ(), momentWithTZ()]
}
export function between(a: string, b: string) {
  return [momentWithTZ(a), momentWithTZ(b)]
}
export function outer() {
  return range()
}
export function loop(n: number): number {
  return n ? loop(n - 1) + range().length : 0
}
EOF
cat >"$WEB/src/Detail.ts" <<'EOF'
import { momentWithTZ } from './utils'
export function detail(s: string) {
  return momentWithTZ(s)
}
EOF
cat >"$WEB/src/DateToggle.tsx" <<'EOF'
import { momentWithTZ } from './utils'
import moment from 'moment'
export function pick(withTz: boolean) {
  const m = withTz ? momentWithTZ : moment
  return m()
}
EOF
printf '{"name": "moment"}\n' >"$WEB/node_modules/moment/package.json"
printf 'declare function moment(): Date;\nexport default moment;\n' >"$WEB/node_modules/moment/moment.d.ts"
echo node_modules >"$WEB/.gitignore"
cat >"$WEB/.fake-lsp.json" <<'EOF'
{"momentWithTZ": {"def": ["src/utils.ts:1:14"],
   "refs": ["src/Events.ts:1:10", "src/Events.ts:3:11", "src/Events.ts:3:27", "src/Events.ts:6:11", "src/Events.ts:6:28",
            "src/Detail.ts:1:10", "src/Detail.ts:3:10", "src/DateToggle.tsx:1:10", "src/DateToggle.tsx:4:22"],
   "callers": [{"name": "range", "def": "src/Events.ts:2:17", "at": ["src/Events.ts:3:11", "src/Events.ts:3:27"]},
               {"name": "between", "def": "src/Events.ts:5:17", "at": ["src/Events.ts:6:11", "src/Events.ts:6:28"]},
               {"name": "detail", "def": "src/Detail.ts:2:17", "at": "src/Detail.ts:3:10"}]},
 "range": {"def": ["src/Events.ts:2:17"],
   "callers": [{"name": "outer", "def": "src/Events.ts:8:17", "at": "src/Events.ts:9:10"},
               {"name": "loop", "def": "src/Events.ts:11:17", "at": "src/Events.ts:12:28"}],
   "callees": [{"name": "momentWithTZ", "at": "src/utils.ts:1:14", "sites": ["src/Events.ts:3:11", "src/Events.ts:3:27"]}]},
 "loop": {"def": ["src/Events.ts:11:17"], "callers": [{"name": "loop", "def": "src/Events.ts:11:17", "at": "src/Events.ts:12:14"}]},
 "moment": {"def": ["node_modules/moment/moment.d.ts:1:18"]}}
EOF
G -C "$WEB" init -q -b main && G -C "$WEB" add -A && G -C "$WEB" commit -qm init

start_app
for r in "$API" "$WEB"; do rade source add "$r" >/dev/null; done

echo "== Ctrl held over a name in the Viewer: a link (underline, hand pointer)"
cd "$API"
V=$(rade open src/main.rs:2)
sleep 0.5
vop "$V" link 3:30 >/dev/null
check "Ctrl over helper: it is the link, underlined" test "$(vinfo "$V" '"\(.link.word) \(.link.line):\(.link.col) \(.link.underlined)"')" = "helper 3:29 true"
check "… the pointer is a hand" test "$(vinfo "$V" .pointer_cursor)" = pointer
vop "$V" link 3:32 >/dev/null
check "moving along the same name keeps it" test "$(vinfo "$V" .link.word)" = helper
vop "$V" link 2:14 >/dev/null
check "onto another name: that one (api, not helper)" test "$(vinfo "$V" '"\(.link.word) \(.link.line)"')" = "api 2"
vop "$V" link 2:2 >/dev/null
check "over indentation: no link, the text cursor" test "$(vinfo "$V" '"\(.link) \(.pointer_cursor)"')" = "null text"
vop "$V" link 2:11 >/dev/null
check "… over a symbol (=): no link" test "$(vinfo "$V" .link)" = null
vop "$V" link 2:6 >/dev/null
check "… over a keyword (let): no link" test "$(vinfo "$V" .link)" = null
vop "$V" link 3:30 >/dev/null
sleep 0.3
shot link-viewer-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
vop "$V" link_off >/dev/null
vop "$V" link 3:30 >/dev/null
shot link-viewer-dark
rade debug action color_scheme_light >/dev/null
vop "$V" link_off >/dev/null
check "Ctrl let go: plain text, text cursor" test "$(vinfo "$V" '"\(.link) \(.pointer_cursor)"')" = "null text"

echo "== Ctrl+click navigates, the selection stays"
vop "$V" select 1:1-1:10 >/dev/null
check "a selection on line 1" test "$(vinfo "$V" .selection)" = "1:1-1:10"
J=$(vop "$V" ctrl_click 2:18)
check "Ctrl+click on greet (2 definitions): navigated" test "$(jq -r .navigated <<<"$J")" = true
check "… the results popover lists both" eventually 5 is "rows 'select(.kind == \"item\") | \"\(.path):\(.line)\"'" "src/lib.rs:1,src/lib.rs:9"
check "… the selection and caret did not move" test "$(vinfo "$V" '"\(.selection) \(.cursor_line):\(.cursor_col)"')" = "1:1-1:10 1:1"
J=$(vop "$V" ctrl_click 2:5)
check "Ctrl+click on blank space: nothing (not claimed)" test "$(jq -r .navigated <<<"$J")" = false
vop "$V" link 3:30 >/dev/null
vop "$V" ctrl_click 3:30 >/dev/null
check "Ctrl+click on helper (1 definition): jumps" eventually 5 is "vinfo $V '\"\\(.path | sub(\".*/api/\"; \"\")):\\(.line)\"'" "src/lib.rs:5"
vop "$V" link_off >/dev/null
check "… Ctrl let go after the jump: the text cursor again (not a stuck hand)" test "$(vinfo "$V" '"\(.link) \(.pointer_cursor)"')" = "null text"
vkey "$V" alt+Left

echo "== a std file reached by navigation: the same server, not a second one"
vop "$V" ctrl_click 4:24 >/dev/null
check "Ctrl+click on core_fmt: the std file outside the Worktree opens" eventually 5 is "vinfo $V .path" "$STD/core/src/fmt.rs"
check "one rust server, at the api" test "$(servers rust)" = "$API"
rade debug viewer "$V" place 2:6 >/dev/null
vkey "$V" F12
check "F12 in the std file: answered by the api's server (jumps to inner)" eventually 5 is "vinfo $V .line" 4
check "… still one rust server" test "$(servers rust)" = "$API"
J=$(q def "$STD/core/src/fmt.rs:2:6")
check "rade code def on the std file: the same server answers" test "$(jq -r '"\(.state) \(.source) \(.items[0].line)"' <<<"$J")" = "ready fake-lsp 4"
check "… still one rust server" test "$(servers rust)" = "$API"
vkey "$V" alt+Left
vkey "$V" alt+Left

J=$(q def src/main.rs:5:13)
check "def into another Source's file (web)" test "$(jq -r '.items[0].path' <<<"$J")" = "$WEB/src/utils.ts"
J=$(q def "$WEB/src/utils.ts:1:14")
check "… that file is asked through its own Worktree's server, not the api's" test "$(jq -r '"\(.root) \(.items[0].line)"' <<<"$J")" = "$WEB 1"

echo "== node_modules: asked through the project's server"
cd "$WEB"
J=$(q def src/DateToggle.tsx:2:8)
check "def of moment → node_modules/moment/moment.d.ts" test "$(jq -r '.items[0].path | sub(".*/web/"; "")' <<<"$J")" = "node_modules/moment/moment.d.ts"
J=$(q def node_modules/moment/moment.d.ts:1:18)
check "a query inside node_modules is answered" test "$(jq -r .state <<<"$J")" = ready
check "… by the web root's server, no server at node_modules/moment" test "$(servers typescript)" = "$WEB"

echo "== rade code callers: every call site, --depth, other references"
J=$(q callers src/utils.ts:1:14)
check "5 call sites (not 3 callers), each named by its caller" test "$(jq -r '[.items[] | "\(.path | sub(".*/web/"; "")):\(.line):\(.col) \(.name)"] | join(",")' <<<"$J")" = "src/Detail.ts:3:10 detail,src/Events.ts:3:11 range,src/Events.ts:3:27 range,src/Events.ts:6:11 between,src/Events.ts:6:28 between"
check "JSON calls: each caller with its from_ranges" test "$(jq -r '[.calls[] | "\(.name):\(.from_ranges | length)"] | join(",")' <<<"$J")" = "detail:1,range:2,between:2"
check "… other references: the non-call use (imports left out)" test "$(jq -r '[.others[] | "\(.path | sub(".*/web/"; "")):\(.line):\(.col)"] | join(",")' <<<"$J")" = "src/DateToggle.tsx:4:22"
check "… no children at depth 1" test "$(jq -r '[.calls[] | .children // [] | length] | add' <<<"$J")" = 0
T1=$(rade code callers src/utils.ts:1:14)
check "text: a header with counts" test "$(head -1 <<<"$T1")" = "# callers momentWithTZ · 5 call sites in 3 callers in 2 files · 1 other reference · fake-lsp"
check "… the other references after their own header" bash -c "grep -A1 '^# other references (not calls): 1' <<<'$T1' | tail -1 | grep -q '^src/DateToggle.tsx:4:22'"
T3=$(rade code callers src/utils.ts:1:14 --depth 3)
echo "$T3" | sed 's/^/    /'
check "--depth 3: callers of range indented under it" bash -c "grep -q \$'^  src/Events.ts:9:10\touter\t' <<<'$T3' && grep -q \$'^  src/Events.ts:12:28\tloop\t' <<<'$T3'"
check "… loop's own call of itself one level deeper" bash -c "grep -q \$'^    src/Events.ts:12:14\tloop\t' <<<'$T3'"
J3=$(q callers src/utils.ts:1:14 --depth 4)
check "--depth 4 JSON: loop → loop is marked recursive, not expanded" test "$(jq -r '.calls[1].children[1].children[0] | "\(.name) \(.recursive) \(.children // [] | length)"' <<<"$J3")" = "loop true 0"
set +e; rade code callers src/utils.ts:1:14 --depth 0 >/dev/null 2>&1; c=$?; set -e
check "--depth 0: usage error" test "$c" = 2

echo "== the results popover: a call hierarchy tree"
VW=$(rade open src/utils.ts:1)
rade debug viewer "$VW" place 1:14 >/dev/null
vkey "$VW" alt+shift+h
check "Alt+Shift+H: Callers of momentWithTZ, call sites counted" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.summary)\"'" "Callers of momentWithTZ|5 call sites in 3 callers in 2 files · 1 other reference · fake-lsp"
check "three callers open with their call sites and own callers loaded" eventually 5 is "rows 'select(.kind != \"note\") | \"\(.kind):\(.name // .line):\(.depth // 0)\"'" "call:detail:1,site:3:1,call:range:1,site:3:1,site:3:1,call:outer:2,call:loop:2,call:between:1,site:6:1,site:6:1,others:null:0,other:4:0"
check "… callers nobody calls say so" test "$(rows 'select(.kind == "note") | .text')" = "No callers,No callers"
check "… the call counts" test "$(rows 'select(.kind == "call") | "\(.name)=\(.count)"')" = "detail=1,range=2,outer=1,loop=1,between=2"
code_ui '{"op":"activate","name":"loop","depth":2}' >/dev/null
check "opening loop: its call site and its caller loop, marked recursive" eventually 5 is "rows 'select(.depth == 3 or (.kind == \"site\" and .line == 12)) | \"\(.kind):\(.name // .line):\(.recursive // \"\")\"'" "site:12:,call:loop:true"
code_ui '{"op":"activate","name":"loop","depth":3}' >/dev/null
sleep 0.5
check "… a recursive node does not open into itself again" test "$(code_ui | jq '[.rows[] | select(.depth == 4)] | length')" = 0
code_ui '{"op":"show"}' >/dev/null
sleep 0.4
shot callers-tree-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot callers-tree-dark
rade debug action color_scheme_light >/dev/null
code_ui '{"op":"activate","name":"range","depth":1}' >/dev/null
check "closing range hides its sites and callers" test "$(rows 'select(.kind == "call") | .name')" = "detail,range,between"
n=$(grep -c 'request callHierarchy/incomingCalls range' "$FAKE_LSP_LOG")
code_ui '{"op":"activate","name":"range","depth":1}' >/dev/null
sleep 0.5
check "opening it again: no new request (asked once)" test "$(grep -c 'request callHierarchy/incomingCalls range' "$FAKE_LSP_LOG")" = "$n"
check "… its callers are back, loop still open" test "$(rows 'select(.kind == "call") | .name')" = "detail,range,outer,loop,loop,between"
code_ui '{"op":"activate","path":"src/Events.ts","line":6}' >/dev/null
check "a call site row opens the Viewer there" eventually 5 is "vinfo $VW '\"\\(.path | sub(\".*/web/\"; \"\")):\\(.line)\"'" "src/Events.ts:6"
code_ui '{"op":"goto","name":"outer"}' >/dev/null
check "a caller's Go to button: its definition" eventually 5 is "vinfo $VW '\"\\(.path | sub(\".*/web/\"; \"\")):\\(.line):\\(.cursor_col)\"'" "src/Events.ts:8:17"
code_ui '{"op":"activate","kind":"others"}' >/dev/null
check "the Other references group folds" test "$(rows 'select(.kind == "other") | .line')" = ""
code_ui '{"op":"activate","kind":"others"}' >/dev/null
code_ui '{"op":"activate","path":"src/DateToggle.tsx","line":4}' >/dev/null
check "… an other reference opens its line" eventually 5 is "vinfo $VW '\"\\(.path | sub(\".*/web/\"; \"\")):\\(.line)\"'" "src/DateToggle.tsx:4"
VR=$(rade open "$WEB/src/Events.ts:2")
rade debug viewer "$VR" place 2:17 >/dev/null
vkey "$VR" alt+shift+h
eventually 5 is "code_ui | jq -r .title" "Callers of range" || true
code_ui '{"op":"mode","arg":"callees"}' >/dev/null
check "Callees of range: momentWithTZ with its 2 call sites in range" eventually 5 is "rows '\"\(.kind):\(.name // .line):\(.count // \"\")\"' | cut -d, -f1-3" "call:momentWithTZ:2,site:3:,site:3:"
check "… the title and summary" test "$(code_ui | jq -r '"\(.title)|\(.summary)"')" = "Calls from range|1 callee · fake-lsp"
sleep 0.4
shot callees-tree-light

echo "== Review: Ctrl over a name is a link; Ctrl+click keeps the row selection"
cd "$API"
FEAT=$(rade worktree add api feat)
cp "$API/.fake-lsp.json" "$FEAT/.fake-lsp.json"
cat >>"$FEAT/src/lib.rs" <<'EOF'

pub fn more() -> String {
    helper()
}
EOF
RV=$(rade review open --source api --worktree feat)
check "the Review lists the change" eventually 10 bash -c "rade debug review $RV info --json | jq -r .visible_text | grep -q 'helper()'"
rv point 'pub fn more() -> String {|more' >/dev/null
SEL=$(rv info | jq -c .selected)
rv link '    helper()|helper' >/dev/null
check "Ctrl over helper on a diff line: the link, a hand pointer" test "$(rv info | jq -r '"\(.link) \(.pointer_cursor)"')" = "helper pointer"
sleep 0.3
shot link-review-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
rv link_off >/dev/null
rv link '    helper()|helper' >/dev/null
shot link-review-dark
rade debug action color_scheme_light >/dev/null
rv link_off >/dev/null
check "Ctrl let go: no link, the default pointer" test "$(rv info | jq -r '"\(.link) \(.pointer_cursor)"')" = "null null"
rv ctrl_click '    helper()|helper' >/dev/null
check "Ctrl+click on helper: the Tab's Viewer at its definition" eventually 5 bash -c "rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\") | \"\(.kind.path):\(.kind.line)\"] | join(\" \")' | grep -q 'feat/src/lib.rs:5'"
check "… the selected rows did not change" test "$(rv info | jq -c .selected)" = "$SEL"

echo "== a real typescript-language-server on the momentWithTZ shape"
if [[ -n $TSLS && -x $TSLS && -d $TSMOD ]]; then
    TS=$W/tsreal
    mkdir -p "$TS/src" "$TS/node_modules"
    ln -s "$TSMOD" "$TS/node_modules/typescript"
    printf '{"compilerOptions": {"target": "es2020", "module": "esnext", "moduleResolution": "bundler", "jsx": "preserve", "strict": false, "noEmit": true}, "include": ["src"]}\n' >"$TS/tsconfig.json"
    printf '{"name": "tsreal"}\n' >"$TS/package.json"
    cp "$WEB/src/utils.ts" "$WEB/src/Events.ts" "$WEB/src/Detail.ts" "$TS/src/"
    printf "import { momentWithTZ } from './utils'\nexport function pick(withTz: boolean) {\n  const m = withTz ? momentWithTZ : (x?: string) => new Date(x ?? 1)\n  return m()\n}\n" >"$TS/src/DateToggle.tsx"
    echo node_modules >"$TS/.gitignore"
    G -C "$TS" init -q -b main && G -C "$TS" add -A && G -C "$TS" commit -qm init
    rade source add "$TS" >/dev/null
    cat >"$CONF" <<EOF
[code.server.rust]
command = ["python3", "$FAKE"]
[code.server.typescript]
command = ["$TSLS", "--stdio"]
EOF
    sleep 1
    cd "$TS"
    J=$(timeout 120 rade code callers src/utils.ts:1:14 --wait 60 --json </dev/null)
    jq -c '{state, source, hint, items: [.items[] | "\(.path | sub(".*/tsreal/"; "")):\(.line):\(.col) \(.name)"], others: [.others[]? | "\(.path | sub(".*/tsreal/"; "")):\(.line)"]}' <<<"$J"
    check "tsls: 5 call sites, named range / between / detail" test "$(jq -r '[.items[] | "\(.path | sub(".*/tsreal/"; "")):\(.line):\(.col) \(.name)"] | join(",")' <<<"$J")" = "src/Detail.ts:3:10 detail,src/Events.ts:3:11 range,src/Events.ts:3:27 range,src/Events.ts:6:11 between,src/Events.ts:6:28 between"
    check "… from typescript-language-server" test "$(jq -r .source <<<"$J")" = typescript-language-server
    check "… other references: the DateToggle use, not the imports" test "$(jq -r '[.others[] | "\(.path | sub(".*/tsreal/"; "")):\(.line)"] | join(",")' <<<"$J")" = "src/DateToggle.tsx:3"
    J=$(timeout 60 rade code callers src/utils.ts:1:14 --depth 2 --json </dev/null)
    check "tsls --depth 2: range's callers are outer and loop" test "$(jq -r '[.calls[] | select(.name == "range") | .children[].name] | sort | join(",")' <<<"$J")" = "loop,outer"
else
    echo "  SKIP real typescript-language-server: none here (npm i -g typescript-language-server typescript@5, or RADE_E2E_TSLS/RADE_E2E_TS)"
fi

stop_app
echo "M31 (0.7.1): $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
