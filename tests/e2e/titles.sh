#!/usr/bin/env bash
# Pane captions and Tab tooltips follow terminal titles (R1, docs/internals/checks/M12-titles.md;
# since 1.0.0 the Tab label is the user's, Agent's or Worktree's name, R103.2; since
# M51 an Agent Tab shows its lead session's title):
# OSC 0/2 sanitized (spinner glyphs stripped), `rade list --json` title,
# rename sticks over new titles and survives a relaunch, an Agent Grid
# Parent's title drives its Tab, throttled spinner. Headless Weston (ADR-0006).
# Usage: tests/e2e/titles.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-titles
source "$HERE/lib.sh"

mkdir -p "$XDG_CONFIG_HOME/rade"
cat > "$XDG_CONFIG_HOME/rade/config.toml" <<EOF
[agents.lead]
command = "bash"
EOF
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
# Raw socket call (debug.tabs has no CLI subcommand).
rpc() { python3 - "$SOCK" "$1" "${2:-{\}}" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": sys.argv[2], "params": json.loads(sys.argv[3])}) + "\n").encode())
buf = b""
while not buf.endswith(b"\n"): buf += s.recv(65536)
r = json.loads(buf); print(json.dumps(r.get("result", r.get("error"))))
PY
}
tabs() { rpc debug.tabs; }
label() { tabs | jq -r --arg t "$1" '.tabs[] | select(.tab==$t) | .label'; }
tooltip() { tabs | jq -r --arg t "$1" '.tabs[] | select(.tab==$t) | .tooltip'; }
info() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
tip_has() { tooltip "$1" | grep -qF -- "$2"; }
rename() { rpc debug.tabs "$(jq -nc --arg n "$1" '{op: "rename", name: $n}')" | jq -e '.tabs' >/dev/null; }
# A title from the program in a Pane: printf then hold (no prompt redraws it).
title_cmd() { printf "printf '\\\\e]2;%s\\\\a'; exec sleep 600" "$1"; }

echo "== start"
start_app
first=$(rade list --json | jq -r '.panes[0].pane')
t1=$(info "$first" tab)
# 1.0.0 (R103.2): a Tab without a user or Agent name shows its Worktree's
# name; the shell's own title (its cwd) goes to the tooltip.
check "shell Tab label = its Worktree 'main' (got '$(label "$t1")')" eventually 5 is "label $t1" "main"
check "…its title '~' in the tooltip" eventually 5 tip_has "$t1" "~"

echo "== OSC 2 with Claude's ✳ → label, list --json title"
p=$(rade spawn --tab new --cmd "$(title_cmd '✳ Refactor auth')" --json | jq -r .pane)
t=$(info "$p" tab)
rade focus "$p" >/dev/null
check "label stays the Worktree's 'main', not the title (got '$(label "$t")')" eventually 5 is "label $t" "main"
check "list --json title" test "$(info "$p" title)" = "Refactor auth"
check "tooltip has the full title" tip_has "$t" 'Refactor auth'

echo "== spinner frames (Codex braille, 20/s) collapse to one title"
long=$(printf 'word%.0s ' {1..30})
spin="for i in \$(seq 40); do for g in ⠋ ⠙ ⠹ ⠸; do printf '\\e]0;%s Build docs\\a' \$g; sleep 0.05; done; done; printf '\\e]0;$long\\a'; exec sleep 600"
s=$(rade spawn --tab new --cmd "$spin" --json | jq -r .pane)
ts=$(info "$s" tab)
check "spinner → 'Build docs'" eventually 5 is "info $s title" "Build docs"
check "the long title arrives in the tooltip" eventually 15 tip_has "$ts" "word word word word word word word word word word"
check "…and never reaches the label" is "label $ts" "main"
check "list keeps the long title whole" eventually 5 is "info $s title | wc -c" "$(( ${#long} ))"

# Distinct titles at 20/s: throttled to ≤ 4/s, the last one always lands.
c=$(rade spawn --tab new --cmd "for i in \$(seq 40); do printf '\\e]2;step %s\\a' \$i; sleep 0.05; done; exec sleep 600" --json | jq -r .pane)
check "throttled titles end on the last one ('step 40')" eventually 10 is "info $c title" "step 40"

echo "== rename sticks over new titles"
rade focus "$p" >/dev/null
sleep 0.3
check "debug.tabs rename via the rename entry" rename 'auth work'
check "label = 'auth work'" eventually 3 is "label $t" "auth work"
# A new title in the same Pane: the label stays, the tooltip follows.
q=$(rade spawn --split right --cmd "$(title_cmd '✻ Something else')" --json | jq -r .pane)
check "new title arrives (pane $q)" eventually 5 is "info $q title" "Something else"
rade focus "$q" >/dev/null
sleep 0.5
check "label still 'auth work'" is "label $t" "auth work"
check "tooltip shows the new title" tip_has "$t" 'Something else'

echo "== Agent Grid Parent's title drives its Tab"
lead=$(rade spawn --agent lead --name lead --json | jq -r .pane)
tl=$(info "$lead" tab)
eventually 10 screen_has "$lead" '$' || true
rade send "$lead" "rade spawn --agent lead --name kid >/dev/null; printf '\\e]2;✶ Lead plan\\a'; exec sleep 600" --enter
check "kid in lead's grid" eventually 10 bash -c "rade list --json | jq -e '.panes[] | select(.name==\"kid\" and .parent==\"$lead\")' >/dev/null"
kid=$(rade list --json | jq -r '.panes[] | select(.name=="kid") | .pane')
eventually 10 screen_has "$kid" '$' || true
rade send "$kid" "printf '\\e]2;⠙ Kid task\\a'; exec sleep 600" --enter
check "kid title" eventually 5 is "info $kid title" "Kid task"
rade focus "$kid" >/dev/null
check "grid Tab label = Parent's name 'lead' while kid is focused, Agent count 2 (R103.2)" eventually 5 is "label $tl; tabs | jq -r --arg t $tl '.tabs[] | select(.tab==\$t) | .count'" "lead
2"
check "…the Parent's title 'Lead plan' in the tooltip" tip_has "$tl" "Lead plan"
shot titles
rpc debug.tabs | jq -c '.tabs[] | [.tab, .label]' > "$OUT/labels-before.txt"

echo "== R103-m: an Agent Tab is labelled by its lead session's title (M51)"
a1=$(rade spawn --agent lead --tab new --json | jq -r .pane)
a2=$(rade spawn --agent lead --tab new --json | jq -r .pane)
ta1=$(info "$a1" tab) ta2=$(info "$a2" tab)
eventually 10 screen_has "$a2" '$' || true
check "R103-m before a title: the Preset 'lead'" eventually 5 is "label $ta1" "lead"
rade send "$a1" "printf '\\e]2;✳ Fix login\\a'; exec sleep 600" --enter
rade send "$a2" "printf '\\e]2;Tính năng projects trên SCRATCH\\a'; exec sleep 600" --enter
check "R103-m Tab 1 = 'Fix login'" eventually 5 is "label $ta1" "Fix login"
check "R103-m Tab 2 = the title cut to 24 characters" eventually 5 is "label $ta2" "Tính năng projects trên…"
check "R103-m …the whole title in its tooltip" tip_has "$ta2" "Tính năng projects trên SCRATCH"
rade focus "$a2" >/dev/null
shot tabs-session
rade close "$a1" >/dev/null; rade close "$a2" >/dev/null

echo "== relaunch: custom name restored, titles kept until programs set new ones"
stop_app
check "last title persisted in state.json" test "$(jq -r --arg p "$p" '.. | objects | select(.id?==$p) | .title' "$STATE")" = "Refactor auth"
start_app
check "renamed Tab label 'auth work' after relaunch" eventually 5 is "label $t" "auth work"
check "rename persisted in state.json" test "$(jq -r --arg t "$t" '.. | objects | select(.id?==$t) | .renamed' "$STATE")" = true
rade focus "$p" >/dev/null 2>&1 || true
check "empty rename → follows the title again" rename ''
# Restored Panes are shells again; theirs is the new title.
check "label back to the Worktree's name 'main'" eventually 5 is "label $t" "main"
shot relaunch

echo
echo "titles: $PASS passed, $FAIL failed"
((FAIL == 0))
