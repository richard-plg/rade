# M11 live: the all-files Review Pane scrolled with the real mouse wheel
# (frame times measured on the real GPU), a Review Comment made with a real
# click + keys that follows its line when the file is edited (then goes
# outdated), and a real Codex turn cut by kill -9 → restored `interrupted`,
# then one tiny prompt finishes it. Codex: one interrupted turn + one "reply
# PONG" turn; it trusts only the temp repo through a `-c` override on its
# own Preset (`m11codex`), ~/.codex/config.toml is not written. Everything
# lives in the run's temp dir.
X=$T/files/m11repo
mkdir -p "$X/src"
seq 1 6000 | sed 's/^/let a = /;s/$/;/' >"$X/src/big.rs"
for i in $(seq 1 30); do seq 1 50 | sed "s/^/line $i /" >"$X/src/f$i.py"; done
printf 'fn main() {\n    let count = 1;\n    println!("{}", count);\n}\n' >"$X/src/a.rs"
G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
G -C "$X" init -q -b main && G -C "$X" add -A && G -C "$X" commit -qm init
python3 - "$X" <<'PY'
import sys; r = sys.argv[1]
L = open(r + '/src/big.rs').read().splitlines()
for i in range(0, 5000, 2): L[i] += ' // changed'
open(r + '/src/big.rs', 'w').write('\n'.join(L) + '\n')
for i in range(1, 31):
    p = f'{r}/src/f{i}.py'; L = open(p).read().splitlines(); L[10] = 'changed'; L[30] = 'also'
    open(p, 'w').write('\n'.join(L) + '\n')
PY
printf 'fn main() {\n    let count = 2;\n    println!("{}", count);\n}\n' >"$X/src/a.rs"
rade source add "$X" --name l11 >/dev/null
L11=$(rade source list --json | jq -r '.sources[] | select(.name == "l11") | .source')
if ! grep -q '^\[agents.m11codex\]' "$CONFIG"; then
    printf '\n[agents.m11codex]\ncommand = "codex --no-daemon -c check_for_update_on_startup=false -c '"'"'sources={\\"%s\\"={trust_level=\\"trusted\\"}}'"'"'"\nhooks = "codex"\nresume = "codex --no-daemon -c check_for_update_on_startup=false resume {session_id}"\n' "$X" >>"$CONFIG"
    sleep 1 # live reload
fi
want "M11 config reloaded with the m11codex Preset" 5 "rade --json debug palette '>spawn m11codex' | jq -r '.results[0].title'" "Spawn m11codex"
proj_index() { rade source list --json | jq -r --arg p "$1" '[.sources[].name] | index($p) + 1'; }
rv() { rade debug review "$RV" info --json | jq -r "$1"; }
pstatus() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
pfield() { state | jq -r --arg p "$1" "[.. | objects | select(.id? == \$p) | $2][0]"; }

echo "-- M11-1 all-files Review, real wheel scroll"
key "alt+$(proj_index l11)"
want "M11-1 Alt+N → Source l11" 3 "state | jq -r .active_source" "$L11"
P0=$(cur_pane)
key ctrl+shift+r
sleep 1
RV=$(cur_pane)
want "M11-1 Ctrl+Shift+R: a Review Pane with all 32 files" 20 "rade debug review $RV info --json | jq -r '.files | length'" 32
want "M11-1 one list holds every file (no truncation)" 3 "rv .rows | awk '{print (\$1 > 8000)}'" 1
read -r x y <<<"$(rade debug review "$RV" locate 'let a = 3;' --json | jq -r '"\(.x) \(.y)"')"
y0=$(rv .scroll_y)
if pointer_to "$x" "$y" && at "$x" "$y"; then
    guard
    for _ in $(seq 30); do ydotool mousemove -w -x 0 -y -1 >/dev/null; sleep 0.03; done
    sleep 0.5
    want "M11-1 the real wheel scrolls the list" 3 "rv .scroll_y | awk -v a=$y0 '{print (\$1 != a)}'" 1
    shot m11-review-wheel
else
    bad "M11-1 pointer over the diff (not placed)"
fi
rade debug review "$RV" perf >/dev/null
want "M11-1 scrolled through 8,000+ rows (perf run done)" 15 "rv '.perf != null'" true
rade debug review "$RV" info --json | jq -c .perf | tee "$OUT/m11-perf.json"
# Smooth = 95% of frames within one 60 Hz frame; the slowest frame is the
# one-time highlighting of the 5,000-line file as it first appears.
want "M11-1 smooth on the real GPU (p95 < 17 ms)" 1 "jq -r '.p95_ms < 17' $OUT/m11-perf.json" true

echo "-- M11-2 a comment made with real input follows its line"
rade debug review "$RV" select src/a.rs >/dev/null
sleep 0.8
read -r x y <<<"$(rade debug review "$RV" locate 'let count = 2' --json | jq -r '"\(.x) \(.y)"')"
if click "$x" "$y"; then
    key c
    want "M11-2 'c' opens the comment bar" 3 "rv .comment_bar" true
    type_text "Rename count."
    key enter
    want "M11-2 comment on src/a.rs:2" 5 "rade review list --source l11 --json | jq -r '.comments[0] | \"\(.path):\(.line)\"'" src/a.rs:2
else
    bad "M11-2 click on the diff line (pointer not placed)"
fi
printf '// one\n// two\nfn main() {\n    let count = 2;\n    println!("{}", count);\n}\n' >"$X/src/a.rs"
want "M11-2 edited above: the comment moved to line 4" 10 "rade review list --source l11 --json | jq -r '.comments[0].line'" 4
want "M11-2 …its mark is on the new line on screen" 8 "rade debug review $RV info --json | jq -r '.gutter[]' | grep -c '^ *4|●\$'" 1
shot m11-reanchored
printf '// one\n// two\nfn main() {\n    println!("{}", 3);\n}\n' >"$X/src/a.rs"
want "M11-2 its line deleted: outdated" 10 "rade review list --source l11 --json | jq -r '.comments[0].outdated'" true
want "M11-2 …shown as outdated in the Review" 6 "rv .outdated" 1
sleep 0.3
shot m11-outdated

echo "-- M11-3 kill -9 during a real Codex turn → interrupted"
CX=$(rade spawn --source l11 --agent m11codex --name cx --split right \
    "Run this shell command: sleep 25. Then reply with exactly: PONG" --json | jq -r .pane)
want "M11-3 Codex reported its session (hook)" 90 "pfield $CX .session_id | grep -vc '^null\$'" 1
want "M11-3 mid-turn (running or waiting)" 5 "pstatus $CX | grep -cE '^(running|waiting)\$'" 1
SID=$(pfield "$CX" .session_id)
sleep 6 # inside the turn; the state is saved at least every 5 s
kill -9 "$APP_PID"
wait "$APP_PID" 2>/dev/null
APP_PID=
start_app
want "M11-3 restored as interrupted" 10 "pstatus $CX" interrupted
want "M11-3 …resumed into its session" 3 "pfield $CX .kind.command | grep -c 'resume.*$SID'" 1
rade wait "$CX" --until idle --timeout 90 >/dev/null
rade read "$CX" --scrollback >"$OUT/m11-codex-resumed.txt"
want "M11-3 …still interrupted once Codex is idle (no auto-continue)" 1 "pstatus $CX" interrupted
rade focus "$CX" >/dev/null
sleep 0.5
shot m11-codex-interrupted
rade send "$CX" "Do not run anything. Reply with exactly: PONG" --enter >/dev/null
want "M11-3 a prompt makes it work again: done" 180 "rade wait $CX --until done --timeout 5 2>/dev/null" done
want "M11-3 …and it replied PONG" 5 "rade read $CX --scrollback | grep -v 'exactly' | grep -c PONG | awk '{print (\$1 >= 1)}'" 1
shot m11-codex-pong
rade close "$CX" --force >/dev/null 2>&1
