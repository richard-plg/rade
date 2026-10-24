#!/usr/bin/env bash
# System monitor (R116): the Statusline cell over a fake proc / sys root
# (text, high state, narrow drop order, off when the Statusline is hidden or
# `system_monitor = false`), then on the real /proc a busy Pane first in the
# popover's BY PANE list and a click focusing it. Headless Weston (ADR-0006).
# Usage: tests/e2e/sysmon.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-sysmon
export RADE_SYSTEM_MONITOR=1
source "$HERE/lib.sh"

SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
rpc() { python3 - "$SOCK" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
mon() { rpc debug.ui "{\"op\":\"monitor\"${1:+,\"arg\":\"$1\"}}" | jq -c .monitor; }
field() { mon | jq -r ".$1"; }
chrome() { rpc debug.ui '{"op":"chrome"}' | jq -c .chrome; }
samples() { field samples; }

# ---- fake roots: 34 % CPU busy between two samples, 61 % memory ----
P=$T/proc S=$T/sys
mkdir -p "$P/net" "$S/class/net/lo" "$S/class/net/docker0" "$S/class/net/enp1s0/device"
stat() { printf 'cpu  %s 0 %s %s %s 0 0 0 0 0\ncpu0 1 0 1 1 1 0 0 0\ncpu1 1 0 1 1 1 0 0 0\n' "$@" > "$P/stat"; }
mem() { printf 'MemTotal: 1000000 kB\nMemAvailable: %s kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n' "$1" > "$P/meminfo"; }
net() { printf 'Inter-|\n face |\n    lo: 9 1 0 0 0 0 0 0 9 1 0 0 0 0 0 0\n  docker0: %s 1 0 0 0 0 0 0 %s 0 0 0 0 0 0 0\n  enp1s0: %s 9 0 0 0 0 0 0 %s 3 0 0 0 0 0 0\n' "$1" "$1" "$2" "$3" > "$P/net/dev"; }
stat 100 100 700 100; mem 390000; net 0 0 0
echo "3.20 2.90 2.50 2/2000 1" > "$P/loadavg"
export RADE_PROC_ROOT=$P RADE_SYS_ROOT=$S

echo "== start (fake proc root)"
start_app
q() { mon | jq -e "$1" >/dev/null; }   # a jq predicate on debug.ui monitor
check "the cell is built and shown, sampling (fast while active, slow headless)" \
    eventually 8 q '.visible and .mode != "off"'
eventually 8 q '.samples >= 1'
# Second sample: +1000 jiffies, 660 idle → 34 %; enp1s0 +2.4 MB / +624 KB; docker0 ignored.
stat 300 240 1300 160; net 999999999 2400000 624000
text_ok() { field text | grep -Eqx 'cpu  34% · mem  61% · ↓[0-9.]+[KM] ↑[0-9.]+[KB]'; }
check "R116-a cell text: cpu  34% · mem  61% · ↓…M ↑…K (no gpu, only enp1s0)" eventually 12 text_ok
echo "  text: $(field text)"
check "R116-a icons instead of words: cpu, memory, down, up (no GPU here)" \
    test "$(mon | jq -r '.icons | join(",")')" = "ph-cpu-symbolic,ph-memory-symbolic,ph-arrow-down-symbolic,ph-arrow-up-symbolic"
check "R116-a …mapped in the Statusline" q '.mapped'
sleep 0.5; shot statusline-monitor
mem 40000
check "R116-b memory 96 % → the cell turns err" eventually 12 q '.classes | index("err")'
shot statusline-monitor-err
mem 390000
check "R116-b …and back once it is low" eventually 12 q '(.classes | index("err")) == null'
check "R116-e a tick costs ≤ 1 ms on the worker and ≤ 1 ms on the UI thread (fake root: $(field worker_us) µs / $(field ui_us) µs)" \
    q '.worker_us <= 1000 and .ui_us <= 1000'

echo "== R116-c narrow: hints drop before the monitor"
for w in 1100 900 760 640; do
    rpc debug.ui "{\"op\":\"layout\",\"width\":$w,\"height\":800}" >/dev/null
    sleep 1
    d=$(chrome | jq -r '.statusline.dropped | join(",")')
    check "R116-c at $w px dropped [$d]: the monitor only after both hints" \
        bash -c "[[ ',$d,' != *,monitor,* || ',$d,' == *,hint,hint,* ]]"
done
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null

echo "== R116-e off: hidden Statusline, system_monitor = false"
mkdir -p "$XDG_CONFIG_HOME/rade"
rade debug action toggle_status_bar >/dev/null
check "Statusline hidden → mode off" eventually 8 is "field mode" off
a=$(samples); sleep 6; b=$(samples)
check "R116-e …no sample taken for 6 s ($a → $b)" test "$b" = "$a"
rade debug action toggle_status_bar >/dev/null
check "Statusline shown again → sampling again" eventually 8 q '.mode != "off"'
printf 'system_monitor = false\n' > "$XDG_CONFIG_HOME/rade/config.toml"
check "system_monitor = false → no cell, off" eventually 8 q '.enabled == false and .visible == false and .mode == "off"'
printf '# monitor back on\n' > "$XDG_CONFIG_HOME/rade/config.toml"
check "…back on with the config" eventually 8 q '.visible'
stop_app

echo "== R116-d real /proc: the busy Pane leads BY PANE, a click focuses it"
unset RADE_PROC_ROOT RADE_SYS_ROOT
start_app
idle=$(rade list --json | jq -r '.panes[0].pane')
busy=$(rade spawn --tab new --name burner --cmd "bash -c 'while :; do :; done'" --json | jq -r .pane)
rade focus "$idle" >/dev/null
sleep 1
mon open >/dev/null
first_row() { mon | jq -r '.rows[]' | grep -A1 -x 'BY PANE' | tail -1; }
burner_first() { first_row | grep -Eq '^burner +(8[0-9]|9[0-9]|1[0-9][0-9])%'; }
check "R116-d the popover lists 'burner' first with ≥ 80 % of a core" eventually 15 burner_first
echo "  rows: $(mon | jq -c .rows)"
check "R116-d …and an 'outside RADE' line" bash -c "jq -r '.[]' <<<'$(mon | jq -c .rows)' | grep -q '^outside RADE'"
check "R116-e with the popover open a tick stays ≤ 50 ms on the worker ($(field worker_us) µs)" q '.worker_us <= 50000'
sleep 0.3; shot monitor-popover
mon open >/dev/null; sleep 0.5
echo "  popover open: $(field popover)"
rpc debug.ui "{\"op\":\"monitor\",\"path\":\"$OUT/popover-dark.png\"}" >/dev/null
rade debug action color_scheme_light >/dev/null; sleep 2.5
rpc debug.ui "{\"op\":\"monitor\",\"path\":\"$OUT/popover-light.png\"}" >/dev/null
rade debug action color_scheme_dark >/dev/null
mon click_first >/dev/null
focused_busy() { rade debug state | jq -e --arg p "$busy" '[.. | objects | select(.focused? == $p)] | length > 0' >/dev/null; }
check "R116-d a click on the row focuses 'burner'" eventually 5 focused_busy
rade close "$busy" --force >/dev/null 2>&1 || true
stop_app

echo
echo "SYSMON E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
[[ $FAIL == 0 ]]
