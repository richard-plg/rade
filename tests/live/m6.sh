# M6 live: typing `clw` (real keys) in a shell with the file tree shown —
# the keyboard stays in the Terminal after it becomes an Agent Pane (A2
# regression), `/exit` → `exited` from zsh's OSC 133 (R10.2); a real agy
# turn keeps its hook-reported status (no fallback); status dots in light and
# dark (screenshots to look at, R10.3); kill -9 with 3 Sources and a real
# Claude Pane → identical layout and the same conversation resumed (R11).
rade source add "$ROOT" --name repo >/dev/null 2>&1 || true
repo=$(rade source list --json | jq -r '.sources[] | select(.name == "repo") | .source')
lst() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane == \$p) | .$2"; }

echo "-- A2 + R10.2: clw typed in a shell (file tree shown), then /exit"
[[ $(state | jq -r .file_tree) == true ]] || key ctrl+shift+e
H=$(rade spawn --source "$repo" --name hand --json | jq -r .pane)
rade focus "$H" >/dev/null
sleep 1.5
type_text "clw"
key enter
want "clw registers as an Agent (SessionStart hook) → caption appears" 40 "lst $H agent" claude
sleep 4 # Claude's UI settles
want "A2 keyboard focus is still on its Terminal (file tree shown)" 3 "rade debug window --json | jq -r .focus.pane" "$H"
type_text "/exit"
sleep 0.5
key enter
want "A2 typed keys reached Claude: /exit quit it; R10.2 zsh OSC 133 → exited" 30 "lst $H status" exited
want "R10.2 …status_source heuristic" 2 "lst $H status_source" heuristic
shot m6-hand-exited
rade close "$H" --force >/dev/null
[[ $(state | jq -r .file_tree) == false ]] || key ctrl+shift+e

echo "-- R10.2 real agy turn: hooks report, no fallback"
AG=$(rade spawn --agent agy --source "$repo" --name ag6 --json 'reply with exactly: PONG' | jq -r .pane)
want "agy → done" 1 "rade wait $AG --until done --timeout 300" done
sleep 8 # past idle_seconds: a fallback would have fired by now
want "agy status stays done, status_source hook" 1 "echo \$(lst $AG status) \$(lst $AG status_source)" "done hook"
rade close "$AG" --force >/dev/null
na "R10.2 agy permission prompt → waiting by output idle: agy 1.2.14 asks no permission for a plain reply; the idle rule is proven headless (tests/e2e/m6.sh fagy)"
na "R16.4 Codex hint: the owner's Codex hooks are trusted (would need untrusting them); proven headless (tests/e2e/m6.sh)"

echo "-- R10.3 status dots, light and dark"
dots() {
    local a b c d
    a=$(rade spawn --cmd bash --source "$repo" --name d-run --json | jq -r .pane)
    rade focus "$a" >/dev/null
    b=$(rade spawn --cmd bash --split right --source "$repo" --name d-wait --json | jq -r .pane)
    c=$(rade spawn --cmd bash --split down --source "$repo" --name d-done --json | jq -r .pane)
    d=$(rade spawn --cmd bash --source "$repo" --name d-err --json | jq -r .pane)
    for x in "$a:running" "$b:waiting" "$c:done" "$d:error"; do
        rade status "${x#*:}" --agent claude --pane "${x%%:*}" >/dev/null
    done
    rade focus "$a" >/dev/null
    sleep 1
}
for scheme in prefer-light prefer-dark; do
    stop_app
    start_app ADW_DEBUG_COLOR_SCHEME=$scheme
    dots
    shot "m6-dots-${scheme#prefer-}"
    for p in $(rade list --json | jq -r '.panes[] | select(.name | test("^d-")) | .pane'); do rade close "$p" --force >/dev/null; done
done
na "R10.3 dots readable in light and dark: judged by eye from target/live/m6-dots-{light,dark}.png (palette contrast is unit-tested ≥ 3:1)"
stop_app
start_app

echo "-- R11 kill -9 with 3 Sources and a real Claude Pane"
for k in k1 k2; do mkdir -p "$T/files/$k" && rade source add "$T/files/$k" >/dev/null; done
C=$(rade spawn --agent claude --source "$repo" --name kc --json 'reply with exactly: PONG' | jq -r .pane)
want "claude turn done" 1 "rade wait $C --until done --timeout 300" done
check "claude said PONG" screen_has "$C" PONG
SID=$(q "[.. | objects | select(.id? == \"$C\")][0].session_id // empty")
rade open "$ROOT/docs/internals/PRD.md:10" --pane "$C" >/dev/null
rade focus "$C" >/dev/null
sleep 2 # debounced save
shape() { jq -S '{active_source, notifications_collapsed, sources: [.sources[] | {id, name, active_worktree,
    worktrees: [.worktrees[] | {id, active_tab, tabs: [.tabs[] | {id, name, focused, zoomed, grid,
    panes: [.. | objects | select(.kind?) | {id, name, agent, parent, kind: (.kind | del(.command))}]}]}]}]}'; }
BEFORE=$(state | shape)
kill -9 "$APP_PID"
wait "$APP_PID" 2>/dev/null
APP_PID=
check "R11.1 state.json on disk = the model at kill -9" test "$(shape <"$STATE")" = "$BEFORE"
start_app
check "R11 restart: identical Sources (3 + General), Tabs, Layouts, Viewer" test "$(state | shape)" = "$BEFORE"
CMD=$(q "[.. | objects | select(.id? == \"$C\")][0].kind.command // empty")
check "R11 Claude resumes its session $SID ($CMD)" grep -q -- "$SID" <<<"$CMD"
want "R11 resumed Claude shows the earlier conversation (PONG)" 60 "screen_has $C PONG && echo y" y
shot m6-killed-restored
rade close "$C" --force >/dev/null
