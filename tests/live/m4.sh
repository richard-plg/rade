# M4 live: Ctrl+click with the real mouse (hover look, Viewer at the line), the
# Viewer's keys and header buttons, Markdown look and file tree icons in light
# and dark (screenshots to look at), Ctrl+Shift+E, the tree following Sources,
# git colours, a real click on a tree row, and the VS Code button with a
# separate VS Code instance. The smart-notification "window not active" branch
# (M3-3) runs while that VS Code window has focus.
X=$T/files/tp
mkdir -p "$X/src" "$X/docs" "$X/build"
seq -f 'fn line_%g() {}' 1 80 >"$X/src/main.rs"
printf 'one\ntwo\n' >"$X/src/lib.rs"
printf 'build/\n' >"$X/.gitignore"
echo out >"$X/build/out.o"
printf '# Live doc\n\nSome *text*, a table:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n```mermaid\ngraph LR; A-->B\n```\n' >"$X/docs/guide.md"
git -C "$X" init -q && git -C "$X" add -A && git -C "$X" -c user.email=t@t -c user.name=t commit -qm init
rade source add "$X" --name tp >/dev/null
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }
viewer() { q '[tab | .layout | .. | objects | select(.kind?.type? == "viewer")][0] | "\(.kind.path):\(.kind.line)"'; }
viewer_id() { q '[tab | .layout | .. | objects | select(.kind?.type? == "viewer")][0].id // "none"'; }
tree_row() { rade debug tree rows --json | jq -r --arg p "$1" ".rows[] | select(.path == \$p) | .$2"; }

echo "-- M4-1 Ctrl+click with the real mouse"
key "alt+$(proj_index tp)"
want "M4-4 Alt+N → Source tp" 3 "state | jq -r .active_source" "$(rade source list --json | jq -r '.sources[] | select(.name == "tp") | .source')"
P=$(cur_pane)
rade send "$P" "clear; echo 'see src/main.rs:42 and docs/guide.md'" --enter >/dev/null
# Wait for `clear` to have run: the command line also contains the path.
eventually 5 is "rade read $P | head -1 | cut -c1-7" "see src" || true
sleep 0.3
read -r x y <<<"$(text_xy "$P" 'main.rs:42 and')"
if pointer_to "$x" "$y" && at "$x" "$y"; then
    guard
    ydotool key 29:1 >/dev/null
    ydotool mousemove -x 3 -y 0 >/dev/null # motion with Ctrl held: Ghostty's link hover
    sleep 0.1
    ydotool mousemove -x -3 -y 0 >/dev/null
    sleep 0.5
    shot m4-ctrl-hover # look: underline + hand pointer on the path
    ydotool click 0xC0 >/dev/null
    ydotool key 29:0 >/dev/null
    want "M4-1 Ctrl+click (real) → Viewer at src/main.rs:42" 5 viewer "$X/src/main.rs:42"
    want "M4-1 the Viewer has focus" 3 "[[ \$(cur_pane) == \$(viewer_id) ]] && echo y" y
    sleep 0.5
    shot m4-viewer-line42
else
    bad "M4-1 Ctrl+click (pointer not placed)"
fi

echo "-- M4-2 Viewer keys and header"
rade open "$X/src/lib.rs" --pane "$P" >/dev/null
want "M4-2 rade open → same Viewer shows lib.rs" 3 viewer "$X/src/lib.rs:null"
key alt+left
want "M4-2 / R7.3 Alt+Left (real key) → back to main.rs at line 42" 3 viewer "$X/src/main.rs:42"
if read -r x y <<<"$(widget_xy 'Copy path')" && click "$x" "$y"; then
    CLIP_TOUCHED=1
    want "M4-2 Copy path button (real click) → the Wayland clipboard has the path" 3 "timeout 3 wl-paste -n" "$X/src/main.rs"
else
    bad "M4-2 Copy path click (pointer not placed)"
fi
V=$(viewer_id)
rade focus "$V" >/dev/null
key esc
want "M4-2 Esc (real key) closes the Viewer" 3 viewer_id none
want "M4-2 focus back on the Terminal" 3 cur_pane "$P"

echo "-- M4-4 file tree"
FT0=$(state | jq -r .file_tree)
key ctrl+shift+e
want "M4-4 Ctrl+Shift+E toggles the file tree" 3 "state | jq -r .file_tree" "$([[ $FT0 == true ]] && echo false || echo true)"
[[ $(state | jq -r .file_tree) == true ]] || key ctrl+shift+e
want "M4-4 tree rooted at tp" 3 "rade debug tree rows --json | jq -r .root" "$X"
key "alt+$(proj_index General)"
want "M4-4 Alt+N → the tree follows the Source" 3 "[[ \$(rade debug tree rows --json | jq -r .root) != $X ]] && echo y" y
key "alt+$(proj_index tp)"
want "M4-4 back → tp" 3 "rade debug tree rows --json | jq -r .root" "$X"
if read -r x y <<<"$(widget_xy 'Show files ignored by .gitignore')" && click "$x" "$y"; then
    want "M4-4 eye button (real click) shows ignored build/" 3 "tree_row $X/build ignored" true
    read -r x y <<<"$(widget_xy 'Show files ignored by .gitignore')" && click "$x" "$y"
else
    bad "M4-4 eye button click (pointer not placed)"
fi
echo new >"$X/new.txt"
want "M4-4 new file → untracked" 3 "tree_row $X/new.txt git" untracked
git -C "$X" add new.txt
want "M4-4 git add → colour updates within 2 s" 2 "[[ \$(tree_row $X/new.txt git) != untracked ]] && echo y" y
if read -r x y <<<"$(widget_xy "$X/docs")" && click "$x" "$y"; then
    want "M4-4 clicking a folder row (real click) expands it" 3 "tree_row $X/docs expanded" true
    sleep 0.3
    if read -r x y <<<"$(widget_xy "$X/docs/guide.md")" && click "$x" "$y"; then
        want "M4-4 clicking a file row (real click) opens it in the Viewer" 5 viewer "$X/docs/guide.md:null"
    else bad "M4-4 file row click (pointer not placed)"; fi
else
    bad "M4-4 folder row click (pointer not placed)"
fi
sleep 1.5 # Mermaid renders

echo "-- keyboard focus when a hand-started Agent registers (file tree shown)"
K=$(rade spawn --source tp --name kf --json | jq -r .pane)
rade focus "$K" >/dev/null
sleep 1.5
rade send "$K" "bash $T/sink.sh" --enter >/dev/null
eventually 10 screen_has "$K" 'SINK READY' || true
key a
# What an Agent's SessionStart hook does in a shell Pane (`clw`): the Pane
# becomes an Agent Pane and gets its caption.
rade status --agent claude --pane "$K" >/dev/null
sleep 1
key b
want "R1/R9 typing still reaches the Terminal after its Agent registers (file tree shown)" 3 "sink_text $K" ab
rade close "$K" --force >/dev/null

echo "-- M4-3 / M4-6 look: Markdown + tree icons, light and dark (screenshots)"
# (simulates the system preference; the owner's GNOME setting is not touched)
for scheme in prefer-light prefer-dark; do
    stop_app
    start_app ADW_DEBUG_COLOR_SCHEME=$scheme
    sleep 2.5
    shot "m4-look-${scheme#prefer-}"
done
na "M4-3/M4-6 look (typography, icon sharpness): judged by eye from target/live/m4-look-{light,dark}.png at the real 2560x1440 scale 1; comparison with VS Code's own tree not automated"
stop_app
start_app

echo "-- M4-5 VS Code button (separate instance) + M3-3 window not active"
vs_windows() { timeout 20 code --user-data-dir "$T/vscode" --extensions-dir "$T/vscode-ext" --status 2>/dev/null | grep -c 'window \[' ; }
vs_titles() { timeout 20 code --user-data-dir "$T/vscode" --extensions-dir "$T/vscode-ext" --status 2>/dev/null | grep 'window \['; }
rade focus "$P" >/dev/null
rade open "$X/src/main.rs:5" --pane "$P" >/dev/null
sleep 1
if read -r x y <<<"$(widget_xy 'Open in VS Code')" && click "$x" "$y"; then
    want "M4-5 VS Code button (real click) → one VS Code window on tp" 60 vs_windows 1
    want "M4-5 window shows main.rs" 10 "vs_titles | grep -c main.rs" 1
    # RADE is now in the background: smart must send for the Pane on screen.
    if ! active; then
        sleep 1.2
        rade send "$P" "printf '\\e]9;while away\\a'" --enter >/dev/null
        want "M3-3 window not active → banner for the Pane on screen (smart)" 5 "notified 'while away'" 1
        want "M3-3 desktop log: send, window not active" 3 "rade debug desktop --json | jq -r '.log[-1] | \"\(.event) \(.window_active)\"'" "send false"
    else
        na "M3-3 window-not-active branch: VS Code did not take focus"
    fi
    # Second click: RADE is not the active window, so no real input may be
    # sent; the button's own handler (debug viewer open) runs instead.
    rade open "$X/src/lib.rs:2" --pane "$P" >/dev/null
    sleep 0.5
    rade debug viewer "$(viewer_id)" open >/dev/null
    sleep 4
    want "M4-5 again with another file → still one window, now lib.rs" 20 "vs_titles | grep -c 'lib.rs'" 1
    check "M4-5 no second window" test "$(vs_windows)" -eq 1
else
    bad "M4-5 VS Code button click (pointer not placed)"
fi
vscode_kill
want "M4-5 test VS Code instance closed" 10 "vscode_pids | wc -w" 0
rade close "$P" --force >/dev/null
# GNOME does not give focus back to RADE when VS Code goes away, and present()
# without an activation token can't take it: a relaunched window gets it.
stop_app
start_app
