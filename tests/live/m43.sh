# Live (R117-p, M53 amendment of R117.8): the Agent Overview's frame budgets on
# the real GNOME desktop (the GPU paints; headless m43 checks only RADE's own
# work and a 100 ms first frame). 40 Agent Panes (`--agent claude --cmd
# "sleep 600"`: no real Agent runs) in 5 Tabs; the Overview opens and closes
# 5 times through `debug.ui overview`, then `debug.ui perf` gives the p95s.
#   tests/live/run.sh m43   → target/live/results.txt, r117-p-overview.png
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(60); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"):
    c = s.recv(65536)
    if not c: sys.exit("rpc: the app closed the connection")
    buf += c
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
ov() { rpc debug.ui "{\"op\": \"overview\", \"arg\": \"${1:-state}\"}"; }
le() { python3 -c 'import sys; a, b = sys.argv[1:]; sys.exit(0 if a != "none" and float(a) <= float(b) else 1)' "$1" "$2"; }
st=(running done running exited)
for i in $(seq 1 40); do
    p=$(rade spawn --agent claude --cmd "sleep 600" --name "ov$i" $([[ $((i % 8)) == 1 ]] && echo --tab new) --json | jq -r .pane)
    rade status "${st[$((i % 4))]}" --pane "$p" >/dev/null
done
check "R117-p 40 Agents listed" test "$(ov open | jq -r .count | cut -d' ' -f1)" = 40
sleep 0.8
rade debug screenshot "$OUT/r117-p-overview.png" >/dev/null 2>&1 || true
rpc debug.ui '{"op": "key", "keys": "escape"}' >/dev/null
rpc debug.ui '{"op": "perf", "arg": "reset"}' >/dev/null
for _ in 1 2 3 4 5; do ov open >/dev/null; sleep 0.6; rpc debug.ui '{"op": "key", "keys": "escape"}' >/dev/null; sleep 0.3; done
perf=$(rpc debug.ui '{"op": "perf"}')
first=$(jq -r '.samples.overview.p95 // "none"' <<<"$perf")
anim=$(jq -r '.samples["overview:anim"].p95 // "none"' <<<"$perf")
echo "  first frame p95 $first ms, animation frame work p95 $anim ms" | tee -a "$RESULTS"
check "R117-p first frame within 50 ms (p95: $first)" le "$first" 50
check "R117-p animation frame work within 16.7 ms (p95: $anim)" le "$anim" 16.7
for i in $(seq 1 40); do rade close "ov$i" >/dev/null 2>&1 || true; done
