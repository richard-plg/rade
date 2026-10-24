# M16 live (R30): a real click on a Project header's chevron folds it, a
# real right-click → New Worktree… form driven by the keyboard (Named Base
# dropdown, Tab, Enter) creates the Worktree from that base's branch, real
# Alt+<n> follows the sidebar order with a Source in two Projects, and
# screenshots on the real GPU (light and dark: the app's own style, the
# GNOME theme is never touched). Everything lives in the run's temp dir;
# fake Agents only. Run it with a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) tests/live/run.sh m16
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LV=$T/files/ws-m16
for r in api app web; do
    mkdir -p "$LV/$r" && LG -C "$LV/$r" init -q -b main && echo x >"$LV/$r/README.md" &&
        LG -C "$LV/$r" add -A && LG -C "$LV/$r" commit -qm init
done
LG -C "$LV/api" branch beacon-dev && LG -C "$LV/api" checkout -q -b beacon-prod &&
    echo p >"$LV/api/p.txt" && LG -C "$LV/api" add -A && LG -C "$LV/api" commit -qm prod && LG -C "$LV/api" checkout -q main
mkdir -p "$LV/.rade"
printf 'name = "LiveV"\n' >"$LV/.rade/workspace.toml"
rade workspace add "$LV" >/dev/null
for r in api app web; do rade source add "$LV/$r" >/dev/null; done
rade workspace switch livev >/dev/null
rade project add BEACON --source api:dev=beacon-dev,prod=beacon-prod --source app >/dev/null
mkdir -p "$LV/.rade/projects/otp"
printf 'name = "OTP"\n[[source]]\nid = "web"\n[[source]]\nid = "api"\n' >"$LV/.rade/projects/otp/project.toml"
sb() { rpc_ui sidebar | jq -r "$1"; }
rpc_ui() { python3 - "$RADE_SOCKET" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.ui", "params": {"op": sys.argv[2]}}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}
dlg() { python3 - "$RADE_SOCKET" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.dialog", "params": {}}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}
active_src() { state | jq -r .active_source; }
want "Projects in the sidebar" 5 "sb '[.sidebar[] | select(.kind==\"project\") | .title] | join(\",\")'" "BEACON,OTP"

echo "-- R30.2 real Alt+<n>: BEACON (api, app), OTP (web, api again), then General"
key alt+1
want "real Alt+1 = api" 3 active_src api
key alt+2
want "real Alt+2 = app" 3 active_src app
key alt+3
want "real Alt+3 = web (api counted once)" 3 active_src web

echo "-- R30.1 a real click on the BEACON chevron folds it"
read -r x y <<<"$(widget_xy rade-chevron 0)"
if click "$x" "$y"; then
    want "BEACON folded" 3 "sb '.sidebar[] | select(.kind==\"project\" and .id==\"beacon\") | .collapsed'" true
    want "…its Sources hidden" 3 "sb '[.sidebar[] | select(.kind==\"source\") | .id] | join(\",\")'" "web,api,general@livev"
    click "$x" "$y"
    want "a second click unfolds it" 3 "sb '.sidebar[] | select(.kind==\"project\" and .id==\"beacon\") | .collapsed'" false
else
    bad "pointer over the BEACON chevron (not placed)"
fi

echo "-- R30.3 right-click → New Worktree…, the form by keyboard"
read -r x y <<<"$(widget_xy rade-project-beacon)"
if click "$x" "$y" right; then
    sleep 1
    key enter # New Worktree…, the first item, is preselected
    want "the form opens" 3 "dlg | jq -r .heading" "New Worktree in BEACON"
    sleep 0.5
    type_text "live-p"
    want "the name is typed into it" 3 "dlg | jq -r .fields.name" live-p
    key shift+tab space
    sleep 0.6
    key down enter
    want "Named Base dropdown by keyboard: prod" 3 "dlg | jq -r .fields.base" prod
    want "…the hint names its branch" 3 "dlg | jq -r .fields.hint" "Starts from beacon-prod."
    shot m16-new-worktree-light
    key tab enter
    want "Enter creates it from beacon-prod" 10 "rade worktree list api --json | jq -c '.worktrees[] | select(.worktree==\"live-p\") | [.project, .base]'" '["beacon","beacon-prod"]'
else
    bad "pointer over the BEACON header (not placed)"
fi

echo "-- R30-e screenshots on the real GPU"
shot m16-sidebar-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot m16-sidebar-dark
rade debug action color_scheme_light >/dev/null
rade worktree remove api live-p --force --delete-branch >/dev/null
rade workspace switch default >/dev/null
