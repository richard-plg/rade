# M12 live (PRD R18): the Browser Pane on the real desktop. Ctrl+Shift+O with
# the real keyboard opens a Browser on the dev server's port (started in a
# Terminal), Ctrl+L + real typing in the URL bar + Enter navigates, a real
# mouse click on a page button changes the page, Alt+Left goes back, and one
# short real Agent turn (agy, in a temp Source) uses `rade browser` to read
# the page title (M12_NO_AGENT=1 skips that turn). Everything (site, Source, browser data) lives in the
# run's temp dir.
S=$T/files/m12site
mkdir -p "$S"
cat >"$S/index.html" <<'HTML'
<!doctype html><html><head><meta charset="utf-8"><title>RADE live page</title>
<style>body{font:16px sans-serif;margin:32px} button{font-size:18px;padding:10px 24px}</style></head>
<body><h1>Live check</h1>
<button id="b" onclick="document.getElementById('o').textContent='clicked by a real mouse'">Click me</button>
<p id="o">not clicked</p><a href="page2.html">Second page</a></body></html>
HTML
printf '<!doctype html><title>Page two</title><h1>Second</h1>\n' >"$S/page2.html"
git -C "$S" init -q
rade source add "$S" --name m12 >/dev/null
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
burl() { rade browser eval 'location.href' 2>/dev/null; }
btitle() { rade browser eval 'document.title' 2>/dev/null; }

key "alt+$(proj_index m12)"
want "M12 Alt+N → Source m12" 3 "state | jq -r .active_source" "$(rade source list --json | jq -r '.sources[] | select(.name == "m12") | .source')"
SRV=$(rade spawn --cmd "python3 -m http.server $PORT --bind 127.0.0.1 -d $S" --source m12 --name devserver --json | jq -r .pane)
eventually 10 curl -sf "http://127.0.0.1:$PORT/" || bad "M12 dev server did not start"
want "M12 R18.4 rade browser ports: the dev server's port in its Pane" 5 "rade browser ports --json | jq -r --arg p $SRV '.ports[] | select(.pane == \$p) | .port'" "$PORT"
rade focus "$SRV" >/dev/null
sleep 0.3

echo "-- M12-1 Ctrl+Shift+O (real keys) → Browser on the port"
key ctrl+shift+o
want "M12-1 a Browser opens beside the server on http://localhost:$PORT/" 10 burl "http://localhost:$PORT/"
want "M12-1 page title" 5 btitle "RADE live page"
B=$(cur_pane)
want "M12-1 the Browser has focus" 3 "q 'tab | .layout | .. | objects | select(.kind?.type? == \"browser\") | .id'" "$B"
sleep 0.5
shot m12-browser-opened

echo "-- M12-2 R18-b URL bar: Ctrl+L, real typing, Enter"
key ctrl+l
want "M12-2 Ctrl+L focuses the URL bar" 3 "rade debug window --json | jq -r '.focus.widget'" GtkText
type_text "localhost:$PORT/page2.html"
key enter
want "M12-2 typed URL loads" 10 burl "http://localhost:$PORT/page2.html"
want "M12-2 its title" 5 btitle "Page two"
key alt+left
want "M12-2 Alt+Left (real) → back" 10 btitle "RADE live page"

echo "-- M12-3 R18-b real mouse click on a page button"
# Page CSS px = window px at zoom 1: the WebView's bounds + the button's rect.
rect=$(rade browser eval 'const r = document.getElementById("b").getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]' --json | jq -r '.result | "\(.[0]) \(.[1])"')
read -r bx by <<<"$rect"
read -r wx wy <<<"$(rade debug window --widget rade-browser-web --json | jq -r '.widget | "\(.x) \(.y)"')"
x=$(python3 -c "print(round($wx + $bx))"); y=$(python3 -c "print(round($wy + $by))")
if click "$x" "$y"; then
    want "M12-3 the page handled a real click" 3 "rade browser eval \"document.getElementById('o').textContent\"" "clicked by a real mouse"
    shot m12-browser-clicked
else
    bad "M12-3 real click (pointer not placed at $x,$y)"
fi

echo "-- M12-4 a real Agent (agy) reads the page title through rade browser"
if [[ -n ${M12_NO_AGENT:-} ]]; then
    na "M12-4 skipped (M12_NO_AGENT=1: one real Agent turn per session is enough)"
elif command -v agy >/dev/null; then
    prompt="Run the shell command: rade browser snapshot   Then reply with exactly one line TITLE= followed by the page title shown in its output. Do nothing else."
    AG=$(rade spawn --cmd "agy --dangerously-skip-permissions -i $(printf '%q' "$prompt")" --source m12 --name agy-m12 --split right --json | jq -r .pane)
    if eventually 240 screen_has "$AG" "TITLE=RADE live page"; then
        ok "M12-4 agy ran rade browser and reported TITLE=RADE live page"
    else
        bad "M12-4 agy did not report the title (screen: $(rade read "$AG" | tail -5 | tr '\n' '|'))"
    fi
    shot m12-agent
    rade close "$AG" --force >/dev/null
else
    na "M12-4 agy not installed"
fi
rade close "$SRV" --force >/dev/null
