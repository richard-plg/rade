#!/usr/bin/env bash
# M13 end-to-end tests (ADR-0006): basic actions (R19). `rade tab
# list|rename|close [--others|--right]` (Fan-out Tabs kept), the Tab menu
# (Rename, Duplicate, Close Others with its confirmation), Pane / Source
# rename (ids stable, uniqueness), Pause / Resume (processes gone, placeholder,
# kept across a restart, spawn refused, Agents resumed with their session),
# Remove Source (files kept), Open in VS Code / Files, the Terminal search
# bar (Ghostty's search: count, next / previous, close) and the Terminal
# menu (items, Open link, Split, Clear, a mouse-reporting program keeps its
# right-click). Light + dark screenshots of menus, placeholder, search bar.
# Usage: tests/e2e/m13.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m13
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade" "$XDG_DATA_HOME/applications"
# "Open in VS Code" / "Open in Files" land in logs (stand-ins, never real apps).
cat >"$T/editor.sh" <<EOF
#!/bin/sh
echo "\$@" >>$T/editor.log
EOF
cat >"$T/files.sh" <<EOF
#!/bin/sh
echo "\$@" >>$T/files.log
EOF
chmod +x "$T/editor.sh" "$T/files.sh"
cat >"$XDG_DATA_HOME/applications/rade-test-files.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Test Files
Exec=$T/files.sh %u
MimeType=inode/directory;
EOF
printf '[Default Applications]\ninode/directory=rade-test-files.desktop\n' >"$XDG_CONFIG_HOME/mimeapps.list"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[viewer]
editor = "$T/editor.sh"
[agents.fake]
command = "$FAKE"
session_id_flag = "--session-id"
resume = "$FAKE --resume {session_id}"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
spawn() { rade spawn "$@" --json | jq -r .pane; }
state() { rade debug state; }
tabs() { rade tab list --source "$1" --json | jq -r '.tabs[].tab'; }
label() { rade tab list --json | jq -r --arg t "$1" '.tabs[] | select(.tab==$t) | .label'; }
pname() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .name'; }
pstatus() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
tab_of() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .tab'; }
items() { rade debug menu "$@" | jq -r '[.items[].label] | join("|")'; }
dialog() { rade debug dialog | jq -r "$1"; }
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
# Processes of a Pane of THIS app (RADE_SOCKET too: another RADE numbers its
# Panes the same way). A script, so `bash -c` checks can run it.
cat >"$T/procs" <<'PY'
#!/usr/bin/env python3
import glob, os, sys
sock = os.path.join(os.environ['XDG_RUNTIME_DIR'], 'rade/rade-dev.sock').encode()
want = [b'RADE_PANE_ID=' + p.encode() for p in sys.argv[1:]]
n = 0
for f in glob.glob('/proc/[0-9]*/environ'):
    try: env = open(f, 'rb').read().split(b'\0')
    except OSError: continue
    if b'RADE_SOCKET=' + sock in env and any(w in env for w in want): n += 1
print(n)
PY
chmod +x "$T/procs"
procs() { "$T/procs" "$@"; }

R=$T/repo
mkdir -p "$R/src"
printf 'fn main() {}\n' >"$R/src/a.rs"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init
P2=$T/plain
mkdir -p "$P2"

echo "== start"
start_app
rade source add "$R" --name proj >/dev/null
t1p=$(spawn --source proj --tab new)
t2p=$(spawn --source proj --tab new)
t3p=$(spawn --source proj --tab new)
t4p=$(spawn --source proj --tab new)
mapfile -t T4 < <(tabs proj)
check "R19.1 tab list: 4 Tabs in proj" test "${#T4[@]}" = 4

echo "== R19.1 rade tab rename / close --right / --others"
rade tab rename "${T4[1]}" build >/dev/null
check "rename: label 'build'" test "$(label "${T4[1]}")" = build
check "a Tab addressed by name" test "$(rade tab rename build build2 --json | jq -r .tab)" = "${T4[1]}"
rade tab rename "${T4[1]}" "" >/dev/null
check "empty name: follows the title again (not 'build2')" bash -c "[[ \$(rade tab list --json | jq -r '.tabs[] | select(.tab==\"${T4[1]}\") | .renamed') == false ]]"
rade tab close "${T4[2]}" --right >/dev/null
check "close --right: only the 4th went" test "$(tabs proj | tr '\n' ' ')" = "${T4[0]} ${T4[1]} ${T4[2]} "
check "its Pane is gone from rade list" test -z "$(pstatus "$t4p"; rade list --json | jq -r --arg p "$t4p" '.panes[] | select(.pane==$p) | .pane')"
check "its process gets SIGHUP and goes" eventually 15 is "procs $t4p" 0
check "unknown Tab → NOT_FOUND (exit 4)" test "$(code rade tab close t_nope)" = 4

echo "== R19.7 New Tab goes right after the active Tab"
rade focus "$t1p" >/dev/null
rade debug action new_tab >/dev/null
eventually 5 bash -c "[[ \$(rade tab list --source proj --json | jq '.tabs | length') == 4 ]]"
mapfile -t N4 < <(tabs proj)
check "R19-e New Tab from the 1st of 3 Tabs is 2nd, the others keep their order" \
    test "${N4[0]} ${N4[2]} ${N4[3]}" = "${T4[0]} ${T4[1]} ${T4[2]}"
rade tab close "${N4[1]}" >/dev/null
eventually 5 bash -c "[[ \$(rade tab list --source proj --json | jq '.tabs | length') == 3 ]]"

echo "== R19.1 Fan-out Tabs are kept"
rade fanout --agents fake,fake --source proj "hello" >/dev/null
fan=$(rade fanout status fanout-1 --source proj --json | jq -r .tab)
check "a Fan-out Tab exists" test -n "$fan"
r=$(rade tab close "${T4[0]}" --others --json)
check "close --others: closed the 2 other plain Tabs" test "$(jq -r '.closed | length' <<<"$r")" = 2
check "…and kept the Fan-out Tab" test "$(jq -r '.kept[0].fanout' <<<"$r")" = fanout-1
check "closing the Fan-out Tab alone is refused (USAGE)" test "$(code rade tab close "$fan")" = 2
check "…naming rade fanout discard" grep -q "rade fanout discard fanout-1" "$T/err"
echo "== R19.3 a working Fan-out: pause needs --force"
check "pause refused (exit 2)" test "$(code rade source pause proj)" = 2
check "…saying --force" grep -q -- "--force" "$T/err"
rade fanout discard fanout-1 --source proj >/dev/null

echo "== R19.1 Tab menu (same items and actions as the right-click)"
a=$(spawn --source proj --tab new)
b=$(spawn --source proj --tab new)
c=$(spawn --source proj --tab new)
ta=$(tab_of "$a") tb=$(tab_of "$b") tc=$(tab_of "$c")
check "items of the middle Tab" test "$(items tab "$tb")" = "Rename…|Duplicate|Close|Close Other Tabs|Close Tabs to the Right"
check "the last Tab: no 'to the Right'" bash -c "! rade debug menu tab $(tabs proj | tail -1) | grep -q 'to the Right'"
rade send "$b" "cd $R/src" --enter >/dev/null # shell integration reports it (OSC 7)
check "OSC 7 cwd tracked" eventually 5 is "rade list --json | jq -r --arg p $b '.panes[] | select(.pane==\$p) | .cwd'" "$R/src"
n0=$(tabs proj | wc -l)
rade debug menu tab "$tb" activate Duplicate >/dev/null
check "Duplicate: one more Tab" eventually 5 is "tabs proj | wc -l" $((n0 + 1))
dup=$(tabs proj | grep -A1 "^$tb\$" | tail -1)
dp=$(rade tab list --json | jq -r --arg t "$dup" '.tabs[] | select(.tab==$t) | .panes[0]')
check "…right after the source, selected" test "$(rade tab list --json | jq -r --arg t "$dup" '.tabs[] | select(.tab==$t) | .active')" = true
check "…a shell at the source's cwd" eventually 10 screen_has "$dp" '$'
rade send "$dp" "pwd" --enter >/dev/null
check "…pwd = $R/src" eventually 5 screen_has "$dp" "$R/src"
rade debug menu tab "$tb" activate "Rename…" >/dev/null
sleep 0.3
check "Rename… opens the rename entry (renamed via it)" test "$(rade debug rename menu-name | jq -r .applied)" = true
check "…label menu-name" eventually 3 is "label $tb" menu-name
# Close Other Tabs with an Agent at work asks once.
ag=$(spawn --source proj --agent fake --tab new spin)
check "agent running" eventually 10 is "pstatus $ag" running
tag=$(tab_of "$ag")
rade debug menu tab "$ta" activate "Close Other Tabs" >/dev/null
check "a confirmation opens" eventually 3 is "dialog .open" true
check "…naming the working Agent" bash -c "rade debug dialog | jq -r .body | grep -q 'fake — running'"
sleep 0.6; shot tab-close-dialog-light
rade debug dialog respond cancel >/dev/null
check "Cancel: nothing closed" test "$(tabs proj | wc -l)" = $((n0 + 2))
rade debug menu tab "$ta" activate "Close Other Tabs" >/dev/null
sleep 0.3
rade debug dialog respond close >/dev/null
check "Close: only the right-clicked Tab is left" eventually 5 is "tabs proj" "$ta"
check "the Agent's process is gone" eventually 15 is "procs $ag" 0

echo "== R19.9 Pane ✕ and close confirmation"
paneui() { rade debug ui pane "{\"pane\":\"$1\"}" | jq -r ".pane.$2"; }
c1=$(spawn --source proj --tab new)
check "one Pane: no ✕" eventually 3 is "paneui $c1 close" false
rade focus "$c1" >/dev/null
c2=$(RADE_PANE_ID=$c1 spawn --split right)
check "…split in the same Tab" test "$(tab_of "$c2")" = "$(tab_of "$c1")"
check "R19-g split: both Panes have a ✕" eventually 3 bash -c "[ \"\$(rade debug ui pane '{\"pane\":\"$c1\"}' | jq -r .pane.close)\$(rade debug ui pane '{\"pane\":\"$c2\"}' | jq -r .pane.close)\" = truetrue ]"
check "…a shell's header is named Terminal" is "paneui $c1 header" Terminal
rade debug ui pane_close "{\"pane\":\"$c2\"}" >/dev/null
check "✕ asks" eventually 3 is "dialog .open" true
check "…naming the Pane and its title" eventually 3 bash -c "rade debug dialog | jq -r .heading | grep -q '^Close Pane “Terminal — .\+”?$'"
sleep 0.6; shot pane-close-dialog-light
rade debug dialog respond cancel >/dev/null
check "Cancel keeps the Pane" bash -c "rade list --json | jq -e --arg p $c2 '.panes[] | select(.pane==\$p)' >/dev/null"
rade debug ui pane_close "{\"pane\":\"$c2\"}" >/dev/null
sleep 0.3
rade debug dialog respond close >/dev/null
check "Close closes it" eventually 5 bash -c "! rade list --json | jq -e --arg p $c2 '.panes[] | select(.pane==\$p)' >/dev/null"
check "…and the last Pane loses its ✕" eventually 3 is "paneui $c1 close" false
rade focus "$c1" >/dev/null
rade debug action close_tab >/dev/null
check "close_tab asks, naming the Tab" eventually 3 bash -c "rade debug dialog | jq -r .heading | grep -q '^Close Tab “'"
rade debug dialog respond cancel >/dev/null
check "rade close never asks" test "$(code rade close "$c1")" = 0
check "…no dialog" is "dialog .open" false

echo "== R19.4 Pane rename"
rade rename "$a" server >/dev/null
check "rade list shows the name" test "$(pname "$a")" = server
check "addressable by it" rade read server
s2=$(spawn --source proj --split right)
check "a second 'server' in the same Worktree is refused (USAGE)" test "$(code rade rename "$s2" server)" = 2
check "an id-like name is refused" test "$(code rade rename "$s2" p_99)" = 2
rade rename "$s2" worker >/dev/null
rade focus "$a" >/dev/null
sleep 0.5
check "captions show the names" eventually 3 bash -c "rade debug window --widget rade-caption --json | jq -e '.widget.width > 0' >/dev/null"
shot pane-names-light
rade debug menu terminal "$s2" activate "Rename Pane…" >/dev/null
sleep 0.3
check "menu Rename Pane… → taken name refused in the entry" test "$(rade debug rename server | jq -r .error | grep -c 'already named')" = 1
check "…then a free one applies" test "$(rade debug rename builder | jq -r .applied)" = true
check "…name builder" test "$(pname "$s2")" = builder
rade rename builder "" >/dev/null
check "empty clears it" test "$(pname "$s2")" = null

echo "== R19.4 Source rename (id stable)"
rade source rename proj Shop >/dev/null
check "source list: name Shop, id proj" test "$(rade source list --json | jq -r '.sources[] | select(.source=="proj") | .name')" = Shop
check "addressable by the new name" test "$(spawn --source Shop --tab new | head -c2)" = p_
rade send "$a" 'echo "proj=$RADE_SOURCE"' --enter >/dev/null
check "RADE_SOURCE unchanged" eventually 5 screen_has "$a" "proj=proj"
check "a clash with another Source's name is refused" bash -c "rade source add $P2 --name other >/dev/null; [[ \$(set +e; rade source rename proj other >/dev/null 2>&1; echo \$?) == 2 ]]"
check "Source menu items" test "$(items source proj)" = "Rename…|Pause|Open in editor.sh|Open in Files|New Worktree…|Review Changes|Remove Source…"
check "General: Rename and Open in Files only" test "$(items source general)" = "Rename…|Open in Files"
rade debug menu source proj activate "Rename…" >/dev/null
sleep 0.3
check "menu Rename… renames through the entry" test "$(rade debug rename Store | jq -r .applied)" = true
check "…Store" eventually 3 is "rade source list --json | jq -r '.sources[] | select(.source==\"proj\") | .name'" Store

echo "== R19.2 Open in editor / Files"
rade debug menu source proj activate "Open in editor.sh" >/dev/null
check "editor ran with the Source folder" eventually 5 grep -qx "$R" "$T/editor.log"
rade debug menu source proj activate "Open in Files" >/dev/null
check "the folder's default app got it" eventually 5 grep -q "$R" "$T/files.log"

echo "== R19.3 Pause / Resume"
mkdir -p "$P2/pz"
rade source add "$P2/pz" --name pz >/dev/null
z1=$(spawn --source pz --agent fake --name zdone "hello")
rade wait "$z1" --until done --timeout 20 >/dev/null
sid=$(state | jq -r --arg p "$z1" '.. | objects | select(.id?==$p) | .session_id')
z2=$(spawn --source pz --agent fake --name zspin --tab current spin)
check "zspin running" eventually 10 is "pstatus $z2" running
zs=$(spawn --source pz --tab new)
check "processes run" eventually 10 bash -c "[[ \$($T/procs $z1) -gt 0 && \$($T/procs $z2) -gt 0 && \$($T/procs $zs) -gt 0 ]]"
rade focus "$z1" >/dev/null
rade debug menu source pz activate Pause >/dev/null
check "Pause with an Agent at work asks" eventually 3 is "dialog .open" true
check "…naming zspin" bash -c "rade debug dialog | jq -r .body | grep -q 'zspin — running'"
sleep 0.6; shot pause-dialog-light
rade debug dialog respond pause >/dev/null
check "paused (source list)" eventually 3 is "rade source list --json | jq -r '.sources[] | select(.source==\"pz\") | .paused'" true
check "every process stopped" eventually 15 is "procs $z1 $z2 $zs" 0
check "Panes kept" test "$(rade list --source pz --json | jq '.panes | length')" = 3
check "zspin was cut mid-turn: interrupted" test "$(pstatus "$z2")" = interrupted
check "placeholder with Resume shown" eventually 3 bash -c "rade debug window --widget rade-resume --json | jq -e '.widget.width > 0' >/dev/null"
sleep 0.6; shot paused-light
check "Source menu says Resume" bash -c "rade debug menu source pz | jq -r '[.items[].label] | join(\"|\")' | grep -q '|Resume|'"
check "spawn into it: USAGE (exit 2)" test "$(code rade spawn --source pz --tab new)" = 2
check "…suggesting rade source resume" grep -q "rade source resume pz" "$T/err"
check "send to its Pane: USAGE" test "$(code rade send "$z1" hi --enter)" = 2
check "fanout into it: USAGE" test "$(code rade fanout --agents fake,fake --source pz go)" = 2
check "pause again: no-op" test "$(code rade source pause pz)" = 0
echo "-- restart"
stop_app
start_app
check "still paused after a restart" test "$(rade source list --json | jq -r '.sources[] | select(.source=="pz") | .paused')" = true
sleep 1
check "nothing started at launch" is "procs $z1 $z2 $zs" 0
rade source resume pz >/dev/null
check "resumed (source list)" test "$(rade source list --json | jq -r '.sources[] | select(.source=="pz") | .paused')" = false
check "the fake Agent resumed into its session ($sid)" eventually 15 screen_has "$z1" "resumed $sid"
check "…without replaying its prompt" bash -c "! rade read $z1 --scrollback | grep -q 'thinking about: hello'"
check "zspin back interrupted, idle" test "$(pstatus "$z2")" = interrupted
check "the shell runs again" eventually 10 bash -c "[[ \$($T/procs $zs) -gt 0 ]]"
rade source pause pz >/dev/null
rade focus "$zs" >/dev/null
check "CLI pause (nothing working): no dialog, paused" test "$(rade source list --json | jq -r '.sources[] | select(.source=="pz") | .paused')" = true
sleep 1

echo "== R19.2 Remove Source (files kept)"
wt=$(rade worktree add proj side --json | jq -r .path)
rade debug menu source proj activate "Remove Source…" >/dev/null
check "Remove asks" eventually 3 is "dialog .open" true
check "…saying no files are deleted and the Worktrees stay" bash -c "rade debug dialog | jq -r .body | tr '\n' ' ' | grep -q 'No files are deleted.*Worktrees RADE created'"
sleep 0.6; shot remove-dialog-light
rade debug dialog respond remove >/dev/null
check "proj is gone" eventually 3 bash -c "! rade source list --json | jq -e '.sources[] | select(.source==\"proj\")' >/dev/null"
check "its folder is untouched" test -f "$R/src/a.rs"
check "its Worktree folder stays" test -d "$wt"

echo "== R19.5 Terminal search"
sp=$(spawn --source other --tab new)
rade focus "$sp" >/dev/null
check "shell up" eventually 10 screen_has "$sp" '$'
rade send "$sp" 'for i in $(seq 1 17); do echo "find-me $i Tiếng"; done; seq 1 300' --enter >/dev/null
check "output in scrollback" eventually 5 screen_has "$sp" "find-me 1 "
s() { rade debug search "$sp" "$@" | jq -r '"\(.open) \(.count)"'; }
rade debug search "$sp" open >/dev/null
check "start_search (Ctrl+Shift+F's binding) opens the bar, focused" eventually 3 is "rade debug search $sp | jq -r '\"\\(.open) \\(.focused)\"'" "true true"
rade debug search "$sp" type find-me >/dev/null
# 17 output lines + the typed command line.
check "18 matches, incl. scrollback" eventually 5 is "s | cut -d' ' -f2 | cut -d/ -f2" 18
rade debug search "$sp" next >/dev/null
check "Enter: a match is selected (n/18)" eventually 3 bash -c "rade debug search $sp | jq -r .count | grep -qE '^[1-9][0-9]*/18$'"
c1=$(rade debug search "$sp" | jq -r .count)
rade debug search "$sp" next >/dev/null
check "next moves on" eventually 3 bash -c "[[ \$(rade debug search $sp | jq -r .count) != $c1 ]]"
rade debug search "$sp" prev >/dev/null
check "previous comes back" eventually 3 is "rade debug search $sp | jq -r .count" "$c1"
rade debug search "$sp" type "Tiếng" >/dev/null
check "accented text matches (entry text as typed)" eventually 5 is "s | cut -d' ' -f2 | cut -d/ -f2" 18
rade debug search "$sp" type "find-me 1 " >/dev/null
check "a narrower needle: 1 match" eventually 5 is "s | cut -d' ' -f2 | cut -d/ -f2" 1
shot search-light
rade debug search "$sp" close >/dev/null
check "Esc/close hides it" test "$(rade debug search "$sp" | jq -r .open)" = false

echo "== R19.6 Terminal menu"
check "items" test "$(items terminal "$sp")" = "Paste|Select All|Search…|Clear|Split Right|Split Down|Zoom|Rename Pane…|Close Pane"
check "R19-f each item shows its key (configured RADE keys, the Terminal's own)" \
    test "$(rade debug menu terminal "$sp" | jq -r '[.items[] | "\(.label)=\(.accel)"] | join("|")')" \
    = "Paste=<Ctrl><Shift>v|Select All=<Ctrl><Shift>a|Search…=<Ctrl><Shift>f|Clear=|Split Right=<Ctrl><Shift>Return|Split Down=<Ctrl><Shift>d|Zoom=<Ctrl><Shift>z|Rename Pane…=|Close Pane=<Ctrl><Shift>w"
mkdir -p "$P2/src" && printf 'x\ny\n' >"$P2/src/b.rs"
rade send "$sp" "clear; cd $P2; echo see src/b.rs:2 here" --enter >/dev/null
check "path printed" eventually 5 bash -c "rade read $sp | grep -qx 'see src/b.rs:2 here'"
check "on a path: Open it first" bash -c "rade debug menu terminal $sp --text src/b.rs | jq -r '.items[0].label' | grep -q 'Open “src/b.rs:2”'"
check "R113-c …then Open to the Side (a file)" bash -c "rade debug menu terminal $sp --text src/b.rs | jq -r '.items[1].label' | grep -qx 'Open to the Side'"
r=$(rade debug menu terminal "$sp" rightclick --text "see")
check "right-click (no mouse reporting): menu opens" test "$(jq -r '"\(.consumed) \(.menu_open)"' <<<"$r")" = "false true"
check "…Ghostty selected the word: Copy offered" bash -c "rade debug menu terminal $sp | jq -r '.items[].label' | grep -qx Copy"
# (headless Weston has no seat for a real popup's grab: `open` shows it without)
rade debug menu terminal "$sp" close >/dev/null
rade debug menu terminal "$sp" open >/dev/null
sleep 0.4; rade debug menu terminal "$sp" shot "$OUT/terminal-menu-light.png" >/dev/null && echo "  shot target/e2e-m13/terminal-menu-light.png"
rade debug menu terminal "$sp" close >/dev/null
rade debug menu terminal "$sp" activate "Open" --text src/b.rs >/dev/null
check "Open → Viewer at line 2" eventually 5 bash -c "rade list --json | jq -e '.panes[] | select(.cwd==\"$P2/src/b.rs\")' >/dev/null"
n=$(rade list --json | jq '.panes | length')
rade debug menu terminal "$sp" activate "Split Down" >/dev/null
check "Split Down adds a Pane" eventually 3 is "rade list --json | jq '.panes | length'" $((n + 1))
rade debug menu terminal "$sp" activate Clear >/dev/null
check "Clear empties the screen" eventually 3 bash -c "! rade read $sp | grep -q 'see src'"
# A program that turned on mouse reporting keeps its right-clicks; Shift+right-click still opens the menu.
rade send "$sp" "printf '\\e[?1000h'; echo MOUSE-ON" --enter >/dev/null
check "mouse reporting on" eventually 5 bash -c "rade read $sp | grep -qx MOUSE-ON"
r=$(rade debug menu terminal "$sp" rightclick --text MOUSE-ON)
check "right-click goes to the program, no menu" test "$(jq -r '"\(.consumed) \(.menu_open)"' <<<"$r")" = "true false"
r=$(rade debug menu terminal "$sp" rightclick shift --text MOUSE-ON)
check "Shift+right-click: the menu" test "$(jq -r '"\(.consumed) \(.menu_open)"' <<<"$r")" = "false true"
rade debug menu terminal "$sp" close >/dev/null
rade send "$sp" "printf '\\e[?1000l'" --enter >/dev/null
rade debug menu source other open >/dev/null
sleep 0.4
rade debug menu source other shot "$OUT/source-menu-light.png" >/dev/null && echo "  shot target/e2e-m13/source-menu-light.png"
rade debug menu source other close >/dev/null
rade debug menu tab "$(tab_of "$sp")" open >/dev/null
sleep 0.4
rade debug menu tab "$(tab_of "$sp")" shot "$OUT/tab-menu-light.png" >/dev/null && echo "  shot target/e2e-m13/tab-menu-light.png"
rade debug menu tab "$(tab_of "$sp")" close >/dev/null

echo "== dark"
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not start"; exit 1; }
sleep 1
zs=$(rade list --source pz --json | jq -r '.panes[0].pane')
rade focus "$zs" >/dev/null
sleep 0.8
shot paused-dark
sp=$(spawn --source other --tab new)
rade focus "$sp" >/dev/null
check "shell up (dark)" eventually 10 screen_has "$sp" '$'
sd=$(RADE_PANE_ID=$sp spawn --split right)
check "dark: split → ✕ on both headers" eventually 3 is "paneui $sd close" true
sleep 0.5; shot pane-close-buttons-dark
rade close "$sd" >/dev/null
rade send "$sp" 'for i in $(seq 1 5); do echo "find-me $i"; done' --enter >/dev/null
check "printed" eventually 5 screen_has "$sp" "find-me 5"
rade debug search "$sp" open >/dev/null
rade debug search "$sp" type find-me >/dev/null
check "dark: 6 matches (5 lines + the command)" eventually 5 is "rade debug search $sp | jq -r .count | cut -d/ -f2" 6
rade debug search "$sp" next >/dev/null
sleep 0.4
shot search-dark
rade debug search "$sp" close >/dev/null
rade debug menu terminal "$sp" open >/dev/null
sleep 0.4
rade debug menu terminal "$sp" shot "$OUT/terminal-menu-dark.png" >/dev/null && echo "  shot target/e2e-m13/terminal-menu-dark.png"
rade debug menu terminal "$sp" close >/dev/null
rade debug menu source pz open >/dev/null
sleep 0.4
rade debug menu source pz shot "$OUT/source-menu-dark.png" >/dev/null && echo "  shot target/e2e-m13/source-menu-dark.png"
rade debug menu source pz close >/dev/null
rade debug menu source pz activate "Remove Source…" >/dev/null
sleep 0.5
shot remove-dialog-dark
rade debug dialog respond cancel >/dev/null
stop_app

echo
echo "M13: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
