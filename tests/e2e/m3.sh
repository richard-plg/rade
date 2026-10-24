#!/usr/bin/env bash
# M3 end-to-end tests (ADR-0006): Notifications (PRD R3) — OSC 9/777/99,
# bell, `rade notify`, de-duplication, the sidebar panel (click, gone Pane,
# collapse, mark all read, clear), Unread counts, Ctrl+Shift+G, and desktop
# notifications (smart/always/off, click from the desktop, withdraw) against
# tests/e2e/notifyd.py on a private session bus. Driven only through `rade`.
# Usage: tests/e2e/m3.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m3
source "$HERE/lib.sh"

CONFIG=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade"
: > "$CONFIG"

state() { rade debug state; }
notes() { state | jq -c '.notifications // []'; }
count_notes() { notes | jq length; }
last_note() { notes | jq -c '.[-1]'; }
note_field() { notes | jq -r --argjson i "$1" ".[] | select(.id==\$i) | .$2"; }
unread() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .unread'; }
counts() { rade debug panel counts --json; }
desk() { rade debug desktop --json; }
desk_last() { desk | jq -c --argjson i "$1" '[.log[] | select(.id==$i and .event!="withdraw")][-1]'; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
as_pane() { local p=$1; shift; RADE_PANE_ID=$p RADE_SOCKET=$XDG_RUNTIME_DIR/rade/rade-dev.sock "$@"; }
focused() { state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | .focused'; }
active_source() { state | jq -r .active_source; }
notifyd_count() { grep -c '^NOTIFY' "$NOTIFYD_LOG" || true; }
set_mode() { # desktop_notify mode; waits for the live reload
    local n; n=$(grep -c "reloaded" "$OUT/app.log" || true)
    echo "desktop_notify = \"$1\"" > "$CONFIG"
    eventually 5 bash -c "[[ \$(grep -c reloaded '$OUT/app.log') -gt $n ]]"
}
new_note_since() { # count before → wait for one more entry
    eventually 5 bash -c "[[ \$(rade debug state | jq '.notifications // [] | length') -gt $1 ]]"
}

echo "== start"
start_app
g=$(rade list --json | jq -r '.panes[0].pane')
mkdir -p "$T/x"
rade source add "$T/x" >/dev/null
a=$(rade spawn --cmd bash --source x --name a --json | jq -r .pane)
h=$(rade spawn --cmd bash --source x --name h --json | jq -r .pane)
eventually 10 screen_has "$a" '$' || true
eventually 10 screen_has "$h" '$' || true
rade focus "$a"   # Source x, Tab 1 on screen; h's Tab 2 is hidden
tab_a=$(rade list --json | jq -r --arg p "$a" '.panes[] | select(.pane==$p) | .tab')
tab_h=$(rade list --json | jq -r --arg p "$h" '.panes[] | select(.pane==$p) | .tab')
check "h is in another (hidden) Tab" test "$tab_a" != "$tab_h"

echo "== R3.1/R3-a: OSC 9 in a hidden Tab"
n0=$(count_notes)
rade send "$h" "printf '\\e]9;hello\\a'" --enter
check "OSC 9 → Notification" new_note_since "$n0"
id_hello=$(last_note | jq -r .id)
check "on the exact Pane" test "$(note_field "$id_hello" pane)" = "$h"
check "text hello" test "$(note_field "$id_hello" text)" = hello
check "path Source / Worktree / Tab / Pane" test "$(note_field "$id_hello" path)" = "x / main / Tab 2 / h"
check "rade list: h unread 1" test "$(unread "$h")" = 1
check "Tab 2 unread 1, Tab 1 unread 0" test "$(counts | jq -c --arg a "$tab_a" --arg h "$tab_h" '[.tabs[$h], (.tabs[$a] // 0)]')" = "[1,0]"
check "Source x unread 1, total 1" test "$(counts | jq -c '[.sources.x, .total]')" = "[1,1]"
check "R3.5 smart: sent, reason 'pane not visible'" test "$(desk_last "$id_hello" | jq -r '[.event, .reason] | join(",")')" = "send,pane not visible"
check "notification server got it (Focus button)" eventually 5 grep -q "	x / main / Tab 2 / h	'hello'	.*'Focus'" "$NOTIFYD_LOG"

echo "== OSC 777 / OSC 99 / bell"
# Ghostty passes on at most one OSC notification per second (app-wide).
sleep 1.2
n0=$(count_notes)
rade send "$h" "printf '\\e]777;notify;Build;finished ok\\a'" --enter
check "OSC 777 → Notification" new_note_since "$n0"
check "OSC 777 title + body" test "$(last_note | jq -r '[.title, .text] | join("|")')" = "Build|finished ok"
# OSC 99: the pinned Ghostty fork has a kitty_notification parser that is not
# wired into its OSC dispatcher, so nothing reaches RADE (docs/checks/M3.md).
sleep 1.2
n0=$(count_notes)
rade send "$h" "printf '\\e]99;;hello99\\e\\\\'; echo OSC99-SENT" --enter
eventually 5 screen_has "$h" OSC99-SENT; sleep 1
echo "  info: OSC 99 entries: $(( $(count_notes) - n0 )) (0 expected with the pinned Ghostty)"
n0=$(count_notes)
rade send "$h" "printf '\\a'" --enter
check "bell in a hidden Pane → Notification" new_note_since "$n0"
check "bell text" test "$(last_note | jq -r '[.pane, .text] | join(",")')" = "$h,Bell"
n0=$(count_notes)
rade send "$a" "printf '\\a'; echo BELL-SENT" --enter
eventually 5 screen_has "$a" BELL-SENT; sleep 1
check "bell from the focused visible Pane → none" test "$(count_notes)" = "$n0"
check "h unread 3 (OSC 9, OSC 777, bell)" test "$(unread "$h")" = 3

echo "== rade notify + dedupe"
r=$(as_pane "$h" rade notify "from cli" --title T --json)
check "notify with RADE_PANE_ID → that Pane" test "$(echo "$r" | jq -r .pane)" = "$h"
id_cli=$(echo "$r" | jq -r .notification)
check "title/text stored" test "$(note_field "$id_cli" title)/$(note_field "$id_cli" text)" = "T/from cli"
check "no RADE_PANE_ID and no --pane → USAGE (exit 2)" test "$(exit_code rade notify "nowhere")" = 2
n0=$(count_notes)
d1=$(rade notify --pane "$h" "dup" --json | jq -r .notification)
d2=$(rade notify --pane "$h" "dup" --json | jq -r .notification)
d3=$(rade notify --pane "$h" "dup" --json | jq -r .notification)
check "burst of 3 identical → 1 entry" test "$(count_notes)" = $((n0 + 1))
check "repeats return notification null" test "$d1,$d2,$d3" = "$d1,null,null"
check "another Pane with the same text is separate" test "$(rade notify --pane "$a" "dup" --json | jq -r .notification)" != null
check "desktop: one per entry (dup sent once)" test "$(desk | jq --argjson i "$d1" '[.log[] | select(.id==$i and .event=="send")] | length')" = 1
check "rade list unread = panel pane count" test "$(unread "$h")" = "$(counts | jq --arg p "$h" '.panes[$p]')"

echo "== screenshot: panel with several entries, badges"
rade focus "$g"
sleep 0.5
shot panel-general
rade focus "$a"
sleep 0.5
shot panel-source-x

echo "== R3.3 click a Notification → its Source/Worktree/Tab/Pane, read"
before_close=$(grep -c '^CLOSE' "$NOTIFYD_LOG" || true)
rade focus "$g"
check "activate row → pane h" test "$(rade debug panel activate "$id_hello" --json | jq -r .pane)" = "$h"
check "Source x on screen" test "$(active_source)" = x
check "focus on h" test "$(focused)" = "$h"
check "h read (unread 0)" test "$(unread "$h")" = 0
check "all of h's entries read" test "$(notes | jq --arg p "$h" '[.[] | select(.pane==$p and (.read|not))] | length')" = 0
check "read in-app → desktop notifications withdrawn" eventually 5 bash -c "[[ \$(grep -c '^CLOSE' '$NOTIFYD_LOG') -gt $before_close ]]"
check "withdraw logged" test "$(desk | jq --argjson i "$id_hello" '[.log[] | select(.event=="withdraw" and .id==$i)] | length')" = 1

echo "== R3.4 focusing a Pane (Tab switch) reads it"
rade notify --pane "$a" "for a" >/dev/null
check "a unread ≥ 1" test "$(unread "$a")" -ge 1
rade debug action next_tab
check "Tab switch focuses a → read" eventually 3 is 'unread "$a"' 0

echo "== gone Pane: greyed, click only marks read"
z=$(rade spawn --cmd bash --source x --name z --json | jq -r .pane)
id_z=$(rade notify --pane "$z" "bye" --json | jq -r .notification)
rade close "$z" --force >/dev/null
view_before=$(focused)
check "activate gone → pane null" test "$(rade debug panel activate "$id_z" --json | jq -r .pane)" = null
check "gone entry read" test "$(note_field "$id_z" read)" = true
check "focus unchanged" test "$(focused)" = "$view_before"
check "path kept from when it arrived" test "$(note_field "$id_z" path)" = "x / main / Tab 3 / z"

echo "== Ctrl+Shift+G: newest unread"
rade focus "$g"
rade notify --pane "$a" "older" >/dev/null
sleep 1
id_new=$(rade notify --pane "$h" "newest" --json | jq -r .notification)
rade debug action jump_unread
check "jump_unread → newest unread's Pane" test "$(focused)" = "$h"
check "it is read" test "$(note_field "$id_new" read)" = true
rade debug action jump_unread
check "again → next newest (a)" test "$(focused)" = "$a"
check "default binding <Ctrl><Shift>u" grep -q 'jump_unread.*<Ctrl><Shift>g' "$ROOT/crates/rade-core/src/config.rs"

echo "== R3.5 desktop: click from the desktop focuses the Pane"
rade focus "$g"
before=$(notifyd_count)
id_d=$(rade notify --pane "$h" "desktop click" --json | jq -r .notification)
check "smart + hidden → sent" eventually 5 bash -c "[[ \$(grep -c '^NOTIFY' '$NOTIFYD_LOG') -gt $before ]]"
nid=$(grep '^NOTIFY' "$NOTIFYD_LOG" | tail -1 | cut -f1 | cut -d' ' -f2)
gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Invoke "$nid" default >/dev/null
check "desktop click (default action) → focus h" eventually 5 is focused "$h"
check "and read" test "$(note_field "$id_d" read)" = true
rade focus "$g"
before=$(notifyd_count)
id_d=$(rade notify --pane "$a" "focus button" --json | jq -r .notification)
eventually 5 bash -c "[[ \$(grep -c '^NOTIFY' '$NOTIFYD_LOG') -gt $before ]]"
nid=$(grep '^NOTIFY' "$NOTIFYD_LOG" | tail -1 | cut -f1 | cut -d' ' -f2)
button=$(grep '^NOTIFY' "$NOTIFYD_LOG" | tail -1 | grep -o "app.focus-pane('[^']*')" | head -1)
check "Focus button targets the Pane id" test "$button" = "app.focus-pane('$a')"
gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Invoke "$nid" "$button" >/dev/null
check "Focus button → focus a" eventually 5 is focused "$a"

echo "== R3.5 modes: off / always / smart on screen"
set_mode off
before=$(notifyd_count)
id_o=$(rade notify --pane "$h" "mode off" --json | jq -r .notification)
check "off: skipped, reason" test "$(desk_last "$id_o" | jq -r '[.event, .reason] | join(",")')" = "skip,desktop_notify = off"
set_mode always
id_w=$(rade notify --pane "$a" "mode always" --json | jq -r .notification)
check "always: sent even for the focused visible Pane" test "$(desk_last "$id_w" | jq -r '[.event, .reason, .on_screen] | join(",")')" = "send,desktop_notify = always,true"
set_mode smart
id_s=$(rade notify --pane "$a" "mode smart" --json | jq -r .notification)
rec=$(desk_last "$id_s")
echo "  smart, visible Pane: $rec"
check "smart + on screen: send iff window not active" test "$(echo "$rec" | jq -r 'if .window_active then [.event, .reason] == ["skip", "pane visible in the active window"] else [.event, .reason] == ["send", "window not active"] end')" = true
check "off sent nothing to the server" test "$(grep -c "mode off" "$NOTIFYD_LOG" || true)" = 0
check "no sound hint" bash -c "! grep -q sound '$NOTIFYD_LOG'"

echo "== screenshot: Unread on a Pane corner badge and an Agent caption"
s1=$(rade spawn --cmd bash --source x --name s1 --json | jq -r .pane)
s2=$(as_pane "$s1" rade spawn --cmd bash --name s2 --split right --json | jq -r .pane)
rade focus "$s2"
rade notify --pane "$s1" "left pane" >/dev/null
rade notify --pane "$s1" "left again" >/dev/null
rade notify --pane "$s2" "captioned" >/dev/null
check "s1 unread 2 (badge), s2 unread 1 (caption)" test "$(unread "$s1"),$(unread "$s2")" = "2,1"
sleep 0.5
shot pane-badges

echo "== R3.2 collapse (kept in state.json), mark all read, clear"
check "toggle → collapsed" test "$(rade debug panel toggle --json | jq -r .collapsed)" = true
sleep 0.5
shot panel-collapsed
stop_app
check "collapsed saved" test "$(jq -r .notifications_collapsed "$STATE")" = true
start_app
check "collapsed after restart" test "$(state | jq -r .notifications_collapsed)" = true
check "Notifications survive restart" test "$(count_notes)" -gt 5
check "toggle → expanded" test "$(rade debug panel toggle --json | jq -r .collapsed)" = false
check "restart sends no old Notification to the desktop" test "$(desk | jq '[.log[] | select(.event=="send")] | length')" = 0
rade notify --pane "$h" "one more" >/dev/null
rade debug panel mark_all_read >/dev/null
check "mark all read → total 0" test "$(counts | jq .total)" = 0
rade debug panel clear >/dev/null
check "clear → empty" test "$(count_notes)" = 0
shot panel-empty

echo
echo "M3 E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
stop_app
[[ $FAIL == 0 ]]
