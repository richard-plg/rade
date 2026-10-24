# M5 live: the real Wayland clipboard (wl-copy on the owner's session) pasted
# with real Ctrl+Shift+V / Ctrl+V / middle click, into a raw sink and into real
# Agents; the Shift+Enter matrix (R6) typed with real keys into each Agent.
CLIP_TOUCHED=1 # cleanup restores the owner's text clipboard
PASTE=$XDG_CACHE_HOME/rade/paste
rade source list --json | jq -e '.sources[] | select(.name == "repo")' >/dev/null ||
    rade source add "$ROOT" --name repo >/dev/null
python3 - "$T/files/shot.png" <<'PY'
import struct, sys, zlib
w = h = 32
raw = b"".join(b"\0" + bytes([200, 60, 40, 255] * w) for _ in range(h))
chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
open(sys.argv[1], "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                              + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
PY
cp "$T/files/shot.png" "$T/files/My Shot.png"
# The last bracketed paste on a sink's screen.
pasted() { rade read "$1" --scrollback | tr -d '\n' | sed -n 's/.*\^\[\[200~\(.*\)\^\[\[201~.*/\1/p'; }
is_png() { [[ $(head -c 8 "$1" | od -An -tx1 | tr -d ' \n') == 89504e470d0a1a0a ]]; }
copy() { # mime < data: a real Wayland selection owned by wl-copy
    wl-copy --type "$1"
    sleep 0.3
}

echo "-- M5-1 / M5-3 real clipboard → Ctrl+Shift+V (real keys) into a raw sink"
S=$(sink paste)
copy image/png <"$T/files/shot.png"
key ctrl+shift+v
want "M5-1 image-only clipboard → a saved PNG's path is pasted" 5 \
    "pasted $S | grep -cE '^$PASTE/[0-9]{8}-[0-9]{6}-[0-9]{3}\.png\$'" 1
F=$(pasted "$S")
check "M5-1 the file is a 32x32 PNG (the clipboard image, re-encoded)" bash -c "file -b \"$F\" | grep -q \"PNG image data, 32 x 32\""
check "M5-1 paste file is -rw------- ($(stat -c %A "$F" 2>/dev/null))" test "$(stat -c %a "$F" 2>/dev/null)" = 600
rade close "$S" --force >/dev/null
S=$(sink text)
printf 'hello live clipboard' | copy text/plain
key ctrl+shift+v
want "M5-3 text clipboard → pasted as is" 5 "pasted $S" "hello live clipboard"
rade close "$S" --force >/dev/null
S=$(sink uris)
printf 'file://%s\r\n' "${T// /%20}/files/My%20Shot.png" | copy text/uri-list
key ctrl+shift+v
want "M5-3 a copied file (text/uri-list) → its quoted path" 5 "pasted $S" "'$T/files/My Shot.png'"
na "M5-3 copying in Nautilus itself: not driven (another app's UI); the uri-list it offers was put on the real clipboard instead"
rade close "$S" --force >/dev/null

echo "-- M5-3 primary selection: double-click a word, middle-click in another Pane"
A=$(rade spawn --cmd bash --name prim --json | jq -r .pane)
rade focus "$A" >/dev/null
sleep 1
rade send "$A" "clear; echo PRIMARYWORD" --enter >/dev/null
eventually 5 screen_has "$A" "PRIMARYWORD" || true
rade debug action split_right >/dev/null
sleep 0.3
B=$(cur_pane)
rade send "$B" "bash $T/sink.sh" --enter >/dev/null
eventually 5 screen_has "$B" "SINK READY" || true
# After `clear` the first occurrence is the echo's output; aim inside the word.
read -r x y <<<"$(rade debug window --pane "$A" --text 'PRIMARYWORD' --json | jq -r '"\(.target.x + 30) \(.target.y)"')"
if pointer_to "$x" "$y" && at "$x" "$y"; then
    guard
    ydotool click -r 2 -D 60 0xC0 >/dev/null # double click selects the word
    sleep 0.3
    read -r bx by bw bh <<<"$(pane_box "$B")"
    if click "$(jq -n "$bx + $bw / 2")" "$(jq -n "$by + $bh / 2")" middle; then
        want "M5-3 middle click pastes the primary selection" 5 "pasted $B" "PRIMARYWORD"
    else bad "M5-3 middle click (pointer not placed)"; fi
else
    bad "M5-3 double click (pointer not placed)"
fi
rade close "$A" --force >/dev/null
rade close "$B" --force >/dev/null

echo "-- M5-1 image into real Agents (Ctrl+Shift+V = RADE, Ctrl+V = the Agent's own)"
agent_up() { # name, spawn args... → pane, focused, idle at its input
    local p
    p=$(rade spawn --source repo --name "$1" --json "${@:2}" | jq -r .pane)
    rade focus "$p" >/dev/null
    rade wait "$p" --until idle --timeout 90 >/dev/null
    echo "$p"
}
CL=$(agent_up cl --agent claude)
copy image/png <"$T/files/shot.png"
key ctrl+shift+v
want "M5-1 Claude (clw preset): Ctrl+Shift+V attaches the image" 10 "rade read $CL | grep -c '\[Image #1\]'" 1
key ctrl+v
want "M5-1 Claude: Ctrl+V → Claude's own clipboard paste (a second image)" 10 "rade read $CL | grep -c '\[Image #2\]'" 1
shot m5-claude-images
rade close "$CL" --force >/dev/null
CX=$(agent_up cx --agent codex)
copy image/png <"$T/files/shot.png"
key ctrl+shift+v
want "M5-1 Codex: Ctrl+Shift+V attaches the image" 10 "rade read $CX | grep -ciE '\[image' | awk '{print (\$1 >= 1)}'" 1
shot m5-codex-image
rade close "$CX" --force >/dev/null
na "M5-1 'what is in this image?' answer: not asked (token cost); attachment shown by the Agent is checked"
na "M5-2 drag and drop from Nautilus/Firefox: needs driving another app's drag source; the drop handler is covered headless (debug drop)"

echo "-- M5-4 / R6 Shift+Enter matrix (real keys)"
R6=$OUT/r6.txt
: >"$R6"
three_lines() { # pane: "say PONG" / "only that" / "no more" on consecutive rows
    rade read "$1" | awk 'index($0, "say PONG") { i = NR } i && index($0, "only that") && NR == i + 1 { j = NR }
        j && index($0, "no more") && NR == j + 1 { ok = 1 } END { exit !ok }'
}
replied() { rade read "$1" --scrollback | grep -v 'say PONG' | grep -q 'PONG'; }
matrix() { # label, submit(yes/no), pane
    local label=$1 submit=$2 p=$3 nl sub=n/a
    type_text "say PONG"
    key shift+enter
    type_text "only that"
    key ctrl+j
    type_text "no more"
    sleep 1.5
    if three_lines "$p" && ! replied "$p"; then nl=PASS; ok "R6 $label: Shift+Enter and Ctrl+J add lines, nothing sent"
    else nl=FAIL; bad "R6 $label: Shift+Enter/Ctrl+J newline (screen: $(rade read "$p" | grep -E 'say PONG|only that|no more' | tr '\n' '|' | head -c 300))"; fi
    shot "m5-r6-$label"
    if [[ $submit == yes ]]; then
        key enter
        if eventually 120 replied "$p"; then sub=PASS; ok "R6 $label: Enter submits (reply PONG)"
        else sub=FAIL; bad "R6 $label: Enter submits"; fi
    fi
    echo "$label $nl $sub" >>"$R6"
    rade close "$p" --force >/dev/null
}
matrix claude yes "$(agent_up r6claude --agent claude)"
P=$(agent_up r6clw) # a shell Pane (the owner's zsh, where `clw` is an alias)
rade send "$P" "clw" --enter >/dev/null
rade wait "$P" --until '/bypass permissions/' --timeout 90 >/dev/null
rade wait "$P" --until idle --timeout 60 >/dev/null
# Work around the focus bug checked in m4.sh (Agent registering in a shell
# Pane while the file tree is shown drops keyboard focus): focus it again.
rade focus "$P" >/dev/null
matrix clw yes "$P"
matrix codex yes "$(agent_up r6codex --agent codex)"
matrix agy yes "$(agent_up r6agy --agent agy)"
matrix opencode no "$(agent_up r6oc --agent opencode)"
cat "$R6"
