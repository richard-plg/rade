#!/usr/bin/env bash
# M29 end-to-end tests (ADR-0030): code navigation in the Review Pane and
# a diff's Impact, through a FAKE language server (tests/e2e/fake-lsp.py)
# and FAKE Agents only — F12 / Ctrl+click / Shift+F12 / Alt+Shift+H on
# diff lines: new side through the server, a removed line's name looked
# up in the current tree else `git grep` at the base, labelled (R87); the
# Review's Impact section and `rade code impact` (changed / added /
# removed symbols, callers outside the diff, text matches in tests,
# didChangeWatchedFiles, indexing and a hung server within the budget)
# (R88); the Impact section in the Fan-out Judge's prompt, a hung server
# not holding the Judge past the budget (R88.4). Light + dark.
# Usage: tests/e2e/m29.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m29
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
FLSP=$HERE/fake-lsp.py
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin" "$T/agents"
cat >"$T/bin/agent.sh" <<EOF
#!/usr/bin/env bash
{ printf 'ARGV'; printf ' [%s]' "\$@"; echo; } >"$T/agents/\$RADE_PANE_ID.log"
exec "$FAKE" "\$@"
EOF
# A Fan-out Agent's work: greet's body changes.
cat >"$T/bin/edit-greet.sh" <<'EOF'
#!/usr/bin/env bash
sed -i 's/"hi {name}"/"hey {name}"/' src/lib.rs
EOF
# … and the language server hangs on documentSymbol in this Worktree.
cat >"$T/bin/edit-hang.sh" <<'EOF'
#!/usr/bin/env bash
sed -i 's/"hi {name}"/"hey {name}"/' src/lib.rs
sed -i 's/^{/{"@hang_on": "textDocument\/documentSymbol", /' .fake-lsp.json
EOF
chmod +x "$T/bin/"*.sh
cat >"$CONF" <<EOF
idle_seconds = 1
[agents.claude]
command = "$T/bin/agent.sh"
[code.server.rust]
command = ["python3", "$FLSP"]
EOF
export FAKE_LSP_LOG=$T/lsp.log FAKE_PROMPTS=$T/prompts.log FAKE_ENV=$T/fake.env
: >"$FAKE_LSP_LOG" && : >"$T/prompts.log" && : >"$T/fake.env"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code_ui() { rpc debug.code "${1:-{\}}"; }
rv() { rade debug review "$RV" "$@" --json; }
rinfo() { rade debug review "$RV" info --json | jq -r "$1"; }
vinfo() { rpc debug.viewer "{\"pane\": \"$1\", \"op\": \"info\"}" | jq -r "$2"; }
vkey() { rade debug viewer "$1" key "$2" >/dev/null; }
# The Viewer in the Review's Tab.
viewer() { rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer") | .id][0] // empty'; }
here() { vinfo "$(viewer)" '"\(.path | sub(".*/(api|feat)[^/]*/"; "")):\(.line):\(.cursor_col)"'; }
export -f code_ui rinfo vinfo viewer here rpc
export RV=

V=$HOME/workspaces/acme
API=$V/api
mkdir -p "$API/src" "$API/tests"
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

pub fn legacy() -> u32 {
    greet("old").len() as u32
}
EOF
cat >"$API/src/old.rs" <<'EOF'
pub fn old() -> u32 {
    crate::legacy()
}
EOF
cat >"$API/src/main.rs" <<'EOF'
fn main() {
    let é = api::greet("you");
    println!("{é} {}", api::helper());
}
EOF
cat >"$API/tests/t.rs" <<'EOF'
#[test]
fn t() {
    assert!(!api::greet("t").is_empty());
}
EOF
# Answers for the changed tree (fresh replaces legacy at lines 9–11).
cat >"$API/.fake-lsp.json" <<'EOF'
{"greet": {"def": ["src/lib.rs:1:8"],
           "refs": ["src/main.rs:2:18", "src/lib.rs:6:5", "src/lib.rs:10:5"],
           "callers": [{"name": "main", "def": "src/main.rs:1:4", "at": "src/main.rs:2:18"},
                       {"name": "helper", "def": "src/lib.rs:5:8", "at": "src/lib.rs:6:5"},
                       {"name": "fresh", "def": "src/lib.rs:9:8", "at": "src/lib.rs:10:5"}]},
 "helper": {"def": ["src/lib.rs:5:8"]},
 "fresh": {"def": ["src/lib.rs:9:8"]},
 "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8", "end": "src/lib.rs:3:1"},
                             {"name": "helper", "at": "src/lib.rs:5:8", "end": "src/lib.rs:7:1"},
                             {"name": "fresh", "at": "src/lib.rs:9:8", "end": "src/lib.rs:11:1"}]}}
EOF
echo target >"$API/.gitignore"
G -C "$API" init -q -b main && G -C "$API" add -A && G -C "$API" commit -qm init
G -C "$API" config user.name "Acme Dev" && G -C "$API" config user.email dev@acme.test
BASE=$(G -C "$API" rev-parse main)
mkdir -p "$V/.rade"
printf 'name = "Acme"\n' >"$V/.rade/workspace.toml"

start_app
rade workspace add "$V" >/dev/null
rade source add "$API" >/dev/null
rade workspace switch acme >/dev/null
rade project add beacon --source api >/dev/null

# The feat Worktree: greet's body changed, legacy removed (old.rs still
# calls it), fresh added.
FEAT=$(rade worktree add api feat)
cat >"$FEAT/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hello {name}")
}

pub fn helper() -> String {
    greet("x")
}

pub fn fresh() -> String {
    greet("fresh")
}
EOF
cd "$FEAT"

echo "== R88.3 rade code impact (JSON and text)"
J=$(rade code impact --json)
check "state ready from the fake server" test "$(jq -r '"\(.state) \(.source) \(.branch)"' <<<"$J")" = "ready fake-lsp main"
check "symbols: greet changed, fresh added, legacy removed" test "$(jq -r '[.symbols[] | "\(.change):\(.name):\(.line)"] | join(" ")' <<<"$J")" = "changed:greet:1 added:fresh:9 removed:legacy:9"
check "greet's callers outside the diff: helper, main (server), the test (text)" test "$(jq -r '[.symbols[0].callers[] | "\(.path | sub(".*/feat/"; "")):\(.line):\(.name // "-"):\(.test):\(.text)"] | join(" ")' <<<"$J")" = "src/lib.rs:6:helper:false:false src/main.rs:2:main:false:false tests/t.rs:3:-:true:true"
check "fresh's caller in the diff is not listed" test "$(jq -r '.symbols[1].callers | length' <<<"$J")" = 0
check "legacy is still used in old.rs (text match)" test "$(jq -r '.symbols[2].callers | map("\(.path | sub(".*/feat/"; "")):\(.line):\(.text)") | join(" ")' <<<"$J")" = "src/old.rs:2:true"
rade code impact >"$T/impact.txt"
cat >"$T/impact.want" <<'EOF'
2 changed · 3 call sites in 3 files (1 in tests) · 1 removed, still used · fake-lsp
~ greet (function) src/lib.rs:1 — 3 call sites
    src/lib.rs:6 helper: greet("x")
    src/main.rs:2 main: let é = api::greet("you");
    tests/t.rs:3 [test, text match]: assert!(!api::greet("t").is_empty());
+ fresh (function) src/lib.rs:9 — no callers outside the diff
- legacy (function) src/lib.rs:9 — still used at 1 place (text matches)
    src/old.rs:2 [text match]: crate::legacy()
EOF
check "text output exactly" diff -u "$T/impact.want" "$T/impact.txt"
check "the server was told the diff's files changed (didChangeWatchedFiles)" grep -q '^watched 2:src/lib.rs' "$FAKE_LSP_LOG"
check "--worktree feat from the main checkout: the same" test "$(cd "$API" && rade code impact --worktree feat --json | jq -r '[.symbols[].name] | join(",")')" = "greet,fresh,legacy"
check "--base HEAD: the same uncommitted changes" test "$(rade code impact --base HEAD --json | jq -r '.symbols | length')" = 3
set +e; (cd "$API" && rade code impact --worktree nope >/dev/null 2>&1); c=$?; set -e
check "an unknown Worktree exits 4" test "$c" = 4
set +e; rade code impact --base nope-ref >/dev/null 2>&1; c=$?; set -e
check "an unknown --base exits 2 (not a quiet fallback)" test "$c" = 2

echo "== R87 Review Pane: navigation on diff lines (new side through the server)"
RV=$(rade review open --source api --worktree feat)
check "the Review lists lib.rs" eventually 10 bash -c "rade debug review $RV info --json | jq -r .visible_text | grep -q 'greet(\"x\")'"
rv point 'greet("x")|greet' >/dev/null
check "a click puts the point on greet: the footer names it" test "$(rinfo .note)" = "greet — F12 definition · Ctrl+I docs"
rv key F12 >/dev/null
check "F12 on a context line: the Tab's Viewer at lib.rs:1, caret on greet" eventually 5 is here "src/lib.rs:1:8"
rv ctrl_click 'pub fn fresh() -> String {|fresh' >/dev/null
check "Ctrl+click on an added line's name: the same Viewer jumps to lib.rs:9" eventually 5 is here "src/lib.rs:9:8"
check "… Back is shown" test "$(vinfo "$(viewer)" .back)" = true
vkey "$(viewer)" alt+Left
check "Alt+Left in the Viewer: back to the first jump (lib.rs:1)" eventually 3 is "vinfo \$(viewer) .line" 1
rv point 'greet("fresh")|greet' >/dev/null
rv key shift+F12 >/dev/null
check "Shift+F12: References to greet on the results popover, 3 results" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.state)|\(.summary)\"'" "References to greet|ready|3 results in 2 files · fake-lsp"
rv key alt+shift+h >/dev/null
check "Alt+Shift+H: Callers of greet" eventually 5 is "code_ui | jq -r '\"\(.title)|\([.rows[] | select(.kind==\"call\") | .name] | join(\",\"))\"'" "Callers of greet|helper,fresh,main"
code_ui '{"op":"activate","path":"src/main.rs","line":2}' >/dev/null
check "a results popover row from the Review opens the Tab's Viewer" eventually 5 is here "src/main.rs:2:18"

echo "== R87.2 removed lines: the current tree, else the base (text matches, labelled)"
rv point 'pub fn legacy() -> u32 {|legacy' >/dev/null
rv key shift+F12 >/dev/null
check "legacy is gone: text matches at the base" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.state)|\(.source)|\(.summary)\"'" "References to legacy|base|text|2 results in 2 files · text matches at the base"
check "… the hint names the base commit" bash -c "code_ui | jq -r .hint | grep -qF 'legacy is not in the current tree: text matches at the base (${BASE:0:7})'"
check "… rows at the base, each a text match" test "$(code_ui | jq -r '[.rows[] | select(.kind=="item") | "\(.path):\(.line):\(.text_match)"] | join(" ")')" = "src/lib.rs:9:true src/old.rs:2:true"
sleep 0.5
shot code-base-light
rade debug action color_scheme_dark >/dev/null
sleep 0.6
shot code-base-dark
rade debug action color_scheme_light >/dev/null
rv point 'greet("old").len() as u32|greet' >/dev/null
rv key shift+F12 >/dev/null
check "greet on a removed line: looked up in the current tree, the server answers" eventually 5 is "code_ui | jq -r '\"\(.title)|\(.state)|\(.source)\"'" "References to greet|ready|fake-lsp"
check "… the hint says so" bash -c "code_ui | jq -r .hint | grep -qF 'Removed line: greet looked up in the current tree (src/lib.rs:1)'"
rv key F12 >/dev/null
check "F12 on it: the Viewer at greet's definition" eventually 5 is here "src/lib.rs:1:8"

echo "== R88.2 the Impact section"
rv impact >/dev/null
check "Impact: ready, summary with counts" eventually 10 is "rinfo '.impact | \"\(.state)|\(.summary)\"'" "ready|2 changed · 3 call sites in 3 files (1 in tests) · 1 removed, still used"
check "… rows: symbols with their callers, tags" test "$(rinfo '.impact.rows | join("\n")')" = "$(printf '%s\n' \
    '~ greet function src/lib.rs:1 3 callers' \
    '    src/lib.rs:6 helper greet("x")' \
    '    src/main.rs:2 main let é = api::greet("you");' \
    '    tests/t.rs:3 assert!(!api::greet("t").is_empty()); [test] [text match]' \
    '+ fresh function src/lib.rs:9' \
    '− legacy function src/lib.rs:9 1 use left' \
    '    src/old.rs:2 crate::legacy() [text match]')"
# Screenshots with the Review alone in its Tab.
rade debug viewer "$(viewer)" escape >/dev/null
sleep 0.6
shot review-impact-light
rade debug action color_scheme_dark >/dev/null
sleep 0.6
shot review-impact-dark
rade debug action color_scheme_light >/dev/null
rv impact_activate 'tests/t.rs:3' >/dev/null
check "a caller row opens the Viewer at it" eventually 5 is "here | cut -d: -f1,2" "tests/t.rs:3"
rv impact_activate '~ greet' >/dev/null
check "a symbol row opens its declaration" eventually 5 is here "src/lib.rs:1:8"
printf '\npub fn later() -> String {\n    greet("later")\n}\n' >>"$FEAT/src/lib.rs"
jq -c '."@symbols"."src/lib.rs" += [{"name": "later", "at": "src/lib.rs:13:8", "end": "src/lib.rs:15:1"}]' "$FEAT/.fake-lsp.json" >"$T/f.json" && mv "$T/f.json" "$FEAT/.fake-lsp.json"
check "the diff changed while shown: computed again (later added)" eventually 15 bash -c "rinfo '.impact.rows | join(\"\n\")' | grep -q '^+ later function src/lib.rs:13'"

echo "== R88 budget: an indexing server, a hung server"
SLOW=$(cd "$API" && rade worktree add api slow)
sed -i 's/"hi {name}"/"yo {name}"/' "$SLOW/src/lib.rs"
sed -i 's/^{/{"@index_ms": 9000, /' "$SLOW/.fake-lsp.json"
t0=$(date +%s)
J=$(cd "$SLOW" && rade code impact --wait 2 --json)
check "indexing past the budget: text matches within it" test "$(jq -r '"\(.state)|\(.source)|\([.symbols[] | "\(.name):\(.text)"] | join(","))"' <<<"$J")" = "indexing|text|greet:true"
check "… in about 2 s" test $(($(date +%s) - t0)) -le 5
check "… the hint says the server is indexing" bash -c "jq -r .hint <<<'$(jq -c . <<<"$J")' | grep -q 'fake-lsp is indexing'"
RS=$(rade review open --source api --worktree slow)
rade debug review "$RS" impact >/dev/null
check "the Review's Impact while indexing: text matches, then the server's answer by itself" eventually 25 is "rade debug review $RS info --json | jq -r '.impact | \"\(.state)|\(.source)\"'" "ready|fake-lsp"
HUNG=$(cd "$API" && rade worktree add api hung)
(cd "$HUNG" && "$T/bin/edit-hang.sh")
t0=$(date +%s)
J=$(cd "$HUNG" && rade code impact --wait 3 --json)
check "a server hanging on documentSymbol: text matches within the budget" test "$(jq -r '"\(.source)|\([.symbols[] | .name] | join(","))"' <<<"$J")" = "text|greet"
check "… in about 3 s, saying why" bash -c "(( $(date +%s) - $t0 <= 6 )) && jq -r .hint <<<'$(jq -c . <<<"$J")' | grep -qE 'timed out|No answer'"

echo "== R88.4 Fan-out Judge prompt: an Impact section per candidate"
FO=$(rade fanout --source api --agents claude,claude "run:$T/bin/edit-greet.sh" --json | jq -r .fanout)
rade fanout wait "$FO" --timeout 30 >/dev/null
rade fanout judge "$FO" >/dev/null
JD=$(rade fanout status "$FO" --json | jq -r .judge.dir)
for want in "## Impact of $FO-1" "## Impact of $FO-2" "~ greet (function) src/lib.rs:1"; do
    check "Judge prompt: $want" grep -qF -- "$want" "$JD/prompt.md"
done

echo "== R88.4 a hung server never holds a Judge past the budget (20 s)"
FH=$(rade fanout --source api --agents claude,claude "run:$T/bin/edit-hang.sh" --json | jq -r .fanout)
rade fanout wait "$FH" --timeout 30 >/dev/null
ts=$(date +%s)
rade fanout judge "$FH" >/dev/null
JH=$(rade fanout status "$FH" --json | jq -r .judge.dir)
check "the Judge prompt is written within the budget" eventually 30 test -s "$JH/prompt.md"
check "… within 25 s" test $(($(date +%s) - ts)) -le 25
check "… its Impact: text matches, saying why" bash -c "grep -qF '· text matches' '$JH/prompt.md' && grep -qE 'timed out|No answer' '$JH/prompt.md' && grep -qF '~ greet (function, text) src/lib.rs:1' '$JH/prompt.md'"

echo "== M28 Low: a Terminal's process inherits no app fds above 2"
printf '#!/usr/bin/env bash\nls -l /proc/$$/fd >%s/fds.txt\nsleep 30\n' "$T" >"$T/bin/fds.sh" && chmod +x "$T/bin/fds.sh"
P=$(rade spawn --source api --cmd "$T/bin/fds.sh" --json | jq -r .pane)
check "only 0, 1, 2 (and bash's own script)" eventually 10 bash -c "[[ -s '$T/fds.txt' ]] && ! grep -E ' [0-9]+ -> ' '$T/fds.txt' | grep -vE ' [012] -> |fds.sh' | grep -q ."
rade close "$P" --force >/dev/null 2>&1 || true

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
