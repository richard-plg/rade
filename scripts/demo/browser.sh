#!/usr/bin/env bash
# Demo scene: the Browser Pane and Browser Annotations — a storefront page
# served by the dev server, a Claude Agent beside it, two picks (an element
# and a region) with notes, Send to Agent and the Agent acting on them.
# Stills: browser, annotate. Recording: annotate (.mp4 + .gif, ≤ 25 s).
# Usage: NO_BUILD=1 scripts/demo/browser.sh   (KEEP=1 keeps the temp dir)
set -uo pipefail
NAME=browser
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
PORT=5173
T0=$(date +%s)

# ---- The page: written into shop-web/preview like an Agent would ------------
mkdir -p "$WEB/preview"
cat >"$WEB/preview/index.html" <<'HTML'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Acme Supply</title>
  <link rel="stylesheet" href="styles.css">
</head>
<body>
  <header class="top">
    <a class="logo" href="#">acme<span>supply</span></a>
    <nav><a href="#">Shop</a><a href="#">Mugs</a><a href="#">Apparel</a><a href="#">About</a></nav>
    <a class="cart" href="#">Cart</a>
  </header>
  <section class="hero" id="hero">
    <p class="eyebrow">Autumn collection</p>
    <h1>Gear for people who ship.</h1>
    <p class="lead">Small-batch mugs, tees and caps. Free shipping over $50.</p>
  </section>
  <main class="grid">
    <article class="card">
      <div class="art mug"></div>
      <h3>Enamel mug</h3>
      <p class="meta">350 ml · speckled white</p>
      <p class="price">$18.00</p>
      <button class="add">Add to cart</button>
    </article>
    <article class="card">
      <div class="art tee"></div>
      <h3>Logo tee</h3>
      <p class="meta">Organic cotton · 4 colors</p>
      <p class="price">$26.00</p>
      <button class="add">Add to cart</button>
    </article>
    <article class="card">
      <div class="art cap"></div>
      <h3>Field cap</h3>
      <p class="meta">Waxed canvas · one size</p>
      <p class="price">$22.00</p>
      <button class="add">Add to cart</button>
    </article>
  </main>
  <footer>Shipping in 3–5 days · 30-day returns</footer>
</body>
</html>
HTML
cat >"$WEB/preview/styles.css" <<'CSS'
:root { --paper: #fbfaf7; --ink: #14201f; --muted: #66726f; --line: #e3e0d8; --card: #ffffff; --brand: #0f766e; }
@media (prefers-color-scheme: dark) {
  :root { --paper: #12181a; --ink: #e8eeec; --muted: #8b9a96; --line: #26302f; --card: #192123; }
}
* { box-sizing: border-box; }
body { margin: 0; font: 15px/1.5 Inter, system-ui, sans-serif; background: var(--paper); color: var(--ink); }
.top { display: flex; align-items: center; gap: 24px; padding: 14px 28px; border-bottom: 1px solid var(--line); }
.top a { color: inherit; text-decoration: none; }
.logo { font-weight: 700; font-size: 18px; letter-spacing: -0.02em; }
.logo span { color: var(--brand); }
.top nav { display: flex; gap: 18px; flex: 1; color: var(--muted); }
.cart { font-weight: 600; }
.hero { padding: 72px 32px; background: linear-gradient(135deg, #0f766e, #134e4a); color: #f0fdfa; }
.eyebrow { margin: 0 0 8px; text-transform: uppercase; letter-spacing: .12em; font-size: 12px; color: #99f6e4; }
.hero h1 { margin: 0 0 10px; font-size: 38px; line-height: 1.1; letter-spacing: -0.03em; }
.lead { margin: 0; max-width: 420px; color: #ccfbf1; }
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; padding: 28px; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 14px; }
.card h3 { margin: 12px 0 0; font-size: 16px; }
.meta { margin: 2px 0 8px; color: var(--muted); font-size: 13px; }
.price { margin: 0 0 12px; font-weight: 600; }
.art { height: 120px; border-radius: 8px; position: relative; background: #e6efe9; }
.art::after { content: ""; position: absolute; inset: 0; margin: auto; }
.mug::after { width: 56px; height: 64px; background: #f7f3ea; border-radius: 6px 6px 14px 14px; box-shadow: 18px 0 0 -10px #f7f3ea, inset 0 -10px 0 #0f766e; }
.tee { background: #e8e6f2; }
.tee::after { width: 84px; height: 80px; background: #3b4a54; clip-path: polygon(0 18%, 28% 0, 40% 10%, 60% 10%, 72% 0, 100% 18%, 86% 40%, 76% 34%, 76% 100%, 24% 100%, 24% 34%, 14% 40%); }
.cap { background: #f1e8dc; }
.cap::after { width: 84px; height: 44px; background: #b4572f; border-radius: 42px 42px 6px 6px; box-shadow: 14px 22px 0 -8px #8f4222; }
.add { padding: 6px 12px; background: #3b4a54; color: #fff; border: 0; border-radius: 6px; font: inherit; font-size: 13px; cursor: pointer; }
footer { padding: 18px 28px; color: var(--muted); font-size: 13px; border-top: 1px solid var(--line); }
CSS
# The play edits these two lines; keep them exactly as the play expects.
grep -q '^\.hero { padding: 72px 32px;' "$WEB/preview/styles.css" || { echo "styles drifted from 70-annotate.play"; exit 1; }
grep -q '^\.add { padding: 6px 12px; background: #3b4a54;' "$WEB/preview/styles.css" || { echo "styles drifted from 70-annotate.play"; exit 1; }

# ---- An Agent whose prompt line stays readable ---------------------------------
# Send to Agent pastes ~25 lines (with cache paths) into the Terminal. This
# wrapper sits between the Pane and the scripted Agent: while the scene's
# flag file $T/noecho exists the Terminal does not echo input, and the pasted
# block of annotations shows as a label (like a real agent's "[Pasted text]"
# chip). Typed prompts are echoed as usual.
cat >"$T/agent-wrap.py" <<'PY'
import os, re, select, subprocess, sys, termios, threading, time
flag = os.path.join(os.path.dirname(os.path.abspath(__file__)), "noecho")
tty = open("/dev/tty", "wb", buffering=0)
def echo(on):
    a = termios.tcgetattr(0)
    a[3] = (a[3] | termios.ECHO) if on else (a[3] & ~termios.ECHO)
    termios.tcsetattr(0, termios.TCSANOW, a)
def watch():
    while True:
        if os.path.exists(flag):
            echo(False)
        time.sleep(0.02)
threading.Thread(target=watch, daemon=True).start()
p = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def pump():
    while True:
        d = os.read(p.stdout.fileno(), 65536)
        if not d:
            os._exit(0)
        tty.write(d)
threading.Thread(target=pump, daemon=True).start()
while True:
    buf = os.read(0, 65536)
    if not buf:
        break
    while select.select([0], [], [], 0.3)[0]:
        m = os.read(0, 65536)
        if not m:
            break
        buf += m
    text = buf.decode(errors="replace")
    if text.startswith("Browser annotations"):
        try: os.remove(flag)
        except OSError: pass
        echo(True)
        n = len(re.findall(r"^\d+\. ", text, re.M))
        tty.write(b"\x1b[2m[Browser annotations \xc2\xb7 %d picks]\x1b[0m" % n)
    p.stdin.write(text.replace("\n", " ").rstrip().encode() + b"\n")
    p.stdin.flush()
p.stdin.close()
p.wait()
PY
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' DEMO_SPEED=0.75 python3 $T/agent-wrap.py $AGENT"
C

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade workspace switch acme >/dev/null

echo "== dev server, Agent, Browser"
dev=$(rade spawn --source shop-web --name dev-server --tab new --cmd "cd $WEB/preview && $DEMO/devserver.sh $PORT" --json | id)
eventually 15 curl -sf "http://127.0.0.1:$PORT/" || { echo "dev server did not start"; exit 1; }
agent=$(rade spawn --source shop-web --agent claude --name storefront --tab new "Build the storefront preview page" --json | id)
eventually 30 is "pane_status $agent" done
rade focus "$agent" >/dev/null
b=$(RADE_PANE_ID=$agent rade browser open ":$PORT/" --json | jq -r .pane)
sleep 2
rade focus "$agent" >/dev/null
dbg() { rpc debug.browser "{\"pane\":\"$b\",$1}"; }
ref_of() { rade browser snapshot | grep -E -- "$1" | head -1 | sed -E 's/.*\[ref=(e[0-9]+)\].*/\1/'; }
notoasts
still browser

echo "== annotate: region + element"
pick_both() {
    dbg '"op":"toggle"' >/dev/null
    sleep 1.2
    dbg '"op":"pick","x":10,"y":62,"w":550,"h":235,"comment":"Hero takes too much room. Tighten the padding."' >/dev/null
    sleep 1.5
    REF=$(ref_of 'button "Add to cart"')
    dbg "\"op\":\"pick\",\"ref\":\"$REF\",\"comment\":\"Too small. Make Add to cart bolder and full width.\"" >/dev/null
    sleep 1.5
}
pick_both
notoasts
still annotate

echo "== recording: annotate → Send to Agent → the Agent edits → reload"
dbg '"op":"clear"' >/dev/null
dbg '"op":"toggle"' >/dev/null
rade focus "$agent" >/dev/null
sleep 1
rec_start annotate
sleep 1.5
pick_both
touch "$T/noecho"; sleep 0.2
dbg '"op":"send"' >/dev/null
eventually 40 is "pane_status $agent" running
eventually 60 is "pane_status $agent" done
sleep 3
rec_stop
echo "total $(( $(date +%s) - T0 )) s"
