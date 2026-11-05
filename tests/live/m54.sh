# Live (R146-g, M54): a Worktree of the real `m53-solid` branch (PR #10 of
# richard-plg/rade, merged) reads its real pull request and checks through the
# owner's own `gh` login. Read only: RADE runs `gh pr list`, nothing else; no
# git network call (the clone's origin/m53-solid comes from this checkout).
#   tests/live/run.sh m54   → target/live/results.txt, r146-g-navigator.png
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
GH_DIR=${GH_CONFIG_DIR:-$REAL_XDG_CONFIG_HOME/gh}
if ! GH_CONFIG_DIR=$GH_DIR timeout 20 gh auth status >/dev/null 2>&1 </dev/null; then
    na "R146-g gh is not logged in ($GH_DIR): skipped"
else
    W=$T/ws-live
    mkdir -p "$W/.rade"
    printf 'name = "Live"\n[env]\nGH_CONFIG_DIR = "%s"\n' "$GH_DIR" >"$W/.rade/workspace.toml"
    git clone -q --no-recurse-submodules "$ROOT" "$W/rade" </dev/null
    git -C "$W/rade" remote set-url origin git@github.com:richard-plg/rade.git
    rade workspace add "$W" >/dev/null
    rade source add "$W/rade" >/dev/null
    rade worktree add rade m53-solid --base origin/m53-solid >/dev/null
    wt=$(rade worktree list rade --json | jq -r '.worktrees[] | select(.worktree == "m53-solid") | .path')
    git -C "$wt" branch -q --set-upstream-to=origin/m53-solid
    st=$(rade pr status --source rade --worktree m53-solid --refresh --json)
    echo "  $(jq -c '.pr | {number, state, checks, failing, stale}' <<<"$st")" | tee -a "$RESULTS"
    check "R146-g the real PR #10 is found" test "$(jq -r .pr.number <<<"$st")" = 10
    check "R146-g it reads as merged" test "$(jq -r .pr.state <<<"$st")" = merged
    check "R146-g its checks were read ($(jq -r .pr.checks <<<"$st"))" test "$(jq -r '.pr.checks | IN("passing","failing","pending","none")' <<<"$st")" = true
    chip=$(rpc debug.ui '{"op": "navigator"}' | jq -r '[.. | objects | select(.pr_chip?) | .pr_chip.text] | first // ""')
    check "R146-g the Navigator chip says #10 merged ($chip)" test "$chip" = "#10 merged"
    rade debug screenshot "$OUT/r146-g-navigator.png" >/dev/null 2>&1 || true
fi
