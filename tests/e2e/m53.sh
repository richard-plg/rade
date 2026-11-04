#!/usr/bin/env bash
# M53 end-to-end tests (ADR-0006, ADR-0052): the UI thread stays free (R142).
#   R142-b  an idle app with 4 Terminals: RADE's own timers woke the loop <= 50 times in 10 s
#           (`debug.ui {"op":"wakeups"}`)
#   R142-a  60 fake Agents each reporting a status every 200 ms for 30 s (tests/e2e/hook-load.py):
#           UI-thread stall p99 <= 16 ms and no frame with two `sync`s (`debug.ui {"op":"perf"}`;
#           the timing is skipped under E2E_SKIP_PERF)
#   R142.3  `rade wait --until /regex/` finds text that scrolled off the screen, before or after it began
#   R142-d  a `git` that takes 5 s (a wrapper first on PATH): with the window in front for 12 s, at
#           most one facts read was in flight at any time
# The numbers are written to target/e2e-m53/numbers.txt (docs/internals/checks/M53.md quotes them).
# Usage: tests/e2e/m53.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir,
#        PARTS="b a f d" to pick parts, AGENTS=60 to use fewer Agents in part a)
set -euo pipefail
[[ -n ${DEBUG_M53:-} ]] && set -x
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m53
source "$HERE/lib.sh"

mkdir -p "$T/bin" "$XDG_CONFIG_HOME/rade"
{
    echo '#!/usr/bin/env bash'
    echo 'FAKE_AGENT=claude'
    tail -n +2 "$HERE/fake-agent.sh"
} >"$T/bin/claude"
chmod +x "$T/bin/claude"
export PATH=$T/bin:$PATH
printf 'idle_seconds = 60\n' >"$XDG_CONFIG_HOME/rade/config.toml"
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
NUMBERS=$OUT/numbers.txt
: >"$NUMBERS"
N=${AGENTS:-60}

repo() { mkdir -p "$1" && git -C "$1" init -q -b main && git -C "$1" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init; }
perf() { rpc debug.ui '{"op":"perf"}'; }
wakeups() { rpc debug.ui "{\"op\":\"wakeups\",\"arg\":\"$1\"}"; }
note() { echo "$*" | tee -a "$NUMBERS"; }
want() { [[ " ${PARTS:-b a f d} " == *" $1 "* ]]; }
fresh() { rm -rf "$XDG_DATA_HOME/rade-dev"; } # no session to restore

repo "$T/src/app"

if want b; then
    echo "== R142-b: idle app, 4 Terminals, 10 s"
    fresh
    start_app
    rade source add "$T/src/app" >/dev/null
    for i in 1 2 3 4; do rade spawn --source app --name "sh$i" --tab new >/dev/null; done
    eventually 15 is 'rade list --json | jq "[.panes[]] | length"' 5 || true
    sleep 4 # started, prompts drawn, first facts read
    wakeups reset >/dev/null
    sleep 10
    w=$(wakeups no)
    timer=$(jq -r .timer <<<"$w")
    note "R142-b idle wakeups in 10 s: timer=$timer fd=$(jq -r .fd <<<"$w")"
    check "R142-b idle: $timer timer wakeups in 10 s (<= 50)" test "$timer" -le 50
    stop_app
fi

if want a; then
    echo "== R142-a: $N Agents reporting every 200 ms for 30 s"
    fresh
    start_app
    # Six Sources of one Workspace, the Agents spread over them in Tabs of 3.
    for s in 1 2 3 4 5 6; do
        repo "$T/src/s$s"
        rade source add "$T/src/s$s" >/dev/null
    done
    for i in $(seq "$N"); do
        s=$(((i - 1) % 6 + 1)); k=$(((i - 1) / 6))
        tab=current; ((k % 3 == 0)) && tab=new
        rade spawn --agent claude --source "s$s" --name "a$i" --tab "$tab" "hello $i" >/dev/null
    done
    eventually 90 is 'rade list --json | jq "[.panes[] | select(.status == \"done\")] | length"' "$N" || true
    check "R142-a $N Agents finished their first turn" is 'rade list --json | jq "[.panes[] | select(.status == \"done\")] | length"' "$N"
    PANES=$(rade list --json | jq -r '.panes[] | select(.agent != null) | .pane')
    sleep 1
    rpc debug.ui '{"op":"perf","arg":"reset"}' >/dev/null
    python3 "$HERE/hook-load.py" "$SOCK" $PANES --every 200 --for 30 >"$OUT/load.json"
    p=$(perf)
    echo "$p" >"$OUT/perf.json"
    p99=$(jq -r '.stall.p99' <<<"$p"); pmax=$(jq -r '.stall.max' <<<"$p")
    syncs=$(jq -r '.sync.n' <<<"$p"); same=$(jq -r '.sync.same_frame' <<<"$p")
    note "R142-a load: $(jq -c . "$OUT/load.json"); stall p50=$(jq -r '.stall.p50' <<<"$p") p99=$p99 max=$pmax ms over $(jq -r '.stall.n' <<<"$p") loop turns; sync n=$syncs in 30 s ($(echo "scale=1; $syncs / 30" | bc)/s) same_frame=$same"
    check "R142-a every report was answered" test "$(jq .errors "$OUT/load.json")" = 0 -a "$(jq .calls "$OUT/load.json")" -ge $((N * 130))
    perf_check "R142-a UI-thread stall p99 $p99 ms (<= 16)" test "$(echo "$p99 <= 16" | bc)" = 1
    check "R142-a sync ran at most once per frame (same_frame $same)" test "$same" = 0
    check "R142-a the model still follows the reports" is 'rade list --json | jq "[.panes[] | select(.status == \"running\" or .status == \"done\")] | length"' "$N"
    shot a-agents
    stop_app
fi

if want f; then
    echo "== R142.3: a regex wait reads only what was added (text that scrolled off the screen too)"
    fresh
    start_app
    rade source add "$T/src/app" >/dev/null
    sh1=$(rade spawn --source app --name w1 --tab new --json | jq -r .pane)
    eventually 15 screen_has "$sh1" '$' || true
    # Output already in the scrollback when the wait starts.
    rade send "$sh1" 'seq 1 400; echo EARLY-MARK; seq 401 900' --enter >/dev/null
    eventually 15 screen_has "$sh1" '900' || true
    check "R142.3 a wait finds a mark that scrolled off before it began" \
        test "$(rade wait "$sh1" --until '/EARLY-MARK/' --timeout 10 2>&1)" = "EARLY-MARK"
    # A wait that began first sees a mark printed later, then scrolled off.
    rade wait "$sh1" --until '/LATER-MARK/' --timeout 30 >"$OUT/wait-later.txt" 2>&1 &
    waiter=$!
    sleep 1
    rade send "$sh1" 'echo LATER-MARK; seq 1 600' --enter >/dev/null
    wrc=0; wait "$waiter" || wrc=$?
    check "R142.3 a running wait finds a mark that was printed after it began (exit $wrc)" \
        bash -c "[[ $wrc == 0 ]] && grep -q LATER-MARK '$OUT/wait-later.txt'"
    # And a wait for text that never comes times out (exit 6) instead of matching old output.
    rc=0; rade wait "$sh1" --until '/NEVER-PRINTED/' --timeout 2 >/dev/null 2>&1 || rc=$?
    check "R142.3 a wait for a mark that never came times out (exit $rc)" test "$rc" = 6
    stop_app
fi

if want d; then
    echo "== R142-d: a git that takes 5 s, the window in front for 12 s"
    mkdir -p "$T/slow"
    REAL_GIT=$(command -v git)
    cat >"$T/slow/git" <<EOF
#!/usr/bin/env bash
# Only the facts read (it ends with \`git config user.email\`) is slow; everything else runs at once.
if [[ " \$* " == *" config user.email "* ]]; then
    echo "+ \$(date +%s.%N)" >>"$T/slow.log"
    sleep 5
    echo "- \$(date +%s.%N)" >>"$T/slow.log"
fi
exec "$REAL_GIT" "\$@"
EOF
    chmod +x "$T/slow/git"
    : >"$T/slow.log"
    fresh
    PATH=$T/slow:$PATH start_app
    rade source add "$T/src/app" >/dev/null
    # The Source is the focused checkout.
    rade focus "$(rade spawn --source app --name sh --tab new --json | jq -r .pane)" >/dev/null
    # A headless compositor never puts the window in front: say it is.
    rpc debug.ui '{"op":"force_active"}' >/dev/null
    sleep 12
    active=$(rade debug window --json | jq -r .active)
    stop_app
    most=$(awk '$1 == "+" { n++; if (n > m) m = n } $1 == "-" { n-- } END { print m + 0 }' < <(sort -k2 -n "$T/slow.log"))
    started=$(grep -c '^+' "$T/slow.log" || true)
    note "R142-d window active: $active; facts reads started in 12 s: $started, most in flight at once: $most"
    check "R142-d reads happened ($started)" test "$started" -ge 1
    check "R142-d at most one facts read in flight ($most)" test "$most" -le 1
fi

echo
echo "m53: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
