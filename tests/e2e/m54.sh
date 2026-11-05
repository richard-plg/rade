#!/usr/bin/env bash
# M54 end-to-end tests (ADR-0006, ADR-0053, ADR-0054): PR status of Worktrees, CI failures back to the
# Agent, `rade pr`, Branch sets (PRD R146–R151). A fake `gh` (tests/e2e/fake-gh.sh, fixtures in
# $FAKE_GH_DIR) and real git repositories with bare remotes in a temp Workspace `acme`: Project shop
# links shop-web (base develop), shop-api (main) and shared-ui (release/2.4).
#   status  R146-e  20 Worktrees, window not active, 10 min of test clock: <= 2 `gh pr list` per
#                   Worktree (one Workspace gh slot); `pr_status = false` -> 0; a gh that sleeps 60 s ->
#                   `unavailable` after ~20 s, one Activity row, UI-thread stall p99 <= 16 ms
#           R146-f  Create PR from the Changes card -> `rade worktree list` shows `pr` at once, a relaunch keeps it
#   ci      R147-d  failing fixture + run logs + a fake Agent: Send to Agent -> "CI failed on PR #7" on its screen
#           R147-f  `rade pr failures --json`: same jobs; no PR -> NOT_FOUND; gh missing -> its message, exit 1
#   auto    R147-e  `ci_failures = "send"`: nothing while running, one prompt when waiting, one per head, none after the 3rd
#   cli     R148-b  `rade pr wait` returns passing within 25 s of the fixture turning; `--timeout 3` while pending exits 6
#   sets    R149-e  `rade worktree add --project shop feat-x --sources all`, refusal, failing post-checkout hook
#           R151-c  `rade spawn --set` -> two --add-dir + the prompt; a Review Comment reaches the Agent of the set
#   ui      R150-a  the New branch across sources form via `debug.ui dialog_*`
#           R150-b  Navigator markers + tooltip, Project home grouped row, palette rows, screenshots dark + light
#           R151-d  Create Pull Requests for Set: 3 pr create + 3 pr edit with cross-links; a failure on Source 2, Retry
#   budget  E8      `ci_failures = "send"`: the fixture turns failing -> the prompt is on the Agent's screen
#   chips   R146-d  the chips follow the fixture (skipped until the chip UI task lands)
# The numbers are written to target/e2e-m54/numbers.txt.
# Usage: tests/e2e/m54.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir,
#        PARTS="status ci auto cli sets ui budget chips" to pick parts)
set -euo pipefail
[[ -n ${DEBUG_M54:-} ]] && set -x
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m54
source "$HERE/lib.sh"

want() { [[ " ${PARTS:-status ci auto cli sets ui budget chips} " == *" $1 "* ]]; }
NUMBERS=$OUT/numbers.txt
: >"$NUMBERS"
note() { echo "$*" | tee -a "$NUMBERS"; }
# No session to restore, and a fake GitHub with no PRs (numbers start at 1 again).
fresh() {
    rm -rf "$XDG_DATA_HOME/rade-dev" "$FAKE_GH_DIR"
    mkdir -p "$FAKE_GH_DIR/prs" "$FAKE_GH_DIR/runs" "$FAKE_GH_DIR/edits"
}
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin"

# ---- Tools: the fake gh (its env is re-read on every call from $GHENV), a fake claude -----------------
export GHENV=$T/gh.env FAKE_GH_DIR=$T/gh FAKE_GH_LOG=$T/gh.log FAKE_PROMPTS=$T/prompts.log FAKE_ARGV=$T/argv.log
: >"$GHENV"
mkdir -p "$FAKE_GH_DIR/prs" "$FAKE_GH_DIR/runs" "$FAKE_GH_DIR/edits"
cat >"$T/bin/gh" <<SH
#!/usr/bin/env bash
[[ -f "\$GHENV" ]] && source "\$GHENV"
exec "$HERE/fake-gh.sh" "\$@"
SH
{
    echo '#!/usr/bin/env bash'
    echo 'FAKE_AGENT=claude'
    tail -n +2 "$HERE/fake-agent.sh"
} >"$T/bin/claude"
chmod +x "$T/bin/gh" "$T/bin/claude"
export PATH=$T/bin:$PATH
: >"$FAKE_GH_LOG"
: >"$FAKE_PROMPTS"
: >"$FAKE_ARGV"

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false -c init.defaultBranch=main "$@"; }
prstate() { rpc debug.pr '{"op":"state"}'; }
prtick() { rpc debug.pr '{"op":"tick"}'; }
clock() { rpc debug.schedule "{\"op\": \"clock\", \"offset\": $1}" >/dev/null; }
act() { ui activity | jq -r '.activity[].line'; }
ghcalls() { grep -c -- "$1" "$FAKE_GH_LOG" || true; }
# `wtj <source> <worktree> <jq path>` → a field of `rade worktree list`.
wtj() { rade worktree list "$1" --json | jq -r --arg w "$2" ".worktrees[] | select(.worktree==\$w) | $3"; }
export -f prstate prtick act ui rpc field wtj

# The scene: Workspace acme with three repositories (bare remotes), Project shop.
#   shop-web (main checkout on develop, develop 1 commit ahead of main), shop-api (main), shared-ui (on release/2.4)
scene() { # name → $A, repos, remotes
    A=$T/$1/acme
    REM=$T/$1/remotes
    WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui
    mkdir -p "$REM" "$A/.rade"
    echo 'name = "acme"' >"$A/.rade/workspace.toml"
    for r in "$WEB" "$API" "$UI"; do
        mkdir -p "$r" && G -C "$r" init -q -b main
        printf 'base\n' >"$r/README.md" && G -C "$r" add -A && G -C "$r" commit -qm init
        G init -q --bare -b main "$REM/$(basename "$r").git"
        G -C "$r" remote add origin "$REM/$(basename "$r").git"
    done
    G -C "$WEB" checkout -qb develop && echo d >"$WEB/dev.txt" && G -C "$WEB" add -A && G -C "$WEB" commit -qm "dev 1"
    G -C "$UI" checkout -qb release/2.4 && echo r >"$UI/rel.txt" && G -C "$UI" add -A && G -C "$UI" commit -qm "release 2.4"
    for r in "$WEB" "$API" "$UI"; do
        G -C "$r" push -q origin --all
        G -C "$r" fetch -q origin
    done
}
register() { # the Workspace, Sources, Project shop
    rade workspace add "$A" >/dev/null
    for r in "$WEB" "$API" "$UI"; do rade source add "$r" --workspace acme >/dev/null; done
    rade workspace switch acme >/dev/null
    rade project add Shop --workspace acme --source shop-web:develop --source shop-api:main --source shared-ui:release/2.4 >/dev/null
}
# A Worktree of a Source with its branch pushed (an upstream): prints its path.
wt() { # source name
    local p
    p=$(rade worktree add "$1" "$2" --project shop)
    G -C "$p" push -q -u origin "$2" >/dev/null 2>&1
    echo "$p"
}
# A gh fixture for a branch: state = passing|failing|pending|none; dir = the Worktree (head oid).
fixture() { # branch dir state [number] [run-base] [repo]
    local br=$1 dir=$2 st=$3 n=${4:-7} run=${5:-100} repo=${6:-web} oid checks u
    oid=$(git -C "$dir" rev-parse HEAD)
    u=https://github.com/acme/$repo/actions/runs/$run/job
    case $st in
    passing) checks='[{"__typename":"CheckRun","name":"test","workflowName":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"'$u/$((run + 1))'"}]' ;;
    pending) checks='[{"__typename":"CheckRun","name":"test","workflowName":"CI","status":"IN_PROGRESS","conclusion":"","detailsUrl":"'$u/$((run + 1))'"}]' ;;
    failing) checks='[{"__typename":"CheckRun","name":"test","workflowName":"CI","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"'$u/$((run + 1))'"},
                      {"__typename":"CheckRun","name":"lint","workflowName":"CI","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"'$u/$((run + 2))'"}]' ;;
    none) checks='[]' ;;
    esac
    jq -nc --argjson n "$n" --arg o "$oid" --arg t "Feature $br" --arg u "https://github.com/acme/$repo/pull/$n" --argjson c "$checks" \
        '[{number: $n, title: $t, url: $u, state: "OPEN", isDraft: false, baseRefName: "main", headRefOid: $o,
           reviewDecision: "", mergeStateStatus: "CLEAN", statusCheckRollup: $c}]' >"$FAKE_GH_DIR/prs/${br//\//__}.json"
}
runlog() { # job-id text
    printf '2026-11-05T10:00:00.0000000Z ##[group]Run cargo test\n2026-11-05T10:00:01.0000000Z \033[31m%s\033[0m\n2026-11-05T10:00:02.0000000Z ##[endgroup]\n' "$2" >"$FAKE_GH_DIR/runs/$1.log"
}
# An Agent Pane (fake claude) in a Worktree; waits for its first turn to end.
agent_in() { # source worktree name → pane
    local pane
    pane=$(rade spawn --agent claude --source "$1" --worktree "$2" --name "$3" "hello" --json | jq -r .pane)
    eventually 30 is "field $pane status" done || true
    echo "$pane"
}

if want status; then
    echo "== R146-e: 20 Worktrees, window not active, 10 min of test clock"
    fresh
    scene status
    start_app
    register
    for i in $(seq 20); do
        d=$(wt shop-web "feat-$i")
        fixture "feat-$i" "$d" passing $((100 + i)) $((1000 + i * 10))
    done
    : >"$FAKE_GH_LOG"
    rpc debug.pr '{"op":"activate","on":false}' >/dev/null
    clock 0
    prtick >/dev/null
    clock 599
    prtick >/dev/null
    c599=$(ghcalls "pr list")
    clock 650
    prtick >/dev/null
    per=$(grep -o -- '--head feat-[0-9]*' "$FAKE_GH_LOG" | sort | uniq -c | awk '{print $1}' | sort -n | uniq -c | tr '\n' ' ')
    most=$(grep -o -- '--head feat-[0-9]*' "$FAKE_GH_LOG" | sort | uniq -c | awk '{print $1}' | sort -n | tail -1)
    branches=$(grep -o -- '--head feat-[0-9]*' "$FAKE_GH_LOG" | sort -u | wc -l)
    total=$(ghcalls "pr list")
    note "R146-e 20 Worktrees, hidden, 650 s of test clock: $total gh pr list calls ($c599 before 599 s), $branches branches read, most calls on one branch: $most [branches by calls: $per]"
    check "R146-e every one of the 20 Worktrees was read (got $branches)" test "$branches" = 20
    check "R146-e gh ran at most twice per Worktree in 10 min (most $most)" test "$most" -le 2
    check "R146-e no Workspace has a gh call left in flight" bash -c "[[ \$(prstate | jq '[.workspaces[] | select(.in_flight)] | length') == 0 ]]"
    stop_app

    echo "-- R146-e [scm] pr_status = false: gh never runs"
    printf '[scm]\npr_status = false\n' >"$CONF"
    start_app
    : >"$FAKE_GH_LOG"
    rpc debug.pr '{"op":"activate","on":false}' >/dev/null
    clock 0
    prtick >/dev/null
    clock 650
    prtick >/dev/null
    sleep 6
    check "R146-e pr_status = false: 0 gh calls in 10 min ($(ghcalls 'pr list'))" test "$(ghcalls 'pr list')" = 0
    stop_app
    : >"$CONF"

    echo "-- R146-e a gh that sleeps 60 s: unavailable after ~20 s, one Activity row, no UI stall"
    start_app
    rpc debug.pr '{"op":"activate","on":false}' >/dev/null
    echo 'export FAKE_GH_SLEEP=60' >"$GHENV"
    rpc debug.ui '{"op":"perf","arg":"reset"}' >/dev/null
    t0=$(date +%s.%N)
    clock 1300
    until [[ $(prstate | jq -r '[.workspaces[] | .message] | map(select(. != null)) | length') != 0 ]] || (($(echo "$(date +%s.%N) - $t0 > 60" | bc))); do sleep 0.5; done
    secs=$(echo "$(date +%s.%N) - $t0" | bc)
    msg=$(prstate | jq -r '[.workspaces[] | .message] | map(select(. != null)) | .[0]')
    rows=$(act | grep -c 'pull request status unavailable' || true)
    p=$(rpc debug.ui '{"op":"perf"}')
    p99=$(jq -r '.stall.p99' <<<"$p")
    note "R146-e sleeping gh: unavailable after $secs s ('$msg'), $rows Activity row(s), UI stall p99 $p99 ms (n=$(jq -r '.stall.n' <<<"$p"))"
    check "R146-e unavailable after ~20 s ($secs s, between 15 and 40)" test "$(echo "$secs >= 15 && $secs <= 40" | bc)" = 1
    check "R146-e exactly one Activity row about it ($rows)" test "$rows" = 1
    check "R146-e the Workspace stays paused (down_until set)" bash -c "[[ \$(prstate | jq -r '[.workspaces[] | .down_until] | map(select(. != null)) | length') -ge 1 ]]"
    perf_check "R146-e UI-thread stall p99 $p99 ms (<= 16)" test "$(echo "$p99 <= 16" | bc)" = 1
    : >"$GHENV"
    stop_app

    echo "== R146-f: Create PR from the Changes card is recorded on the Worktree at once and survives a relaunch"
    fresh
    scene create
    start_app
    register
    d=$(wt shop-api feat-pr)
    echo work >"$d/work.txt"
    G -C "$d" add -A
    G -C "$d" commit -qm "feat: work"
    G -C "$d" push -q
    sh=$(rade spawn --source shop-api --worktree feat-pr --name sh --tab new --json | jq -r .pane)
    rade focus "$sh" >/dev/null
    scm() { rpc debug.scm "${1:-{\}}"; }
    eventually 15 is "scm | jq -r '[.cards[] | select(.source==\"shop-api\")][0].path'" "$d" || true
    check "R146-f the Changes card of shop-api is on the Worktree" is "scm | jq -r '[.cards[] | select(.source==\"shop-api\")][0].path'" "$d"
    check "R146-f setup: no PR recorded before" is "wtj shop-api feat-pr .pr" null
    scm '{"op":"card_menu_activate","source":"shop-api","item":"Create Pull Request…"}' >/dev/null
    eventually 5 is "rade debug dialog | jq -r .open" true || true
    scm '{"op":"pr_fill","title":"Work from the e2e"}' >/dev/null
    rade debug dialog respond create >/dev/null
    check "R146-f gh pr create ran in the Worktree" eventually 10 grep -q -- "pr create --head feat-pr" "$FAKE_GH_LOG"
    prnum() { wtj shop-api feat-pr .pr.number; }
    export -f prnum
    check "R146-f rade worktree list shows the new PR's number (pr.number $(prnum))" eventually 5 is prnum 1
    check "R146-f …with its URL" is "wtj shop-api feat-pr .pr.url" "https://github.com/acme/shop-api/pull/1"
    sleep 6 # state.json is saved within 5 s
    stop_app
    echo 'export FAKE_GH_SLEEP=60' >"$GHENV" # no read finishes before the checks below
    start_app
    check "R146-f after a relaunch the Worktree still records #1" eventually 10 is prnum 1
    check "R146-f …before the first read no status is held (the chip shows #1 greyed)" \
        bash -c "[[ \$(prstate | jq -r '[.worktrees[] | .status] | map(select(. != null)) | length') == 0 ]]"
    : >"$GHENV"
    stop_app
fi

if want ci; then
    echo "== R147-d / R147-f: a failing PR, its job logs, a fake Agent"
    fresh
    scene ci
    start_app
    register
    d=$(wt shop-web feat-ci)
    agent=$(agent_in shop-web feat-ci ci-agent)
    : >"$FAKE_PROMPTS" # the Agent's own first prompt
    runlog 101 "error[E0432]: unresolved import crate::widgets (test)"
    runlog 102 "warning: unused variable x (lint)"
    fixture feat-ci "$d" pending 7 100
    rpc debug.pr '{"op":"activate","on":false}' >/dev/null
    clock 0
    prtick >/dev/null
    nf() { state | jq -r --arg p "$agent" '[.notifications[]? | select(.pane==$p and (.text | test("checks failed")))] | length'; }
    export -f nf
    export agent
    check "R147-d setup: the first read (pending) is silent: no PR Notification" is nf 0
    fixture feat-ci "$d" failing 7 100
    clock 700
    prtick >/dev/null
    check "R147-d pending -> failing: one Notification on the Worktree's Agent Pane" eventually 5 is nf 1
    check "R147-d …and one Activity row '#7 checks failed (2 jobs)'" is "act | grep -c '#7 checks failed (2 jobs)'" 1
    check "R147-d [ask] nothing was sent to the Agent" test ! -s "$FAKE_PROMPTS"
    echo "  SKIP R147-d TODO: Send to Agent click -> preview (job heading + tail) -> Send: the preview dialog hook is the UI task's;"
    echo "       until then the prompt reaches the Agent through the opt-in path checked in R147-e / E8"

    echo "-- R147-f rade pr failures --json"
    rc=$(code rade pr failures --worktree feat-ci --source shop-web --json)
    cp "$T/out" "$OUT/failures.json"
    check "R147-f exit 0 and two jobs: CI / test and CI / lint" \
        bash -c "[[ $rc == 0 ]] && jq -e '(.jobs | map(.workflow + \" / \" + .job)) == [\"CI / test\", \"CI / lint\"]' '$T/out' >/dev/null"
    check "R147-f …the log is cleaned (no timestamp, no ANSI, no ##[group]) and holds the failing line" \
        bash -c "jq -e '(.jobs[0].log | test(\"unresolved import\")) and (.jobs | all(.log | (test(\"2026-11-05T\") or test(\"##\\\\[\") or test(\"\\u001b\")) | not))' '$T/out' >/dev/null"
    check "R147-f …and the prompt starts 'CI failed on PR #7' with the job heading and the closing line" \
        bash -c "jq -e '(.prompt | startswith(\"CI failed on PR #7 \")) and (.prompt | test(\"## CI / test\")) and (.prompt | test(\"then commit and push.\"))' '$T/out' >/dev/null"
    rc=$(code rade pr failures --worktree feat-ci --source shop-web)
    check "R147-f the text form prints the same prompt" bash -c "[[ $rc == 0 ]] && grep -q '^## CI / lint' '$T/out'"
    wt shop-web feat-none >/dev/null
    rc=$(code rade pr failures --worktree feat-none --source shop-web --json)
    check "R147-f no PR -> NOT_FOUND, exit 4 ($rc)" bash -c "[[ $rc == 4 ]] && grep -q 'no pull request for feat-none' '$T/err'"
    rc=$(code rade pr status --worktree feat-none --source shop-web --json)
    check "R147-f pr status with no PR: exit 0, pr null" bash -c "[[ $rc == 0 ]] && jq -e '.pr == null' '$T/out' >/dev/null"
    stop_app

    echo "-- R147-f gh missing"
    mkdir -p "$T/nogh"
    for f in /usr/bin/* /bin/*; do [[ ${f##*/} == gh || ${f##*/} == rade || ${f##*/} == rade-app || -e $T/nogh/${f##*/} ]] || ln -s "$f" "$T/nogh/${f##*/}" 2>/dev/null || true; done
    PATH=$T/nogh:$BIN start_app
    rc=$(code rade pr failures --worktree feat-ci --source shop-web)
    cat "$T/err" >"$OUT/failures-no-gh.txt"
    note "R147-f gh missing: rade pr failures exit $rc: $(cat "$T/err")"
    check "R147-f gh missing -> its message on stderr, nonzero exit ($rc)" \
        bash -c "[[ $rc != 0 ]] && grep -q 'GitHub CLI (gh) is not installed' '$T/err'"
    stop_app
fi

# A scene with one Agent in shop-web's feat-ci Worktree and PR #7 (jobs 101 test, 102 lint of run 100).
OFF=0
ci_setup() { # scene-name
    fresh
    scene "$1"
    start_app
    register
    d=$(wt shop-web feat-ci)
    agent=$(agent_in shop-web feat-ci ci-agent)
    : >"$FAKE_PROMPTS" # the Agent's own first prompt
    runlog 101 "error[E0432]: unresolved import crate::widgets (test)"
    runlog 102 "warning: unused variable x (lint)"
    rpc debug.pr '{"op":"activate","on":false}' >/dev/null
    OFF=0
    clock 0
    export agent d
}
# A new commit on the Worktree: a new head.
newhead() { # tag
    echo "$1" >>"$d/f.txt"
    G -C "$d" add -A
    G -C "$d" commit -qm "fix $1"
}
# The fixture takes a state for the current head, 700 s of test clock later, then the due reads run.
turn() { # passing|pending|failing
    fixture feat-ci "$d" "$1" 7 100
    OFF=$((OFF + 700))
    clock "$OFF"
    prtick >/dev/null
}
sent() { grep -c 'CI failed on PR #7' "$FAKE_PROMPTS" || true; }
export -f sent

if want auto; then
    echo "== R147-e: ci_failures = \"send\" (running Agent, one send per head, none after the 3rd)"
    printf '[scm]\nci_failures = "send"\n' >"$CONF"
    ci_setup auto
    rade status running --pane "$agent" >/dev/null
    turn pending
    newhead h1
    turn failing
    sleep 3
    check "R147-e a failing head while the Agent runs: nothing is sent" test "$(sent)" = 0
    rade status done --pane "$agent" >/dev/null
    check "R147-e the Agent waits (done): exactly one prompt follows" eventually 20 is sent 1
    check "R147-e …it reached the Agent's screen: 'CI failed on PR #7'" eventually 10 screen_has "$agent" "CI failed on PR #7"
    check "R147-e …with the job heading and the tail of the log" bash -c "grep -q '## CI / test' '$FAKE_PROMPTS' || rade read '$agent' --scrollback | grep -q 'unresolved import'"
    eventually 20 is "field $agent status" done || true
    clock $((OFF += 700))
    prtick >/dev/null
    sleep 3
    check "R147-e the same head failing again: no second prompt ($(sent))" test "$(sent)" = 1
    newhead h2
    turn failing
    check "R147-e a new failing head: a second prompt" eventually 20 is sent 2
    eventually 20 is "field $agent status" done || true
    newhead h3
    turn failing
    check "R147-e the third failing head: a third prompt" eventually 20 is sent 3
    eventually 20 is "field $agent status" done || true
    newhead h4
    turn failing
    sleep 4
    check "R147-e the 4th failing head sends nothing ($(sent))" test "$(sent)" = 3
    stl() { state | jq -r --arg p "$agent" '[.notifications[]? | select(.pane==$p and (.text | test("CI still failing on #7 after 3 tries")))] | length'; }
    export -f stl
    state | jq .notifications >"$OUT/auto-notifications.json"
    check "R147-e …and posts the Notification 'CI still failing on #7 after 3 tries'" eventually 5 is stl 1
    check "R147-e each send is an Activity row (3)" is "act | grep -c 'CI failure sent to'" 3
    stop_app
    : >"$CONF"
fi

if want cli; then
    echo "== R148-b: rade pr wait"
    : >"$CONF"
    ci_setup cli
    fixture feat-ci "$d" pending 7 100
    rc=$(code rade pr wait --worktree feat-ci --source shop-web --timeout 3)
    check "R148-b --timeout 3 while pending: exit 6 ($rc), naming the PR" bash -c "[[ $rc == 6 ]] && grep -q '#7 checks still pending after 3 s' '$T/err'"
    rade pr wait --worktree feat-ci --source shop-web --timeout 120 >"$T/wait.out" 2>"$T/wait.err" &
    waiter=$!
    sleep 2
    fixture feat-ci "$d" passing 7 100
    t0=$(date +%s.%N)
    wrc=0
    wait "$waiter" || wrc=$?
    secs=$(echo "$(date +%s.%N) - $t0" | bc)
    note "R148-b rade pr wait: the fixture turned passing, the command returned after $secs s (exit $wrc): $(cat "$T/wait.out")"
    check "R148-b it returned 0 within 25 s of the fixture turning passing ($secs s)" bash -c "[[ $wrc == 0 ]] && (( \$(echo '$secs <= 25' | bc) ))"
    check "R148-b …and printed the status: '#7 open · checks passing'" grep -q '#7 open · checks passing' "$T/wait.out"
    rc=$(code rade pr status --worktree feat-ci --source shop-web --json)
    check "R148-b rade pr status --json: number 7, checks passing, not stale" \
        bash -c "[[ $rc == 0 ]] && jq -e '.pr.number == 7 and .pr.checks == \"passing\" and .pr.stale == false and .pr.state == \"open\"' '$T/out' >/dev/null"
    stop_app
fi

wtlists() { for r in "$WEB" "$API" "$UI"; do G -C "$r" worktree list --porcelain; done; }
branches() { for r in "$WEB" "$API" "$UI"; do G -C "$r" for-each-ref --format='%(refname)' refs/heads; done; }

if want sets; then
    echo "== R149-e: a Branch set across shop-web (develop), shop-api (main), shared-ui (release/2.4)"
    : >"$CONF"
    fresh
    scene sets
    start_app
    register
    rade project show shop --json >"$OUT/project-shop.json"
    rc=$(code rade worktree add --project shop feat-x --sources all --json)
    cp "$T/out" "$OUT/set-add.json"
    check "R149-e --sources all: exit 0, result lists three Worktrees (shop-web, shop-api, shared-ui)" \
        bash -c "[[ $rc == 0 ]] && jq -e '.set == \"feat-x\" and ([.worktrees[].source] == [\"shop-web\",\"shop-api\",\"shared-ui\"])' '$T/out' >/dev/null"
    ok_bases=0
    for pair in "$WEB:develop" "$API:main" "$UI:release/2.4"; do
        repo=${pair%%:*} base=${pair#*:}
        w=$(jq -r --arg s "$(basename "$repo")" '.worktrees[] | select(.source==$s) | .path' "$OUT/set-add.json")
        mb=$(G -C "$w" merge-base HEAD "$base" 2>/dev/null || echo none)
        tip=$(G -C "$repo" rev-parse "$base")
        if [[ $mb == "$tip" && $(G -C "$w" symbolic-ref --short HEAD) == feat-x ]]; then ok_bases=$((ok_bases + 1)); fi
    done
    check "R149-e each Worktree is on feat-x with git merge-base = its own link's base ($ok_bases of 3)" test "$ok_bases" = 3
    check "R149-e …the paths are R12's (.worktrees-dev/<source>/feat-x)" \
        bash -c "[[ \$(jq -r '[.worktrees[].path | sub(\".*/\\\\.worktrees-dev/\"; \"\")] | join(\",\")' '$OUT/set-add.json') == 'shop-web/feat-x,shop-api/feat-x,shared-ui/feat-x' ]]"
    check "R149-e rade worktree list gives set = feat-x on all three" \
        is "for s in shop-web shop-api shared-ui; do wtj \$s feat-x .set; done | tr '\n' ' '" "feat-x feat-x feat-x "
    check "R149-e state.json records \"set\": \"feat-x\" on three Worktrees" \
        eventually 15 is "jq '[.. | objects | select(.set? == \"feat-x\")] | length' '$STATE'" 3

    echo "-- R149-e a branch that already exists in link 3: USAGE, nothing changed"
    G -C "$UI" branch feat-y
    wtlists >"$T/wtl-before"
    branches >"$T/br-before"
    ls -R "$A/.worktrees-dev" >"$T/dirs-before" 2>&1 || true
    rc=$(code rade worktree add --project shop feat-y --sources all)
    check "R149-e exit 2 (USAGE) and the message names shared-ui ($rc)" bash -c "[[ $rc == 2 ]] && grep -q 'shared-ui' '$T/err'"
    wtlists >"$T/wtl-after"
    branches >"$T/br-after"
    check "R149-e git worktree list is unchanged in all three Sources" cmp -s "$T/wtl-before" "$T/wtl-after"
    check "R149-e …no branch was created and no folder either" bash -c "cmp -s '$T/br-before' '$T/br-after' && [[ \"\$(ls -R '$A/.worktrees-dev' 2>&1)\" == \"\$(cat '$T/dirs-before')\" ]]"
    check "R149-e …and no Worktree named feat-y is in the model" test "$(rade worktree list shop-web --json | jq '[.worktrees[] | select(.worktree == "feat-y")] | length')" = 0

    echo "-- R149-e a failing post-checkout hook in link 2: link 1 and 2 cleaned up"
    printf '#!/bin/sh\necho "hook says no" >&2\nexit 1\n' >"$API/.git/hooks/post-checkout"
    chmod +x "$API/.git/hooks/post-checkout"
    wtlists >"$T/wtl-before"
    branches >"$T/br-before"
    ls -R "$A/.worktrees-dev" >"$T/dirs-before" 2>&1 || true
    rc=$(code rade worktree add --project shop feat-z --sources all)
    cat "$T/err" >"$OUT/set-rollback.txt"
    note "R149-e failing post-checkout in shop-api: exit $rc: $(head -c 400 "$T/err")"
    check "R149-e it fails (exit $rc) naming shop-api and what was undone" \
        bash -c "[[ $rc != 0 ]] && grep -q 'shop-api' '$T/err' && grep -qiE 'removed|undone|rolled back' '$T/err'"
    wtlists >"$T/wtl-after"
    branches >"$T/br-after"
    check "R149-e afterwards every git worktree list is as before (link 1 and 2 cleaned, link 3 never touched)" cmp -s "$T/wtl-before" "$T/wtl-after"
    check "R149-e …the branch feat-z exists nowhere, and no folder is left" \
        bash -c "cmp -s '$T/br-before' '$T/br-after' && [[ ! -e '$A/.worktrees-dev/shop-web/feat-z' && ! -e '$A/.worktrees-dev/shop-api/feat-z' && ! -e '$A/.worktrees-dev/shared-ui/feat-z' ]]"
    check "R149-e …and the model holds no feat-z Worktree" test "$(for s in shop-web shop-api shared-ui; do rade worktree list $s --json; done | jq -s '[.[].worktrees[] | select(.worktree == "feat-z")] | length')" = 0
    rm -f "$API/.git/hooks/post-checkout"

    echo "-- R149-d remove --set refuses while one is dirty, then removes all"
    echo dirty >"$(jq -r '.worktrees[] | select(.source=="shared-ui") | .path' "$OUT/set-add.json")/dirty.txt"
    rc=$(code rade worktree remove --project shop --set feat-x)
    check "R149-d removing the set with a dirty 3rd Worktree is refused naming it, nothing removed ($rc)" \
        bash -c "[[ $rc != 0 ]] && grep -q 'shared-ui' '$T/err' && [[ \$(for s in shop-web shop-api shared-ui; do wtj \$s feat-x .set; done | grep -c feat-x) == 3 ]]"
    rc=$(code rade worktree remove --project shop --set feat-x --force --delete-branch)
    check "R149-e remove --set --force --delete-branch removes the whole set ($rc)" \
        bash -c "[[ $rc == 0 ]] && [[ \$(for s in shop-web shop-api shared-ui; do rade worktree list \$s --json; done | jq -s '[.[].worktrees[] | select(.worktree == \"feat-x\")] | length') == 0 ]]"
    rade worktree add shop-web feat-x --project shop >/dev/null
    check "R149-e a Worktree with the same name made otherwise is not in a set (set null)" is "wtj shop-web feat-x .set" null
    stop_app

    echo "== R151-c: one Agent for the set, a Review Comment on the 2nd Worktree reaches it"
    fresh
    scene sets2
    start_app
    register
    rade worktree add --project shop feat-x --sources all --json >"$OUT/set2-add.json"
    wpath() { jq -r --arg s "$1" '.worktrees[] | select(.source==$s) | .path' "$OUT/set2-add.json"; }
    : >"$FAKE_ARGV"
    : >"$FAKE_PROMPTS"
    rc=$(code rade spawn --set feat-x --project shop --agent claude "go" --json)
    lead=$(jq -r .pane "$T/out")
    check "R151-c rade spawn --set opens one Agent Pane (exit $rc)" bash -c "[[ $rc == 0 && -n '$lead' && '$lead' != null ]]"
    eventually 15 test -s "$FAKE_ARGV" || true
    cp "$FAKE_ARGV" "$OUT/set-argv.txt"
    check "R151-c the Agent's cwd is the lead Worktree (first link, shop-web)" \
        is "rade list --json | jq -r --arg p '$lead' '.panes[] | select(.pane==\$p) | .cwd'" "$(wpath shop-web)"
    check "R151-c its argv has two --add-dir (shop-api and shared-ui Worktrees)" \
        bash -c "[[ \$(grep -o -- '--add-dir' '$FAKE_ARGV' | wc -l) == 2 ]] && grep -qF -- '--add-dir $(wpath shop-api)' '$FAKE_ARGV' && grep -qF -- '--add-dir $(wpath shared-ui)' '$FAKE_ARGV' && ! grep -qF -- '--add-dir $(wpath shop-web)' '$FAKE_ARGV'"
    check "R151-c …and the prompt starts with the set paragraph and ends with the task" \
        bash -c "tr '\\n' ' ' <'$FAKE_ARGV' >'$T/argv1.txt'; grep -qF 'This task spans 3 repositories, one Worktree each on branch feat-x: shop-web $(wpath shop-web) (base develop), shop-api $(wpath shop-api) (base main), shared-ui $(wpath shared-ui) (base release/2.4). Commit in each one; do not push unless asked.  go ' '$T/argv1.txt'"
    check "R151-c exactly one Agent was started" test "$(grep -c '^p_' "$FAKE_ARGV")" = 1
    eventually 30 is "field $lead status" done || true
    : >"$FAKE_PROMPTS"
    A2=$(wpath shop-api)
    printf 'one\ntwo\nthree\n' >"$A2/api.txt"
    G -C "$A2" add -A
    G -C "$A2" commit -qm "api work"
    rade review add "$A2/api.txt:2" "Rename two to second." --source shop-api --worktree feat-x >/dev/null
    check "R151-c a Review Comment is stored on the 2nd Worktree" is "rade review list --source shop-api --worktree feat-x --json | jq '.comments | length'" 1
    rc=$(code rade review send --source shop-api --worktree feat-x --json)
    check "R151-c Send goes to the set's Agent, not a new one (exit $rc, pane $(jq -r .pane "$T/out" 2>/dev/null))" \
        bash -c "[[ $rc == 0 ]] && [[ \$(jq -r .pane '$T/out') == '$lead' ]]"
    check "R151-c the Agent received the comment text" eventually 15 grep -q 'Rename two to second' "$FAKE_PROMPTS"
    check "R151-c …and still only one Agent exists in the Project" is "rade list --json | jq '[.panes[] | select(.agent != null)] | length'" 1
    stop_app
fi

dui() { rpc debug.ui "$1"; }
dlg() { dui '{"op":"dialog"}' | jq '.dialog'; }
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
export -f dui dlg
# Commit a file in every Worktree of a set; prints nothing.
commit_set() { # set-name
    local s w
    for s in shop-web shop-api shared-ui; do
        w=$(wtpath "$s" "$1")
        echo "$1 in $s" >"$w/$1.txt"
        G -C "$w" add -A
        G -C "$w" commit -qm "feat: $1 in $s"
    done
}
wtpath() { rade worktree list "$1" --json | jq -r --arg w "$2" '.worktrees[] | select(.worktree==$w) | .path'; }

if want ui; then
    : >"$CONF"
    fresh
    scene ui
    start_app
    register
    echo 'export FAKE_GH_BY_REPO=1' >"$GHENV" # one branch name in three Sources: one PR each
    echo "== R150-a: the New branch across sources form"
    dui '{"op":"dialog_open","kind":"branch_set","project":"shop"}' >/dev/null
    check "R150-a the form opens with a row per link, all on, each with its base" \
        eventually 5 bash -c "dlg | jq -e '.kind == \"branch_set\" and ([.rows[] | .source + \":\" + .base + \":\" + (.include|tostring)] == [\"shop-web:develop:true\",\"shop-api:main:true\",\"shared-ui:release/2.4:true\"])' >/dev/null"
    dui '{"op":"dialog_name","text":"feat-form"}' >/dev/null
    check "R150-a the plan text names each link's base" eventually 10 \
        bash -c "dlg > '$OUT/dialog-form.json'; jq -e '.refused == false and .planning == false and (.rows[0].plan | test(\"develop\")) and (.rows[1].plan | test(\"main\")) and (.rows[2].plan | test(\"release/2.4\"))' '$OUT/dialog-form.json' >/dev/null"
    shot form-dark
    light
    shot form-light
    dark
    dui '{"op":"dialog_primary"}' >/dev/null
    check "R150-a Create: three Worktrees of the set feat-form exist" \
        eventually 30 is "for s in shop-web shop-api shared-ui; do wtj \$s feat-form .set; done | tr '\n' ' '" "feat-form feat-form feat-form "
    check "R150-a …and the dialog closes" eventually 10 is "dlg | jq -r .open" false

    echo "-- R150-a the optional Agent: Preset and prompt"
    dui '{"op":"dialog_open","kind":"branch_set","project":"shop"}' >/dev/null
    dui '{"op":"dialog_name","text":"feat-agent"}' >/dev/null
    dui '{"op":"dialog_row","source":"shared-ui","include":false}' >/dev/null
    dui '{"op":"dialog_agent","preset":"claude"}' >/dev/null
    dui '{"op":"dialog_prompt","text":"build it"}' >/dev/null
    check "R150-a with shared-ui off the plan covers two links and Create is enabled" eventually 10 \
        bash -c "dlg | jq -e '.primary_enabled == true and .planning == false and ([.rows[] | select(.include)] | length == 2) and .agent == \"claude\"' >/dev/null"
    : >"$FAKE_ARGV"
    dui '{"op":"dialog_primary"}' >/dev/null
    check "R150-a …Create makes two Worktrees and one Agent in the lead (first link)" eventually 30 \
        bash -c "[[ \$(wtj shop-web feat-agent .set) == feat-agent && \$(wtj shop-api feat-agent .set) == feat-agent && \$(grep -c '^p_' '$FAKE_ARGV') == 1 ]]"
    check "R150-a …whose argv has one --add-dir and the set paragraph" \
        bash -c "[[ \$(grep -o -- '--add-dir' '$FAKE_ARGV' | wc -l) == 1 ]] && tr '\n' ' ' <'$FAKE_ARGV' | grep -q 'This task spans 2 repositories'"

    echo "== R150-b: markers, grouped row, palette rows"
    nav_rows() { dui '{"op":"sidebar"}' | jq -c '.sidebar[] | select(.kind == "worktree") | {title, set_marker, set_tooltip}'; }
    nav_rows >"$OUT/nav-set-rows.txt"
    check "R150-b Navigator: a set's Worktree keeps its place under its link with the marker ⧉" \
        bash -c "[[ \$(jq -r 'select(.title==\"feat-form\") | .set_marker' '$OUT/nav-set-rows.txt' | sort -u) == '⧉' ]] && [[ \$(jq -r 'select(.title==\"feat-form\")' '$OUT/nav-set-rows.txt' | jq -s length) == 3 ]]"
    check "R150-b …the tooltip says 'also in' the other links" \
        bash -c "jq -r 'select(.title==\"feat-form\") | .set_tooltip' '$OUT/nav-set-rows.txt' | grep -q 'also in'"
    check "R150-b a Worktree outside any set has no marker" \
        bash -c "w=\$(rade worktree add shop-web plain --project shop) && [[ \$(dui '{\"op\":\"sidebar\"}' | jq -r '[.sidebar[] | select(.kind==\"worktree\" and .title==\"plain\") | .set_marker] | .[0]') == null ]]"
    dui '{"op":"nav_click","kind":"project","title":"Shop"}' >/dev/null
    check "R150-b Project home is shown" eventually 5 is "dui '{\"op\":\"chrome\"}' | jq -r .chrome.breadcrumb" "Shop / Home"
    home() { dui '{"op":"home"}' | jq '.home'; }
    check "R150-b Project home: the header button 'New branch across sources…' sits between Settings and Open terminal" \
        is "home | jq -r '.buttons | join(\",\")'" "Settings,New branch across sources…,Open terminal"
    home >"$OUT/home-sets.json"
    check "R150-b Project home: a set is one row '<name> · <n> sources' with its Worktrees under it" \
        bash -c "jq -e '[.worktrees[] | select(.group == true) | .name] | sort == [\"feat-agent · 2 sources\", \"feat-form · 3 sources\"]' '$OUT/home-sets.json' >/dev/null && jq -e '[.worktrees[] | select(.set == \"feat-form\" and .group == false)] | length == 3' '$OUT/home-sets.json' >/dev/null"
    check "R150-b …the set's Worktrees follow their set row" \
        bash -c "jq -e '.worktrees as \$w | [\$w | to_entries[] | select(.value.group) | .key] | length == 2 and all(.[]; (\$w[. + 1].set == \$w[.].set) and (\$w[. + 1].group | not))' '$OUT/home-sets.json' >/dev/null"
    rade --json debug palette --limit 60 -- "feat-form" >"$OUT/palette-set.json"
    check "R150-b palette: the set row '<name> (set) · <Project>'" \
        bash -c "jq -e '[.results[] | select(.title == \"feat-form (set)\") | .detail] | length >= 1' '$OUT/palette-set.json' >/dev/null && jq -r '.results[] | select(.title == \"feat-form (set)\") | .detail' '$OUT/palette-set.json' | grep -q 'Shop'"
    rade --json debug palette --limit 60 -- "branch across" >"$OUT/palette-new.json"
    check "R150-b palette: 'New Branch Across Sources…'" \
        bash -c "jq -r '.results[].title' '$OUT/palette-new.json' | grep -qxF 'New Branch Across Sources…'"
    shot home-sets-dark
    light
    shot home-sets-light
    dark

    echo "== R151-d: Create Pull Requests for Set"
    commit_set feat-form
    : >"$FAKE_GH_LOG"
    dui '{"op":"dialog_open","kind":"set_prs","project":"shop","set":"feat-form"}' >/dev/null
    check "R151-d the dialog lists a row per Source with commits (3), each with its base" eventually 15 \
        bash -c "dlg | jq -e '.kind == \"set_prs\" and .loading == false and ([.rows[] | .source + \":\" + .base] == [\"shop-web:develop\",\"shop-api:main\",\"shared-ui:release/2.4\"])' >/dev/null"
    dui '{"op":"dialog_title","text":"Feature form across shop"}' >/dev/null
    dui '{"op":"dialog_body","text":"Adds the form."}' >/dev/null
    dlg >"$OUT/dialog-set-prs.json"
    shot set-prs-dark
    light
    shot set-prs-light
    dark
    dui '{"op":"dialog_primary"}' >/dev/null
    check "R151-d Create made the 3 PRs and the dialog closed" eventually 60 is "dlg | jq -r '.open'" false
    check "R151-d gh got 3 pr create" test "$(ghcalls 'pr create')" = 3
    check "R151-d …and 3 pr edit (one per PR)" eventually 20 is "ghcalls 'pr edit'" 3
    check "R151-d every PR body ends with the cross-links to the other PRs of the set" \
        bash -c "ok=0; for f in '$FAKE_GH_DIR'/edits/*; do grep -q 'Part of feat-form: ' \"\$f\" && grep -q 'shop-web#' \"\$f\" && grep -q 'shop-api#' \"\$f\" && grep -q 'shared-ui#' \"\$f\" && ok=\$((ok + 1)); done; [[ \$ok == 3 ]]"
    cp "$FAKE_GH_DIR"/edits/* "$OUT/" 2>/dev/null || true
    check "R151-d …and the Worktrees record their PR numbers (1 2 3)" \
        eventually 10 is "for s in shop-web shop-api shared-ui; do wtj \$s feat-form .pr.number; done | sort | tr '\n' ' '" "1 2 3 "

    echo "-- R151-d gh fails on Source 2: 1 PR, then Retry makes the other 2, none twice"
    rade worktree add --project shop feat-fail --sources all >/dev/null
    commit_set feat-fail
    : >"$FAKE_GH_LOG"
    rm -f "$FAKE_GH_DIR"/edits/*
    echo shop-api >"$FAKE_GH_DIR/fail-create"
    dui '{"op":"dialog_open","kind":"set_prs","project":"shop","set":"feat-fail"}' >/dev/null
    eventually 15 is "dlg | jq -r '.loading'" false || true
    dui '{"op":"dialog_title","text":"Failing feature"}' >/dev/null
    dui '{"op":"dialog_primary"}' >/dev/null
    check "R151-d with gh failing on shop-api: 1 PR made, the failure is reported on shop-api" eventually 60 \
        bash -c "dlg | jq -e '(.made | length) == 1 and .failed.source == \"shop-api\" and .busy == false' >/dev/null"
    dlg >"$OUT/dialog-set-prs-failed.json"
    check "R151-d …the button now says Retry" bash -c "jq -e '.buttons[1] == \"Retry\"' '$OUT/dialog-set-prs-failed.json' >/dev/null"
    shot set-prs-failed
    dui '{"op":"dialog_primary"}' >/dev/null
    check "R151-d Retry makes the other 2 and the dialog closes" eventually 60 is "dlg | jq -r '.open'" false
    check "R151-d gh ran pr create 4 times in all (3 PRs + the 1 failure)" test "$(ghcalls 'pr create')" = 4
    check "R151-d …the fake GitHub holds exactly one PR per Source (0 duplicates)" \
        bash -c "[[ \$(ls '$FAKE_GH_DIR'/prs/*@feat-fail.json | wc -l) == 3 ]] && [[ \$(jq -s 'map(length) | join(\",\")' '$FAKE_GH_DIR'/prs/*@feat-fail.json) == '\"1,1,1\"' ]]"
    check "R151-d …3 pr edit with the cross-links on the 3 PRs" eventually 20 \
        bash -c "[[ \$(grep -c 'pr edit' '$FAKE_GH_LOG') == 3 ]] && [[ \$(grep -l 'Part of feat-fail: ' '$FAKE_GH_DIR'/edits/* | wc -l) == 3 ]]"
    : >"$GHENV"
    stop_app
fi

if want budget; then
    echo "== E8: failing fixture -> the prompt on the Agent's screen (ci_failures = \"send\")"
    printf '[scm]\nci_failures = "send"\n' >"$CONF"
    ci_setup budget
    turn pending
    for round in 1 2 3; do
        turn passing
        newhead "b$round"
        fixture feat-ci "$d" pending 7 100
        before=$(sent)
        fixture feat-ci "$d" failing 7 100
        t0=$(date +%s.%N)
        OFF=$((OFF + 700))
        clock "$OFF"
        prtick >/dev/null
        until [[ $(sent) -gt $before ]] || (($(echo "$(date +%s.%N) - $t0 > 30" | bc))); do sleep 0.05; done
        t1=$(date +%s.%N)
        until screen_has "$agent" "CI failed on PR #7" || (($(echo "$(date +%s.%N) - $t0 > 30" | bc))); do sleep 0.05; done
        t2=$(date +%s.%N)
        p_s=$(echo "$t1 - $t0" | bc)
        s_s=$(echo "$t2 - $t0" | bc)
        note "E8 round $round: fixture failing -> tick -> prompt received by the Agent after $p_s s, on its screen after $s_s s"
        check "E8 round $round: the prompt reached the Agent within 10 s ($p_s s)" test "$(echo "$p_s <= 10" | bc)" = 1
        eventually 20 is "field $agent status" done || true
    done
    stop_app
    : >"$CONF"
fi

if want chips; then
    echo "== R146-d: the chips follow the fixture (Navigator, Changes card, Project home KPI)"
    echo "  SKIP R146-d: the chip UI (Navigator chip, Changes card chip, Pull requests KPI) is another task's; this part is filled in when it lands"
fi

echo
echo "m54: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
