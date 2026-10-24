# M1 live: default keys through the real keyboard path (compositor → GTK →
# RADE's key handler), keys that must reach the program, IBus Bamboo, split
# handle drag + restore, config live reload. Sourced by run.sh.

for n in pa pb; do mkdir -p "$T/files/$n" && git -C "$T/files/$n" init -q; done
rade source add "$T/files/pa" --name pa >/dev/null
rade source add "$T/files/pb" --name pb >/dev/null
proj() { state | jq -r .active_source; }
nth_source() { rade source list --json | jq -r ".sources[$1].source"; }

echo "-- M1-2 default keys"
key alt+3 # General (Sources are listed first, General last)
want "M1-2 Alt+3 → General" 3 proj general
T0=$(ntabs)
key ctrl+shift+t
want "M1-2 Ctrl+Shift+T opens a Tab" 3 ntabs $((T0 + 1))
PA=$(cur_pane)
eventually 5 screen_has "$PA" "" || true
key ctrl+shift+enter
want "M1-2 Ctrl+Shift+Enter splits right" 3 "q 'tab | .layout | \"\(.type) \(.dir)\"'" "split right"
PB=$(cur_pane)
key ctrl+shift+d
want "M1-2 Ctrl+Shift+D splits down" 3 "q 'tab | .layout.b.dir'" down
PC=$(cur_pane)
check "M1-2 split focuses the new Pane" test "$PA $PB $PC" == "$(tab_panes)" -a "$PC" != "$PB"
key ctrl+shift+h
want "M1-2 Ctrl+Shift+H focus left" 3 cur_pane "$PA"
key ctrl+shift+l ctrl+shift+k
want "M1-2 Ctrl+Shift+L, Ctrl+Shift+K focus right/up" 3 cur_pane "$PB"
key ctrl+shift+j
want "M1-2 Ctrl+Shift+J focus down" 3 cur_pane "$PC"
key ctrl+shift+z
want "M1-2 Ctrl+Shift+Z zooms" 3 "q 'tab | .zoomed'" "$PC"
shot m1-zoomed
key ctrl+shift+z
want "M1-2 Ctrl+Shift+Z again restores" 3 "q 'tab | .zoomed'" null
TB=$(cur_tab)
key ctrl+shift+pageup
want "M1-2 Ctrl+Shift+PageUp previous Tab" 3 "[[ \$(cur_tab) != $TB ]] && echo other" other
key ctrl+shift+pagedown
want "M1-2 Ctrl+Shift+PageDown next Tab" 3 cur_tab "$TB"
key alt+1
want "M1-2 Alt+1 → first Source" 3 proj "$(nth_source 0)"
key alt+2
want "M1-2 Alt+2 → second Source" 3 proj "$(nth_source 1)"
key alt+3
want "M1-2 Alt+3 → General, same Tab" 3 cur_tab "$TB"
key ctrl+shift+w
want "M1-2 Ctrl+Shift+W closes the focused Pane" 3 tab_panes "$PA $PB"
na "M1-2 Ctrl+Shift+N Add Source: opens the portal folder dialog, a window outside the test instance that the harness must not type into (owner PASS M1-3)"

echo "-- M1-2 keys reach the program"
S=$(sink keys)
key ctrl+c ctrl+d ctrl+r ctrl+v shift+enter enter
# Legacy (no kitty protocol) Shift+Enter is Ghostty's modifyOtherKeys form.
want "M1-2 Ctrl+C/D/R, Ctrl+V, Shift+Enter, Enter reach the program (^C^D^R^V^[[27;2;13~^M)" \
    3 "sink_text $S" '^C^D^R^V^[[27;2;13~^M'
rade close "$S" --force >/dev/null
S=$(sink kitty kitty)
key shift+enter enter
want "M1-2 Shift+Enter under the kitty protocol arrives as CSI 13;2u (not eaten)" \
    3 "sink_text $S" '^[[13;2u^M'
rade close "$S" --force >/dev/null

echo "-- M1-2 split handle drag, quit, relaunch"
rade focus "$PA" >/dev/null
sleep 0.3
read -r ax ay aw ah <<<"$(pane_box "$PA")"
read -r bx by bw bh <<<"$(pane_box "$PB")"
R0=$(q 'tab | .layout.ratio')
hx=$(jq -n "($ax + $aw + $bx) / 2") hy=$(jq -n "$ay + $ah / 2")
if drag "$hx" "$hy" "$(jq -n "$hx - 400")" "$hy"; then
    R1=$(q 'tab | .layout.ratio')
    check "M1-2 dragging the split handle 400 px left changes the ratio ($R0 → $R1)" \
        jtrue "($R0 - $R1) > 0.1"
    stop_app
    start_app
    check "M1-2 ratio kept after quit + relaunch" \
        jtrue "(($(q 'tab | .layout.ratio') - $R1) | fabs) < 0.002"
else
    bad "M1-2 split handle drag (real pointer could not be placed)"
fi

echo "-- M1-2 config live reload"
cp "$CONFIG" "$T/config.bak"
printf '\n[keybindings]\nnew_tab = "<Ctrl><Shift>y"\n' >>"$CONFIG"
sleep 1.5
T0=$(ntabs)
key ctrl+shift+y
want "M1-2 new_tab = Ctrl+Shift+Y (live reload) opens a Tab" 3 ntabs $((T0 + 1))
cp "$T/config.bak" "$CONFIG"
sleep 1.5
rade close "$(cur_pane)" --force >/dev/null

echo "-- M1-1 IME (IBus Bamboo, telex)"
C=$(rade spawn --cmd cat --name ime --json | jq -r .pane)
rade focus "$C" >/dev/null
sleep 0.5
ime_line() { # pane → typed "Tiếng Việt" + Enter: cat echoes the line back
    ib engine Bamboo
    sleep 0.3
    type_text "Tieesng Vieejt"
    sleep 0.5
    shot "$2"
    key enter
    ib engine xkb:us::eng
    eventually 3 is "rade read $1 | grep -cx 'Tiếng Việt'" "$3"
}
check "M1-1 Bamboo: 'Tieesng Vieejt' + Enter → 'Tiếng Việt' reaches the program" ime_line "$C" m1-ime-preedit 2
key ctrl+shift+enter
C2=$(cur_pane)
rade send "$C2" "exec cat" --enter >/dev/null
sleep 0.5
check "M1-1 Bamboo in a second split" ime_line "$C2" m1-ime-split 2
key alt+1
sleep 0.3
key alt+3
want "M1-1 back from another Source (Alt+1, Alt+3) the split is focused" 3 cur_pane "$C2"
check "M1-1 Bamboo after switching Sources" ime_line "$C2" m1-ime-source 4
na "M1-1 Super/Ctrl+Space input-source switch: taken by gnome-shell before any app (RADE can't see it); toggling it would change the owner's GNOME input-source order"
na "M1-4 cross-UID socket: needs sudo/another user (owner PASS 2026-09-30)"
na "M1-3 Add Source dialog: portal dialog outside the test window (owner PASS 2026-09-30)"
rade close "$C" --force >/dev/null
rade close "$C2" --force >/dev/null
