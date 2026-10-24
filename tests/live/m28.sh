# M28 live (R84, R85; ADR-0029) on the real desktop with real input and a
# FAKE language server (tests/e2e/fake-lsp.py): a real click puts the caret
# on a name, a real F12 jumps to its definition, a real Alt+Left goes back
# and Alt+Right forward, a real Ctrl+click jumps, a real Shift+F12 fills the
# results popover and a real click on a row opens that line. Light and dark
# screenshots on the real GPU. Run it with a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m28
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LA=$T/files/ws-m28/api
mkdir -p "$LA/src"
printf '[package]\nname = "api"\nversion = "0.1.0"\nedition = "2021"\n' >"$LA/Cargo.toml"
cat >"$LA/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub fn helper() -> String {
    greet("x")
}
EOF
cat >"$LA/src/main.rs" <<'EOF'
fn main() {
    let s = api::greet("you");
    println!("{s} {}", api::helper());
}
EOF
cat >"$LA/.fake-lsp.json" <<'EOF'
{"greet": {"def": ["src/lib.rs:1:8"], "refs": ["src/main.rs:2:18", "src/lib.rs:6:5"]},
 "helper": {"def": ["src/lib.rs:5:8"]}}
EOF
LG -C "$LA" init -q -b main && LG -C "$LA" add -A && LG -C "$LA" commit -qm init
cat >>"$CONFIG" <<EOF

[code.server.rust]
command = ["python3", "$ROOT/tests/e2e/fake-lsp.py"]
EOF
rade source add "$LA" >/dev/null
rade debug action color_scheme_light >/dev/null
V=$(rade open "$LA/src/main.rs:2")
sleep 1
vj() { rade debug viewer "$V" info --json | jq -r "$1"; }
where() { vj '"\(.path | sub(".*/api/"; "")):\(.line)"'; }
click_at() { # a real click (with modifiers) on line:col of the Viewer
    local x y
    read -r x y <<<"$(rade debug viewer "$V" locate "$1" --json 2>/dev/null | jq -r '"\(.x) \(.y)"')"
    [[ -n $x && $x != null ]] && click "$x" "$y" left "${@:2}"
}

code_title() { python3 - "$RADE_SOCKET" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "debug.code", "params": {}}) + "\n").encode())
b = b""
while not b.endswith(b"\n"): b += s.recv(65536)
r = json.loads(b)["result"]; print(r["title"] if r["visible"] else "")
PY
}

echo "-- R84 a real click on greet, then a real F12"
if click_at 2:19; then
    want "the click put the caret on greet" 3 "vj .cursor_col" 19
    key f12
    want "F12: the definition in lib.rs:1" 5 where "src/lib.rs:1"
    sleep 0.6
    shot m28-f12-light
    key alt+left
    want "Alt+Left: back at main.rs:2" 5 where "src/main.rs:2"
    key alt+right
    want "Alt+Right: forward to lib.rs:1" 5 where "src/lib.rs:1"
    key alt+left
    want "… and back again" 5 where "src/main.rs:2"
else
    bad "real click on greet (pointer not placed)"
fi

echo "-- R84 a real Ctrl+click on helper"
if click_at 3:30 ctrl; then
    want "Ctrl+click: helper's definition, lib.rs:5" 5 where "src/lib.rs:5"
    key alt+left
    want "… Alt+Left returns" 5 where "src/main.rs:3"
else
    bad "real Ctrl+click on helper (pointer not placed)"
fi

echo "-- R85 a real Shift+F12, then a real click on a results popover row"
if click_at 2:19; then
    key shift+f12
    want "Shift+F12: the results popover shows References to greet" 6 code_title "References to greet"
    sleep 0.6
    shot m28-code-page-light
    rade debug action color_scheme_dark >/dev/null
    sleep 0.6
    shot m28-code-page-dark
    rade debug action color_scheme_light >/dev/null
    # The lib.rs row's preview text: greet("x").
    read -r x y <<<"$(widget_xy rade-search-hit 0)"
    if [[ -n $x && $x != null ]] && click "$x" "$y"; then
        want "a row click opens lib.rs:6" 5 where "src/lib.rs:6"
    else
        bad "real click on a results popover row (pointer not placed)"
    fi
else
    bad "real click before Shift+F12 (pointer not placed)"
fi
rade debug viewer "$V" escape >/dev/null 2>&1 || true
