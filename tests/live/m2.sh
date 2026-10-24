# M2 live: real Agents driven through `rade` from inside a Pane (R2-a), Session
# Restore of a real Codex, permission prompt → waiting (R10), agy, and the
# Agent Grid with the real mouse (Title Bar click, divider drag). Prompts are
# tiny ("reply with exactly: PONG") to keep token use minimal.
rade source add "$ROOT" --name repo >/dev/null

echo "-- M2-3 R2-a: spawn codex from a Pane, wait, read"
L=$(rade spawn --cmd bash --source repo --name lead --json | jq -r .pane)
rade focus "$L" >/dev/null
sleep 1
# The marker is split in the command so only the output can match it.
rade send "$L" "rade spawn --agent codex --name rev 'reply with exactly: PONG' >/dev/null && rade wait rev --until done --timeout 300 && rade read rev --scrollback | grep -q PONG && echo R2A-O''K" --enter >/dev/null
want "M2-3 from a Pane: rade spawn codex rev → wait done → read has PONG" 330 "screen_has $L R2A-OK && echo y" y
REV=$(rade list --json | jq -r '.panes[] | select(.name == "rev") | .pane')
# Codex's own OSC 9 on turn end ("PONG") is merged into the hook's "rev finished".
entries() { state | jq -r --arg p "$1" '[.notifications[] | select(.pane == $p)] | length'; }
sleep 2
want "R3.1 one Notification per finished turn (codex)" 1 "entries $REV" 1
want "R3.1 …hook text first, Codex's own OSC 9 merged in ('rev finished — …')" 1 \
    "state | jq -r --arg p $REV '[.notifications[] | select(.pane == \$p)][0].text | startswith(\"rev finished — \")'" true
check "R3.1 at most one new desktop banner for the turn (a merge replaces it in place)" test "$(notify_calls 'rev finished' new)" -le 1
want "M2-3 rev records Parent lead and Preset codex" 2 "rade list --json | jq -r '.panes[] | select(.name == \"rev\") | \"\(.parent) \(.agent) \(.status)\"'" "$L codex done"
check "M2-3 rade list --tree shows rev under lead" bash -c "rade list --tree | grep -A1 'lead' | grep -q 'rev'"
SID=$(q "[.. | objects | select(.id? == \"$REV\")][0].session_id // empty")
check "M2-3 codex reported its session id (hook)" test -n "$SID"
na "M2-3 Claude itself using the rade skill to do this: not re-run (token cost); owner PASS 2026-09-30"

echo "-- R11 restore: quit + relaunch resumes the real codex"
stop_app
start_app
CMD=$(q "[.. | objects | select(.id? == \"$REV\")][0].kind.command // empty")
check "R11 restored rev runs codex resume $SID ($CMD)" grep -q "resume.*$SID" <<<"$CMD"
want "R11 restored rev shows the earlier conversation (PONG)" 60 "screen_has $REV PONG && echo y" y
shot m2-restored
rade close "$REV" --force >/dev/null
rade close "$L" --force >/dev/null

echo "-- M2-4 / M3-5 permission prompt → waiting (hidden Tab) → desktop banner → Focus"
rade focus "$(rade spawn --cmd bash --source repo --name front --json | jq -r .pane)" >/dev/null
N0=$(notified "perm needs input")
NB0=$(notify_calls "perm needs input" new)
W=$(rade spawn --source repo --name perm --json \
    --cmd "CLAUDE_CONFIG_DIR=$HOME/.claude-work claude --permission-mode default 'Run this shell command: curl -s https://example.com'" | jq -r .pane)
want "M2-4 claude --permission-mode default asking to curl → waiting" 1 "rade wait $W --until waiting --timeout 180" waiting
want "M2-4 rade list: perm waiting, unread" 3 "rade list --json | jq -r '.panes[] | select(.pane == \"$W\") | \"\(.status) \(.unread > 0)\"'" "waiting true"
want "M3-5 desktop banner 'perm needs input' sent to the real notification server" 5 "echo \$((\$(notified 'perm needs input') > $N0))" 1
shot m2-waiting-hidden
sleep 3 # Claude's own OSC 777 for the same prompt arrives a moment later
# One event, one Notification: Claude Code's own OSC 777 ("Claude needs your
# permission") is merged into the hook's "perm needs input" (R3.1 coalescing).
want "R3.1 one Notification per permission prompt" 1 "entries $W" 1
want "R3.1 …'perm needs input — Claude Code: Claude needs your permission'" 1 \
    "state | jq -r --arg p $W '[.notifications[] | select(.pane == \$p)][0].text'" "perm needs input — Claude Code: Claude needs your permission"
check "R3.1 one new desktop banner for the prompt (Claude's OSC 777 replaced it in place)" test "$(notify_calls 'perm needs input' new)" -eq $((NB0 + 1))
check "R3.1 …and the replacement carries the merged text" test "$(notify_calls 'perm needs input — ' replace)" -ge 1
focus_action "$W"
want "M3-5 Focus action lands in the Claude Pane, unread cleared" 3 "echo \$(cur_pane) \$(rade list --json | jq -r '.panes[] | select(.pane == \"$W\") | .unread')" "$W 0"
rade close "$W" --force >/dev/null

echo "-- M2-5 agy"
AG=$(rade spawn --agent agy --source repo --name ag --json 'reply with exactly: PONG' | jq -r .pane)
want "M2-5 agy → done" 1 "rade wait $AG --until done --timeout 300" done
check "M2-5 agy answer has PONG" screen_has "$AG" PONG
check "M2-5 agy session id (conversationId) recorded" test -n "$(q "[.. | objects | select(.id? == \"$AG\")][0].session_id // empty")"
rade close "$AG" --force >/dev/null
na "M2-5 opencode: account has no funds (model call fails, not RADE); owner/coordinator PASS for done + session id"

echo "-- M2-6 Agent Grid with the real mouse"
G=$(rade spawn --cmd bash --source repo --name gl --json | jq -r .pane)
rade focus "$G" >/dev/null
sleep 1
# 9 Subagents with max_children = 10 (config): 8 tiled + 1 Title Bar.
rade send "$G" "rade status --agent claude; for i in 1 2 3 4 5 6 7 8 9; do rade spawn --cmd bash --name s\$i >/dev/null; done" --enter >/dev/null
want "M2-6 9 Subagents: one Title Bar" 20 "q 'tab | .title_bars | length'" 1
BAR=$(q 'tab | .title_bars[0].id')
if read -r x y <<<"$(widget_xy rade-title-bar)" && click "$x" "$y"; then
    want "M2-6 clicking the Title Bar (real mouse) zooms it" 3 "q 'tab | .zoomed'" "$BAR"
    shot m2-titlebar-zoom
    key ctrl+shift+z
    want "M2-6 Ctrl+Shift+Z restores" 3 "q 'tab | .zoomed'" null
else
    bad "M2-6 Title Bar click (pointer not placed)"
fi
S1=$(rade list --json | jq -r '.panes[] | select(.name == "s1") | .pane')
read -r ax ay aw ah <<<"$(pane_box "$G")"
read -r bx by bw bh <<<"$(pane_box "$S1")"
hx=$(jq -n "($ax + $aw + $bx) / 2") hy=$(jq -n "$ay + $ah / 2")
if drag "$hx" "$hy" "$(jq -n "$hx + 300")" "$hy"; then
    want "M2-6 dragging the grid divider stops auto-arranging" 3 "q 'tab | .grid // \"none\"'" none
    rade send "$G" "rade spawn --cmd bash --name s10 >/dev/null" --enter >/dev/null
    want "M2-6 next Subagent splits right of its Parent instead of re-tiling" 10 \
        "q '[tab | .layout | .. | objects | select(.type? == \"split\" and .a.pane.id? == \"$G\")][0] | \"\\(.dir) \\(.b.pane.name)\"'" "right s10"
    shot m2-after-drag
else
    bad "M2-6 divider drag (pointer not placed)"
fi
for p in $(rade list --json | jq -r '.panes[] | select(.name | test("^(gl|s[0-9]+|front)$")) | .pane'); do
    rade close "$p" --force >/dev/null
done
