#!/usr/bin/env bash
# M28 end-to-end tests (ADR-0029): code navigation in the Viewer and for
# Agents through a FAKE language server (tests/e2e/fake-lsp.py) —
# F12 / Ctrl+click definition, Shift+F12 references, Alt+Shift+H callers,
# Ctrl+F12 implementations, Alt+Left / Alt+Right (R84); the results
# popover, text matches while indexing, the missing-server hint, crash
# and Restart (R85); `rade code` JSON and text (R86); idle stop, the
# max_servers LRU, Worktree removal, an untrusted `.rade.toml` server never
# started, the per-repo switch (R83). One real-server smoke check when a
# rust-analyzer binary exists (skipped with the reason otherwise).
# Usage: tests/e2e/m28.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m28
# Before lib.sh swaps HOME: a real rust-analyzer (not the rustup proxy).
# rust-analyzer runs cargo: the real toolchains (read only), a temp CARGO_HOME.
export RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export RA_REAL=${RA_REAL:-${RADE_E2E_RA:-$(rustup which rust-analyzer 2>/dev/null </dev/null || true)}}
source "$HERE/lib.sh"
export CARGO_HOME=$T/cargo

FAKE=$HERE/fake-lsp.py
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade"
export FAKE_LSP_LOG=$T/lsp.log
: >"$FAKE_LSP_LOG"
conf() { # extra TOML appended to the base config
    cat >"$CONF" <<EOF
[code]
idle_minutes = ${IDLE:-10}
max_servers = ${MAX:-4}
[code.server.rust]
command = ["python3", "$FAKE"]
[code.server.typescript]
command = ["python3", "$FAKE"]
$*
EOF
}
conf

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
starts() { grep -c "^start " "$FAKE_LSP_LOG" || true; }
q() { rade code "$@" --json; }
code_ui() { rpc debug.code "${1:-{\}}"; }
vkey() { rade debug viewer "$1" key "$2" >/dev/null; }
vinfo() { rpc debug.viewer "{\"pane\": \"$1\", \"op\": \"info\"}" | jq -r "$2"; }
servers() { rade code status --json | jq '.servers | length'; }
export -f code_ui vinfo rpc servers

W=$HOME/workspaces/acme
API=$W/api
WEB=$W/web
mkdir -p "$API/src" "$WEB/src"
# Its own Cargo workspace: with TMPDIR under target/ it would otherwise
# sit inside RADE's and the real rust-analyzer's cargo metadata fails.
cat >"$API/Cargo.toml" <<'EOF'
[package]
name = "api"
version = "0.1.0"
edition = "2021"

[workspace]
EOF
cat >"$API/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub fn helper() -> String {
    greet("x")
}

pub trait Speak {
    fn speak(&self);
}

pub fn crash_now() {}

pub fn garbage_now() {}
EOF
cat >"$API/src/main.rs" <<'EOF'
fn main() {
    let é = api::greet("you");
    println!("{é} {}", api::helper());
}
EOF
cat >"$API/.fake-lsp.json" <<'EOF'
{"greet": {"def": ["src/lib.rs:1:8"],
           "refs": ["src/main.rs:2:19", "src/lib.rs:6:5"],
           "callers": [{"name": "main", "def": "src/main.rs:1:4", "at": "src/main.rs:2:19"},
                       {"name": "helper", "def": "src/lib.rs:5:8", "at": "src/lib.rs:6:5"}]},
 "helper": {"def": ["src/lib.rs:5:8"], "callees": [{"name": "greet", "at": "src/lib.rs:1:8"}]},
 "Speak": {"def": ["src/lib.rs:9:11"], "impl": ["src/main.rs:1:4"]},
 "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8"}, {"name": "Speak", "at": "src/lib.rs:9:11",
              "children": [{"name": "speak", "at": "src/lib.rs:10:8"}]}]}}
EOF
printf '[code.server.rust]\ncommand = ["sh", "-c", "touch %s/pwned; exit 1"]\n' "$T" >"$API/.rade.toml"
echo 'target' >"$API/.gitignore"
G -C "$API" init -q -b main && G -C "$API" add -A && G -C "$API" commit -qm init
cat >"$WEB/package.json" <<'EOF'
{"name": "web"}
EOF
cat >"$WEB/src/a.ts" <<'EOF'
export function total(xs: number[]): number {
  return xs.reduce((a, b) => a + b, 0);
}
export const sum = total([1, 2]);
EOF
cat >"$WEB/.fake-lsp.json" <<'EOF'
{"@index_ms": 4500, "total": {"def": ["src/a.ts:1:17"], "refs": ["src/a.ts:4:20"]}}
EOF
G -C "$WEB" init -q -b main && G -C "$WEB" add -A && G -C "$WEB" commit -qm init

start_app
rade source add "$API" >/dev/null
rade source add "$WEB" >/dev/null
cd "$API"

echo "== R86 rade code: def / refs / callers / callees / impl / symbols (JSON + text)"
J=$(q def src/main.rs:2:19)
check "def: lib.rs:1:8 from the fake server, ready" test "$(jq -r '"\(.state) \(.source) \(.word) \(.items[0].path | sub(".*/api/"; "")):\(.items[0].line):\(.items[0].col)"' <<<"$J")" = "ready fake-lsp greet src/lib.rs:1:8"
check "a column past a multi-byte character (é) is a character column" test "$(q def src/main.rs:2:22 | jq -r .word)" = greet
check "refs: two places, sorted by file" test "$(q refs src/lib.rs:1:8 | jq -r '[.items[] | "\(.path | sub(".*/api/"; "")):\(.line):\(.col)"] | join(" ")')" = "src/lib.rs:6:5 src/main.rs:2:19"
check "callers: helper and main at their call sites" test "$(q callers src/lib.rs:1:8 | jq -r '[.items[] | "\(.name)@\(.line)"] | join(" ")')" = "helper@6 main@2"
check "callees of helper: greet at its definition" test "$(q callees src/lib.rs:5:9 | jq -r '[.items[] | "\(.name)@\(.line):\(.col)"] | join(" ")')" = "greet@1:8"
check "impl of Speak" test "$(q impl src/lib.rs:9:12 | jq -r '.items[0] | "\(.path | sub(".*/api/"; "")):\(.line)"')" = "src/main.rs:1"
check "symbols of a file, nested names" test "$(rade code symbols src/lib.rs --json | jq -r '[.items[].name] | join(" ")')" = "greet Speak Speak.speak"
rade code refs src/lib.rs:1:8 >"$T/refs.txt"
printf '# refs greet · 2 results in 2 files · fake-lsp\nsrc/lib.rs:6:5\t\tgreet("x")\nsrc/main.rs:2:19\t\tlet é = api::greet("you");\n' >"$T/refs.want"
check "text output exactly" diff -u "$T/refs.want" "$T/refs.txt"
check "no name at the position: says so" test "$(q def src/lib.rs:4:1 | jq -r .state)" = no-word
set +e; rade code def nope.rs:1:1 >/dev/null 2>"$T/err"; c=$?; set -e
check "a missing file exits 4 (NOT_FOUND)" test "$c" = 4
check "one server so far, with the spike's rust-analyzer settings" bash -c "[[ \$(grep -c '^start ' '$FAKE_LSP_LOG') == 1 ]] && grep -q '\"checkOnSave\": false' '$FAKE_LSP_LOG' && grep -q '\"watcher\": \"server\"' '$FAKE_LSP_LOG' && grep -q '\"targetDir\": true' '$FAKE_LSP_LOG'"

echo "== R83.4 an untrusted .rade.toml [code] server never starts; trusted, it does"
check "explain: the repo's command is untrusted, the user's wins" bash -c "rade config explain code.server.rust.command --source api | grep -q 'untrusted'"
check "its command never ran" test ! -e "$T/pwned"

echo "== R84 Viewer: F12, Ctrl+click, Alt+Left / Alt+Right, Shift+F12, Alt+Shift+H, Ctrl+F12"
V=$(rade open src/main.rs:2)
rade debug viewer "$V" place 2:20 >/dev/null
vkey "$V" F12
check "F12: the definition in lib.rs at the name" eventually 5 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line):\(.cursor_col)\"'" "src/lib.rs:1:8"
check "Back is shown, Forward not" test "$(vinfo "$V" '"\(.back) \(.forward)"')" = "true false"
vkey "$V" alt+Left
check "Alt+Left: back at main.rs:2" eventually 3 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line)\"'" "src/main.rs:2"
check "… and Forward is shown" test "$(vinfo "$V" .forward)" = true
vkey "$V" alt+Right
check "Alt+Right: forward to lib.rs:1" eventually 3 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line)\"'" "src/lib.rs:1"
vkey "$V" alt+Left
rade debug viewer "$V" ctrl_click 3:30 >/dev/null
check "Ctrl+click on helper (main.rs:3): lib.rs:5" eventually 5 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line)\"'" "src/lib.rs:5"
# Same file, other line: also a place on Back.
rade debug viewer "$V" place 6:6 >/dev/null
vkey "$V" F12
check "F12 inside lib.rs: line 1" eventually 5 is "vinfo $V .line" 1
vkey "$V" alt+Left
check "… Back: to the caret, lib.rs:6" eventually 3 is "vinfo $V .line" 6
rade debug viewer "$V" place 1:9 >/dev/null
vkey "$V" shift+F12
check "Shift+F12: the results popover shows References to greet" eventually 5 is "code_ui | jq -r '\"\(.visible) \(.title) \(.state)\"'" "true References to greet ready"
check "rows grouped by file with counts" test "$(code_ui | jq -r '[.rows[] | if .kind == "file" then "[\(.path) \(.count)]" else "\(.line)" end] | join(" ")')" = "[src/lib.rs 1] 6 [src/main.rs 1] 2"
check "summary names the server" test "$(code_ui | jq -r .summary)" = "2 results in 2 files · fake-lsp"
check "the results show in a popover on the Viewer" test "$(rpc debug.code '{"op":"popover"}' | jq -r '"\(.open) \(.kind)"')" = "true results"
check "the Right Panel has no Code page" test "$(rpc debug.ui '{"op":"chrome"}' | jq -r '.chrome.right_panel.pages | join(",")')" = "changes,files,search"
sleep 0.6
shot code-refs-light
rade debug action color_scheme_dark >/dev/null
sleep 0.4
sleep 0.6
shot code-refs-dark
rade debug action color_scheme_light >/dev/null
code_ui '{"op":"activate","path":"src/main.rs","line":2}' >/dev/null
check "a row opens the Viewer at its line" eventually 3 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line):\(.cursor_col)\"'" "src/main.rs:2:19"
vkey "$V" alt+Left
check "… and Back returns" eventually 3 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line)\"'" "src/lib.rs:1"
code_ui '{"op":"activate","path":"src/main.rs"}' >/dev/null
check "a file row folds" test "$(code_ui | jq -r '[.rows[] | .kind] | join(",")')" = "file,item,file"
rade debug viewer "$V" place 1:9 >/dev/null
vkey "$V" alt+shift+h
check "Alt+Shift+H: Callers of greet, with names" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.modes)|\([.rows[] | select(.kind==\"call\") | .name] | join(\",\"))\"'" "Callers of greet|callers|helper,main"
sleep 0.6
shot code-callers-light
rade debug action color_scheme_dark >/dev/null
sleep 0.4
sleep 0.6
shot code-callers-dark
rade debug action color_scheme_light >/dev/null
rade debug viewer "$V" place 5:9 >/dev/null
vkey "$V" alt+shift+h
eventually 5 is "code_ui | jq -r .title" "Callers of helper" || true
code_ui '{"op":"mode","arg":"callees"}' >/dev/null
check "Callees switch: Calls from helper → greet" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.modes)|\([.rows[] | select(.kind==\"call\") | .name] | join(\",\"))\"'" "Calls from helper|callees|greet"
rade debug viewer "$V" place 9:12 >/dev/null
vkey "$V" ctrl+F12
check "Ctrl+F12 with one implementation jumps there" eventually 5 is "vinfo $V '\"\(.path | sub(\".*/api/\"; \"\")):\(.line)\"'" "src/main.rs:1"
printf '# x\n' >"$API/README.md"
V2=$(rade open "$API/README.md")
sleep 0.5
check "keys only in a code file: F12 on Markdown is not handled" test "$(rpc debug.viewer "{\"pane\":\"$V2\",\"op\":\"key\",\"arg\":\"F12\"}" | jq -r .handled)" = false
rm "$API/README.md"
rade debug viewer "$V" escape >/dev/null 2>&1 || true

echo "== R85 indexing: text matches first, then the server (TypeScript repo)"
cd "$WEB"
J=$(q refs src/a.ts:1:18 --wait 0)
check "cold: indexing, text matches labelled" test "$(jq -r '"\(.state) \(.source)"' <<<"$J")" = "indexing text"
check "… the hint says so" bash -c "jq -r .hint <<<'$(jq -c . <<<"$J")' | grep -q 'is indexing'"
check "… text matches include the declaration line" test "$(jq -r '[.items[] | select(.path | endswith("a.ts")) | .line] | join(",")' <<<"$J")" = "1,4"
VW=$(rade open src/a.ts:4)
rade debug viewer "$VW" place 4:20 >/dev/null
vkey "$VW" shift+F12
check "the results popover retries by itself and lands on the server's answer" eventually 15 is "code_ui | jq -r '\"\(.state) \(.source) \(.summary)\"'" "ready fake-lsp 1 result · fake-lsp"
check "CLI waits for indexing by default" test "$(q def src/a.ts:4:20 | jq -r '"\(.state) \(.items[0].line)"')" = "ready 1"

echo "== R85 missing server: the install command; R83.4 per-repo off"
conf "[sources.web]
code = false"
check "[sources.web] code = false: off, text matches" test "$(q refs src/a.ts:1:18 | jq -r '"\(.state) \(.source)"')" = "off text"
if command -v typescript-language-server >/dev/null; then
    echo "  SKIP missing-server hint: typescript-language-server is on PATH here"
else
    conf
    sed -i '/^\[code.server.typescript\]/,+1d' "$CONF"
    rade code restart src/a.ts >/dev/null
    J=$(q refs src/a.ts:1:18)
    check "no server: state no-server with the install command" test "$(jq -r '"\(.state)|\(.hint)"' <<<"$J")" = "no-server|typescript-language-server is not installed: npm i -g typescript-language-server typescript@5"
    VW2=$(rade open src/a.ts:1)
    rade debug viewer "$VW2" place 1:18 >/dev/null
    vkey "$VW2" shift+F12
    check "the results popover shows the hint, text matches with a pill" eventually 5 is "code_ui | jq -r '\"\(.hint)|\(.rows[1].text_match)|\(.retry)\"'" "typescript-language-server is not installed: npm i -g typescript-language-server typescript@5|true|Check again"
    shot code-missing-light
    rade debug action color_scheme_dark >/dev/null
    sleep 0.4
    shot code-missing-dark
    rade debug action color_scheme_light >/dev/null
fi
conf
cd "$API"

echo "== R83.3 crash: text matches now, restart on the next request; 3 crashes → Restart"
n0=$(starts)
J=$(q def src/lib.rs:13:9)
check "a crash answers with text matches and why" test "$(jq -r '.state' <<<"$J")|$(jq -r .hint <<<"$J" | grep -o 'crashed on purpose')" = "error|crashed on purpose"
check "the next request starts a new server" test "$(q def src/main.rs:2:19 | jq -r .state)|$(($(starts) - n0))" = "ready|1"
q def src/lib.rs:13:9 >/dev/null; q def src/lib.rs:13:9 >/dev/null
check "after 3 crashes: crashed, no new start" test "$(q def src/main.rs:2:19 | jq -r .state)" = crashed
V=$(rade open src/main.rs:2)
rade debug viewer "$V" place 2:19 >/dev/null
vkey "$V" shift+F12
check "results popover: Restart" eventually 5 is "code_ui | jq -r .retry" Restart
code_ui '{"op":"retry"}' >/dev/null
check "Restart: the server answers again" eventually 10 is "code_ui | jq -r .state" ready

echo "== R83.3 a broken server (huge message, then hangs): the app survives, its process is killed"
rade code restart src/lib.rs >/dev/null
q def src/main.rs:2:19 >/dev/null
BPID=$(rade code status --json | jq -r --arg r "$API" '.servers[] | select(.root == $r) | .pid')
check "a huge Content-Length answers with text matches" test "$(q def src/lib.rs:15:9 | jq -r .state)" = error
check "… the app is alive" rade list
check "… and the next request kills the old server and starts a new one" bash -c "rade code def src/main.rs:2:19 --json | jq -e '.state == \"ready\"' >/dev/null && ! kill -0 $BPID 2>/dev/null"

echo "== R83.3 max_servers LRU, idle stop, Worktree removal, quit"
MAX=2 IDLE=10 conf
WT2=$(rade worktree add api wt2)
sd0=$(grep -c '^shutdown ' "$FAKE_LSP_LOG" || true)
q def src/main.rs:2:19 >/dev/null                 # api (rust)
(cd "$WEB" && q def src/a.ts:4:20 >/dev/null)     # web (ts)
q def "$WT2/src/main.rs:2:19" >/dev/null           # wt2 (rust): api is the least recently used
check "two servers at most" test "$(servers)" = 2
check "the least recently used (api) was shut down" test "$(rade code status --json | jq -r --arg r "$API" '[.servers[].root] | index($r)')|$(($(grep -c '^shutdown ' "$FAKE_LSP_LOG") - sd0))" = "null|1"
rade worktree remove api wt2 --force >/dev/null
check "removing a Worktree stops its server" eventually 5 is "rade code status --json | jq -r '[.servers[].root] | map(select(startswith(\"$WT2\"))) | length'" 0
IDLE=0.05 conf
q def src/main.rs:2:19 >/dev/null
check "idle (3 s): every server stops" eventually 10 is servers 0
check "… with shutdown (no params) then exit" bash -c "grep -q '^shutdown .* False' '$FAKE_LSP_LOG' && grep -q '^exit ' '$FAKE_LSP_LOG'"
conf
q def src/main.rs:2:19 >/dev/null
PID=$(rade code status --json | jq -r '.servers[0].pid')
stop_app
check "quit stops the servers" bash -c "! kill -0 $PID 2>/dev/null"
start_app

echo "== R83.4 trusted .rade.toml: its server is used"
rade config trust api >/dev/null
check "trusted: the repo's command runs (and fails to start)" test "$(q def src/main.rs:2:19 | jq -r .state)" = error
check "… it ran" test -e "$T/pwned"
rm -f "$API/.rade.toml" && G -C "$API" rm -q --cached .rade.toml 2>/dev/null || true

echo "== smoke: a real rust-analyzer"
if [[ -n $RA_REAL && -x $RA_REAL ]]; then
    conf "" && sed -i '/^\[code.server.rust\]/,+1d' "$CONF"
    printf '[code.server.rust]\ncommand = ["%s"]\n' "$RA_REAL" >>"$CONF"
    rade code restart src/main.rs >/dev/null || true
    J=$(timeout 120 rade code def src/main.rs:2:19 --wait 60 --json </dev/null)
    echo "$J" | head -c 600
    check "rust-analyzer: def of api::greet → src/lib.rs:1" test "$(jq -r '"\(.source) \(.items[0].path | sub(".*/api/"; "")):\(.items[0].line)"' <<<"$J")" = "rust-analyzer src/lib.rs:1"
    J=$(timeout 60 rade code callers src/lib.rs:1:8 --json </dev/null)
    check "rust-analyzer: callers of greet are helper and main" test "$(jq -r '[.items[].name] | sort | join(",")' <<<"$J")" = "helper,main"
    check "… no cargo check target under the Worktree (checkOnSave off)" test ! -d "$API/target/debug"
else
    echo "  SKIP real rust-analyzer: none here (rustup component add rust-analyzer, or RADE_E2E_RA=<path>)"
fi

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
