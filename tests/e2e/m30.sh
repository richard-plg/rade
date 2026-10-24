#!/usr/bin/env bash
# M30 end-to-end tests: Terminals start under load (R89, ADR-0031: no
# io_uring ring, many Terminals), hover / peek popovers in the Viewer and
# the Review Pane (R90), the outline (R91), workspace symbols in the
# palette (`$`) and `rade code symbols --query` (R92), Python and Go
# servers found on PATH with install hints (R93), a server hung in
# `initialize` failing fast afterwards — all through the FAKE language
# server (tests/e2e/fake-lsp.py); fake `gopls` / `basedpyright-langserver`
# wrappers on PATH stand for the real ones. Light + dark screenshots.
# Usage: tests/e2e/m30.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m30
source "$HERE/lib.sh"

FAKE=$HERE/fake-lsp.py
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin"
export FAKE_LSP_LOG=$T/lsp.log
: >"$FAKE_LSP_LOG"
cat >"$CONF" <<EOF
[code.server.rust]
command = ["python3", "$FAKE"]
[code.server.typescript]
command = ["python3", "$FAKE"]
EOF
# Built-in Go command `gopls`: a fake one first on PATH (the real one may
# be installed too). Python: none yet (the hint), later basedpyright only.
printf '#!/bin/sh\nexec python3 %s "$@"\n' "$FAKE" >"$T/bin/gopls"
chmod +x "$T/bin/gopls"
export PATH=$T/bin:$PATH
# Hermetic Python: a pyright / basedpyright installed on this machine is
# hidden (the checks want "not installed" first, then the fake basedpyright).
np= n=0
IFS=: read -ra dirs <<<"$PATH"
for d in "${dirs[@]}"; do
    if [[ $d != "$T/bin" && ( -e $d/pyright-langserver || -e $d/basedpyright-langserver ) ]]; then
        n=$((n + 1))
        mkdir -p "$T/shadow$n"
        for f in "$d"/*; do
            case ${f##*/} in pyright | pyright-langserver | basedpyright | basedpyright-langserver) ;; *) ln -s "$f" "$T/shadow$n/" ;; esac
        done
        d=$T/shadow$n
    fi
    np=${np:+$np:}$d
done
export PATH=$np

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
q() { rade code "$@" --json; }
code_ui() { rpc debug.code "${1:-{\}}"; }
pop() { rpc debug.code '{"op":"popover"}' | jq -r "$1"; }
vkey() { rade debug viewer "$1" key "$2" >/dev/null; }
vinfo() { rpc debug.viewer "{\"pane\": \"$1\", \"op\": \"info\"}" | jq -r "$2"; }
pal() { rade --json debug palette --ui "$@"; }
pinfo() { pal info | jq -r "$1"; }
rv() { rade debug review "$RV" "$@" --json; }
here() { vinfo "$1" '"\(.path | sub(".*/(api|feat|web|py|goapp)/"; "")):\(.line):\(.cursor_col)"'; }
uring() { ls -l "/proc/$APP_PID/fd" 2>/dev/null | grep -c 'io_uring' || true; }
export -f code_ui pop vinfo rpc pal pinfo here

W=$HOME/workspaces/acme
API=$W/api
WEB=$W/web
PY=$W/py
GO=$W/goapp
mkdir -p "$API/src" "$WEB/src" "$PY/pkg" "$GO"
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

pub struct Greeter;

impl Greeter {
    pub fn new() -> Self { Greeter }
}
EOF
cat >"$API/src/main.rs" <<'EOF'
fn main() {
    let é = api::greet("you");
    println!("{é} {}", api::helper());
}
EOF
cat >"$API/.fake-lsp.json" <<'EOF'
{"greet": {"def": ["src/lib.rs:1:8"],
           "hover": "```rust\npub fn greet(name: &str) -> String\n```\n---\nSays **hi** to `name`."},
 "helper": {"def": ["src/lib.rs:5:8"], "hover": "```rust\npub fn helper() -> String\n```"},
 "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8", "end": "src/lib.rs:3:1"},
                             {"name": "helper", "at": "src/lib.rs:5:8", "end": "src/lib.rs:7:1"},
                             {"name": "Greeter", "at": "src/lib.rs:9:12", "kind": 23,
                              "children": [{"name": "new", "at": "src/lib.rs:12:12", "kind": 6}]}]},
 "@workspace": [{"name": "greet", "at": "src/lib.rs:1:8"}, {"name": "Greeter", "at": "src/lib.rs:9:12", "kind": 23},
                {"name": "helper", "at": "src/lib.rs:5:8"}]}
EOF
echo target >"$API/.gitignore"
G -C "$API" init -q -b main && G -C "$API" add -A && G -C "$API" commit -qm init
cat >"$WEB/package.json" <<'EOF'
{"name": "web"}
EOF
cat >"$WEB/src/a.ts" <<'EOF'
export function total(xs: number[]): number {
  return xs.length;
}
export const n = total([1]);
EOF
cat >"$WEB/.fake-lsp.json" <<'EOF'
{"@index_ms": 3000, "total": {"def": ["src/a.ts:1:17"], "hover": "```ts\nfunction total(xs: number[]): number\n```"},
 "@workspace": [{"name": "total", "at": "src/a.ts:1:17"}]}
EOF
G -C "$WEB" init -q -b main && G -C "$WEB" add -A && G -C "$WEB" commit -qm init
cat >"$PY/pyproject.toml" <<'EOF'
[project]
name = "py"
EOF
cat >"$PY/pkg/a.py" <<'EOF'
def total(xs):
    return len(xs)

n = total([1])
EOF
cat >"$PY/.fake-lsp.json" <<'EOF'
{"total": {"def": ["pkg/a.py:1:5"], "hover": "```python\ndef total(xs) -> int\n```"}}
EOF
G -C "$PY" init -q -b main && G -C "$PY" add -A && G -C "$PY" commit -qm init
cat >"$GO/go.mod" <<'EOF'
module goapp

go 1.21
EOF
cat >"$GO/a.go" <<'EOF'
package goapp

// Total counts them.
func Total(xs []int) int { return len(xs) }

var N = Total([]int{1})
EOF
cat >"$GO/.fake-lsp.json" <<'EOF'
{"Total": {"def": ["a.go:4:6"], "hover": "```go\nfunc Total(xs []int) int\n```\nTotal counts them."},
 "@workspace": [{"name": "Total", "at": "a.go:4:6"}]}
EOF
G -C "$GO" init -q -b main && G -C "$GO" add -A && G -C "$GO" commit -qm init

start_app
for r in "$API" "$WEB" "$PY" "$GO"; do rade source add "$r" >/dev/null; done

echo "== R89 Terminals: epoll, no io_uring ring, many Terminals all start"
for i in $(seq 1 16); do
    if (( i == 1 || i == 9 )); then tab=(--tab new); else tab=(); fi
    rade spawn --source api --name "t$i" "${tab[@]}" --cmd "echo started-$i; exec sleep 600" >/dev/null
done
check "16 Terminals: each one's process started" eventually 20 bash -c 'for i in $(seq 1 16); do rade read t$i | grep -q "started-$i" || exit 1; done'
check "… and no 'failed to create ghostty surface'" bash -c "! grep -q 'failed to create ghostty surface' '$OUT/app.log'"
check "the app holds no io_uring ring (they count against the user's locked-memory limit)" test "$(uring)" = 0
for i in $(seq 1 16); do rade close "t$i" --force >/dev/null 2>&1 || true; done

echo "== R90 hover in the Viewer: Ctrl+I, Ctrl+hover, Esc; text and missing-server answers"
cd "$API"
V=$(rade open src/main.rs:2)
rade debug viewer "$V" place 2:20 >/dev/null
vkey "$V" ctrl+i
check "Ctrl+I: a hover popover with the server's Markdown" eventually 5 is "pop '\"\(.open) \(.kind) \(.title)\"'" "true hover fake-lsp"
check "… its text: the signature and the docs" bash -c "pop .text | grep -q 'pub fn greet(name: &str) -> String' && pop .text | grep -q 'Says \*\*hi\*\*'"
sleep 0.4
shot hover-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot hover-dark
rade debug action color_scheme_light >/dev/null
check "Esc closes the popover, the Viewer stays" bash -c "rpc debug.viewer '{\"pane\":\"$V\",\"op\":\"key\",\"arg\":\"Escape\"}' | jq -e .handled >/dev/null && [[ \$(pop .open) == false ]] && [[ \$(vinfo $V .path) == *main.rs ]]"
rade debug viewer "$V" hover 3:30 >/dev/null
check "Ctrl with the pointer resting on helper: its hover, opened by the pointer" eventually 5 is "pop '\"\(.kind) \(.by_pointer) \(.text | split(\"\n\")[1])\"'" "hover true pub fn helper() -> String"
rade debug viewer "$V" hover 2:2 >/dev/null
sleep 0.6
check "… resting on blank space changes nothing" test "$(pop '"\(.open) \(.text | split("\n")[1])"')" = "true pub fn helper() -> String"
rade debug viewer "$V" hover 1:1 >/dev/null
sleep 0.8
check "… on a name the server knows nothing about (fn): no popover" test "$(pop .open)" = false
rpc debug.code '{"op":"popover_close"}' >/dev/null
rade debug viewer "$V" place 2:9 >/dev/null
vkey "$V" ctrl+i
check "a name the server knows nothing about: says so" eventually 5 is "pop .text" "No information about é"
rpc debug.code '{"op":"popover_close"}' >/dev/null

echo "== R90.2 peek: Alt+F12 shows the definition's lines; Open jumps"
rade debug viewer "$V" place 3:30 >/dev/null
vkey "$V" alt+F12
check "Alt+F12: a peek popover at lib.rs:5" eventually 5 is "pop '\"\(.kind) \(.title)\"'" "peek src/lib.rs:5"
check "… showing the definition's lines" bash -c "pop .text | grep -q '5  pub fn helper() -> String' && pop .text | grep -q '6      greet'"
sleep 0.4
shot peek-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot peek-dark
rade debug action color_scheme_light >/dev/null
rpc debug.code '{"op":"popover_open"}' >/dev/null
check "Open: the Viewer jumps there (caret on the name), the popover closes" eventually 5 is "here $V" "src/lib.rs:5:8"
check "… popover closed" test "$(pop .open)" = false
vkey "$V" ctrl+i
eventually 5 is "pop .open" true || true
vkey "$V" alt+Left
check "a hover closes when the Viewer goes Back (it was about the old place)" test "$(pop .open)" = false
check "… Back returns" eventually 3 is "vinfo $V '\"\\(.path | sub(\".*/api/\"; \"\")):\\(.line)\"'" "src/main.rs:3"

echo "== R91 outline: Alt+Shift+O and the header's button → the results popover"
vkey "$V" alt+Left >/dev/null 2>&1 || true
V=$(rade open src/lib.rs:1)
vkey "$V" alt+shift+o
check "Alt+Shift+O: Outline of lib.rs with kinds and nesting, no file row" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.summary)|\([.rows[] | \"\(.kind):\(.name):\(.symbol_kind)\"] | join(\",\"))\"'" "Outline of lib.rs|4 symbols · fake-lsp|item:greet:function,item:helper:function,item:Greeter:struct,item:Greeter.new:method"
sleep 0.6
shot outline-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot outline-dark
rade debug action color_scheme_light >/dev/null
code_ui '{"op":"activate","path":"src/lib.rs","line":5}' >/dev/null
check "an outline row jumps the Viewer" eventually 5 is "here $V" "src/lib.rs:5:8"
code_ui '{"op":"show"}' >/dev/null
rpc debug.code '{"op":"info"}' >/dev/null
rade debug viewer "$V" outline >/dev/null
check "the header's Outline button does the same" eventually 5 is "code_ui | jq -r .title" "Outline of lib.rs"
printf '# x\n' >"$API/README.md"
V2=$(rade open "$API/README.md")
sleep 0.5
check "no outline / hover keys on Markdown" test "$(rpc debug.viewer "{\"pane\":\"$V2\",\"op\":\"key\",\"arg\":\"ctrl+i\"}" | jq -r .handled)" = false
rm "$API/README.md"
rade debug viewer "$V2" escape >/dev/null 2>&1 || true

echo "== R92 rade code hover / symbols --query"
check "rade code hover: the doc after the header" bash -c "rade code hover src/main.rs:2:20 | head -2 | tail -1 | grep -qx '\`\`\`rust'"
check "… JSON doc" test "$(q hover src/main.rs:2:20 | jq -r '.doc | split("\n")[1]')" = "pub fn greet(name: &str) -> String"
check "rade code symbols --query gree: greet and Greeter with kinds" test "$(rade code symbols --query gree --json | jq -r '[.items[] | "\(.name):\(.kind)"] | join(",")')" = "greet:function,Greeter:struct"
check "… text output names the kind" bash -c "rade code symbols --query gree | grep -q \$'src/lib.rs:9:12\tGreeter (struct)'"
set +e; rade code symbols >/dev/null 2>&1; c=$?; set -e
check "symbols without a file or --query: usage (exit 2)" test "$c" = 2

echo "== R92 palette \$: the Worktree's symbols"
rade spawn --source api --name sh --cmd bash >/dev/null
rade focus sh >/dev/null
rade debug action command_palette >/dev/null
eventually 3 is "pinfo .open" true
pal type '$gree' >/dev/null
check "\$gree lists the server's greet and Greeter" eventually 5 is "pinfo '[.rows[] | \"\(.kind):\(.title):\(.detail)\"] | join(\",\")'" "symbol:greet:function · src/lib.rs:1,symbol:Greeter:struct · src/lib.rs:9"
check "… the status names the server" eventually 3 bash -c "pinfo .status | grep -q 'fake-lsp'"
sleep 0.4
shot palette-symbols-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot palette-symbols-dark
rade debug action color_scheme_light >/dev/null
pal key down >/dev/null
pal key enter >/dev/null
check "Enter: the Viewer at Greeter, caret on the name" eventually 5 is "here \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\") | .id][0]')" "src/lib.rs:9:12"
rade debug action command_palette >/dev/null
eventually 3 is "pinfo .open" true
pal type '$' >/dev/null
check "a bare \$ asks for a name" eventually 3 is "pinfo .status" "Type a symbol name"
pal type 'gree' >/dev/null
check "back to All: model items, no symbols" eventually 3 bash -c "[[ \$(pinfo '[.rows[] | select(.kind==\"symbol\")] | length') == 0 ]]"
pal key escape >/dev/null

echo "== R92 an indexing server: text matches first, then its answer by itself (TypeScript)"
rade open "$WEB/src/a.ts:1" >/dev/null
rade debug action command_palette >/dev/null
eventually 3 is "pinfo .open" true
pal type '$tot' >/dev/null
check "while indexing: the declaration as a text match, labelled" eventually 5 bash -c "pinfo .status | grep -q 'text matches (indexing)' && [[ \$(pinfo '.rows[0].title') == total ]]"
check "… then the server's answer by itself" eventually 15 bash -c "pinfo .status | grep -q 'fake-lsp'"
pal key escape >/dev/null

echo "== R93 Go and Python servers: found on PATH, install hints"
check "Go: the built-in gopls command from PATH answers" test "$(q def "$GO/a.go:6:9" | jq -r '"\(.state) \(.source) \(.items[0].line)"')" = "ready fake-lsp 4"
check "… rade code status shows gopls for the go repo" bash -c "rade code status --json | jq -r '.servers[] | select(.language==\"go\") | .command[0]' | grep -q '$T/bin/gopls'"
check "… hover through it" test "$(q hover "$GO/a.go:6:9" | jq -r '.doc | split("\n") | last')" = "Total counts them."
J=$(q hover "$PY/pkg/a.py:4:5")
check "Python without a server: the declaring line as a text match" test "$(jq -r '"\(.state) \(.source)"' <<<"$J")" = "no-server text"
check "… the hint names pyright and basedpyright" test "$(jq -r .hint <<<"$J")" = "pyright-langserver is not installed: npm i -g pyright (or: pipx install basedpyright)"
check "… doc = the def line" test "$(jq -r '.doc | split("\n")[1]' <<<"$J")" = "def total(xs):"
rade open "$PY/pkg/a.py:4" >/dev/null
VP=$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer" and (.kind.path | endswith("a.py"))) | .id][0]')
rade debug viewer "$VP" place 4:6 >/dev/null
vkey "$VP" ctrl+i
check "the hover popover: a text match at its place, why under it" eventually 5 bash -c "[[ \$(pop .title) == 'text matches · pkg/a.py:1' ]] && pop .note | grep -q 'pyright-langserver is not installed'"
sleep 0.4
shot hover-text-light
rpc debug.code '{"op":"popover_close"}' >/dev/null
printf '#!/bin/sh\nexec python3 %s "$@"\n' "$FAKE" >"$T/bin/basedpyright-langserver"
chmod +x "$T/bin/basedpyright-langserver"
check "basedpyright when pyright is not installed" test "$(q hover "$PY/pkg/a.py:4:5" | jq -r '"\(.state) \(.source) \(.doc | split("\n")[1])"')" = "ready fake-lsp def total(xs) -> int"
check "… started as basedpyright-langserver --stdio" bash -c "rade code status --json | jq -r '.servers[] | select(.language==\"python\") | .command | join(\" \")' | grep -q 'basedpyright-langserver --stdio'"

echo "== R90 Review Pane: Ctrl+I / Alt+F12 at the point"
FEAT=$(cd "$API" && rade worktree add api feat)
cp "$API/.fake-lsp.json" "$FEAT/.fake-lsp.json"
cat >>"$FEAT/src/lib.rs" <<'EOF'

pub fn more() -> String {
    helper()
}
EOF
RV=$(rade review open --source api --worktree feat)
check "the Review lists the change" eventually 10 bash -c "rade debug review $RV info --json | jq -r .visible_text | grep -q 'helper()'"
rv point '    helper()|helper' >/dev/null
rv key ctrl+i >/dev/null
check "Ctrl+I on an added line's name: its hover" eventually 5 is "pop '\"\(.kind) \(.text | split(\"\n\")[1])\"'" "hover pub fn helper() -> String"
sleep 0.4
shot review-hover-light
rade debug action color_scheme_dark >/dev/null
sleep 0.8
shot review-hover-dark
rade debug action color_scheme_light >/dev/null
rv key Escape >/dev/null
check "Esc closes it" test "$(pop .open)" = false
rv key alt+F12 >/dev/null
check "Alt+F12: a peek at lib.rs:5" eventually 5 is "pop '\"\(.kind) \(.title)\"'" "peek src/lib.rs:5"
rpc debug.code '{"op":"popover_open"}' >/dev/null
check "Open: the Tab's Viewer at the definition" eventually 5 bash -c "rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\") | .kind.path] | join(\" \")' | grep -q 'feat/src/lib.rs'"

echo "== R83 a server hung in initialize fails fast afterwards"
HUNG=$W/hung
mkdir -p "$HUNG/src"
printf '[package]\nname = "hung"\nversion = "0.1.0"\n' >"$HUNG/Cargo.toml"
printf 'pub fn a() {}\n' >"$HUNG/src/lib.rs"
printf '{"@hang_on": "initialize"}\n' >"$HUNG/.fake-lsp.json"
G -C "$HUNG" init -q -b main && G -C "$HUNG" add -A && G -C "$HUNG" commit -qm init
rade source add "$HUNG" >/dev/null
s=$(date +%s)
J=$(q def "$HUNG/src/lib.rs:1:8" --wait 0)
check "first ask waits out the 30 s start, answers with text matches and why" bash -c "[[ \$(jq -r .state <<<'$J') == error ]] && jq -r .hint <<<'$J' | grep -q 'did not answer .initialize. within 30 s'"
s=$(date +%s%N)
J=$(q def "$HUNG/src/lib.rs:1:8" --wait 0)
e=$((($(date +%s%N) - s) / 1000000))
check "the next ask answers at once (${e} ms), same hint" bash -c "(( $e < 2000 )) && jq -r .hint <<<'$J' | grep -q 'tried again in 5 minutes'"

stop_app
echo "M30: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
