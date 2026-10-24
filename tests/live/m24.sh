# M24 live (R71, R72): Browser Annotations on the real desktop. Ctrl+Shift+A
# (real keys) in a Browser Pane turns annotate mode on; a real mouse click on
# a page button and real typing + Enter add an annotation; a real drag over a
# region and typing add a second; a real click on the panel's Send to Agent
# pastes one prompt into a stand-in Agent beside the Browser. Then a headed
# Chromium (`rade browser --engine chromium --headed`, a temp profile) shows
# its annotation toolbar and takes an annotation through trusted CDP input
# (real input stays on RADE's window). Screenshots light and dark on the real
# GPU. Run it with a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m24
LV=$T/files/ws-m24
mkdir -p "$LV/site" "$LV/.rade"
git -C "$LV/site" init -q
cat >"$LV/site/index.html" <<'HTML'
<!doctype html><html><head><meta charset="utf-8"><title>Annotate live</title>
<style>body{font:16px sans-serif;margin:32px} .hero{padding:20px;background:#e8f0fe;border-radius:8px;width:460px}
button{font-size:18px;padding:10px 24px;margin-top:20px}</style></head>
<body><div class="hero" id="hero"><h1>Pricing</h1><p>Plans for every team size.</p></div>
<button id="save" data-testid="save" onclick="document.getElementById('o').textContent='saved'">Save</button>
<p id="o">not saved</p>
<script>console.error('TypeError: price is undefined')</script></body></html>
HTML
cat >"$LV/.rade/workspace.toml" <<EOF
name = "LiveA"
[agents.sink]
command = "bash $T/sink.sh"
EOF
rade workspace add "$LV" >/dev/null
rade source add "$LV/site" --name m24 >/dev/null
rade workspace switch livea >/dev/null
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
SRV=$(rade spawn --cmd "python3 -m http.server $PORT --bind 127.0.0.1 -d $LV/site" --source m24 --name devserver --tab new --json | jq -r .pane)
eventually 10 curl -sf "http://127.0.0.1:$PORT/" || bad "M24 dev server did not start"
AG=$(rade spawn --agent sink --source m24 --name agent --json | jq -r .pane)
eventually 10 screen_has "$AG" 'SINK READY' || bad "M24 sink Agent did not start"
B=$(RADE_PANE_ID=$AG rade browser open ":$PORT/" --json | jq -r .pane)
rpc_b() { python3 - "$RADE_SOCKET" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.browser", "params": json.loads(sys.argv[2])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
bstate() { rpc_b "{\"pane\":\"$B\",\"op\":\"state\"}"; }
# A page element's centre in window coordinates (CSS px = window px at zoom 1).
el_xy() {
    local r w
    r=$(rade browser eval "const r = document.querySelector('$1').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2, r.x, r.y, r.width, r.height]" --json | jq -r '.result | map(tostring) | join(" ")')
    w=$(rade debug window --widget rade-browser-web --json | jq -r '.widget | "\(.x) \(.y)"')
    python3 -c "import sys; r=[float(x) for x in '$r'.split()]; w=[float(x) for x in '$w'.split()]; print(round(w[0]+r[0]), round(w[1]+r[1]), round(w[0]+r[2]), round(w[1]+r[3]), round(r[4]), round(r[5]))"
}

echo "-- M24-1 R71 Ctrl+Shift+A (real keys) in the Browser → annotate mode"
rade focus "$B" >/dev/null
sleep 0.5
key ctrl+shift+a
want "M24-1 annotate mode on, the panel shown" 3 "bstate | jq -r '[.annotating, .panel] | @tsv'" "$(printf 'true\ttrue')"
want "M24-1 the overlay is in the page" 3 "rade browser eval 'document.querySelector(\"rade-annotate\") !== null'" true

echo "-- M24-2 R71-c a real click on the Save button, real typing, Enter"
read -r sx sy _ <<<"$(el_xy '#save')"
if click "$sx" "$sy"; then
    sleep 0.4
    shot m24-comment-box
    type_text "Make it green"
    key enter
    want "M24-2 annotation 1 on the button" 5 "rade browser annotations --json | jq -r '.annotations[0] | \"\(.selector) \(.comment)\"'" '[data-testid="save"] Make it green'
    want "M24-2 the page did not get the click" 2 "rade browser eval \"document.getElementById('o').textContent\"" "not saved"
else
    bad "M24-2 real click (pointer not placed at $sx,$sy)"
fi

echo "-- M24-3 R71-c a real drag over the hero, typing, Enter"
read -r _ _ hx hy hw hh <<<"$(el_xy '#hero')"
if drag "$((hx + 10))" "$((hy + 10))" "$((hx + 210))" "$((hy + 60))"; then
    sleep 0.4
    type_text "Less padding here"
    key enter
    want "M24-3 annotation 2 is a region anchored on #hero" 5 "rade browser annotations --json | jq -r '.annotations[1] | \"\(.kind) \(.selector) \(.comment)\"'" "region #hero Less padding here"
    want "M24-3 its crop exists" 3 "test -s \"\$(rade browser annotations --json | jq -r '.annotations[1].shot')\" && echo yes" yes
else
    bad "M24-3 real drag"
fi
sleep 0.5
shot m24-annotations-light
rade debug action color_scheme_dark >/dev/null
sleep 0.6
shot m24-annotations-dark
rade debug action color_scheme_light >/dev/null

echo "-- M24-4 R71-c a real click on Send to Agent → one prompt in the Agent"
read -r px py <<<"$(widget_xy rade-annotation-send)"
if click "$px" "$py"; then
    want "M24-4 the Agent got both annotations in one prompt" 5 "sink_text $AG | grep -q 'comment: Make it green.*comment: Less padding here' && echo yes" yes
    want "M24-4 with the crop path and the console error" 3 "sink_text $AG | grep -q 'screenshot: /.*price is undefined' && echo yes" yes
    want "M24-4 cleared, the panel says where" 3 "bstate | jq -r .note" "Sent 2 to agent ($AG)"
else
    bad "M24-4 real click on Send"
fi
key ctrl+shift+a
want "M24-4 Ctrl+Shift+A again: off, no overlay" 3 "rade browser eval 'document.querySelector(\"rade-annotate\") === null'" true

echo "-- M24-5 R72 a headed Chromium with the annotation toolbar"
RH=$(getent passwd "$(id -un)" | cut -d: -f6)
CHR=$(ls -d "$RH"/.cache/ms-playwright/chromium-*/chrome-linux64/chrome 2>/dev/null | tail -1)
if [[ -x $CHR ]]; then
    printf '\n[browser]\nchromium = "%s"\n' "$CHR" >>"$CONFIG"
    cr() { rade browser --engine chromium "$@"; }
    if r=$(RADE_PANE_ID=$AG cr open ":$PORT/" --headed --json 2>"$T/headed.err"); then
        sleep 1 # its window takes the focus: no real input until it is closed
        want "M24-5 a headed Chromium (no --headless), temp profile" 3 "tr '\\0' ' ' </proc/$(jq -r .chromium.pid <<<"$r")/cmdline | grep -c -- '--headless'" 0
        want "M24-5 its toolbar is in the page while annotate is off" 5 "cr eval 'document.querySelector(\"rade-annotate\") !== null'" true
        cr annotate on >/dev/null
        cr click "$(cr snapshot | grep 'button "Save"' | sed -E 's/.*\[ref=(e[0-9]+)\].*/\1/')" >/dev/null
        for k in o k Enter; do cr press "$k" >/dev/null; done
        want "M24-5 an annotation from trusted input in the headed window" 5 "cr annotations --json | jq -r '.annotations[0].selector'" '[data-testid="save"]'
        cr screenshot "$OUT/m24-chromium-headed.png" >/dev/null && echo "  shot m24-chromium-headed.png"
        cr annotations --send >/dev/null
        want "M24-5 sent to the Agent that opened it" 5 "sink_text $AG | grep -q 'made in Chromium' && echo yes" yes
        cr close >/dev/null
        rade debug action present >/dev/null 2>&1 || true
        want "M24-5 closed: its profile is gone" 10 "test -d '$(jq -r .chromium.profile <<<"$r")' && echo still || echo gone" gone
    else
        bad "M24-5 headed Chromium did not start: $(tail -1 "$T/headed.err")"
    fi
else
    na "M24-5 no Playwright Chromium on this machine"
fi
rade close "$SRV" --force >/dev/null 2>&1 || true
