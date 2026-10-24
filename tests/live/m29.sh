# M29 live (R87, R88; ADR-0030) on the real desktop with real input and a
# FAKE language server (tests/e2e/fake-lsp.py): in a Review Pane, a real
# click puts the point on a name of a diff line and a real F12 opens its
# definition in the Tab's Viewer; a real Ctrl+click on an added line's
# name jumps there; a real Alt+Left goes back; a real click on the Impact
# toggle shows the Impact and a real click on a caller row opens it; the
# Terminal's and the server's processes hold no GPU fds (M28 Low). Light and
# dark screenshots on the real GPU. Run it with a temp HOME:
#   XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m29
LG() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
LA=$T/files/ws-m29/api
mkdir -p "$LA/src" "$LA/tests"
printf '[package]\nname = "api"\nversion = "0.1.0"\nedition = "2021"\n' >"$LA/Cargo.toml"
cat >"$LA/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub fn helper() -> String {
    greet("x")
}

pub fn legacy() -> u32 {
    7
}
EOF
cat >"$LA/src/main.rs" <<'EOF'
fn main() {
    let s = api::greet("you");
    println!("{s} {}", api::helper());
}
EOF
printf '#[test]\nfn t() {\n    assert!(!api::greet("t").is_empty());\n}\n' >"$LA/tests/t.rs"
cat >"$LA/.fake-lsp.json" <<'EOF'
{"greet": {"def": ["src/lib.rs:1:8"], "refs": ["src/main.rs:2:18", "src/lib.rs:6:5", "src/lib.rs:10:5"],
           "callers": [{"name": "main", "def": "src/main.rs:1:4", "at": "src/main.rs:2:18"},
                       {"name": "helper", "def": "src/lib.rs:5:8", "at": "src/lib.rs:6:5"}]},
 "helper": {"def": ["src/lib.rs:5:8"]},
 "fresh": {"def": ["src/lib.rs:9:8"]},
 "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8", "end": "src/lib.rs:3:1"},
                             {"name": "helper", "at": "src/lib.rs:5:8", "end": "src/lib.rs:7:1"},
                             {"name": "fresh", "at": "src/lib.rs:9:8", "end": "src/lib.rs:11:1"}]}}
EOF
LG -C "$LA" init -q -b main && LG -C "$LA" add -A && LG -C "$LA" commit -qm init
cat >>"$CONFIG" <<EOF

[code.server.rust]
command = ["python3", "$ROOT/tests/e2e/fake-lsp.py"]
EOF
rade source add "$LA" >/dev/null
rade debug action color_scheme_light >/dev/null
FT=$(rade worktree add api feat)
cat >"$FT/src/lib.rs" <<'EOF'
pub fn greet(name: &str) -> String {
    format!("hello {name}")
}

pub fn helper() -> String {
    greet("x")
}

pub fn fresh() -> String {
    greet("fresh")
}
EOF
RV=$(rade review open --source api --worktree feat)
sleep 1.5
viewer() { state | jq -r '[.. | objects | select(.kind?.type? == "viewer") | .id][0] // empty'; }
where() { local v; v=$(viewer); [[ -n $v ]] && rade debug viewer "$v" info --json | jq -r '"\(.path | sub(".*/feat[^/]*/"; "")):\(.line):\(.cursor_col)"'; }
click_word() { # a real click (with modifiers) on a word of a diff line: "ROW TEXT|WORD"
    local x y
    read -r x y <<<"$(rade debug review "$RV" locate_word "$1" --json 2>/dev/null | jq -r '"\(.x) \(.y)"')"
    [[ -n $x && $x != null ]] && click "$x" "$y" left "${@:2}"
}
rinfo() { rade debug review "$RV" info --json | jq -r "$1"; }

echo "-- R87 a real click on greet in a context line, then a real F12"
if click_word 'greet("x")|greet'; then
    want "the click put the point on greet (footer)" 3 "rinfo .note | cut -d' ' -f1" greet
    key f12
    want "F12: the Tab's Viewer at greet's definition" 5 where "src/lib.rs:1:8"
    sleep 0.6
    shot m29-review-f12-light
else
    bad "real click on greet in the diff (pointer not placed)"
fi

echo "-- R87 a real Ctrl+click on fresh (an added line), then Alt+Left"
if click_word 'pub fn fresh() -> String {|fresh' ctrl; then
    want "Ctrl+click: the Viewer jumps to lib.rs:9" 5 where "src/lib.rs:9:8"
    key alt+left
    want "… a real Alt+Left (in the Viewer) returns to lib.rs:1" 5 "where | cut -d: -f1,2" "src/lib.rs:1"
else
    bad "real Ctrl+click on fresh (pointer not placed)"
fi
v=$(viewer)
[[ -n $v ]] && rade debug viewer "$v" escape >/dev/null 2>&1 || true

echo "-- R88 a real click on the Impact toggle, then on a caller row"
sleep 1 # the Review widens once the Viewer is gone
read -r x y <<<"$(widget_xy rade-impact-toggle 0)"
if [[ -n $x && $x != null ]] && click "$x" "$y"; then
    want "Impact shown: ready with counts" 8 "rinfo '.impact | \"\(.state)|\(.summary)\"'" "ready|2 changed · 3 call sites in 3 files (1 in tests) · 1 removed"
    sleep 0.6
    shot m29-impact-light
    rade debug action color_scheme_dark >/dev/null
    sleep 0.8
    shot m29-impact-dark
    rade debug action color_scheme_light >/dev/null
    read -r x y <<<"$(rade debug review "$RV" impact_locate 'src/main.rs:2' --json | jq -r '"\(.x) \(.y)"')"
    if [[ -n $x && $x != null ]] && click "$x" "$y"; then
        want "a real click on the main.rs caller opens it" 5 "where | cut -d: -f1,2" "src/main.rs:2"
    else
        bad "real click on an Impact row (pointer not placed)"
    fi
else
    bad "real click on the Impact toggle (pointer not placed)"
fi

echo "-- M28 Low: no GPU fds in a Terminal's process or the language server"
printf '#!/usr/bin/env bash\nls -l /proc/$$/fd >%s/fds.txt\nsleep 20\n' "$T" >"$T/fds.sh" && chmod +x "$T/fds.sh"
P=$(rade spawn --source api --cmd "$T/fds.sh" --json | jq -r .pane)
want "the Terminal's process: only 0-2 and its script" 10 "[[ -s $T/fds.txt ]] && grep -E ' [0-9]+ -> ' $T/fds.txt | grep -vE ' [012] -> |fds.sh' | wc -l" 0
SP=$(rade code status --json | jq -r '.servers[0].pid // empty')
if [[ -n $SP ]]; then
    want "the language server holds no /dev/dri fds" 3 "ls -l /proc/$SP/fd 2>/dev/null | grep -c /dev/dri" 0
else
    bad "no language server running to inspect"
fi
rade close "$P" --force >/dev/null 2>&1 || true
