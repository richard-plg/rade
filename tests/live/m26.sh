# M26 live (R30; ADR-0028), as 1.0.0 keeps it (R101), on the real desktop
# with real input: a real click on a No Project row's `→ BEACON?` suggestion
# moves it under BEACON; a real `git switch` in a main checkout moves no row.
# Light and dark screenshots on the real GPU. Fake Agents only. Run it with
# a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m26
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LV=$T/files/globex
for r in globex-web globex-server; do
    mkdir -p "$LV/$r" && LG -C "$LV/$r" init -q -b master && echo x >"$LV/$r/README.md" &&
        LG -C "$LV/$r" add -A && LG -C "$LV/$r" commit -qm init
    LG -C "$LV/$r" branch feat/dev/beacon
    LG -C "$LV/$r" config user.name "Live Dev"
    LG -C "$LV/$r" config user.email live@globex.test
done
LG -C "$LV/globex-web" checkout -q -b feat/atlas
mkdir -p "$LV/.rade/projects/beacon"
cat >"$LV/.rade/workspace.toml" <<EOF
name = "globex"
[agents.claude]
command = "$ROOT/tests/e2e/fake-agent.sh"
[agents.fake]
command = "$ROOT/tests/e2e/fake-agent.sh"
EOF
printf '# BEACON\nname = "BEACON"\nbases = ["dev"]\n\n[[source]]\nid = "globex-web"\nbase = { dev = "feat/dev/beacon" }\n\n[[source]]\nid = "globex-server"\nbase = { dev = "feat/dev/beacon" }\n' >"$LV/.rade/projects/beacon/project.toml"
LPF=$LV/.rade/projects/beacon/project.toml
rade workspace add "$LV" >/dev/null
for r in globex-web globex-server; do rade source add "$LV/$r" --workspace globex >/dev/null; done
rade workspace switch globex >/dev/null
rade worktree add globex-web beacon-dev --project beacon >/dev/null
chat=$(rade worktree add globex-web agent-chat)
LG -C "$chat" checkout -q -b feat/beacon-agent-chat
ui() { python3 - "$RADE_SOCKET" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
p = json.loads(sys.argv[2]) if sys.argv[2].startswith("{") else {"op": sys.argv[2]}
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.ui", "params": p}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}
click_w() { # a real click on the nth widget with CSS class / label $1
    local i x y
    for i in 1 2 3; do
        sleep 0.5
        read -r x y <<<"$(widget_xy "$1" "${2:-0}" 2>/dev/null)"
        [[ -n $x && $x != null ]] && click "$x" "$y" && return 0
    done
    return 1
}
rows() { ui sidebar | jq -r '[.sidebar[] | select(.kind != "agent") | "\(.kind):\(.title)\(if .nested then "^" else "" end)"] | join(" ")'; }

rade debug action color_scheme_light >/dev/null
sleep 1
echo "-- R30 a real click on → BEACON? records the Project"
want "agent-chat offers → beacon" 6 "ui sidebar | jq -r '.sidebar[] | select(.title==\"agent-chat\") | .suggest'" beacon
before=$(rows)
shot m26-sidebar-light
if click_w rade-suggest; then
    want "agent-chat is under BEACON now" 5 "rade worktree list globex-web --json | jq -r '.worktrees[] | select(.worktree==\"agent-chat\") | .project'" beacon
    want "… its row is nested under the Project" 5 "ui sidebar | jq -r '.sidebar[] | select(.title==\"agent-chat\") | .nested'" true
else
    bad "click on the suggestion (pointer not placed)"
fi
echo "-- R30-f a real git switch in a main checkout moves no row"
rows1=$(rows)
LG -C "$LV/globex-web" checkout -q feat/dev/beacon
want "the main checkout shows the new branch" 8 "ui sidebar | jq -r '.sidebar[] | select(.kind==\"checkout\" and .id==\"globex-web\") | .meta'" feat/dev/beacon
want "… and the rows are the same" 2 "rows" "$rows1"
rade debug action color_scheme_dark >/dev/null
sleep 0.6
shot m26-sidebar-dark
rade debug action color_scheme_light >/dev/null

rade workspace switch default >/dev/null
