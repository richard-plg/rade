#!/usr/bin/env bash
# M24 end-to-end tests (PRD R71, R72, ADR-0026, ADR-0025 amendment): Browser
# Annotations in a Browser Pane (annotate mode on a local test site and a
# file:// prototype, element + region picks, crops, console errors, edit /
# delete, persistence across a reload and a relaunch, Send to Agent into a
# stand-in Agent); `rade browser --engine chromium` against the same site
# (snapshot / refs / click / fill / press / eval / screenshot / console /
# wait / history, headed with the annotation toolbar) when a Chromium is
# found (else skipped with the reason).
# Usage: tests/e2e/m24.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m24
# A Chromium from the real HOME (Playwright's cache), found before lib.sh
# swaps HOME for a temp one; $RADE_E2E_CHROMIUM overrides.
if [[ -z ${RADE_E2E_BUS:-} && -z ${RADE_E2E_CHROMIUM:-} ]]; then
    for c in "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux*/chrome /opt/chrome-for-testing/chrome \
        "$(command -v chromium || true)" "$(command -v google-chrome-stable || true)"; do
        [[ -n $c && -x $c ]] && { export RADE_E2E_CHROMIUM=$c; break; }
    done
fi
source "$HERE/lib.sh"

exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
js() { rade browser eval "$1"; }
dbg() { rpc debug.browser "$1"; }
ref_of() { rade browser snapshot "${@:2}" | grep -E -- "$1" | head -1 | sed -E 's/.*\[ref=(e[0-9]+)\].*/\1/'; }

RECV=$T/received.txt
: >"$RECV"
printf '#!/bin/bash\necho SINK READY\nexec cat >>%q\n' "$RECV" >"$T/sink.sh"
chmod +x "$T/sink.sh"
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.sink]
command = "$T/sink.sh"
EOF

# ---- the test site ----
SITE=$T/site
mkdir -p "$SITE"
cat >"$SITE/index.html" <<'HTML'
<!doctype html>
<html><head><meta charset="utf-8"><title>Annotate me</title>
<style>
  body { font: 15px sans-serif; margin: 24px; background: #fff; color: #222; }
  @media (prefers-color-scheme: dark) { body { background: #1b1b1f; color: #eee; } }
  .hero { padding: 16px; background: #e8f0fe; border-radius: 8px; width: 420px; }
  @media (prefers-color-scheme: dark) { .hero { background: #26324a; } }
  form { display: grid; gap: 8px; max-width: 320px; margin-top: 16px; }
  button { padding: 6px 12px; }
</style></head>
<body>
<div class="hero" id="hero"><h1>Pricing</h1><p>Plans for every team size.</p></div>
<form id="f">
  <input name="email" placeholder="you@example.com">
  <button type="submit" data-testid="save">Save</button>
</form>
<p id="status">Not saved</p>
<button id="boom" onclick="null.boom()">Break</button>
<a href="page2.html">Next page</a>
<div id="sh"></div>
<script>
  document.getElementById('sh').attachShadow({mode: 'open'}).innerHTML = '<button>Inner</button>';

  document.getElementById('f').addEventListener('submit', function (e) {
    e.preventDefault(); document.getElementById('status').textContent = 'Saved';
  });
  console.error('TypeError: price is undefined');
</script>
</body></html>
HTML
cp "$SITE/index.html" "$T/prototype.html"
printf '<!doctype html><html><head><title>Page two</title></head><body><h1>Second</h1><a href="index.html">Home</a></body></html>\n' >"$SITE/page2.html"
cat >"$SITE/hostile.html" <<'HTML'
<!doctype html><html><head><title>Hostile</title>
<script>
  window.__rade = { snapshot: function () { return 'pwned'; } };
  window.radeAnnotate = function () { document.title = 'pwned'; };
  HTMLElement.prototype.click = function () { document.title = 'hijacked'; };
  document.addEventListener('DOMContentLoaded', function () {
    document.getElementById('go').addEventListener('click', function () {
      document.getElementById('out').textContent = 'clicked for real ' + event.isTrusted;
    });
  });
</script></head>
<body><button id="go">Press me</button><p id="out">waiting</p></body></html>
HTML
cat >"$T/preview-server.py" <<'PYTHON'
import http.server, sys
class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/redirect-youtube':
            self.send_response(302)
            self.send_header('Location', 'https://www.youtube.com/watch?v=aqz-KE-bpKQ')
            self.end_headers()
        else:
            super().do_GET()
http.server.ThreadingHTTPServer(('127.0.0.1', int(sys.argv[1])), Handler).serve_forever()
PYTHON
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')

echo "== start; a dev server, an Agent, a Browser beside it"
start_app
mkdir -p "$T/proj"
git -C "$T/proj" init -q
rade source add "$T/proj" >/dev/null
agent=$(rade spawn --agent sink --source proj --name agent --json | jq -r .pane)
eventually 10 screen_has "$agent" "SINK READY"
rade focus "$agent"
srv=$(rade spawn --cmd "cd $SITE && python3 $T/preview-server.py $PORT" --source proj --name devserver --tab new --json | jq -r .pane)
rade focus "$agent"
eventually 10 curl -sf "http://127.0.0.1:$PORT/" || { echo "server did not start"; exit 1; }
b=$(RADE_PANE_ID=$agent rade browser open ":$PORT/" --json | jq -r .pane)
rade browser console >/dev/null

echo "== R71.1: annotate mode off by default; nothing in the page"
check "off: no overlay element in the page" test "$(js 'document.querySelector("rade-annotate") === null')" = true
check "off: no panel" test "$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r '[.annotating, .panel] | @tsv')" = "$(printf 'false\tfalse')"
check "annotate.js is not reachable from the page" test "$(js 'typeof window.__rade')" = undefined

echo "== R71.1: the header toggle turns it on: overlay + panel"
s=$(dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}")
check "preview identifies its engine" test "$(jq -r .engine <<<"$s")" = webkit
check "on: toggle active, panel shown with the hint" test "$(jq -r '[.annotating, .panel, (.hint | test("Click or drag to annotate"))] | @tsv' <<<"$s")" = "$(printf 'true\ttrue\ttrue')"
check "on: the overlay is in the page" eventually 3 is "rade browser eval 'document.querySelector(\"rade-annotate\") !== null'" true
check "on: Send is off with nothing to send" test "$(jq -r '[.send, .send_sensitive] | @tsv' <<<"$s")" = "$(printf 'Send to Agent\tfalse')"

echo "== P2-04: keyboard selection in the isolated annotation world"
k=$(dbg "{\"pane\":\"$b\",\"op\":\"key\",\"key\":\"next\"}")
check "keyboard selects and names a page element" jq -e '.target != null and .pending == false' <<<"$k"
k=$(dbg "{\"pane\":\"$b\",\"op\":\"key\",\"key\":\"pick\"}")
check "keyboard opens the comment dialog" jq -e '.pending == true' <<<"$k"
k=$(dbg "{\"pane\":\"$b\",\"op\":\"key\",\"key\":\"cancel\"}")
check "cancel returns to picking without ending mode" jq -e '.pending == false and .on == true' <<<"$k"
js 'document.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true})); document.querySelector("rade-annotate").click()' >/dev/null
check "page synthetic events cannot end trusted annotation mode" test "$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r .annotating)" = true

echo "== R71.2: click an element (the Save button by its ref) and comment"
SAVE=$(ref_of 'button "Save"')
r=$(dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"ref\":\"$SAVE\",\"comment\":\"Make Save green\"}")
check "the box names what was picked" test "$(jq -r .what <<<"$r")" = 'button "Save"'
check "kept on the Browser in state.json" eventually 5 bash -c "jq -e '[.. | objects | select(.kind?.type? == \"browser\")][0].kind.annotations | length == 1' '$STATE'"
A=$(rade browser annotations --json)
echo "$A" | jq -c '.annotations[0] | del(.console)' | sed 's/^/    /'
check "selector from the test id" test "$(jq -r '.annotations[0].selector' <<<"$A")" = '[data-testid="save"]'
check "the same ref the snapshot gave" test "$(jq -r '.annotations[0].ref' <<<"$A")" = "$SAVE"
check "url, role, name, text" test "$(jq -r '.annotations[0] | [.url, .role, .name, .text, .kind] | @tsv' <<<"$A")" = "$(printf 'http://localhost:%s/\tbutton\tSave\tSave\telement' "$PORT")"
check "a page box" jq -e '.annotations[0].bbox | .w > 20 and .h > 10 and .y > 100' <<<"$A"
SHOT=$(jq -r '.annotations[0].shot' <<<"$A")
check "a cropped screenshot, about the button's size" python3 - "$SHOT" "$(jq -r '.annotations[0].bbox | "\(.w) \(.h)"' <<<"$A")" <<'PY'
import struct, sys
b = open(sys.argv[1], 'rb').read()
assert b[:8] == b'\x89PNG\r\n\x1a\n', 'not a PNG'
w, h = struct.unpack('>II', b[16:24])
bw, bh = (float(x) for x in sys.argv[2].split())
assert bw + 8 <= w <= 3 * (bw + 16) and bh + 8 <= h <= 3 * (bh + 16), (w, h, bw, bh)
PY
check "crop in \$XDG_CACHE_HOME/rade/annotations" bash -c "[[ '$SHOT' == '$XDG_CACHE_HOME/rade/annotations/'* ]]"
check "the page's console errors" jq -e '.annotations[0].console | any(test("price is undefined"))' <<<"$A"
check "the panel lists it, Send is on" test "$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r '[.send, .send_sensitive, (.rows | length)] | @tsv')" = "$(printf 'Send to Agent (1)\ttrue\t1')"

echo "== R71.2: drag a region"
r=$(dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"x\":30,\"y\":30,\"w\":200,\"h\":60,\"comment\":\"Hero: less padding\"}")
check "region box text" grep -q 'Region 200 × 60' <<<"$(jq -r .what <<<"$r")"
A=$(rade browser annotations --json)
check "region anchored on the smallest element holding it" test "$(jq -r '.annotations[1] | [.kind, .selector, (.bbox.w | floor), (.bbox.h | floor)] | @tsv' <<<"$A")" = "$(printf 'region\t#hero\t200\t60')"
check "region crop written" test -s "$(jq -r '.annotations[1].shot' <<<"$A")"
shot annotate-light

echo "== R71.3: edit, delete, persistence across a reload"
dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"x\":40,\"y\":220,\"comment\":\"temp\"}" >/dev/null
check "three pending" eventually 3 is "rade browser annotations --json | jq '.annotations | length'" 3
dbg "{\"pane\":\"$b\",\"op\":\"edit\",\"id\":1,\"text\":\"Make Save green and bigger\"}" >/dev/null
check "edit in the panel changes the comment" eventually 3 is "rade browser annotations --json | jq -r '.annotations[0].comment'" "Make Save green and bigger"
dbg "{\"pane\":\"$b\",\"op\":\"delete\",\"id\":3}" >/dev/null
check "delete in the panel" eventually 3 is "rade browser annotations --json | jq -c '[.annotations[].id]'" "[1,2]"
rade browser reload >/dev/null
check "after a reload: still pending, still in annotate mode, overlay back" eventually 5 bash -c "[[ \$(rade browser annotations --json | jq '.annotations | length') == 2 && \$(rade browser eval 'document.querySelector(\"rade-annotate\") !== null') == true ]]"

echo "== R71.1: off again: the page is the page"
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null
check "off: overlay gone" eventually 3 is "rade browser eval 'document.querySelector(\"rade-annotate\") === null'" true
rade browser fill "$(ref_of 'textbox')" "a@b.c" >/dev/null
rade browser click "$(ref_of 'button "Save"')" >/dev/null
check "off: page clicks work" test "$(js "document.getElementById('status').textContent")" = Saved
check "off with pending ones: panel still shown" test "$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r .panel)" = true
check "pick while off → USAGE" test "$(exit_code rpc debug.browser "{\"pane\":\"$b\",\"op\":\"pick\",\"x\":5,\"y\":5,\"comment\":\"x\"}" ; rpc debug.browser "{\"pane\":\"$b\",\"op\":\"pick\",\"x\":5,\"y\":5,\"comment\":\"x\"}" | jq -r .message)" = "0
annotate mode is off"

echo "== R71.4: Send to Agent → one prompt in the Agent beside the Browser"
dbg "{\"pane\":\"$b\",\"op\":\"send\"}" >/dev/null
check "the Agent got the prompt (and its Enter)" eventually 5 bash -c "grep -qF \"Browser annotations from the owner (2, made in RADE's Browser Pane)\" '$RECV' && grep -qF 'comment: Hero: less padding' '$RECV'"
sed 's/^/    /' "$RECV"
for want in "page: http://localhost:$PORT/" 'selector: [data-testid="save"]' "ref: $SAVE (" "screenshot: $SHOT" \
    "price is undefined" "comment: Make Save green and bigger" "comment: Hero: less padding" "selector: #hero"; do
    check "prompt has: $want" grep -qF -- "$want" "$RECV"
done
check "sent ones are cleared" test "$(rade browser annotations --json | jq '.annotations | length')" = 0
note=$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r .note)
check "the panel says where they went ($note)" test "$note" = "Sent 2 to agent ($agent)"

echo "== R71.5: rade browser annotations --send; none → USAGE"
check "nothing to send → USAGE (2)" test "$(exit_code rade browser annotations --send)" = 2
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"ref\":\"$(ref_of 'button "Inner"')\",\"comment\":\"inside a shadow root\"}" >/dev/null
check "an element in an open shadow root can be picked (acceptance M4)" eventually 3 is "rade browser annotations --json | jq -r '.annotations[0] | \"\(.role) \(.name)\"'" "button Inner"
check "--to a plain Terminal is refused: page text would run in a shell (acceptance H1)" bash -c "[[ \$(set +e; rade browser annotations --send --to $srv 2>&1; echo \" rc=\$?\") == *'not an Agent Pane'*'rc=2' ]]"
rade browser annotations --send --to "$agent" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null

echo "== R71.1: a local HTML prototype (file://)"
rade browser open "file://$T/prototype.html" --pane "$b" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"x\":60,\"y\":78,\"comment\":\"Bigger title\"}" >/dev/null
sleep 1
A=$(rade browser annotations --json)
jq -c '.annotations[0] | {url, role, selector}' <<<"$A" | sed 's/^/    /'
check "file:// page annotated (heading)" jq -e --arg u "file://$T/prototype.html" '.annotations[0] | .url == $u and .role == "heading" and .selector == "#hero > h1"' <<<"$A"
r=$(rade browser annotations --send --json)
check "--send from the CLI: to the Agent in the Browser's Tab" test "$(jq -r '[.pane, .annotations] | @tsv' <<<"$r")" = "$(printf '%s\t1' "$agent")"
check "prompt arrived" eventually 5 grep -qF "comment: Bigger title" "$RECV"
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null

echo "== R71.3: pending annotations survive a relaunch (dark)"
rade browser open ":$PORT/" --pane "$b" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null
dbg "{\"pane\":\"$b\",\"op\":\"pick\",\"ref\":\"$(ref_of 'button "Break"')\",\"comment\":\"Remove this button\"}" >/dev/null
eventually 3 is "rade browser annotations --json | jq '.annotations | length'" 1
stop_app
# The dev server's Pane comes back as a shell (commands are not re-run).
(cd "$SITE" && python3 "$T/preview-server.py" "$PORT") >/dev/null 2>&1 &
SERVER_PID=$!
eventually 10 curl -sf "http://127.0.0.1:$PORT/"
ADW_DEBUG_COLOR_SCHEME=prefer-dark start_app
check "restored with the Browser" eventually 5 is "rade browser annotations --json | jq -r '.annotations[0].comment'" "Remove this button"
eventually 10 is "rade browser eval document.title 2>/dev/null" "Annotate me"
dbg "{\"pane\":\"$b\",\"op\":\"toggle\"}" >/dev/null
sleep 0.5
shot annotate-dark
rade browser annotations --send >/dev/null 2>&1 || true

echo "== P1-04: known unsafe embedded YouTube navigations are refused"
check "CLI refuses YouTube immediately with a Chromium recommendation" test "$(exit_code timeout 5 rade browser open https://www.youtube.com/watch?v=aqz-KE-bpKQ)" = 2
check "CLI error explains the alternative" grep -q 'Chromium' "$T/err"
rade browser open "http://localhost:$PORT/" >/dev/null
js 'location.href = "https://www.youtube.com/watch?v=aqz-KE-bpKQ"' >/dev/null || true
guard_status() { dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -e '.status | contains("crash RADE")' >/dev/null; }
check "page navigation is blocked with actionable status" eventually 5 guard_status
check "RADE stays alive after the page navigation" rade list
check "blocked URL stays available for browser fallback" test "$(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r .url)" = 'https://www.youtube.com/watch?v=aqz-KE-bpKQ'
js 'location.href = "http://localhost:'"$PORT"'/page2.html"' >/dev/null || true
safe_url() { [[ $(dbg "{\"pane\":\"$b\",\"op\":\"state\"}" | jq -r .url) == "http://localhost:$PORT/page2.html" ]]; }
check "safe page navigation clears stale blocked fallback URL" eventually 5 safe_url
js 'var a = document.createElement("a"); a.target = "_blank"; a.href = "https://youtu.be/aqz-KE-bpKQ"; document.body.append(a); a.click()' >/dev/null || true
check "new-window navigation reaches the unsafe host guard" eventually 5 guard_status
check "RADE stays alive after new-window navigation" rade list
check "redirect fixture remains HTTP 302 after app relaunch" test "$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "http://localhost:$PORT/redirect-youtube")" = '302 https://www.youtube.com/watch?v=aqz-KE-bpKQ'
rade browser open "http://localhost:$PORT/redirect-youtube" --json >"$T/redirect.json"
check "HTTP redirect is blocked with an engine recommendation" jq -e '.error | contains("Chromium")' "$T/redirect.json"
check "RADE stays alive after the HTTP redirect" rade list
rade browser open "http://localhost:$PORT/" >/dev/null

if [[ ${M24_PART:-} == annotate ]]; then
    stop_app; kill "$SERVER_PID"
    echo; echo "M24 E2E (annotate only): $PASS passed, $FAIL failed"; [[ $FAIL == 0 ]]; exit
fi

# ---------------------------------------------------------------------------
echo "== R72: rade browser --engine chromium"
cr() { rade browser --engine chromium "$@"; }
CRSOCK=$XDG_RUNTIME_DIR/rade/rade-dev-chromium.sock
if [[ -z ${RADE_E2E_CHROMIUM:-} ]]; then
    echo "  SKIP the Chromium engine: no Chromium found (Playwright's cache, chromium, google-chrome; or RADE_E2E_CHROMIUM)"
    SKIPPED_CHROMIUM=1
elif [[ ${M24_PART:-} != tasks ]]; then
    check "no Chromium yet: a command → NOT_FOUND (4)" test "$(exit_code cr snapshot)" = 4
    check "close with none running is fine" cr close
    printf '[browser]\nchromium = "/nonexistent/chrome"\n' >>"$XDG_CONFIG_HOME/rade/config.toml"
    check "a configured path that isn't there → NOT_FOUND with the reason" bash -c "[[ \$(set +e; rade browser --engine chromium open :$PORT/ 2>&1; echo \" rc=\$?\") == *'[browser] chromium = /nonexistent/chrome is not an executable'*'rc=4' ]]"
    sed -i "s|^chromium = .*|chromium = \"$RADE_E2E_CHROMIUM\"|" "$XDG_CONFIG_HOME/rade/config.toml"
    r=$(RADE_PANE_ID=$agent cr open ":$PORT/" --json)
    check "open: loaded, pane chromium, engine chromium" test "$(jq -r '[.pane, .url, .title, .loading, .engine] | @tsv' <<<"$r")" = "$(printf 'chromium\thttp://localhost:%s/\tAnnotate me\tfalse\tchromium' "$PORT")"
    CRPID=$(jq -r .chromium.pid <<<"$r"); PROFILE=$(jq -r .chromium.profile <<<"$r")
    CMDLINE=$(tr '\0' ' ' </proc/"$CRPID"/cmdline)
    check "the configured Chromium" test "$(jq -r .chromium.path <<<"$r")" = "$RADE_E2E_CHROMIUM"
    check "its own socket beside RADE's (0600)" bash -c "[[ -S '$CRSOCK' && \$(stat -c %a '$CRSOCK') == 600 ]]"
    check "CDP over a pipe, no TCP port" bash -c "grep -q -- '--remote-debugging-pipe' <<<'$CMDLINE' && ! grep -q -- '--remote-debugging-port' <<<'$CMDLINE'"
    check "a temp profile (0700) under the temp dir, not ~/.config" bash -c "[[ '$PROFILE' == ${TMPDIR:-/tmp}/rade-dev-chromium-profile-* && \$(stat -c %a '$PROFILE') == 700 ]] && grep -qF -- '--user-data-dir=$PROFILE' <<<'$CMDLINE' && [[ ! -e '$HOME/.config/chromium' && ! -e '$HOME/.config/google-chrome' ]]"
    check "headless" grep -q -- '--headless' <<<"$CMDLINE"

    echo "== R72.2: the same snapshot and refs as the Browser Pane"
    WK=$(rade browser snapshot); CR=$(cr snapshot)
    check "snapshot text identical to WebKit's (same page, same library)" test "$CR" = "$WK"
    echo "$CR" | sed 's/^/    /' | head -12
    EM=$(ref_of 'textbox' --engine chromium); SV=$(ref_of 'button "Save"' --engine chromium)
    check "refs stable across snapshots (isolated world kept between calls)" test "$(ref_of 'button "Save"' --engine chromium)" = "$SV"
    cr fill "$EM" "cr@example.com" >/dev/null
    r=$(cr click "$SV" --json)
    check "fill + trusted click submit the form" test "$(cr eval "document.getElementById('status').textContent")" = Saved
    check "click answers the element" test "$(jq -r '.element.name' <<<"$r")" = Save
    cr fill "$EM" "" >/dev/null; cr click "$EM" >/dev/null
    cr press x >/dev/null; cr press Shift+y >/dev/null; cr press Backspace >/dev/null; cr press z >/dev/null
    check "press: trusted keys type at the caret" test "$(cr eval "document.querySelector('input').value")" = xz
    cr eval "document.getElementById('status').textContent = 'x'" >/dev/null
    cr press Enter >/dev/null
    check "press Enter submits the form" test "$(cr eval "document.getElementById('status').textContent")" = Saved
    r=$(cr press Tab --json)
    check "Tab moves focus (focused answers it)" test "$(jq -r .focused.name <<<"$r")" = Save
    check "eval: expression / statements / objects / promises" bash -c "[[ \$(rade browser --engine chromium eval '1 + 1') == 2 && \$(rade browser --engine chromium eval 'const a = 3; return a * 2') == 6 && \$(rade browser --engine chromium eval '({a: [1]})' --json | jq -c .result) == '{\"a\":[1]}' && \$(rade browser --engine chromium eval 'new Promise(r => setTimeout(() => r(\"late\"), 50))') == late ]]"
    check "eval: a thrown error → USAGE (2)" test "$(exit_code cr eval 'throw new Error("x")')" = 2
    cr click "$(ref_of 'button "Break"' --engine chromium)" >/dev/null
    C=$(cr console)
    echo "$C" | sed 's/^/    /'
    check "console: console.error and an uncaught error with source:line" bash -c "grep -qF '[error] TypeError: price is undefined' <<<'$C' && grep -qE \"^\\[error\\] Uncaught TypeError: .*\\(http://localhost:$PORT/:[0-9]+\\)\" <<<'$C'"
    check "console: since the last call" test -z "$(cr console)"
    cr screenshot "$OUT/chromium.png" >/dev/null
    check "screenshot: a 1280-wide PNG" python3 - "$OUT/chromium.png" <<'PY'
import struct, sys
b = open(sys.argv[1], 'rb').read()
assert b[:8] == b'\x89PNG\r\n\x1a\n'
w, h = struct.unpack('>II', b[16:24])
assert w == 1280 and 500 < h <= 800, (w, h)
PY
    check "wait --text / timeout → 6" bash -c "rade browser --engine chromium wait --text Saved --timeout 3 && [[ \$(set +e; rade browser --engine chromium wait --text never --timeout 1 >/dev/null 2>&1; echo \$?) == 6 ]]"
    check "unknown ref → NOT_FOUND (4)" test "$(exit_code cr click e999)" = 4
    r=$(cr click "$(ref_of 'link "Next page"' --engine chromium)" --json)
    check "a link click navigates and waits" test "$(jq -r '[.navigated, .url] | @tsv' <<<"$r")" = "$(printf 'true\thttp://localhost:%s/page2.html' "$PORT")"
    check "old refs gone after navigation → 4" test "$(exit_code cr click "$SV")" = 4
    cr back >/dev/null
    check "back" test "$(cr eval document.title)" = "Annotate me"
    cr forward >/dev/null
    check "forward" test "$(cr eval document.title)" = "Page two"
    check "reload" bash -c "rade browser --engine chromium reload >/dev/null && [[ \$(rade browser --engine chromium eval document.title) == 'Page two' ]]"
    cr back >/dev/null
    check "back at the start: forward then back twice → USAGE" bash -c "rade browser --engine chromium back >/dev/null 2>&1; [[ \$(set +e; rade browser --engine chromium back >/dev/null 2>&1; echo \$?) == 2 ]]"
    cr open ":$PORT/hostile.html" >/dev/null
    check "isolated world: the page can't fake the snapshot" bash -c "rade browser --engine chromium snapshot | grep -qF 'button \"Press me\"'"
    cr click "$(ref_of 'button "Press me"' --engine chromium)" >/dev/null
    check "… and its click is trusted (isTrusted true), the page's click() override unused" test "$(cr eval "document.getElementById('out').textContent")" = "clicked for real true"
    check "--headed while it runs headless → USAGE" test "$(exit_code cr open ":$PORT/" --headed)" = 2
    check "ports stay RADE's (no --engine needed)" bash -c "rade browser --engine chromium ports --json | jq -e '.ports | length >= 0'"

    echo "== R71 + R72: annotate in Chromium (trusted click on the overlay, typed comment)"
    cr open ":$PORT/" >/dev/null
    cr console >/dev/null
    cr annotate on >/dev/null
    check "annotate on: the overlay is in the page" test "$(cr eval 'document.querySelector("rade-annotate") !== null')" = true
    SV=$(ref_of 'button "Save"' --engine chromium)
    cr click "$SV" >/dev/null # lands on the overlay: picks the button, opens the comment box
    for k in o k Space g r e e n; do cr press "$k" >/dev/null; done
    cr press Enter >/dev/null
    check "an annotation from a real click + typing" eventually 5 is "rade browser --engine chromium annotations --json | jq -r '.annotations[0].comment'" "ok green"
    A=$(cr annotations --json)
    jq -c '.annotations[0] | del(.console)' <<<"$A" | sed 's/^/    /'
    check "selector, ref, role from the same library" test "$(jq -r '.annotations[0] | [.selector, .role, .name] | @tsv' <<<"$A")" = "$(printf '[data-testid="save"]\tbutton\tSave')"
    check "the page didn't get that click (overlay on)" test "$(cr eval "document.getElementById('status').textContent")" = "Not saved"
    CS=$(jq -r '.annotations[0].shot' <<<"$A")
    check "crop cut by Chromium: the button's box + 8 px" python3 - "$CS" "$(jq -r '.annotations[0].bbox | "\(.w) \(.h)"' <<<"$A")" <<'PY'
import struct, sys
b = open(sys.argv[1], 'rb').read()
w, h = struct.unpack('>II', b[16:24])
bw, bh = (float(x) for x in sys.argv[2].split())
assert abs(w - (bw + 16)) <= 2 and abs(h - (bh + 16)) <= 2, (w, h, bw, bh)
PY
    check "console errors kept" jq -e '.annotations[0].console | any(test("price is undefined"))' <<<"$A"
    cr annotate off >/dev/null
    check "off: overlay gone, page clicks reach the page" bash -c "[[ \$(rade browser --engine chromium eval 'document.querySelector(\"rade-annotate\") === null') == true ]] && rade browser --engine chromium click $SV >/dev/null && [[ \$(rade browser --engine chromium eval \"document.getElementById('status').textContent\") == Saved ]]"
    check "chromium --to a plain Terminal is refused (acceptance H1)" bash -c "[[ \$(set +e; rade browser --engine chromium annotations --send --to $srv 2>&1; echo \" rc=\$?\") == *'not an Agent Pane'*'rc=2' ]]"
    : >"$RECV"
    r=$(cr annotations --send --json)
    check "send: to the Agent that opened Chromium" test "$(jq -r '[.pane, .annotations] | @tsv' <<<"$r")" = "$(printf '%s\t1' "$agent")"
    check "… its prompt says Chromium and the engine flag" eventually 5 bash -c "grep -qF 'made in Chromium' '$RECV' && grep -qF 'rade browser --engine chromium click' '$RECV' && grep -qF 'comment: ok green' '$RECV'"
    cr open ":$PORT/hostile.html" >/dev/null
    check "the page's world has no binding (only its own fake)" test "$(cr eval 'String(window.radeAnnotate).includes("pwned")')" = true
    cr eval 'window.radeAnnotate(JSON.stringify({op: "add", pick: {comment: "forged", url: location.href}}))' >/dev/null 2>&1 || true
    sleep 0.5
    check "a page can't add annotations (the binding is RADE's world's)" test "$(cr annotations --json | jq '.annotations | length')" = 0

    echo "== R72.3: close: Chromium, its profile and the socket go"
    cr close >/dev/null
    check "closed: socket, process and profile gone" eventually 10 bash -c "[[ ! -e '$CRSOCK' && ! -d '$PROFILE' ]] && ! kill -0 $CRPID 2>/dev/null"

    echo "== R72.3: SIGTERM to the daemon (a logout) cleans up too (acceptance M6)"
    r=$(cr open ":$PORT/" --json)
    DPID=$(jq -r .chromium.daemon <<<"$r"); PROFILE=$(jq -r .chromium.profile <<<"$r")
    kill -TERM "$DPID"
    check "socket and profile gone after SIGTERM" eventually 10 bash -c "[[ ! -e '$CRSOCK' && ! -d '$PROFILE' ]]"

    echo "== R72.3: --headed: a real window with the annotation toolbar"
    if r=$(RADE_PANE_ID=$agent cr open ":$PORT/" --headed --json 2>"$T/headed.err"); then
        check "headed: no --headless" bash -c "! tr '\0' ' ' </proc/$(jq -r .chromium.pid <<<"$r")/cmdline | grep -q -- '--headless'"
        check "headed: the toolbar is there while annotate is off" eventually 5 is "rade browser --engine chromium eval 'document.querySelector(\"rade-annotate\") !== null'" true
        cr screenshot "$OUT/chromium-headed.png" >/dev/null
        cr close >/dev/null
        eventually 10 test ! -e "$CRSOCK"
    else
        echo "  SKIP headed: Chromium could not open a window here (headless Weston has no seat; the live suite checks it): $(tail -1 "$T/headed.err")"
    fi
fi

stop_app
kill "${SERVER_PID:-}" 2>/dev/null || true

echo
[[ -n ${SKIPPED_CHROMIUM:-} ]] && echo "(the Chromium engine was skipped: no Chromium)"
echo "M24 E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
[[ $FAIL == 0 ]]
