# M14 phase 2 live (R21, R22; M45: R120, R121): the Changes and Search pages on
# the real desktop — real Ctrl+Shift+M / Ctrl+Shift+S, the 3 s refresh while the
# window is active, a real click on a row (opens its diff) and on its Stage Changes
# button, a typed commit
# message committed with a real Ctrl+Enter, Generate with a stand-in draft
# command (no Agent, no tokens), a typed search and a real click on a result
# → Viewer at the line. A temp repository; no remote, nothing pushed.
X=$T/files/m14b
mkdir -p "$X/src"
(cd "$X" && git init -q -b main && git config user.name live && git config user.email l@l.test \
    && printf 'fn alpha() {}\nfn beta() {}\n' > src/a.rs && printf '# Rules\n' > CLAUDE.md \
    && git add -A && git commit -qm "chore: init")
cat >>"$CONFIG" <<EOF

[scm]
draft_command = "cat >/dev/null; printf 'feat(R21): live draft\\\\n'"
EOF
sleep 1 # config.toml is reloaded on change
rpc() { python3 - "$RADE_SOCKET" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
print(json.dumps(json.loads(buf)["result"]))
PY
}
scm() { rpc debug.scm "${1:-{\}}"; }
# "M src/a.rs" of one group (merge | staged | changes) of the first card (M45 shape).
sect() { scm | jq -r --arg g "$1" '[.cards[0].groups[] | select(.kind==$g) | .rows[] | select(.kind=="file") | .letter + " " + .path] | join(",")'; }
rade source add "$X" --name l14b >/dev/null
P=$(rade spawn --source l14b --cmd bash --name l14b-sh --json | jq -r .pane)
rade focus "$P" >/dev/null
sleep 1

echo "-- R21.9 Ctrl+Shift+M, R21.8 refresh while active"
key ctrl+shift+m
want "real Ctrl+Shift+M shows the Changes page (a repository in no Project: source mode)" 3 "scm | jq -r '[.visible, .mode] | join(\" \")'" "true source"
printf 'fn alpha() { 1 }\nfn beta() {}\n' > "$X/src/a.rs"
want "an edit outside RADE shows up by itself (3 s poll, window active)" 7 "sect changes" "M src/a.rs"

echo "-- R120.3 real click on a row, R120.2 its Stage Changes button"
read -r x y <<<"$(widget_xy file-row)"
if click "$x" "$y"; then
    want "a real click on the row opens its diff in a Diff Pane" 5 "rpc debug.diff | jq -r .title" "a.rs (Working Tree)"
    want "…and stages nothing" 2 "sect staged" ""
    read -r x y <<<"$(widget_xy "Stage Changes")"
    if click "$x" "$y"; then
        want "the row's Stage Changes button stages it" 5 "sect staged" "M src/a.rs"
    else
        bad "pointer over Stage Changes (not placed)"
    fi
else
    bad "pointer over the row (not placed)"
fi

echo "-- R21.7 Generate (stand-in draft command), R21.4 Ctrl+Enter"
read -r x y <<<"$(widget_xy draft-button)"
if click "$x" "$y"; then
    want "Generate fills the message" 10 "scm | jq -r .cards[0].message" "feat(R21): live draft"
fi
shot m14b-changes
scm '{"op":"message","source":"l14b","text":""}' >/dev/null
read -r x y <<<"$(widget_xy commit-entry)"
if click "$x" "$y"; then
    type_text "feat: live commit"
    want "typed into the message box" 3 "scm | jq -r .cards[0].message" "feat: live commit"
    key ctrl+enter
    want "real Ctrl+Enter commits it" 5 "git -C '$X' log -1 --format=%s" "feat: live commit"
    want "…the box empties, nothing left to commit" 5 "scm | jq -r '.cards[0] | .message + \"|\" + ([.groups[] | select(.kind==\"staged\") | .count] | add // 0 | tostring)'" "|0"
else
    bad "pointer over the message box (not placed)"
fi

echo "-- R22.5 Ctrl+Shift+S, R22.3 click a result"
key ctrl+shift+s
want "real Ctrl+Shift+S: Search page, keyboard in its entry" 3 "rpc debug.find | jq -r '[.visible, .entry_focused] | join(\" \")'" "true true"
type_text "beta"
want "typed query finds it" 5 "rpc debug.find | jq -r .summary" "1 result in 1 file"
shot m14b-search
read -r x y <<<"$(widget_xy rade-search-hit)"
if click "$x" "$y"; then
    want "a real click on the result opens the Viewer at line 2" 5 "state | jq -c '[.. | objects | select(.kind?.type? == \"viewer\") | [.kind.path, .kind.line]] | .[-1]'" "[\"$X/src/a.rs\",2]"
else
    bad "pointer over the result (not placed)"
fi
rade debug action toggle_file_tree >/dev/null
rade close "$P" --force >/dev/null
rade source remove l14b >/dev/null
