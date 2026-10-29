#!/usr/bin/env bash
# E2E (M49, R115.3 amended, R115-g): a Tab holding Agents of two kinds shows
# the lead Agent's glyph then the other kind's (`debug.tabs` `marks`), with a
# per-Agent tooltip; screenshots dark + light → target/e2e-m49/.
# Usage: tests/e2e/m49-tabs.sh   (NO_BUILD=1, KEEP=1)
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m49/tabs
source "$HERE/lib.sh"
FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<EOC
idle_seconds = 30
[agents.codex]
command = "env FAKE_AGENT=codex $FAKE"
[agents.claude]
command = "env FAKE_AGENT=claude $FAKE"
EOC
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
info() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
marks_of() { rpc debug.tabs | jq -c --arg t "$1" '.tabs[] | select(.tab==$t) | .marks'; }

echo "== start"
start_app
cx=$(rade spawn --agent codex --name cx --json | jq -r .pane)
tab=$(info "$cx" tab)
check "one kind: marks [codex]" eventually 10 is "marks_of $tab" '["codex"]'
shot one-kind-dark
RADE_PANE_ID=$cx RADE_SOCKET=$SOCK rade spawn --agent claude --name cl >/dev/null
cl=$(rade list --json | jq -r '[.panes[] | select(.name=="cl")][0].pane')
check "claude spawned into codex's Tab" test "$(info "$cl" tab)" = "$tab"
check "R115-g Tab marks == [codex, claude]" eventually 10 is "marks_of $tab" '["codex","claude"]'
tip=$(rpc debug.tabs | jq -r --arg t "$tab" '.tabs[] | select(.tab==$t) | .tooltip')
check "tooltip has 'cx · codex ·'" grep -q "cx · codex · " <<<"$tip"
check "tooltip has 'cl · claude ·'" grep -q "cl · claude · " <<<"$tip"
rade debug action color_scheme_light >/dev/null; sleep 1; shot two-kinds-light
rade debug action color_scheme_dark >/dev/null; sleep 1; shot two-kinds-dark

echo
echo "M49 tabs: $PASS passed, $FAIL failed (screenshots: ${OUT#$ROOT/})"
stop_app
[[ $FAIL -eq 0 ]]
