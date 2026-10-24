#!/usr/bin/env bash
# M40 end-to-end tests (ADR-0006): Hardening (PRD R111).
# Part 1 (Group 1, performance): R111-e a 1 GB folder of Codex rollouts (first
# scan within 10 s while the main loop answers a 50 ms probe; the next tail reads
# only the appended bytes) · R111-d 20 accounts with 90 days of samples (the Usage
# page and the + menu drawn within 100 / 50 ms) · R111-c 10 Projects × 5 links ×
# 2 Worktrees × 1 Agent = 260 Navigator rows (100 status changes, row update p95
# ≤ 16 ms; an end-to-end scroll, frame p95 ≤ 16.7 ms). Timings come from
# `debug.ui perf` (crates/rade-app/src/perf.rs, test mode).
# Part 2 (Group 2, layouts): scenes A–F at 1280 × 800 and at text scale 200 %.
# Usage: tests/e2e/m40.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp
#        dir, PART=1|2 to run one part, ONLY="c d" to run only those ACs of part 1,
#        MODES="1280" or "200" to run one layout of part 2, HOLD=<s> to keep the app
#        up after part 2's scenes with its env in target/e2e-m40/env.sh)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m40
export WESTON_WIDTH=1440
source "$HERE/lib.sh"

export XDG_STATE_HOME=$T/state
mkdir -p "$XDG_STATE_HOME"
USAGE=$XDG_STATE_HOME/rade-dev/usage
# Agents of the big scene: an idle process (100 of them; no hooks, no output).
IDLE=$T/idle-agent.sh
printf '#!/bin/sh\necho idle agent\nexec sleep 1000000\n' >"$IDLE" && chmod +x "$IDLE"
mkdir -p "$XDG_CONFIG_HOME/rade"
printf 'idle_seconds = 3600\n\n[agents.claude]\ncommand = "%s"\n\n[agents.codex]\ncommand = "%s"\n' "$IDLE" "$IDLE" \
    >"$XDG_CONFIG_HOME/rade/config.toml"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
ui() { rpc debug.ui "{\"op\":\"$1\"${2:+,\"arg\":\"$2\"}}"; }
perf() { ui perf "${1:-}"; }
# `<name>` field `<f>` (p95, n, …) of the perf summary.
pf() { perf | jq -r --arg n "$1" --arg f "$2" '.samples[$n][$f] // "none"'; }
has_sample() { [[ $(pf "$1" n) != none ]]; }
le() { python3 -c 'import sys; a, b = sys.argv[1:]; sys.exit(0 if a != "none" and float(a) <= float(b) else 1)' "$1" "$2"; }
want() { [[ -z ${ONLY:-} || " $ONLY " == *" $1 "* ]]; }
sha8() { printf %s "$(realpath "$1")" | sha256sum | cut -c1-8; }
export -f rpc

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null

if [[ ${PART:-} != 2 ]]; then # ---- Part 1 (Group 1): performance -----------------------
if want e; then
echo "== R111-e a 1 GB folder of Codex rollouts"
free_kb=$(df -Pk "$T" | awk 'NR==2 {print $4}')
if ((free_kb < 3 * 1024 * 1024)); then
    bad "R111-e needs 3 GB free under $T (has $((free_kb / 1024)) MB)"
else
    BIG=$HOME/workspaces/big
    mkdir -p "$BIG/.rade" "$HOME/.codex-big"
    printf 'name = "big"\n\n[[account]]\nagent = "codex"\nname = "big"\nconfig_dir = "~/.codex-big"\ndefault = true\n' \
        >"$BIG/.rade/workspace.toml"
    # 64 files × 16 MiB: conversation lines with a token_count every 200 lines.
    python3 - "$HOME/.codex-big/sessions/2026/10/23" <<'PY'
import json, os, sys
d = sys.argv[1]; os.makedirs(d, exist_ok=True)
chat = json.dumps({"timestamp": "2026-10-23T09:00:02.000Z", "type": "response_item",
                   "payload": {"type": "message", "role": "assistant",
                               "content": [{"type": "output_text", "text": "lorem ipsum " * 400}]}}) + "\n"
def tc(i):
    return json.dumps({"timestamp": "2026-10-23T09:01:00.000Z", "type": "event_msg", "payload": {
        "type": "token_count", "info": {"total_token_usage": {"input_tokens": 1000 * i, "cached_input_tokens": 0,
                                                               "output_tokens": 10 * i}},
        "rate_limits": {"limit_id": "codex", "primary": {"used_percent": 12.0, "window_minutes": 300,
                                                          "resets_at": 1893456000}, "secondary": None}}}) + "\n"
meta = json.dumps({"timestamp": "2026-10-23T09:00:00.000Z", "type": "session_meta",
                   "payload": {"cwd": "/tmp/big", "model": "gpt-big"}}) + "\n"
block = "".join(chat if j else tc(1) for j in range(200)).encode()
for f in range(64):
    with open(os.path.join(d, f"rollout-big-{f:02}.jsonl"), "wb") as w:
        w.write(meta.encode())
        while w.tell() < 16 << 20:
            w.write(block)
PY
    total=$(du -cb "$HOME/.codex-big/sessions/2026/10/23"/*.jsonl | tail -1 | cut -f1)
    check "R111-e the folder holds ≥ 1 GiB of rollouts ($total bytes)" test "$total" -ge $((1 << 30))
    rade workspace add "$BIG" >/dev/null
    # The main loop answers pane.list every 50 ms while the first scan runs.
    python3 - "$XDG_RUNTIME_DIR/rade/rade-dev.sock" "$T/probe.stop" >"$T/probe.out" <<'PY' &
import json, os, socket, sys, time
sock, stop = sys.argv[1:]
worst, n = 0.0, 0
while not os.path.exists(stop):
    t = time.monotonic()
    s = socket.socket(socket.AF_UNIX); s.connect(sock)
    s.sendall(b'{"jsonrpc":"2.0","id":1,"method":"pane.list","params":{}}\n')
    buf = b""
    while not buf.endswith(b"\n"):
        buf += s.recv(65536)
    s.close()
    worst = max(worst, (time.monotonic() - t) * 1000); n += 1
    time.sleep(0.05)
print(f"{n} {worst:.1f}")
PY
    PROBE=$!
    sleep 0.3
    t0=$(date +%s%N)
    rpc usage.refresh '{"workspace":"big","account":"big"}' >"$T/refresh1.json"
    ms=$((($(date +%s%N) - t0) / 1000000))
    touch "$T/probe.stop"; wait "$PROBE" || true
    read -r probes worst <"$T/probe.out"
    read1=$(jq -r '.accounts[0].read_bytes' "$T/refresh1.json")
    echo "  first scan: $ms ms, read $read1 bytes; probe: $probes answers, slowest $worst ms"
    check "R111-e the first scan reads the whole folder ($read1 bytes) within 10 s ($ms ms)" \
        bash -c "[[ $read1 -ge $total && $ms -le 10000 ]]"
    check "R111-e the main loop answered every 50 ms probe during the scan ($probes answers, slowest $worst ms)" \
        bash -c "[[ $probes -ge $((ms / 100)) ]] && python3 -c 'import sys; sys.exit(0 if $worst <= 50 else 1)'"
    # 10 lines appended to one file: the next tail reads exactly them.
    F=$HOME/.codex-big/sessions/2026/10/23/rollout-big-07.jsonl
    before=$(stat -c %s "$F")
    for i in $(seq 10); do head -c 200 /dev/urandom | base64 -w0 | jq -Rc '{type:"response_item",payload:{text:.}}'; done >>"$F"
    added=$(($(stat -c %s "$F") - before))
    rpc usage.refresh '{"workspace":"big","account":"big"}' >"$T/refresh2.json"
    read2=$(jq -r '.accounts[0].read_bytes' "$T/refresh2.json")
    check "R111-e the next tail reads only the 10 appended lines ($read2 of $added bytes)" test "$read2" = "$added"
    rade workspace remove big >/dev/null 2>&1 || true
    rm -rf "$HOME/.codex-big"
fi
fi

if want d; then
echo "== R111-d 20 accounts with 90 days of samples"
W20=$HOME/workspaces/many
mkdir -p "$W20/.rade"
{
    echo 'name = "many"'
    for i in $(seq -w 1 20); do
        if ((10#$i <= 10)); then agent=claude; else agent=codex; fi
        mkdir -p "$HOME/.$agent-$i"
        printf '\n[[account]]\nagent = "%s"\nname = "acct-%s"\nconfig_dir = "~/.%s-%s"\n' "$agent" "$i" "$agent" "$i"
        [[ $i == 01 || $i == 11 ]] && echo 'default = true'
    done
} >"$W20/.rade/workspace.toml"
# A sample of each window every 15 minutes for 90 days, tokens per day ×
# 3 Projects × 2 models (the store as the app writes it, docs/API.md).
python3 - "$USAGE" "$HOME" <<'PY'
import hashlib, json, os, sys, time
usage, home = sys.argv[1:]; os.makedirs(usage, exist_ok=True)
now = int(time.time())
for i in range(1, 21):
    agent = "claude" if i <= 10 else "codex"
    real = os.path.realpath(os.path.join(home, f".{agent}-{i:02}"))
    path = os.path.join(usage, f"{agent}-{hashlib.sha256(real.encode()).hexdigest()[:8]}.jsonl")
    name = f"acct-{i:02}"
    ids = [("five_hour", "Session", "5-hour", 300), ("seven_day", "Weekly", "weekly", 10080)] if agent == "claude" \
        else [("codex", "Codex", "5-hour", 300), ("codex", "Codex", "weekly", 10080)]
    with open(path, "w") as f:
        t = now - 89 * 86400  # 90 days kept: nothing to prune during the run
        while t <= now - 60:
            for lid, group, nm, win in ids:
                period = win * 60
                used = round(100 * ((t % period) / period) * (0.4 + 0.05 * (i % 10)), 1)
                f.write(json.dumps({"kind": "limit", "account": name, "limit_id": lid, "group": group, "name": nm,
                                    "window_minutes": win, "used_percent": used,
                                    "resets_at": t - t % period + period, "source": "statusline", "t": t}) + "\n")
            t += 900
        for d in range(89):
            ts = now - d * 86400
            day = time.strftime("%Y-%m-%d", time.localtime(ts))
            for p in ("Shop", "Billing", "Scratch"):
                for m in ("opus", "sonnet"):
                    f.write(json.dumps({"kind": "tokens", "t": ts, "day": day, "project": p, "model": m,
                                        "input": 1000 + d, "output": 100, "cache_creation": 0, "cache_read": 0}) + "\n")
PY
check "R111-d 20 stores of 90 days written" bash -c "[[ \$(ls '$USAGE'/*.jsonl | wc -l) -ge 20 ]]"
rade workspace add "$W20" >/dev/null
rade workspace switch many >/dev/null
check "R111-d the Workspace declares 20 accounts" \
    bash -c "[[ \$(rade account list --workspace many --json | jq '.accounts | length') == 20 ]]"
# Both opened once per launch: the first launch compiles the renderer's
# shaders (llvmpipe: ~250 ms, cached on disk by Mesa like on any machine),
# so the budgets are checked on the next launch, every RADE cache cold.
open_both() { # <label>
    perf reset >/dev/null
    perf usage >/dev/null
    eventually 10 has_sample usage_page || true
    rows=$(ui usage | jq '.usage.rows | length' 2>/dev/null || echo 0)
    up=$(pf usage_page max)
    echo "  $1 Usage page: first frame with content after $up ms ($rows rows; load $(pf usage_load max) ms, build $(pf usage_build max) ms, frame $(pf usage_page:frame max) ms)"
    shot "r111-d-usage-$1"
    ui usage close >/dev/null
    perf new_menu >/dev/null
    eventually 5 has_sample new_menu || true
    nm=$(pf new_menu max)
    echo "  $1 + menu: first frame after $nm ms (rows computed in $(pf new_menu_compute max) ms, shown in $(pf new_menu_show max) ms, frame $(pf new_menu:frame max) ms)"
    shot "r111-d-new-menu-$1"
    rpc debug.ui '{"op":"new_menu","arg":"close"}' >/dev/null
}
open_both first-launch
stop_app
start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
check "R111-d relaunched on the Workspace with 20 accounts" eventually 10 is "rade workspace list --json | jq -r '.workspaces[] | select(.active) | .workspace'" many
sleep 1
open_both relaunch
check "R111-d the Usage page (20 accounts × 90 days) is drawn within 100 ms ($up ms, $rows rows)" \
    bash -c "[[ $rows == 20 ]] && python3 -c 'import sys; sys.exit(0 if $up <= 100 else 1)'"
check "R111-d the + menu (10 accounts of each agent) is drawn within 50 ms ($nm ms)" le "$nm" 50
[[ -z ${DIAG:-} ]] || source "$DIAG"
fi

if want c; then
echo "== R111-c 10 Projects × 5 links × 2 Worktrees × 1 Agent"
NAV=$HOME/workspaces/nav
mkdir -p "$NAV/.rade" && printf 'name = "nav"\n' >"$NAV/.rade/workspace.toml"
TPL=$T/tpl
mkdir -p "$TPL" && G -C "$TPL" init -q -b main && echo base >"$TPL/README.md" && G -C "$TPL" add -A && G -C "$TPL" commit -qm init
rade workspace add "$NAV" >/dev/null
for p in $(seq -w 1 10); do
    for l in 1 2 3 4 5; do
        cp -a "$TPL" "$NAV/s$p$l"
        rade source add "$NAV/s$p$l" --workspace nav >/dev/null
    done
done
rade workspace switch nav >/dev/null
t0=$(date +%s)
for p in $(seq -w 1 10); do
    rade project add "P$p" --source "s${p}1:main" --source "s${p}2:main" --source "s${p}3:main" \
        --source "s${p}4:main" --source "s${p}5:main" >/dev/null || echo "project add P$p failed"
    pid=p$p
    for l in 1 2 3 4 5; do
        for w in a b; do
            rade worktree add "s$p$l" "w$p$l$w" --project "$pid" >/dev/null || echo "worktree w$p$l$w failed"
        done
    done
done
echo "  50 Sources, 10 Projects, 100 Worktrees in $(($(date +%s) - t0)) s"
t0=$(date +%s)
for p in $(seq -w 1 10); do
    for l in 1 2 3 4 5; do
        for w in a b; do
            rade spawn --source "s$p$l" --worktree "w$p$l$w" --agent claude --name "a$p$l$w" --tab new >/dev/null \
                || echo "spawn a$p$l$w failed"
        done
    done
done
echo "  100 Agents in $(($(date +%s) - t0)) s"
navrows() { ui navigator | jq -r '[.sidebar[] | select(.kind == "project" or .kind == "link" or .kind == "worktree" or .kind == "agent")] | length'; }
check "R111-c the Navigator shows 260 rows (10 + 50 + 100 + 100)" eventually 20 is navrows 260
ui navigator | jq -r '[.sidebar[].kind] | group_by(.) | map("\(.[0])=\(length)") | join(" ")' | sed 's/^/  rows: /'
AG=$(rade list --json | jq -r '.panes[] | select(.name == "a055b") | .pane')
perf reset navigator_sync >/dev/null
for i in $(seq 50); do
    rade status running --pane "$AG" >/dev/null
    rade status waiting --pane "$AG" >/dev/null
done
sleep 0.5
n=$(pf navigator_sync:changed n) p95=$(pf navigator_sync:changed p95)
echo "  Navigator row update: n=$n p95=$p95 ms max=$(pf navigator_sync:changed max) ms"
check "R111-c 100 status changes: the Navigator row update is ≤ 16 ms at p95 ($p95 ms over $n updates)" \
    bash -c "[[ $n != none && $n -ge 100 ]] && python3 -c 'import sys; sys.exit(0 if $p95 <= 16 else 1)'"
shot r111-c-navigator
perf reset nav_scroll >/dev/null
perf "scroll 240" >/dev/null
eventually 30 is "perf | jq -r .scrolling" false || true
work=$(pf nav_scroll:work p95) iv=$(pf nav_scroll:interval p95) frames=$(pf nav_scroll:work n)
echo "  Navigator scroll: $frames frames, work p95 $work ms (max $(pf nav_scroll:work max)), interval p95 $iv ms (max $(pf nav_scroll:interval max))"
# The interval follows the headless compositor (llvmpipe paints every other
# vblank); RADE's own work per frame is the budget here, the interval is
# checked on the real desktop (tests/live/m40.sh, R111-f).
check "R111-c an end-to-end Navigator scroll: each frame's work ≤ 16.7 ms at p95 ($work ms over $frames frames)" \
    bash -c "[[ $frames != none && $frames -ge 200 ]] && python3 -c 'import sys; sys.exit(0 if $work <= 16.7 else 1)'"
shot r111-c-navigator-end
fi
fi

if [[ ${PART:-} != 1 ]]; then # ---- Part 2 (Group 2): layouts at 1280 px and 200 % ------
echo "== Part 2: scenes A–F at 1280 × 800 and at text scale 200 %"
OUT2=$OUT
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
FAKE=$HERE/fake-agent.sh
stop_app
rm -rf "$XDG_DATA_HOME/rade-dev" "$USAGE"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 3600
[agents.claude]
command = "$FAKE --model opus"
[agents.codex]
command = "$FAKE --model gpt-5"
C
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+, $2}}"; }
key() { rpc debug.ui "{\"op\":\"key\",\"keys\":\"$1\"}"; }
chrome() { ui chrome | jq -c .chrome; }
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
notoasts() { rpc debug.ui '{"op":"toasts_clear"}' >/dev/null; sleep 0.6; }
pane_status() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
export -f geo key chrome pane_status ui
# Scene: Workspace acme with Project Shop (shop-web, shop-api, shared-ui on
# develop), Worktree checkout-v2 with planner + tests side by side, scene E's
# accounts (tests/e2e/usage-scene.py).
A=$HOME/workspaces/acme
WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui
for r in "$WEB" "$API" "$UI"; do
    mkdir -p "$r/src" && G -C "$r" init -q -b main
    for f in a b c; do printf '%s\n' "$f" >"$r/src/$f.ts"; done
    G -C "$r" add -A && G -C "$r" commit -qm init && G -C "$r" branch develop
    git -C "$r" config user.email richard@acme.io
done
for d in .claude-work .claude-team .codex .codex-biz .claude .codex-personal .codex-oss; do mkdir -p "$HOME/$d"; done
P=$HOME/workspaces/personal
mkdir -p "$A/.rade" "$P/.rade"
cat >"$A/.rade/workspace.toml" <<'EOF2'
name = "acme"

[identity]
git_email = "richard@acme.io"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-work"
default = true

[[account]]
agent = "claude"
name = "acme-team"
config_dir = "~/.claude-team"

[[account]]
agent = "codex"
name = "acme"
config_dir = "~/.codex"
default = true

[[account]]
agent = "codex"
name = "acme-biz"
config_dir = "~/.codex-biz"
EOF2
cat >"$P/.rade/workspace.toml" <<'EOF2'
name = "personal"

[[account]]
agent = "claude"
name = "personal"
config_dir = "~/.claude"

[[account]]
agent = "codex"
name = "personal"
config_dir = "~/.codex-personal"

[[account]]
agent = "codex"
name = "oss"
config_dir = "~/.codex-oss"
EOF2
scene() { python3 "$HERE/usage-scene.py" "$USAGE" "$HOME" >"$T/scene.json"; }
scene
start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
rade workspace add "$A" >/dev/null
rade workspace add "$P" >/dev/null
for r in "$WEB" "$API" "$UI"; do rade source add "$r" --workspace acme >/dev/null; done
rade workspace switch acme >/dev/null
rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null || echo "project add Shop failed"
CV=$(rade worktree add shop-web checkout-v2 --project shop --json | jq -r .path)
echo v2 >>"$CV/src/a.ts"
id() { jq -r .pane; }
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --account acme-work --name planner --tab new "osc 24 tests passed" --json | id)
eventually 10 is "pane_status $planner" done || true
rade focus "$planner" >/dev/null
tests=$(rade spawn --source shop-web --worktree checkout-v2 --agent codex --account acme --name tests --tab current --split right "oscfirst Which fixtures?" --json | id)
eventually 10 is "pane_status $tests" waiting || true
rade status --pane "$planner" --agent claude >/dev/null
rade status --pane "$tests" --agent codex >/dev/null
rpc usage.refresh '{"workspace":"acme"}' >/dev/null
rade focus "$tests" >/dev/null
sleep 1
[[ $(chrome | jq -r .inspector.shown) == true ]] && key ctrl+shift+e >/dev/null
notoasts

# layout <w> <h> [scale]: resize and wait for the relayout.
layout() { rpc debug.ui "{\"op\":\"layout\",\"width\":$1,\"height\":$2,\"text_scale\":${3:-1.0}}" >/dev/null; sleep 1.2; }
toA() { rpc debug.ui "{\"op\":\"click_agent\",\"pane\":\"$tests\"}" >/dev/null; ui usage close >/dev/null; sleep 0.5; }
lay() { ui layout_info | jq -c .layout; }
# The window and every scrolled window: no sideways scroll; no label clipped.
noscroll() { local h; h=$(ui hscroll); [[ $(jq .count <<<"$h") == 0 && $(jq .window.width <<<"$h") == "$1" ]]; }
noclip() { ui clipped >"$T/clip.json"; [[ $(jq .count "$T/clip.json") == 0 ]] || { jq -c '.labels[:5][] | [.text, .classes, .allocation, .natural, .why]' "$T/clip.json" | sed 's/^/    clipped: /'; false; }; }
# within <a> <b> [tol]: |a − b| ≤ tol (default 1).
within() { python3 -c 'import sys; a, b, t = map(float, sys.argv[1:]); sys.exit(0 if abs(a - b) <= t else 1)' "$1" "$2" "${3:-1}"; }
g() { geo "$1" "${3:-}" | jq -r ".$2"; }
count() { geo "$1" '"all":true' | jq '[.[] | select(.visible)] | length'; }
token() { python3 - "$ROOT/crates/rade-app/src/tokens.toml" "$1" "$2" <<'PY'
import sys, tomllib
t = tomllib.load(open(sys.argv[1], "rb"))
print(t[sys.argv[2]][sys.argv[3]].lower())
PY
}
dropped_ok() { ui chrome | jq -e '.chrome.statusline.dropped | all(. == "hint" or . == "ports" or . == "identity")' >/dev/null; }
kept_ok() { for c in ws counts usage; do [[ $(g "status-cell-$c" visible) == true ]] || return 1; done; }
# grown <widget> <N>: every shown row is at least its text's height + N px
# (the sheets' "rows grow to text height + N px", ±1 px).
grown() { geo "$1" '"all":true' | jq -e --argjson n "$2" '[.[] | select(.visible and .text_extent != null)] | length > 0 and all(.height - .text_extent >= $n - 1)' >/dev/null || { geo "$1" '"all":true' | jq -c '[.[] | select(.visible) | [.label, .height, .text_extent]][:4]' | sed 's/^/    rows: /'; false; }; }
export T
export -f within g count noscroll noclip lay dropped_ok kept_ok grown
# scene_checks <AC> <tag> <window width> <theme>
scene_checks() {
    local ac=$1 tag=$2 W=$3 th=$4 large=false
    [[ $ac == R111-b ]] && large=true
    echo "== $ac $tag $th: scene A"
    toA
    [[ $(lay | jq -r .inspector_overlay_shown) == true ]] && key ctrl+shift+e >/dev/null && sleep 0.5
    check "$ac $tag $th A: window $W px wide, no horizontal scroll" noscroll "$W"
    check "$ac $tag $th A: no label clipped" noclip
    check "$ac $tag $th A: Inspector hidden by default" is "lay | jq -r .inspector_overlay_shown" false
    check "$ac $tag $th A: Statusline drops only hints / port / identity, keeps Workspace, counts, usage" bash -c "$(declare -f dropped_ok kept_ok g); dropped_ok && kept_ok"
    check "$ac $tag $th A: command field 280–440 px" bash -c "w=\$(geo command-field | jq .width); (( w >= 279 && w <= 441 ))"
    if [[ $large == false ]]; then
        check "$ac $tag $th A: Navigator 268 px, nav rows keep 26 px" \
            bash -c "[[ \$(geo navigator | jq .width) == 268 ]] && geo nav-row '\"all\":true' | jq -e '[.[] | select(.visible) | .height] | all(. >= 25 and . <= 27)' >/dev/null"
        check "$ac $tag $th A: the two Panes side by side, each ≥ 400 px" \
            bash -c "geo pane '\"all\":true' | jq -e '[.[] | select(.visible)] | length == 2 and all(.width >= 400) and (.[0].y == .[1].y)' >/dev/null"
    else
        check "$ac $tag $th A: Navigator is the 48 px rail (Project cubes, Scratch, bell with tooltips)" \
            bash -c "within \$(geo nav-rail | jq .width) 48 && [[ \$(geo navigator | jq -r .visible) == false ]] && geo nav-rail-button '\"all\":true' | jq -e '[.[] | .tooltip] == [\"Shop\", \"Scratch\", \"Inbox\"]' >/dev/null"
        check "$ac $tag $th A: one main Pane (tests), planner as a Title Bar" \
            bash -c "[[ \$(geo pane '\"all\":true' | jq '[.[] | select(.visible)] | length') == 1 && \$(geo rade-title-bar '\"all\":true' | jq '[.[] | select(.visible)] | length') == 1 ]] && geo 'pane pane-name' | jq -e '.label == \"tests\"' >/dev/null"
        check "$ac $tag $th A: fonts double (breadcrumb 12.5 → 25 px)" within "$(geo crumb-current | jq .font.size)" 25 0.5
        ui layout_info navigator >/dev/null; sleep 0.6
        check "$ac $tag $th A: a rail click opens the full Navigator (≥ 268 px) over the center, with the shadow" \
            bash -c "[[ \$(geo navigator | jq -r .visible) == true ]] && (( \$(geo navigator | jq .width) >= 268 )) && [[ \$(geo navigator | jq -r .shadow) == '0 20px 60px $(token "$th" shadow)' ]]"
        check "$ac $tag $th A: …no label clipped in it" noclip
        check "$ac $tag $th A: …its rows grow to their text's height + 8 px" grown nav-row 8
        shot "$ac-$tag-a-navigator-$th"
        ui layout_info navigator_close >/dev/null; sleep 0.6
    fi
    shot "$ac-$tag-a-$th"
    key ctrl+shift+e >/dev/null; sleep 0.6
    check "$ac $tag $th A: Ctrl+Shift+E floats the Inspector: 340 px (large text: its content's width) at the right edge, bg1, left border 1 px line2, shadow" \
        bash -c "i=\$(geo inspector); w=\$(jq .width <<<\"\$i\"); { [[ \$w == 340 ]] || [[ $large == true ]] && (( w >= 340 )); } && within \$(jq '.x + .width' <<<\"\$i\") $W && [[ \$(jq -r .background <<<\"\$i\") == '$(token "$th" bg1)ff' && \$(jq .border.left <<<\"\$i\") == 1 && \$(jq -r .border_color.left <<<\"\$i\") == '$(token "$th" line2)' && \$(jq -r .shadow <<<\"\$i\") == '0 20px 60px $(token "$th" shadow)' ]]"
    check "$ac $tag $th A: …no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    check "$ac $tag $th A: …not modal: the keyboard stays on the focused Terminal (tests)" \
        is "rade debug window --json | jq -r .focus.pane" "$tests"
    shot "$ac-$tag-a-inspector-$th"
    key ctrl+shift+e >/dev/null; sleep 0.5
    check "$ac $tag $th A: …and hides it again" is "lay | jq -r .inspector_overlay_shown" false

    echo "== $ac $tag $th: scene B"
    ui home "open shop" >/dev/null; sleep 1
    check "$ac $tag $th B: no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    if [[ $large == false ]]; then
        check "$ac $tag $th B: KPI cards in one row, equal widths" \
            bash -c "geo kpi '\"all\":true' | jq -e 'length == 3 and (map(.y) | unique | length == 1) and ((map(.width) | max) - (map(.width) | min) <= 1)' >/dev/null"
        check "$ac $tag $th B: link table keeps every column; lower grid two columns" \
            bash -c "[[ \$(geo link-table-col '\"all\":true' | jq length) == 6 && \$(geo home-worktrees | jq .y) == \$(geo home-activity | jq .y) ]]"
    else
        check "$ac $tag $th B: KPI cards one per line exactly when a card is narrower than its value + detail" \
            bash -c "geo kpi '\"all\":true' | jq -e '(((1392 - 56 - 40) / 3) < (map(.natural.width) | max)) as \$one | if \$one then (map(.x) | unique | length == 1) else (map(.y) | unique | length == 1) end' >/dev/null"
        check "$ac $tag $th B: CHECKOUT and VS BASE on a second line of each row" \
            bash -c "[[ \$(geo link-table-col '\"all\":true' | jq '[.[].label]' -c) == '[\"SOURCE\",\"BASE\",\"HEAD\",\"STATE\"]' && \$(geo link-row-second '\"all\":true' | jq length) == 3 ]]"
        check "$ac $tag $th B: lower grid one column (Worktrees, then Recent activity)" \
            bash -c "(( \$(geo home-activity | jq .y) > \$(geo home-worktrees | jq .y) ))"
        check "$ac $tag $th B: link rows grow to their text's height + 18 px" grown link-row 18
    fi
    shot "$ac-$tag-b-$th"
    toA

    echo "== $ac $tag $th: scene C"
    rpc debug.ui '{"op":"dialog_open","kind":"new_project"}' >/dev/null
    rpc debug.ui '{"op":"dialog_name","text":"Checkout revamp"}' >/dev/null
    for s in shop-web shop-api shared-ui; do rpc debug.ui "{\"op\":\"dialog_add\",\"source\":\"$s\",\"base\":\"develop\"}" >/dev/null; done
    sleep 1
    check "$ac $tag $th C: no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    if [[ $large == false ]]; then
        check "$ac $tag $th C: dialog keeps 680 px at x $(((W - 680) / 2)), top 92" \
            bash -c "d=\$(geo new-project-dialog); within \$(jq .width <<<\"\$d\") 680 && within \$(jq .x <<<\"\$d\") $(((W - 680) / 2)) && within \$(jq .y <<<\"\$d\") 92"
    else
        check "$ac $tag $th C: dialog min(window − 32, 1000) = 1000 px; each link row on two lines" \
            bash -c "within \$(geo new-project-dialog | jq .width) 1000 && [[ \$(geo link-editor-row-two-line '\"all\":true' | jq length) == 3 ]]"
    fi
    shot "$ac-$tag-c-$th"
    rpc debug.ui '{"op":"dialog_cancel"}' >/dev/null; sleep 0.3

    echo "== $ac $tag $th: scene D"
    key ctrl+shift+p >/dev/null
    eventually 3 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .open) == true ]]" || true
    rade --json debug palette --ui type shared >/dev/null
    eventually 5 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .busy) == false ]]" || true
    sleep 0.5
    check "$ac $tag $th D: no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    if [[ $large == false ]]; then
        check "$ac $tag $th D: palette 720 px at x $(((W - 720) / 2)), top 64, at most $((800 - 128)) px tall" \
            bash -c "p=\$(geo palette); within \$(jq .width <<<\"\$p\") 720 && within \$(jq .x <<<\"\$p\") $(((W - 720) / 2)) && within \$(jq .y <<<\"\$p\") 64 && (( \$(jq .height <<<\"\$p\") <= 672 ))"
    else
        check "$ac $tag $th D: palette min(window − 32, 1100) = 1100 px, scope row on two lines, details under the titles, footer inside the window" \
            bash -c "within \$(geo palette | jq .width) 1100 && [[ \$(geo palette-scopes-wrapped | jq -r .visible) == true ]] && (( \$(geo palette-item-two-line '\"all\":true' | jq length) >= 1 )) && f=\$(geo palette-footer) && (( \$(jq '.y + .height' <<<\"\$f\") <= 900 ))"
        check "$ac $tag $th D: rows grow to their text's height + 12 px" grown palette-item 12
    fi
    shot "$ac-$tag-d-$th"
    rade --json debug palette --ui key escape >/dev/null || true; sleep 0.3

    echo "== $ac $tag $th: scene E"
    ui usage open >/dev/null; ui usage "select claude acme-work acme" >/dev/null; sleep 1.5
    check "$ac $tag $th E: no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    check "$ac $tag $th E: the detail card below the list, full width (list would be < 560 px)" \
        bash -c "[[ \$(ui usage | jq -r .usage.layout.detail_below) == true ]] && (( \$(geo usage-detail | jq .y) > \$(geo usage-accounts | jq '.y + .height') )) && within \$(geo usage-detail | jq .width) \$(geo usage-accounts | jq .width)"
    if [[ $large == true ]]; then
        check "$ac $tag $th E: Plan and Updated on a second line of each row; chart 160 px" \
            bash -c "[[ \$(geo usage-col '\"all\":true' | jq '[.[].label] | index(\"PLAN\")') == null && \$(geo usage-row-second '\"all\":true' | jq length) == 4 ]] && (( \$(geo usage-chart | jq .height) >= 160 ))"
        check "$ac $tag $th E: table rows grow to their text's height + 16 px" grown usage-row 16
    fi
    shot "$ac-$tag-e-$th"
    ui usage close >/dev/null
    toA

    echo "== $ac $tag $th: scene F"
    scene
    rpc debug.ui '{"op":"forecast_check","arg":"reset"}' >/dev/null
    eventually 10 bash -c "[[ \$(geo toast | jq -r .visible) == true ]]" || true
    rpc debug.ui '{"op":"new_menu","arg":"open"}' >/dev/null; sleep 1.2
    check "$ac $tag $th F: no horizontal scroll, no label clipped" bash -c "$(declare -f noscroll noclip); noscroll $W && noclip"
    if [[ $large == false ]]; then
        # Centred over the Tabs column (M39 decision on F.15), not the window.
        check "$ac $tag $th F: + popover keeps 340 px; Toast centred over the Tabs column" \
            bash -c "within \$(geo new-menu | jq .width) 340 && within \$(geo toast | jq '.x + .width / 2') \$(geo toastoverlay | jq '.x + .width / 2')"
    else
        check "$ac $tag $th F: + popover 340 … window − 32 px inside the window; Toast ≤ window − 64 px with its action under the text" \
            bash -c "m=\$(geo new-menu); w=\$(jq .width <<<\"\$m\"); (( w >= 340 && w <= $((W - 32)) )) && (( \$(jq '.y + .height' <<<\"\$m\") <= 900 )) && (( \$(geo toast | jq .width) <= $((W - 64)) )) && [[ \$(geo toast-action-under | jq -r .visible) == true ]]"
        check "$ac $tag $th F: + menu rows grow to their text's height + 10 px" grown new-menu-item 10
    fi
    shot "$ac-$tag-f-$th"
    rpc debug.ui '{"op":"new_menu","arg":"close"}' >/dev/null; notoasts
}
for mode in ${MODES:-1280 200}; do
    if [[ $mode == 1280 ]]; then
        layout 1280 800
        check "R111-a layout 'narrow' at 1280 × 800 (Inspector overlay, full Navigator)" \
            bash -c "[[ \$(ui layout_info | jq -c '.layout | [.mode, .width, .rail, .one_pane]') == '[\"narrow\",1280,false,false]' ]]"
        ac=R111-a tag=1280 W=1280
    else
        layout 1440 900 2.0
        check "R111-b layout 'rail' at 1440 × 900, text scale 2.0" \
            bash -c "[[ \$(ui layout_info | jq -c '.layout | [.mode, .width, .text_scale, .one_pane]') == '[\"rail\",1440,2.0,true]' ]]"
        ac=R111-b tag=200 W=1440
    fi
    for th in dark light; do
        [[ $th == light ]] && light
        scene_checks "$ac" "$tag" "$W" "$th"
        [[ $th == light ]] && dark
    done
done
if [[ -n ${HOLD:-} ]]; then # debugging: keep the app up, its env in $OUT/env.sh
    declare -p XDG_RUNTIME_DIR XDG_DATA_HOME XDG_CONFIG_HOME HOME WAYLAND_DISPLAY PATH tests planner 2>/dev/null | sed 's/^declare -[-x]* /export /' >"$OUT/env.sh"
    echo "holding $HOLD s"; sleep "$HOLD"
fi
layout 1440 900
check "R111-a back at 1440 × 900, 100 %: the wide layout again (Navigator 268, two Panes, no rail, 12.5 px breadcrumb)" \
    bash -c "[[ \$(ui layout_info | jq -r .layout.mode) == wide && \$(geo navigator | jq .width) == 268 && \$(geo nav-rail | jq -r .visible) == false ]] && geo pane '\"all\":true' | jq -e '[.[] | select(.visible)] | length == 2' >/dev/null && within \$(geo crumb-current | jq .font.size) 12.5 0.1"
layout 1440 900
fi

echo
echo "M40: $PASS passed, $FAIL failed"
((FAIL == 0))
