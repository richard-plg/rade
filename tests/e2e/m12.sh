#!/usr/bin/env bash
# M12 end-to-end tests (ADR-0006, PRD R18): the Browser Pane and its agent
# automation. A tiny local site (python3 -m http.server, started in a RADE
# Terminal so its port is detected) with a form, a button, console.log and
# JS errors; `rade browser open|snapshot|click|fill|press|eval|screenshot|
# console|wait|back|forward|ports`, page-script isolation, Ctrl+Shift+O,
# palette "Open localhost:<port>", persistent cookies and restore after a
# relaunch; screenshots light + dark.
# Usage: tests/e2e/m12.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m12
source "$HERE/lib.sh"

state() { rade debug state; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
browsers() { state | jq -c '[.. | objects | select(.kind?.type? == "browser") | {id, url: .kind.url}]'; }
snap() { rade browser snapshot "$@"; }
js() { rade browser eval "$1"; }
ref_of() { # ref of the first snapshot line matching a regex
    snap | grep -E -- "$1" | head -1 | sed -E 's/.*\[ref=(e[0-9]+)\].*/\1/'
}
focused() { state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | .focused'; }

# ---- the test site ----
SITE=$T/site
mkdir -p "$SITE"
cat > "$SITE/index.html" <<'HTML'
<!doctype html>
<html><head><meta charset="utf-8"><title>RADE test page</title>
<style>
  body { font: 15px sans-serif; margin: 24px; background: #fff; color: #222; }
  @media (prefers-color-scheme: dark) { body { background: #1b1b1f; color: #eee; } }
  form { display: grid; gap: 8px; max-width: 320px; }
  .hidden { display: none; }
</style></head>
<body>
<h1>Sign up</h1>
<form id="f">
  <label for="email">Email</label>
  <input id="email" type="email" placeholder="you@example.com">
  <label><input id="agree" type="checkbox"> I agree</label>
  <select id="plan"><option value="free">Free</option><option value="pro">Pro plan</option></select>
  <button type="submit">Save</button>
</form>
<p id="status">Not saved</p>
<p class="hidden">Invisible text</p>
<div id="later"></div>
<a href="page2.html">Next page</a>
<button id="boom" onclick="null.boom()">Break</button>
<div id="card" style="cursor: pointer; border: 1px solid #888; padding: 6px">
  <a href="#sec">Card link</a> <input type="checkbox" id="cardbox" aria-label="Card box">
</div>
<button id="del" onclick="if (confirm('Delete it?')) document.getElementById('status').textContent = 'deleted'">Delete</button>
<h2 id="sec">Section</h2>
<script>
  console.log('page loaded', {n: 1});
  console.warn('careful');
  document.getElementById('f').addEventListener('submit', function (e) {
    e.preventDefault();
    var s = 'Saved ' + email.value + (agree.checked ? ' agreed' : '') + ' plan=' + plan.value;
    document.getElementById('status').textContent = s;
    setTimeout(function () { document.getElementById('later').innerHTML = '<button>Undo</button>'; }, 300);
  });
  setTimeout(function () { throw new Error('async boom'); }, 0);
  window.secret = 42;
</script>
</body></html>
HTML
cat > "$SITE/page2.html" <<'HTML'
<!doctype html><html><head><title>Page two</title></head>
<body><h1>Second</h1><a href="index.html">Home</a></body></html>
HTML
# A hostile page: tries to fake or break the automation from page script.
cat > "$SITE/hostile.html" <<'HTML'
<!doctype html><html><head><title>Hostile</title>
<script>
  window.__rade = { snapshot: function () { return 'pwned'; } };
  JSON.stringify = function () { return '"pwned"'; };
  HTMLElement.prototype.click = function () { document.title = 'hijacked'; };
  Element.prototype.getAttribute = function () { return 'pwned'; };
  document.addEventListener('DOMContentLoaded', function () {
    document.getElementById('go').addEventListener('click', function () {
      document.getElementById('out').textContent = 'clicked for real';
    });
  });
</script></head>
<body><button id="go">Press me</button><p id="out">waiting</p></body></html>
HTML
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')

echo "== start; a dev server in a Terminal"
start_app
mkdir -p "$T/proj"
git -C "$T/proj" init -q
rade source add "$T/proj" >/dev/null
agent=$(rade spawn --cmd bash --source proj --name agent --json | jq -r .pane)
rade focus "$agent"
srv=$(rade spawn --cmd "python3 -m http.server $PORT --bind 127.0.0.1 -d $SITE" --source proj --name devserver --tab new --json | jq -r .pane)
rade focus "$agent"
eventually 10 curl -sf "http://127.0.0.1:$PORT/" || { echo "server did not start"; exit 1; }

echo "== R18.4: listening ports per Pane"
check "rade browser ports lists the dev server's port and Pane" eventually 5 bash -c "rade browser ports --json | jq -e --arg p '$srv' '.ports[] | select(.port==$PORT and .pane==\$p and .name==\"devserver\" and .source==\"proj\")'"
check "palette offers 'Open localhost:$PORT'" bash -c "rade debug palette -- '>localhost' | jq -e '.results[] | select(.title==\"Open localhost:$PORT\" and .target.id==\"browser:http://localhost:$PORT/\")'"
check "no Browser yet: snapshot → NOT_FOUND (4)" test "$(exit_code rade browser snapshot)" = 4

echo "== R18.1/18.2: open beside the Agent, waits for the load"
r=$(RADE_PANE_ID=$agent rade browser open ":$PORT/" --json)
b=$(jq -r .pane <<<"$r")
check "open returns the loaded page" test "$(jq -r '[.url, .title, .loading] | @tsv' <<<"$r")" = "$(printf 'http://localhost:%s/\tRADE test page\tfalse' "$PORT")"
tab=$(rade list --json | jq -r --arg p "$agent" '.panes[] | select(.pane==$p) | .tab')
check "Browser split right of the Agent, same Tab" test "$(state | jq -c --arg t "$tab" '[.. | objects | select(.id?==$t) | .layout | .type, .dir, .a.pane.id, .b.pane.id]')" = "[\"split\",\"right\",\"$agent\",\"$b\"]"
check "focus stays with the Agent" test "$(focused)" = "$agent"
check "rade list shows the URL" test "$(rade list --json | jq -r --arg p "$b" '.panes[] | select(.pane==$p) | .cwd')" = "http://localhost:$PORT/"
check "state.json kind browser + url" eventually 3 bash -c "jq -e '[.. | objects | select(.kind?.type? == \"browser\")][0].kind.url == \"http://localhost:$PORT/\"' '$STATE'"
check "bad URL → USAGE (2)" test "$(exit_code rade browser open 'two words')" = 2

echo "== R18.2: snapshot with refs"
S=$(RADE_PANE_ID=$agent snap)
echo "$S" | sed 's/^/    /'
check "header: url + title" bash -c "head -2 <<<'$S' | tr '\n' '|' | grep -qF 'url: http://localhost:$PORT/|title: \"RADE test page\"|'"
check "heading with level" grep -qF 'heading "Sign up" [level=1]' <<<"$S"
check "labelled textbox with ref + placeholder-free name" grep -qE '^  \[ref=e[0-9]+\] textbox "Email" value=""$' <<<"$S"
check "checkbox named by its wrapping label, unchecked" grep -qE '\[ref=e[0-9]+\] checkbox "I agree" \[unchecked\]' <<<"$S"
check "select as combobox with its value" grep -qE '\[ref=e[0-9]+\] combobox "" value="Free"|\[ref=e[0-9]+\] combobox value="Free"' <<<"$S"
check "button" grep -qE '\[ref=e[0-9]+\] button "Save"' <<<"$S"
check "link with href" grep -qE '\[ref=e[0-9]+\] link "Next page" href=page2.html' <<<"$S"
check "a pointer-cursor card lists its controls, not its text" bash -c "grep -qE '^\[ref=e[0-9]+\] clickable$' <<<'$S' && grep -qE '^  \[ref=e[0-9]+\] link \"Card link\" href=#sec$' <<<'$S' && grep -qE '^  \[ref=e[0-9]+\] checkbox \"Card box\"' <<<'$S'"
check "plain text kept, label text not repeated, hidden text left out" bash -c "grep -qF 'text \"Not saved\"' <<<'$S' && ! grep -qF 'text \"Email\"' <<<'$S' && ! grep -qF Invisible <<<'$S'"
check "-i: only refs, flat" bash -c "rade browser snapshot -i | tail -n +3 | grep -vqE '^\[ref=e[0-9]+\] ' && exit 1 || true"
EMAIL=$(ref_of 'textbox "Email"'); AGREE=$(ref_of 'checkbox "I agree"'); PLAN=$(ref_of 'combobox'); SAVE=$(ref_of 'button "Save"'); NEXT=$(ref_of 'link "Next page"'); BOOM=$(ref_of 'button "Break"')
check "refs are stable across snapshots" test "$(ref_of 'button "Save"')" = "$SAVE"

echo "== R18.2: fill + click change the DOM"
rade browser fill "$EMAIL" "dev@example.com" >/dev/null
rade browser click "$AGREE" >/dev/null
rade browser fill "$PLAN" "Pro plan" >/dev/null
rade browser click "$SAVE" >/dev/null
check "status text changed (snapshot)" bash -c "rade browser snapshot | grep -qF 'text \"Saved dev@example.com agreed plan=pro\"'"
check "checkbox now [checked], textbox shows its value" bash -c "rade browser snapshot | grep -qF 'checkbox \"I agree\" [checked]' && rade browser snapshot | grep -qF 'value=\"dev@example.com\"'"
check "eval reads the DOM" test "$(js "document.getElementById('status').textContent")" = "Saved dev@example.com agreed plan=pro"
check "wait --text for later content" bash -c "rade browser wait --text Undo --timeout 5"
UNDO=$(ref_of 'button "Undo"')
check "a new element gets a new ref; old ones unchanged" bash -c "[[ -n '$UNDO' && '$UNDO' != '$SAVE' && \$(rade browser snapshot | grep -F 'button \"Save\"' | grep -oE 'e[0-9]+') == '$SAVE' ]]"
check "wait --ref" bash -c "rade browser wait --ref $UNDO --timeout 5"
check "wait --text timeout → TIMEOUT (6)" test "$(exit_code rade browser wait --text 'never there' --timeout 1)" = 6
check "unknown ref → NOT_FOUND (4)" test "$(exit_code rade browser click e999)" = 4
check "fill on a checkbox → USAGE (2)" test "$(exit_code rade browser fill "$AGREE" x)" = 2

echo "== R18.2: press keys"
rade browser fill "$EMAIL" "me@x.i" >/dev/null
rade browser click "$EMAIL" >/dev/null
rade browser press o >/dev/null
rade browser press Shift+a >/dev/null
check "typed keys reach the focused field (at the caret)" test "$(js 'email.value')" = 'me@x.ioA'
rade browser press Backspace >/dev/null
check "Backspace edits" test "$(js 'email.value')" = 'me@x.io'
rade browser press Enter >/dev/null
check "Enter submits the form" test "$(js "document.getElementById('status').textContent")" = "Saved me@x.io agreed plan=pro"
rade browser press Tab >/dev/null
check "Tab moves focus to the next field" test "$(js 'document.activeElement.id')" = agree
check "unknown key → USAGE (2)" test "$(exit_code rade browser press Hyper+q)" = 2

echo "== R18.2: eval returns JSON"
check "expression" test "$(js '1 + 1')" = 2
check "statements with return" test "$(js 'const a = 2; return a * 3')" = 6
check "page globals (page world)" test "$(js 'window.secret')" = 42
check "objects as JSON" test "$(rade browser eval '({a: [1, "x"], b: null})' --json | jq -cS .result)" = '{"a":[1,"x"],"b":null}'
check "DOM nodes as their HTML" test "$(js 'document.querySelector("h1")')" = '<h1>Sign up</h1>'
check "node lists as arrays" test "$(rade browser eval 'document.querySelectorAll("label")' --json | jq -r '.result | length')" = 2
check "promises are awaited" test "$(js 'new Promise(r => setTimeout(() => r("late"), 50))')" = late
check "undefined → null" test "$(js 'undefined')" = null
check "a thrown error → USAGE (2)" test "$(exit_code rade browser eval 'throw new Error("x")')" = 2
check "a runtime SyntaxError: the code ran once" bash -c "rade browser eval '(window.runs = (window.runs || 0) + 1, JSON.parse(\"{\"))' >/dev/null 2>&1; [[ \$(rade browser eval window.runs) == 1 ]]"

echo "== R18.2: console + JS errors"
rade browser click "$BOOM" >/dev/null
rade browser click "$(ref_of 'button "Delete"')" >/dev/null
check "confirm() during an action is accepted, not blocking" test "$(js "document.getElementById('status').textContent")" = deleted
C=$(rade browser console)
echo "$C" | sed 's/^/    /'
check "console.log with an object" grep -qF '[log] page loaded {"n":1}' <<<"$C"
check "console.warn" grep -qF '[warn] careful' <<<"$C"
check "uncaught async error with source:line" grep -qE "^\[error\] Uncaught Error: async boom  \(http://localhost:$PORT/:[0-9]+\)$" <<<"$C"
check "automation errors never reach the page's error handlers" bash -c "! grep -qF 'Script error' <<<'$C'"
check "error from a click handler" grep -qE '^\[error\] Uncaught TypeError' <<<"$C"
check "the dialog is logged" grep -qF '[dialog] confirm "Delete it?" → accepted' <<<"$C"
check "since the last call: second read is empty" test -z "$(rade browser console)"

echo "== R18.2: screenshot"
rade browser screenshot "$OUT/page.png" >/dev/null
check "PNG written, sane size" python3 - "$OUT/page.png" <<'PY'
import struct, sys
b = open(sys.argv[1], 'rb').read()
assert b[:8] == b'\x89PNG\r\n\x1a\n', 'not a PNG'
w, h = struct.unpack('>II', b[16:24])
assert 200 < w < 3000 and 200 < h < 2000, (w, h)
PY
shot browser-light

echo "== R18.1: navigation, back / forward"
check "back with no history → USAGE (2)" test "$(exit_code rade browser back)" = 2
rade browser click "$(ref_of 'link "Card link"')" >/dev/null
t0=$(date +%s)
rade browser back >/dev/null
check "a same-page (#hash) move back returns at once, not after the load timeout" bash -c "(( \$(date +%s) - $t0 < 5 )) && [[ \$(rade browser eval location.hash) == '' ]]"
r=$(rade browser click "$NEXT" --json)
check "click on a link navigates (and waits)" test "$(jq -r '[.navigated, .url] | @tsv' <<<"$r")" = "$(printf 'true\thttp://localhost:%s/page2.html' "$PORT")"
check "title after navigation" test "$(js document.title)" = "Page two"
check "old refs are gone after navigation → NOT_FOUND" test "$(exit_code rade browser click "$SAVE")" = 4
check "model follows the page" eventually 3 bash -c "rade list --json | jq -e --arg p '$b' '.panes[] | select(.pane==\$p) | .cwd == \"http://localhost:$PORT/page2.html\"'"
rade browser back >/dev/null
check "back" test "$(js document.title)" = "RADE test page"
rade browser forward >/dev/null
check "forward" test "$(js document.title)" = "Page two"
check "wait --load returns at once on a loaded page" bash -c "rade browser wait --load --timeout 3"

echo "== R18.5: page scripts can't fake or break the automation"
rade browser open ":$PORT/hostile.html" --pane "$b" >/dev/null
H=$(snap)
check "snapshot still real (isolated world)" bash -c "grep -qF 'button \"Press me\"' <<<'$H' && ! grep -q pwned <<<'$H'"
rade browser click "$(ref_of 'button "Press me"')" >/dev/null
check "click is the native one: page handler ran, page's click() override not used" bash -c "[[ \$(rade browser eval \"document.getElementById('out').textContent\") == 'clicked for real' && \$(rade browser eval 'document.title') == Hostile ]]"

echo "== R18.1: Ctrl+Shift+O and the palette"
check "open --pane navigates that Browser" test "$(rade browser open ":$PORT/" --pane "$b" --json | jq -r .pane)" = "$b"
rade browser eval 'document.cookie = "rade_test=kept; max-age=3600; path=/"' >/dev/null
rade focus "$srv"
rade debug action open_browser >/dev/null
check "Ctrl+Shift+O in a Tab without one: a Browser split right, on this Worktree's port, focused" eventually 5 bash -c "rade debug state | jq -e --arg s '$srv' '[.. | objects | select(.layout?.type? == \"split\" and .layout.a.pane.id == \$s)][0] | .layout.b.pane.kind == {type: \"browser\", url: \"http://localhost:$PORT/\"}'"
b2=$(browsers | jq -r --arg b "$b" '[.[] | select(.id != $b)][0].id')
check "the new Browser has focus" test "$(focused)" = "$b2"
rade debug action open_browser >/dev/null
check "again: no second Browser in the Tab" test "$(browsers | jq length)" = 2
rade close "$b2" >/dev/null
check "rade close removes a Browser" test "$(browsers | jq length)" = 1
rade focus "$srv"
rade debug action command_palette >/dev/null
rade debug palette --ui type ">localhost:$PORT" >/dev/null
rade debug palette --ui key enter >/dev/null
check "palette 'Open localhost:$PORT' opens a Browser there" eventually 5 bash -c "[[ \$(rade debug state | jq '[.. | objects | select(.kind?.type? == \"browser\")] | length') == 2 ]]"
b3=$(browsers | jq -r --arg b "$b" '[.[] | select(.id != $b)][0].id')
rade close "$b3" >/dev/null
rade focus "$agent"
sleep 1 # the Tab switch settles
shot browser-beside-agent

echo "== R11/R18.1: restore after a relaunch (dark appearance), persistent cookies"
stop_app
# The dev server's Pane comes back as a shell (commands are not re-run).
python3 -m http.server "$PORT" --bind 127.0.0.1 -d "$SITE" >/dev/null 2>&1 &
SERVER_PID=$!
eventually 10 curl -sf "http://127.0.0.1:$PORT/"
ADW_DEBUG_COLOR_SCHEME=prefer-dark start_app
check "Browser restored with its URL" test "$(browsers | jq -c '[.[] | .url]')" = "[\"http://localhost:$PORT/\"]"
check "page loaded again" eventually 10 bash -c "[[ \$(rade browser eval document.title 2>/dev/null) == 'RADE test page' ]]"
check "cookie kept in this flavor's data dir" test "$(js 'document.cookie')" = "rade_test=kept"
check "data in \$XDG_DATA_HOME/rade-dev/browser" test -d "$XDG_DATA_HOME/rade-dev/browser"
check "prefers-color-scheme: dark follows the system" test "$(js "matchMedia('(prefers-color-scheme: dark)').matches")" = true
rade browser snapshot >/dev/null
shot browser-dark
stop_app
kill "$SERVER_PID" 2>/dev/null # lib.sh's cleanup waits for every job

echo
echo "M12 E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
[[ $FAIL == 0 ]]
