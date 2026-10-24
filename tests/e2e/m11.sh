#!/usr/bin/env bash
# M11 end-to-end tests (ADR-0006): known-limit fixes. The Review Pane's
# all-files list (file headers, folds, jump list, comments, light + dark
# screenshots, a large-diff perf measurement), Review Comments re-anchored
# when files change (moved, outdated, still sent), Agents interrupted by a
# kill -9 mid-turn (status `interrupted`, Fan-out progress, Continue), and
# `rade close` / the close key on a Fan-out Agent. Temp git repo, temp HOME,
# the stand-in Agent, driven by the CLI.
# Usage: tests/e2e/m11.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m11
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
RECV=$T/received.txt
mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOF
idle_seconds = 1
[agents.fake]
command = "$FAKE"
[agents.sink]
command = "bash -c 'echo SINK READY; while IFS= read -r l; do printf \"%s\\\\n\" \"\$l\" >>$RECV; done'"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
R=$T/repo
mkdir -p "$R/src"
seq 1 6000 | sed 's/^/let a = /;s/$/;/' >"$R/src/big.rs"
for i in $(seq 1 30); do seq 1 50 | sed "s/^/line $i /" >"$R/src/f$i.py"; done
printf 'fn main() {\n    let count = 1;\n    println!("{}", count);\n}\n' >"$R/src/a.rs"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm init
# 5,000 changed lines in one file (2,500 removed + 2,500 added) + 30 small files.
python3 - "$R" <<'PY'
import sys; r = sys.argv[1]
L = open(r + '/src/big.rs').read().splitlines()
for i in range(0, 5000, 2): L[i] += ' // changed'
open(r + '/src/big.rs', 'w').write('\n'.join(L) + '\n')
for i in range(1, 31):
    p = f'{r}/src/f{i}.py'; L = open(p).read().splitlines(); L[10] = 'changed'; L[30] = 'also'
    open(p, 'w').write('\n'.join(L) + '\n')
PY
printf 'fn main() {\n    let count = 2;\n    println!("{}", count);\n}\n' >"$R/src/a.rs"

st() { rade fanout status "$1" --json | jq -r "$2"; }
bar() { rade debug fanout "$1" info | jq -r "$2"; }
status_of() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
kill_app() { kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=; }

echo "== start"
start_app
rade source add "$R" --name proj >/dev/null
sh0=$(rade list --json | jq -r '.panes[0].pane')

echo "== R13.1 (M11) all files in one list"
t0=$(date +%s%N)
rv=$(rade review open --source proj)
rinfo() { rade debug review "$rv" info --json; }
check "diff loads: 32 files" eventually 20 is "rinfo | jq -r '.files | length'" 32
t1=$(date +%s%N)
echo "  open → listed: $(((t1 - t0) / 1000000)) ms (incl. git and 100 ms polling)"
check "every file's header is in the list" test "$(rinfo | jq -r '.visible_text' | grep -c '^M src/')" = 32
check "…and every file's lines (not one file at a time)" bash -c "rade debug review $rv info --json | jq -r .visible_text | grep -qx 'changed' && rade debug review $rv info --json | jq -r .visible_text | grep -qx 'let a = 4999;'"
check "nothing truncated: 5,000 changed lines of big.rs listed" test "$(rinfo | jq -r '.visible_text' | grep -c '^let a = .*changed$')" = 2500
ROWS=$(rinfo | jq -r .rows)
check "row count = headers + hunks + lines" test "$ROWS" -gt 8000
echo "  rows: $ROWS, rebuild $(rinfo | jq -r '.render_ms | floor') ms"
rade debug review "$rv" select src/f20.py >/dev/null
sleep 0.5
check "jump list: select scrolls f20.py's header to the top" bash -c "rade debug review $rv locate 'M src/f20.py' --json | jq -e '.y < 200' >/dev/null"
check "…and highlights it in the list" test "$(rinfo | jq -r .selected)" = src/f20.py
shot review-jump-light
rade debug review "$rv" fold src/f20.py >/dev/null
check "a click on a file header folds the file" test "$(rinfo | jq -r '[.files[] | select(.path=="src/f20.py") | .folded][0]')" = true
check "…its lines are gone" bash -c "! rade debug review $rv info --json | jq -r .visible_text | grep -q '^line 20 '"
check "…the rows shrink" test "$(rinfo | jq -r .rows)" -lt "$ROWS"
rade debug review "$rv" fold src/f20.py >/dev/null
check "…and unfolds" bash -c "rade debug review $rv info --json | jq -r .visible_text | grep -q '^line 20 9'"
rade debug review "$rv" select src/a.rs >/dev/null
rade debug review "$rv" toggle "@@" >/dev/null
check "a hunk folds (a.rs)" bash -c "! rade debug review $rv info --json | jq -r .visible_text | grep -q 'let count = 2'"
rade debug review "$rv" toggle "@@" >/dev/null
check "a.rs highlighted as Rust" test "$(rinfo | jq -r .language)" = rust

echo "== M11 perf: 5,000-line file in a ${ROWS}-row diff"
check "highlighting the 5,000-line file" bash -c "rade debug review $rv select src/big.rs >/dev/null; sleep 1; rade debug review $rv info --json | jq -e '.highlight_ms > 0' >/dev/null"
echo "  highlight (slowest file): $(rinfo | jq -r '.highlight_ms | floor') ms"
rade debug review "$rv" perf >/dev/null
check "scrolling the whole list (3 s)" eventually 15 bash -c "rade debug review $rv info --json | jq -e '.perf != null' >/dev/null"
rinfo | jq -c .perf | tee "$OUT/perf.json"
perf_check "no frame over 50 ms while scrolling" bash -c "jq -e '.max_ms < 50' $OUT/perf.json >/dev/null"
rade debug review "$rv" select src/big.rs >/dev/null

echo "== M11 re-anchoring Review Comments"
sk=$(rade spawn --source proj --agent sink --name sk --split right --json | jq -r .pane)
eventually 10 screen_has "$sk" 'SINK READY' || true
rade debug review "$rv" select src/a.rs >/dev/null
rade debug review "$rv" comment "let count = 2|let count = 2|Rename count." >/dev/null
rade review add "$R/src/f3.py:31" "Why also?" --source proj >/dev/null
list() { rade review list --source proj --json; }
check "comment at src/a.rs:2" test "$(list | jq -r '.comments[0] | "\(.path):\(.line)"')" = src/a.rs:2
check "…with its anchor (context + hunk)" test "$(list | jq -r '.comments[0].context | "\(.before[0])|\(.after[0])|\(.hunk)"')" = 'fn main() {|    println!("{}", count);|@@ -1,4 +1,4 @@'
check "…marked in the gutter on screen" eventually 5 bash -c "rade debug review $rv info --json | jq -e '.marks_shown >= 1' >/dev/null"
shot review-comment-light
# Two lines inserted above: the comment follows its line (2 → 4).
printf '// one\n// two\nfn main() {\n    let count = 2;\n    println!("{}", count);\n}\n' >"$R/src/a.rs"
check "file edited above: comment moves to src/a.rs:4" eventually 10 is "rade review list --source proj --json | jq -r '.comments[0].line'" 4
check "…not outdated" test "$(list | jq -r '.comments[0].outdated // false')" = false
check "…its mark moved with it" eventually 5 bash -c "rade debug review $rv info --json | jq -r '.gutter[]' | grep -q '^ *4|●\$'"
# f3.py moved by CLI comment without the Review on screen (review.list re-anchors).
rade close "$rv" >/dev/null
sed -i '1i # header' "$R/src/f3.py"
check "CLI comment re-anchored by review list (f3.py:31 → 32)" test "$(list | jq -r '.comments[1].line')" = 32
rv=$(rade review open --source proj)
eventually 20 is "rade debug review $rv info --json | jq -r '.files | length'" 32 || true
# The commented line is deleted: outdated, still listed and sendable.
printf '// one\n// two\nfn main() {\n    println!("{}", 3);\n}\n' >"$R/src/a.rs"
check "line deleted: comment outdated" eventually 10 is "rade review list --source proj --json | jq -r '.comments[0].outdated'" true
check "…the Review shows it as outdated" eventually 5 is "rade debug review $rv info --json | jq -r .outdated" 1
check "…the prompt says so and keeps the quote" bash -c "rade review list --source proj --json | jq -r .prompt | grep -q 'src/a.rs:4 (outdated: the code there changed since)' && rade review list --source proj --json | jq -r .prompt | grep -qF '> +    let count = 2;'"
rade debug review "$rv" select src/a.rs >/dev/null
sleep 0.5
shot review-outdated-light
: >"$RECV"
rade review send --source proj --pane sk >/dev/null
check "outdated comments are still sent" eventually 5 bash -c "grep -q 'outdated' $RECV && grep -q 'f3.py:32' $RECV"

echo "== R13 look in dark"
rade review add "$R/src/f1.py:11" "Dark mode comment" --source proj >/dev/null
stop_app
ADW_DEBUG_COLOR_SCHEME=prefer-dark "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
eventually 15 rade list || { echo "app did not start"; exit 1; }
rv=$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "review") | .id][0]')
rade focus "$rv" >/dev/null
eventually 20 is "rade debug review $rv info --json | jq -r '.files | length'" 32 || true
rade debug review "$rv" select src/f1.py >/dev/null
sleep 1
shot review-dark

echo "== M11 mid-turn restore (kill -9)"
sp=$(rade spawn --source proj --agent fake --name busy "spin" --json | jq -r .pane)
dn=$(rade spawn --source proj --agent fake --name idle "hello" --json | jq -r .pane)
check "busy is running, idle done" eventually 10 bash -c "test \"\$(rade list --json | jq -r '.panes[] | select(.name==\"busy\" or .name==\"idle\") | .status' | sort | tr '\n' ' ')\" = 'done running '"
rade fanout --source proj --name spin --agents fake,fake "spin" >/dev/null
rade fanout --source proj --name quick --agents fake,fake "quick task" >/dev/null
check "fan-out quick finished 2/2" eventually 10 is "st quick '\"\(.done)/\(.total)\"'" 2/2
check "fan-out spin running 0/2" test "$(st spin '"\(.phase) \(.done)/\(.total)"')" = "running 0/2"
sleep 5.5 # the state is saved at least every 5 s; kill -9 must find it saved
kill_app
start_app
check "a mid-turn Agent comes back interrupted" test "$(status_of "$sp")" = interrupted
check "…an idle one with no status" test "$(status_of "$dn")" = null
check "…wait --until done does not return for it (exit 6)" test "$(exit_code rade wait "$sp" --until done --timeout 2)" = 6
check "fan-out spin: interrupted Agents are not done (0/2, running)" test "$(st spin '"\(.phase) \(.done)/\(.total) \(.interrupted)"')" = "running 0/2 2"
check "…members say interrupted" test "$(st spin '[.members[].status] | join(",")')" = "interrupted,interrupted"
check "fan-out quick still finished 2/2" test "$(st quick '"\(.phase) \(.done)/\(.total) \(.interrupted)"')" = "finished 2/2 0"
check "hooks can't set interrupted (exit 2)" test "$(exit_code rade status interrupted --pane "$dn")" = 2
check "judge refused while interrupted" test "$(exit_code rade fanout judge spin)" = 2
TAB=$(st spin .tab)
rade debug action present >/dev/null 2>&1 || true
rade focus "$(st spin '.members[0].pane')" >/dev/null
sleep 1
check "bar: progress says interrupted" test "$(bar spin .progress)" = "0/2 done · 2 interrupted"
check "bar: Continue shown" test "$(bar spin .buttons.continue.visible)" = true
shot fanout-interrupted-light
rade debug fanout spin "click continue" >/dev/null
check "Continue: both Agents got the prompt and finished (2/2)" eventually 15 is "st spin '\"\(.phase) \(.done)/\(.total)\"'" "finished 2/2"
check "…the prompt carried the task" screen_has "$(st spin '.members[0].pane')" "CONTINUE FROM WHERE YOU LEFT OFF"
check "…Continue hidden again" test "$(bar spin .buttons.continue.visible)" = false
rade send "$sp" "after restart" --enter >/dev/null
check "a prompt ends the interrupted state (non-fan-out)" eventually 10 is "status_of $sp" done

echo "== M11 rade close on a Fan-out Agent"
m0=$(st spin '.members[0].pane')
check "rade close refuses a Fan-out Agent (exit 2)" test "$(exit_code rade close "$m0")" = 2
check "…naming discard and --force" bash -c "grep -q 'rade fanout discard spin' $T/err && grep -q -- '--force' $T/err"
check "…the Pane stays" bash -c "rade list --json | jq -e --arg p $m0 '.panes[] | select(.pane==\$p)' >/dev/null"
rade focus "$m0" >/dev/null
rade debug action close_pane >/dev/null
sleep 0.7
check "the close key asks (dialog)" bash -c "rade debug window --widget rade-fanout-close --json | jq -e '.widget.width > 0' >/dev/null"
check "…and does not close it" bash -c "rade list --json | jq -e --arg p $m0 '.panes[] | select(.pane==\$p)' >/dev/null"
shot fanout-close-dialog
stop_app
start_app
check "rade close --force closes it" test "$(exit_code rade close "$m0" --force)" = 0
check "…it counts as finished; the Fan-out stays" test "$(st spin '"\(.done)/\(.total)"')" = 2/2
rade fanout choose spin fake-2 --strategy keep >/dev/null
check "after a Winner, its Agents close freely" test "$(exit_code rade close "$(st spin '.members[1].pane')")" = 0

echo "== M11: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
