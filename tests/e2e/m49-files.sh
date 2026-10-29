#!/usr/bin/env bash
# M49 end-to-end tests, Files part (ADR-0006): Files actions (R129) — header
# (Refresh, Collapse All, root path), row menus for files and folders, Copy
# (Relative) Path, Insert Path in Agent, Reveal in Files, Open Terminal Here,
# keys. Headless Weston has no seat, so wl-paste can't see RADE's clipboard:
# a copy is read by pasting it (`rade debug action paste`) into a `cat -v`
# Terminal, as tests/e2e/m5.sh does. Reveal goes to a fake
# org.freedesktop.FileManager1 (tests/e2e/fake-filemanager.py) on the private
# bus. Screenshots, dark + light → target/e2e-m49/.
# Usage: tests/e2e/m49-files.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m49/files
source "$HERE/lib.sh"

# The tree's inotify watches are off (test mode only): changes show on Refresh.
export RADE_FILES_NO_WATCH=1
FAKE=$HERE/fake-agent.sh
mkdir -p "$XDG_CONFIG_HOME/rade"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.typed]
command = "$FAKE"
prompt = "type"
EOF

FM_LOG=$OUT/filemanager.log
: > "$FM_LOG"
python3 "$HERE/fake-filemanager.py" "$FM_LOG" 2>>"$OUT/filemanager.err" &
FM_PID=$!
trap 'kill $FM_PID 2>/dev/null; cleanup' EXIT
for _ in $(seq 50); do
    # NameHasOwner never activates a service (introspect would start the real file manager).
    gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.NameHasOwner org.freedesktop.FileManager1 2>/dev/null | grep -q true && break
    sleep 0.1
done

# ---- fixture: a git repo ----
X=$T/repo
mkdir -p "$X/src/deep/er" "$X/docs"
echo 'fn main() {}' > "$X/src/a b.rs"
echo 'x' > "$X/src/main.rs"
echo 'x' > "$X/src/deep/er/leaf.rs"
echo '# doc' > "$X/docs/guide.md"
echo '# R' > "$X/README.md"
git -C "$X" init -q
git -C "$X" -c user.name=t -c user.email=t@t -c commit.gpgsign=false add -A
git -C "$X" -c user.name=t -c user.email=t@t -c commit.gpgsign=false commit -qm init

tree() { rade debug tree "$@"; }
files() { rpc debug.ui "{\"op\":\"files\"${1:+,\"arg\":\"$1\"}}" | jq -c .files; }
menu() { rade debug menu files "$@"; }
rows() { tree rows | jq -r '[.rows[].path | sub("^'"$X"'/"; "")] | join(",")'; }
nrows() { tree rows | jq '.rows | length'; }
labels() { menu "$1" items | jq -r '[.items[].label] | join("|")'; }
export -f tree rpc
scene() { # name: a dark and a light shot
    rade debug action color_scheme_dark >/dev/null; sleep 0.8; shot "$1-dark"
    rade debug action color_scheme_light >/dev/null; sleep 0.8; shot "$1-light"
}

cat > "$T/sink.sh" <<'SH'
stty raw -echo
printf 'SINK READY\r\n\033[?2004h'
exec cat -v
SH
# The last bracketed paste on a sink's screen (cat -v shows ESC as ^[).
pasted() { rade read "$1" --scrollback | tr -d '\n' | sed -n 's/.*\^\[\[200~\(.*\)\^\[\[201~.*/\1/p'; }
sink="" # a Terminal running the sink; the clipboard is read by pasting into it
paste_clip() {
    rade focus "$sink" >/dev/null
    rade debug action paste >/dev/null
    sleep 0.4
    pasted "$sink"
}

echo "== start"
start_app
rade debug action color_scheme_light >/dev/null
rade source add "$X" --name repo >/dev/null
sink=$(rade spawn --cmd "bash $T/sink.sh" --source repo --name sink --json | jq -r .pane)
rade focus "$sink" >/dev/null
eventually 10 screen_has "$sink" 'SINK READY' || true
rade debug action toggle_file_tree >/dev/null
check "Files page shows the repo" eventually 10 is 'tree rows | jq -r .visible' true
eventually 5 is 'nrows' 4 || true

echo "== R129-b: header and menus"
H=$(files)
check "header lists ignored, refresh, collapse (right to left)" test "$(jq -c '[.buttons[].name]' <<<"$H")" = '["ignored","refresh","collapse"]'
check "Refresh tooltip" test "$(jq -r '.buttons[1].tooltip' <<<"$H")" = "Refresh"
check "Collapse All tooltip" test "$(jq -r '.buttons[2].tooltip' <<<"$H")" = "Collapse All"
check "Refresh icon" test "$(jq -r '.buttons[1].icon' <<<"$H")" = "view-refresh-symbolic"
check "title keeps the folder name, tooltip = full path (R129.2)" test "$(jq -c '[.title, .title_tooltip]' <<<"$H")" = "[\"repo\",\"$X\"]"
FILE_ITEMS="Open|Open to the Side|Copy Path|Copy Relative Path|Insert Path in Agent|Reveal in Files|Open Terminal Here"
check "file menu: 7 items in order" test "$(labels "$X/src/main.rs")" = "$FILE_ITEMS"
check "folder menu: 5 items in order" test "$(labels "$X/src")" = "Copy Path|Copy Relative Path|Insert Path in Agent|Reveal in Files|Open Terminal Here"
check "Insert Path in Agent is disabled without an Agent Pane" test "$(menu "$X/src/main.rs" items | jq '[.items[] | select(.label=="Insert Path in Agent") | .enabled][0]')" = false
check "the others are enabled" test "$(menu "$X/src/main.rs" items | jq '[.items[] | select(.label!="Insert Path in Agent") | .enabled] | all')" = true
check "Copy Path shows its key" test "$(menu "$X/src/main.rs" items | jq -r '.items[] | select(.label=="Copy Path") | .accel')" = "<Control><Alt>c"
check "a path outside the tree is refused" test "$(set +e; menu /etc/passwd items >/dev/null 2>&1; echo $?)" != 0

tree expand "$X/src" >/dev/null
sleep 0.4
menu "$X/src/main.rs" open >/dev/null; sleep 0.5
menu "$X/src/main.rs" shot "$OUT/files-menu-file-light.png" >/dev/null
menu "$X/src/main.rs" close >/dev/null
rade debug action color_scheme_dark >/dev/null; sleep 0.8
menu "$X/src" open >/dev/null; sleep 0.5
menu "$X/src" shot "$OUT/files-menu-folder-dark.png" >/dev/null
menu "$X/src" close >/dev/null
rade debug action color_scheme_light >/dev/null; sleep 0.5

echo "== R129-c: Copy Path, Copy Relative Path, title click"
menu "$X/src/a b.rs" activate "Copy Path" >/dev/null
check "Copy Path → the absolute path" eventually 5 is 'paste_clip' "$X/src/a b.rs"
menu "$X/src/a b.rs" activate "Copy Relative Path" >/dev/null
check "Copy Relative Path → relative to the root" eventually 5 is 'paste_clip' "src/a b.rs"
menu "$X/src" activate "Copy Relative Path" >/dev/null
check "a folder copies its relative path too" eventually 5 is 'paste_clip' "src"
files title >/dev/null
check "title click copies the root path" eventually 5 is 'paste_clip' "$X"
check "title click Toast" eventually 5 bash -c "rpc debug.ui '{\"op\":\"chrome\"}' | grep -qF 'Copied $X'"

echo "== R129.5: keys on the focused row"
tree focus "$X/src/main.rs" >/dev/null
sleep 0.3
rpc debug.ui '{"op":"key","keys":"ctrl+alt+c"}' >/dev/null
check "Ctrl+Alt+C copies the path" eventually 5 is 'paste_clip' "$X/src/main.rs"
tree focus "$X/src/main.rs" >/dev/null
rpc debug.ui '{"op":"key","keys":"ctrl+shift+alt+c"}' >/dev/null
check "Ctrl+Shift+Alt+C copies the relative path" eventually 5 is 'paste_clip' "src/main.rs"

echo "== R129-d: Insert Path in Agent"
ag=$(rade spawn --agent typed --name ag --source repo --json | jq -r .pane)
eventually 10 screen_has "$ag" 'fake-agent session' || true
check "the fake Agent is an Agent Pane" eventually 10 is "rade list --json | jq -r --arg p $ag '.panes[] | select(.pane==\$p) | .agent'" fake
rade focus "$ag" >/dev/null
check "Insert Path in Agent is enabled now" test "$(menu "$X/src/a b.rs" items | jq '[.items[] | select(.label=="Insert Path in Agent") | .enabled][0]')" = true
before=$(rade read "$ag" --scrollback | wc -l)
menu "$X/src/a b.rs" activate "Insert Path in Agent" >/dev/null
check "the quoted relative path is typed, no Enter (the screen read trims the trailing space)" eventually 5 screen_has "$ag" "'src/a b.rs'"
sleep 0.5
check "nothing was submitted (no REPLY line)" bash -c "! rade read $ag --scrollback | grep -q REPLY"
menu "$X/src" activate "Insert Path in Agent" >/dev/null
check "a folder inserts its path too" eventually 5 screen_has "$ag" "'src/a b.rs' src"
# Focus on the sink (not an Agent): the Tab's Agent Pane still takes it.
rade close "$ag" --force >/dev/null
check "no Agent Pane left → disabled again" eventually 5 is 'menu "$X/src/main.rs" items | jq "[.items[] | select(.label==\"Insert Path in Agent\") | .enabled][0]"' false

echo "== R129-e: Refresh and Collapse All (no inotify watches)"
check "folders expanded" eventually 5 bash -c "tree rows | jq -e '.rows[] | select(.path==\"$X/src\") | .expanded'"
tree expand "$X/src/deep" >/dev/null
sleep 0.3
echo y > "$X/src/new-file.rs"
mkdir -p "$X/zeta"
sleep 1.2
check "a new file is absent without a watch" test "$(rows | grep -c new-file.rs || true)" = 0
files refresh >/dev/null
check "present after Refresh" eventually 5 bash -c "tree rows | jq -e '[.rows[].path] | index(\"$X/src/new-file.rs\")'"
check "a new top-level folder too" eventually 5 bash -c "tree rows | jq -e '[.rows[].path] | index(\"$X/zeta\")'"
check "expanded folders stay expanded" test "$(tree rows | jq -c --arg a "$X/src" --arg b "$X/src/deep" '[.rows[] | select(.path==$a or .path==$b) | .expanded]')" = "[true,true]"
rm "$X/src/main.rs"
files refresh >/dev/null
check "a deleted file is gone after Refresh" eventually 5 bash -c "! tree rows | jq -e '[.rows[].path] | index(\"$X/src/main.rs\")'"
scene files-page
files collapse >/dev/null
check "Collapse All → only top-level rows" eventually 5 bash -c "tree rows | jq -e '[.rows[].depth] | max == 0'"
check "folders are collapsed" test "$(tree rows | jq '[.rows[] | select(.expanded)] | length')" = 0

echo "== R129-f: Open Terminal Here, Reveal in Files"
tabs_before=$(rade list --json | jq '[.panes[].tab] | unique | length')
menu "$X/src" activate "Open Terminal Here" >/dev/null
check "a new Tab" eventually 5 is "rade list --json | jq '[.panes[].tab] | unique | length'" $((tabs_before + 1))
check "its Terminal starts in src/" eventually 5 bash -c "rade list --json | jq -e '[.panes[] | select(.cwd==\"$X/src\")] | length == 1'"
menu "$X/docs/guide.md" activate "Open Terminal Here" >/dev/null
check "a file → its folder" eventually 5 bash -c "rade list --json | jq -e '[.panes[] | select(.cwd==\"$X/docs\")] | length == 1'"
menu "$X/src/a b.rs" activate "Reveal in Files" >/dev/null
check "Reveal in Files → ShowItems with the file URI" eventually 5 grep -qF "ShowItems file://$X/src/a%20b.rs" "$FM_LOG"
echo "  filemanager.log: $(cat "$FM_LOG" | tr '\n' ';')"

echo
echo "passed $PASS, failed $FAIL"
[[ $FAIL -eq 0 ]]
