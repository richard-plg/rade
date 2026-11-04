#!/usr/bin/env bash
# M53 end-to-end tests (ADR-0006, ADR-0052): the UI thread stays free (R142).
#   R142-b  an idle app with 4 Terminals: RADE's own timers woke the loop <= 50 times in 10 s
#           (`debug.ui {"op":"wakeups"}`)
#   R142-a  60 fake Agents each reporting a status every 200 ms for 30 s (tests/e2e/hook-load.py):
#           UI-thread stall p99 <= 16 ms and no frame with two `sync`s (`debug.ui {"op":"perf"}`;
#           the timing is skipped under E2E_SKIP_PERF)
#   R142-g  60 fake Agents spawned in ONE Source (a new Tab every 3rd, as part a; `rade spawn` places
#           them), reporting as in R142-a: every spawn answered within 2 s, `rade list` within 2 s
#           throughout (a prober every 0.5 s), stall p99 <= 16 ms (timing skipped under E2E_SKIP_PERF)
#   R142.3  `rade wait --until /regex/` finds text that scrolled off the screen, before or after it began
#   R142-d  a `git` that takes 5 s (a wrapper first on PATH): with the window in front for 12 s, at
#           most one facts read was in flight at any time
#   R140-c  a test-mode app whose UI thread is held (`debug.ui block`): `rade list --call-timeout 2`
#           exits 6 within 3 s naming the limit; `RADE_TIMEOUT=0 rade list` answers after the block
#   R141-e  after `rade account add` (user + Workspace) and a Settings → Accounts variable,
#           accounts.toml, workspace.toml and state.json are 0600
#   R140-f  two `rade browser --engine chromium open` at once: one daemon, both answer (skipped
#           without a Chromium)
# The numbers are written to target/e2e-m53/numbers.txt (docs/internals/checks/M53.md quotes them).
# Usage: tests/e2e/m53.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir,
#        PARTS="b a o f d c s k" to pick parts, AGENTS=60 to use fewer Agents in parts a and o)
set -euo pipefail
[[ -n ${DEBUG_M53:-} ]] && set -x
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m53
# A Chromium from the real HOME (Playwright's cache), found before lib.sh swaps
# HOME for a temp one (as m24); $RADE_E2E_CHROMIUM overrides.
if [[ -z ${RADE_E2E_BUS:-} && -z ${RADE_E2E_CHROMIUM:-} ]]; then
    for c in "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux*/chrome /opt/chrome-for-testing/chrome \
        "$(command -v chromium || true)" "$(command -v google-chrome-stable || true)"; do
        [[ -n $c && -x $c ]] && { export RADE_E2E_CHROMIUM=$c; break; }
    done
fi
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
want() { [[ " ${PARTS:-b a o f d c s k} " == *" $1 "* ]]; }
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

if want o; then
    echo "== R142-g: $N Agents in one Source, reporting every 200 ms for 30 s"
    fresh
    start_app
    repo "$T/src/one"
    rade source add "$T/src/one" >/dev/null
    # `rade list` every 0.5 s while the Agents start and report: "<seconds> <exit>" per call.
    : >"$T/o-list.txt"
    rm -f "$T/o-stop"
    (
        while [[ ! -e $T/o-stop ]]; do
            t0=$(date +%s.%N) rc=0
            rade list --call-timeout 10 >/dev/null 2>&1 || rc=$?
            echo "$(echo "$(date +%s.%N) - $t0" | bc) $rc" >>"$T/o-list.txt"
            sleep 0.5
        done
    ) &
    PROBER=$!
    : >"$T/o-spawn.txt"
    t_all=$(date +%s.%N)
    for i in $(seq "$N"); do
        # A wedged app: stop after 5 min (the check below fails on the missing spawns).
        (( $(echo "$(date +%s.%N) - $t_all > 300" | bc) )) && { echo "999 124" >>"$T/o-spawn.txt"; break; }
        tab=current; (((i - 1) % 3 == 0)) && tab=new
        t0=$(date +%s.%N) rc=0
        rade spawn --call-timeout 30 --agent claude --source one --name "a$i" --tab "$tab" "hello $i" >/dev/null 2>&1 || rc=$?
        echo "$(echo "$(date +%s.%N) - $t0" | bc) $rc" >>"$T/o-spawn.txt"
    done
    spawn_all=$(echo "$(date +%s.%N) - $t_all" | bc)
    spawn_max=$(sort -n "$T/o-spawn.txt" | tail -1 | cut -d' ' -f1)
    spawn_bad=$(awk '$2 != 0' "$T/o-spawn.txt" | wc -l)
    tabs=$(rade tab list --json --call-timeout 30 2>/dev/null | jq -c '[.tabs[] | .panes | length]' 2>/dev/null || echo "?")
    note "R142-g $N spawns in one Source: $spawn_all s in all, slowest $spawn_max s, $spawn_bad failed; Panes per Tab: $tabs"
    check "R142-g every spawn answered within 2 s (slowest $spawn_max s, $spawn_bad failed)" \
        test "$(echo "$spawn_max <= 2" | bc)$spawn_bad" = 10
    # 90 s of wall clock (a wedged app would make `eventually`'s polls 30 s each).
    done_n='rade list --json --call-timeout 5 | jq "[.panes[] | select(.status == \"done\")] | length"'
    until is "$done_n" "$N" || ((SECONDS > ${o_deadline:=$((SECONDS + 90))})); do sleep 0.5; done
    check "R142-g $N Agents finished their first turn" is "$done_n" "$N"
    PANES=$(rade list --json --call-timeout 10 | jq -r '.panes[] | select(.agent != null) | .pane' || true)
    sleep 1
    rpc debug.ui '{"op":"perf","arg":"reset"}' >/dev/null || true
    python3 "$HERE/hook-load.py" "$SOCK" $PANES --every 200 --for 30 >"$OUT/o-load.json"
    p=$(RPC_TIMEOUT=60 perf || echo '{"stall": {"p50": 99999, "p99": 99999, "max": 99999}}')
    touch "$T/o-stop"
    wait "$PROBER" 2>/dev/null || true
    echo "$p" >"$OUT/o-perf.json"
    p99=$(jq -r '.stall.p99' <<<"$p"); pmax=$(jq -r '.stall.max' <<<"$p")
    list_max=$(sort -n "$T/o-list.txt" | tail -1 | cut -d' ' -f1)
    list_bad=$(awk '$2 != 0' "$T/o-list.txt" | wc -l)
    note "R142-g load: $(jq -c . "$OUT/o-load.json"); stall p50=$(jq -r '.stall.p50' <<<"$p") p99=$p99 max=$pmax ms; rade list: $(wc -l <"$T/o-list.txt") calls, slowest $list_max s, $list_bad failed"
    check "R142-g every report was answered" test "$(jq .errors "$OUT/o-load.json")" = 0 -a "$(jq .calls "$OUT/o-load.json")" -ge $((N * 130))
    check "R142-g rade list answered within 2 s throughout (slowest $list_max s, $list_bad failed)" \
        test "$(echo "$list_max <= 2" | bc)$list_bad" = 10
    perf_check "R142-g UI-thread stall p99 $p99 ms (<= 16)" test "$(echo "$p99 <= 16" | bc)" = 1
    shot o-agents
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

if want c; then
    echo "== R140-c: a call the app does not answer in time"
    fresh
    start_app
    rpc debug.ui '{"op":"block","ms":5000}' >/dev/null # held on the UI thread's next idle
    sleep 0.3
    t0=$(date +%s.%N); rc=0
    rade list --call-timeout 2 >/dev/null 2>"$T/c.err" || rc=$?
    secs=$(echo "$(date +%s.%N) - $t0" | bc)
    note "R140-c rade list --call-timeout 2 on a held UI thread: exit $rc after $secs s: $(cat "$T/c.err")"
    check "R140-c rade list --call-timeout 2 exits 6 (TIMEOUT) (exit $rc)" test "$rc" = 6
    check "R140-c it ends within 3 s ($secs s)" test "$(echo "$secs <= 3" | bc)" = 1
    check "R140-c the message names the limit and the method" grep -qF 'did not answer within 2 s (pane.list)' "$T/c.err"
    check "R140-c RADE_TIMEOUT=0 rade list answers once the block ends" env RADE_TIMEOUT=0 rade list
    stop_app
fi

if want s; then
    echo "== R141-e: files with secrets are 0600 after real edits"
    fresh
    WS=$T/ws-acme
    mkdir -p "$WS/.rade"
    printf 'name = "acme"\n' >"$WS/.rade/workspace.toml"
    chmod 644 "$WS/.rade/workspace.toml"
    ACC=$XDG_CONFIG_HOME/rade/accounts.toml
    printf '# logins\n' >"$ACC"
    chmod 644 "$ACC"
    start_app
    rade workspace add "$WS" >/dev/null
    rade account add --user --agent claude --name m53 --config-dir '~/.m53' >/dev/null
    rade account add --workspace acme --agent claude --name acme-m53 --config-dir '~/.acme-m53' >/dev/null
    rpc debug.ui '{"op":"app_settings_open"}' >/dev/null
    sleep 0.5
    rpc debug.ui '{"op":"app_settings_accounts","do":"select","agent":"claude","name":"m53"}' >/dev/null
    rpc debug.ui '{"op":"app_settings_accounts","do":"add_var","key":"M53_TOKEN","value":"s3cret"}' >/dev/null
    check "R141-e Settings → Accounts wrote the variable" eventually 5 grep -qx 'M53_TOKEN = "s3cret"' "$ACC"
    check "R141-e state.json was saved" eventually 10 test -s "$STATE"
    for f in "$ACC" "$WS/.rade/workspace.toml" "$STATE"; do
        m=$(stat -c %a "$f")
        note "R141-e $(basename "$f"): $m"
        check "R141-e $(basename "$f") is 600 ($m)" test "$m" = 600
    done
    stop_app
fi

if want k; then
    echo "== R140-f: two Chromium opens at once"
    if [[ -z ${RADE_E2E_CHROMIUM:-} ]]; then
        echo "  SKIP R140-f: no Chromium found (Playwright's cache, chromium, google-chrome; or RADE_E2E_CHROMIUM)"
    else
        fresh
        printf '[browser]\nchromium = "%s"\n' "$RADE_E2E_CHROMIUM" >>"$XDG_CONFIG_HOME/rade/config.toml"
        mkdir -p "$T/site" && printf '<title>m53</title><p>hi</p>\n' >"$T/site/index.html"
        PORT=$(free_port)
        python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$T/site" >/dev/null 2>&1 &
        HTTP=$!
        eventually 10 curl -sf "http://127.0.0.1:$PORT/" >/dev/null || true
        start_app
        r1=0 r2=0
        rade browser --engine chromium open ":$PORT/" >"$T/k1.out" 2>&1 & k1=$!
        rade browser --engine chromium open ":$PORT/" >"$T/k2.out" 2>&1 & k2=$!
        wait "$k1" || r1=$?
        wait "$k2" || r2=$?
        check "R140-f both opens answered (exit $r1 / $r2)" test "$r1$r2" = 00
        daemons=$(pgrep -c -u "$(id -u)" -f -- "__chromium $XDG_RUNTIME_DIR/" || true)
        note "R140-f Chromium daemons after two opens: $daemons"
        check "R140-f one Chromium daemon ($daemons)" test "$daemons" = 1
        check "R140-f a third command reaches it" rade browser --engine chromium snapshot
        rade browser --engine chromium close >/dev/null || true
        kill "$HTTP" 2>/dev/null || true
        stop_app
    fi
fi

echo
echo "m53: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
