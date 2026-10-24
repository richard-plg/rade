# M3 live: OSC 9 from a hidden Tab → panel + a real GNOME notification (smart),
# its Focus action, withdrawal when read, smart suppression for the Pane on
# screen, Ctrl+Shift+G through the real keyboard. The "window not active"
# branch of smart runs in m4.sh while the test VS Code window has focus.
unread() { rade list --json | jq -r ".panes[] | select(.pane == \"$1\") | .unread"; }

echo "-- M3-1 OSC 9 from a hidden Tab"
F=$(rade spawn --cmd bash --name m3front --json | jq -r .pane)
rade focus "$F" >/dev/null
H=$(rade spawn --cmd bash --name m3hid --json | jq -r .pane)
sleep 1.5
rade send "$H" "printf '\\e]9;hello from tab 2\\a'" --enter >/dev/null
want "M3-1 panel entry, Pane unread 1" 5 "unread $H" 1
want "M3-1 real desktop notification sent (body 'hello from tab 2')" 5 "notified 'hello from tab 2'" 1
check "M3-1 summary is Source / Worktree / Tab / Pane" grep -qE 'string "[^/"]+ / main / Tab [0-9]+ / m3hid"' "$NOTIFY_LOG"
check "M3-1 actions: default + Focus button = app.focus-pane('$H')" \
    bash -c "grep -A6 'string \"hello from tab 2\"' '$NOTIFY_LOG' | grep -q \"app.focus-pane('$H')\""
want "M3-1 desktop log: send, pane not visible" 3 "rade debug desktop --json | jq -r '.log[-1] | \"\(.event) \(.reason)\"'" "send pane not visible"
shot m3-hidden-notification
C0=$(withdrawn)
focus_action "$H"
want "M3-1 Focus action → RADE on that Tab, Pane focused, entry read" 3 "echo \$(cur_pane) \$(unread $H)" "$H 0"
want "M3-2 read in RADE → notification withdrawn (CloseNotification)" 5 \
    "echo \$((\$(withdrawn) > $C0))" 1
na "M3-2 clicking the entry in GNOME's list (Super+V): gnome-shell UI, not driven (the action it runs is the one invoked above)"

echo "-- M3-3 smart: the Pane on screen in the active window"
sleep 1.2 # Ghostty passes on one OSC notification per second
rade send "$H" "printf '\\e]9;visible now\\a'" --enter >/dev/null
want "M3-3 panel entry for the focused Pane" 5 "rade debug state --json | jq -r '[.notifications[] | select(.text == \"visible now\")] | length'" 1
want "M3-3 no desktop banner (smart: skip, window active and Pane on screen)" 3 \
    "rade debug desktop --json | jq -r '.log[-1] | \"\(.event) \(.window_active) \(.on_screen)\"'" "skip true true"
check "M3-3 nothing reached the notification server" test "$(notified 'visible now')" -eq 0

echo "-- M3-4 Ctrl+Shift+G (real key) → newest unread"
rade focus "$F" >/dev/null
rade notify "older" --pane "$H" >/dev/null
sleep 1.1
N=$(rade spawn --cmd bash --name m3new --json | jq -r .pane)
rade notify "newest" --pane "$N" >/dev/null
sleep 0.3
key ctrl+shift+g
want "M3-4 Ctrl+Shift+G jumps to the newest unread Pane" 3 cur_pane "$N"
key ctrl+shift+g
want "M3-4 again → the next newest" 3 cur_pane "$H"
for p in "$F" "$H" "$N"; do rade close "$p" --force >/dev/null; done
