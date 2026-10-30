#!/usr/bin/env bash
# M52 end-to-end tests (ADR-0006): Schedules (R136–R139). Part 1: runs into
# the Schedule's Tab under a Project link and in Scratch, skips (busy, paused,
# waiting), keep_runs over a renamed Tab, missed runs over a relaunch (skip /
# once), the default account at run time, a hidden Workspace, resume, and
# every `rade schedule` command with its refusals. The clock moves through
# `debug.schedule clock` / `RADE_TEST_CLOCK_OFFSET` (API.md "Test clock").
# Part 2 (PART=2 alone): the Schedules page and form, light + dark.
# Usage: tests/e2e/m52.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir, PART=1|2)
set -euo pipefail
[[ -n ${DEBUG_M52:-} ]] && set -x
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m52
source "$HERE/lib.sh"
PART=${PART:-all}

mkdir -p "$T/bin" "$XDG_CONFIG_HOME/rade"
{
    echo '#!/usr/bin/env bash'
    echo 'FAKE_AGENT=claude'
    tail -n +2 "$HERE/fake-agent.sh"
} >"$T/bin/claude"
chmod +x "$T/bin/claude"
export PATH=$T/bin:$PATH FAKE_ARGV=$T/argv.log FAKE_PROMPTS=$T/prompts.log
: >"$FAKE_ARGV"
: >"$FAKE_PROMPTS"
printf 'idle_seconds = 60\n' >"$XDG_CONFIG_HOME/rade/config.toml"
cat >"$T/envdump.sh" <<EOF
#!/usr/bin/env bash
echo "\$RADE_SCHEDULE \$PWD" >>"$T/env.log"
EOF
chmod +x "$T/envdump.sh"
: >"$T/env.log"

code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo "$c"; }
repo() { mkdir -p "$1" && git -C "$1" init -q -b main && git -C "$1" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init; }
clock() { rpc debug.schedule "{\"op\": \"clock\", \"offset\": $1}" >/dev/null; }
tick() { rpc debug.schedule '{"op": "tick"}' >/dev/null; }
runs() { rade schedule runs "$@" --json; }
nruns() { runs "$@" | jq '.runs | length'; }
pane_of() { rade list --all --json | jq -r --arg p "$1" ".panes[] | select(.pane == \$p) | $2"; }
# Timed Schedules fire at minute M of every hour, M = 30 minutes from now:
# `jump k` puts the clock 30 s after the k-th of those times, so each move
# passes exactly one fire time and lands within the 2-minute start window.
NOW=$(date +%s); B=$(( NOW - NOW % 60 + 1800 )); M=$(date -d "@$B" +%-M)
CRON="$M * * * *"
jump() { clock $(( B + 3600 * $1 + 30 - $(date +%s) )); tick; }

part1() {
start_app
echo "== setup: Project api (one link), a folder, accounts"
repo "$T/src/api"
rade source add "$T/src/api" >/dev/null
rade project add api --source api >/dev/null
mkdir -p "$T/x" "$HOME/c1" "$HOME/c2"
rade account add --agent claude --name a1 --config-dir "$HOME/c1" --default >/dev/null
rade account add --agent claude --name a2 --config-dir "$HOME/c2" >/dev/null

echo "== R138-a add, list, show, edit, on/off, remove"
rade schedule add nightly --project api --cron "0 2 * * *" "check deps" >/dev/null
TOMORROW=$(date -d tomorrow +%F)
echo "codex prompt" | rade schedule add once1 --dir "$T/x" --at "$TOMORROW 09:00" --prompt-file - >/dev/null
l=$(rade schedule list --json)
check "R138-a list has both, by Project then Scratch" \
    test "$(jq -c '[.schedules[] | [.name, .project, .link, .dir]]' <<<"$l")" = "[[\"nightly\",\"api\",\"api\",null],[\"once1\",null,null,\"$T/x\"]]"
check "R138-a when texts" test "$(jq -c '[.schedules[].when_text]' <<<"$l")" = "[\"Daily 02:00\",\"Once, $(date -d "$TOMORROW" '+%b %-d') 09:00\"]"
check "R138-a account null = default, account_now a1" test "$(jq -c '[.schedules[0].account, .schedules[0].account_now]' <<<"$l")" = '[null,"a1"]'
s=$(rade schedule show nightly --json)
check "R138-a show gives 5 next times" test "$(jq '.next_times | length' <<<"$s")" = 5
check "R138-a --prompt-file - read stdin" test "$(rade schedule show once1 --json | jq -r .prompt)" = "codex prompt"
rade schedule edit nightly --cron "0 3 * * *" >/dev/null
check "R138-a edit cron saved" eventually 3 is "jq -r '.schedules[] | select(.name == \"nightly\") | .cron' '$STATE'" "0 3 * * *"
rade schedule off nightly >/dev/null
check "R138-a off: no next" test "$(rade schedule show nightly --json | jq .next)" = null
check "R138-a off saved" eventually 3 is "jq -r '.schedules[] | select(.name == \"nightly\") | .enabled' '$STATE'" false
rade schedule on nightly >/dev/null
check "R138-a on: next set" test "$(rade schedule show nightly --json | jq '.next != null')" = true
rade schedule remove once1 >/dev/null
check "R138-a remove saved" eventually 3 is "jq '[.schedules[] | select(.name == \"once1\")] | length' '$STATE'" 0

echo "== R138-b refusals: exit codes, nothing written"
before=$(jq -c .schedules "$STATE")
check "R138-b bad cron → 2 naming the field" test "$(code rade schedule add b1 --project api --cron '61 * * * *' p)" = 2
grep -q minute "$T/err" && ok "R138-b names minute" || bad "R138-b names minute: $(cat "$T/err")"
check "R138-b unknown Project → 4" test "$(code rade schedule add b2 --project nope --cron '@daily' p)" = 4
check "R138-b missing folder → 2" test "$(code rade schedule add b3 --dir "$T/none" --cron '@daily' p)" = 2
check "R138-b unknown Preset → 2" test "$(code rade schedule add b4 --project api --agent gemini --cron '@daily' p)" = 2
check "R138-b at in the past → 2" test "$(code rade schedule add b5 --project api --at '2020-01-01 09:00' p)" = 2
check "R138-b used name → 2" test "$(code rade schedule add nightly --project api --cron '@daily' p)" = 2
check "R138-b unknown account → 2" test "$(code rade schedule add b6 --project api --account ghost --cron '@daily' p)" = 2
check "R138-b unknown schedule → 4" test "$(code rade schedule show ghost)" = 4
check "R138-b nothing written" test "$(jq -c .schedules "$STATE")" = "$before"

echo "== R137-a a run under the link and one in Scratch"
rade schedule add linked --project api --cron "$CRON" "run:$T/envdump.sh" >/dev/null
rade schedule add folder --dir "$T/x" --cron "$CRON" "run:$T/envdump.sh" >/dev/null
jump 0
check "R137-a linked run 1 started" eventually 5 is "runs linked | jq -r '.runs[0].outcome'" started
check "R137-a folder run 1 started" eventually 5 is "runs folder | jq -r '.runs[0].outcome'" started
LP=$(runs linked | jq -r '.runs[0].pane'); FP=$(runs folder | jq -r '.runs[0].pane')
check "R137-a names <name>-1" test "$(pane_of "$LP" .name) $(pane_of "$FP" .name)" = "linked-1 folder-1"
check "R137-a linked Pane on the api Source" test "$(pane_of "$LP" .source)" = api
check "R137-a folder Pane in Scratch" test "$(pane_of "$FP" .source)" = general
LT=$(rade schedule show linked --json | jq -r .tab)
check "R137-a Tab ⏱ linked" test "$(rade tab list --source api --json | jq -r --arg t "$LT" '.tabs[] | select(.tab == $t) | .name')" = "⏱ linked"
check "R137-a the run records a session and account a1" \
    test "$(runs linked | jq -r '.runs[0] | [(.session != null), .account] | @tsv')" = "$(printf 'true\ta1')"
check "R137-a RADE_SCHEDULE + cwd" eventually 10 is "sort '$T/env.log' | tr '\n' '|'" "folder $T/x|linked $T/src/api|"
rade schedule off linked >/dev/null
rade schedule off folder >/dev/null

echo "== R137-b skip while busy, paused, waiting"
rade schedule add busy --project api --cron "$CRON" "spin" >/dev/null
jump 1
check "R137-b run 1 started" eventually 5 is "runs busy | jq -r '.runs[0].outcome'" started
jump 2
check "R137-b run 2 skipped: previous run still running" eventually 5 is "runs busy | jq -r '.runs[0] | .outcome + \" \" + .reason'" "skipped previous run still running"
check "R137-b no new Pane" test "$(rade list --json | jq '[.panes[] | select((.name // "") | startswith("busy-"))] | length')" = 1
rade close "$(runs busy | jq -r '.runs[1].pane')" --force >/dev/null
rade source pause api --force >/dev/null
jump 3
check "R137-b run 3 skipped: source paused" eventually 5 is "runs busy | jq -r '.runs[0] | .outcome + \" \" + .reason'" "skipped source paused"
rade source resume api >/dev/null
rade schedule edit busy "ask which" >/dev/null
jump 4
check "R137-b run 4 started (asks)" eventually 5 is "runs busy | jq -r '.runs[0].outcome'" started
eventually 10 is "pane_of $(runs busy | jq -r '.runs[0].pane') .status" waiting || true
jump 5
check "R137-b waiting run → skipped" eventually 5 is "runs busy | jq -r '.runs[0] | .outcome + \" \" + .reason'" "skipped previous run still running"
rade schedule off busy >/dev/null

echo "== R137-c keep_runs 2 over a renamed Tab"
rade schedule add keep --project api --cron "@yearly" --keep-runs 2 "hello" >/dev/null
for i in 1 2 3; do
    rade schedule run keep >/dev/null
    eventually 10 is "runs keep | jq -r '.runs[0].status'" done || true
    if [[ $i == 1 ]]; then
        KT=$(rade schedule show keep --json | jq -r .tab)
        rade tab rename "$KT" "my runs" >/dev/null
    fi
done
check "R137-c 3 Runs listed" test "$(nruns keep)" = 3
check "R137-c only the last 2 Panes open" test "$(rade list --json | jq -c '[.panes[] | select((.name // "") | startswith("keep-")) | .name] | sort')" = '["keep-2","keep-3"]'
check "R137-c in the renamed Tab" test "$(rade list --json | jq -r '[.panes[] | select(.name == "keep-3") | .tab][0]')" = "$KT"
check "R137-c final status kept on the closed run" test "$(runs keep | jq -r '.runs[2] | [.pane, .status] | @tsv')" = "$(printf '\tdone')"

echo "== R137-f resume: closed → --resume, open → focus, missed → refused"
S1=$(runs keep | jq -r '.runs[2].session')
r=$(rade schedule resume keep 1 --json)
check "R137-f closed run resumed in a new Pane" test "$(jq -r .resumed <<<"$r")" = true
RP=$(jq -r .pane <<<"$r")
check "R137-f Pane keep-1-r in the Schedule's Tab" test "$(pane_of "$RP" '.name + " " + .tab')" = "keep-1-r $KT"
check "R137-f ran claude --resume <session>" eventually 5 grep -q -- "^$RP --resume $S1" "$FAKE_ARGV"
r=$(rade schedule resume keep 3 --json)
check "R137-f open run focused" test "$(jq -r '.resumed' <<<"$r")" = false
check "R138-d resume with no number = the newest started Run" test "$(rade schedule resume keep --json | jq -r .pane)" = "$(runs keep | jq -r '.runs[0].pane')"

echo "== review fixes: an early run of an at Schedule, then on; remove + add keeps Panes apart; two runs at once"
rade schedule add early --project api --at "$(date -d tomorrow +%F) 10:00" "hello" >/dev/null
rade schedule run early >/dev/null
check "R136-c an early run leaves it done and off" test "$(rade schedule show early --json | jq -c '[.done, .enabled]')" = '[true,false]'
rade schedule on early >/dev/null
check "R136-c on again: not done, its time next" test "$(rade schedule show early --json | jq -c '[.done, .enabled, (.next == .at)]')" = '[false,true,true]'
rade schedule remove keep >/dev/null
rade schedule add keep --project api --cron "@yearly" "hello" >/dev/null
rade schedule run keep >/dev/null
check "R137-7 a re-added Schedule opens its own Tab" eventually 5 bash -c "test \"\$(rade schedule show keep --json | jq -r .tab)\" != '$KT'"
rade schedule add twice --project api --cron "@yearly" "spin" >/dev/null
rade schedule run twice >/dev/null & bg=$!; rade schedule run twice >/dev/null; wait "$bg"
check "R137-3 two runs at once: one started, one skipped" test "$(runs twice | jq -c '[.runs[].outcome] | sort')" = '["skipped","started"]'
check "R137-2 the Pane's number is its Run's" test "$(pane_of "$(runs twice | jq -r '.runs[] | select(.outcome == "started") | .pane')" .name)" = "twice-$(runs twice | jq -r '.runs[] | select(.outcome == "started") | .n')"
rade schedule off twice >/dev/null

echo "== R138-d run now, runs --json"
check "R138-d runs --limit 1" test "$(runs keep --limit 1 | jq '.runs | length')" = 1

echo "== R137-e default account at run time, a hidden Workspace"
rade schedule add acct --project api --cron "$CRON" "hello" >/dev/null
rade account default a2 >/dev/null
root=$HOME/workspaces/w2
mkdir -p "$root/.rade" "$T/y"
printf 'name = "w2"\n' >"$root/.rade/workspace.toml"
rade workspace add "$root" >/dev/null
rade schedule add hidden --workspace w2 --dir "$T/y" --cron "$CRON" "hello" >/dev/null
check "R137-e w2 is hidden" test "$(rpc debug.state | jq -r '.active_workspace // "default"')" != w2
jump 6
check "R137-e runs under the new default a2" eventually 5 is "runs acct | jq -r '.runs[0].account'" a2
check "R137-e the hidden Workspace fired" eventually 5 is "runs hidden --workspace w2 | jq -r '.runs[0].outcome'" started
rade schedule off acct >/dev/null
rade schedule off hidden --workspace w2 >/dev/null

echo "== R137-d missed over a relaunch: skip and once"
rade schedule add hourly --project api --cron "$CRON" "hello" >/dev/null
rade schedule add catchup --project api --cron "$CRON" --catch-up once "hello" >/dev/null
sleep 1.5 # saved
stop_app
echo "== R138-c next without the app"
n=$(rade schedule next "*/20 * * * *" --count 3)
check "R138-c three times" test "$(wc -l <<<"$n")" = 3
d=$(( $(date -d "$(sed -n 2p <<<"$n")" +%s) - $(date -d "$(sed -n 1p <<<"$n")" +%s) ))
check "R138-c 20 minutes apart" test "$d" = 1200
RADE_TEST_CLOCK_OFFSET=$(( B + 3600 * 9 + 30 - $(date +%s) )) start_app
check "R137-d skip: one missed Run of 3" eventually 30 is "runs hourly | jq -r '.runs[0] | .outcome + \" \" + (.missed|tostring)'" "missed 3"
check "R137-d no Pane for it" test "$(rade list --json | jq '[.panes[] | select((.name // "") | startswith("hourly-"))] | length')" = 0
check "R137-d once: one started catch-up Run" eventually 30 is "runs catchup | jq -r '.runs[0] | .outcome + \" \" + (.missed|tostring)'" "started 3"
check "R137-d one Run each" test "$(nruns hourly) $(nruns catchup)" = "1 1"
check "R137-f a missed Run is refused: no session" test "$(code rade schedule resume hourly 1)" = 2
grep -q "has no session to resume" "$T/err" && ok "R137-f says has no session to resume" || bad "R137-f message: $(cat "$T/err")"
shot m52-runs
stop_app
}

ui() { rpc debug.ui "{\"op\": \"schedules\"${1:+, $1}}" | jq '.schedules'; }
focused() { rade debug state | jq -r '.active_source as $a | .sources[] | select(.id==$a) | .active_worktree as $w | .worktrees[] | select(.id==$w) | .active_tab as $t | .tabs[] | select(.id==$t) | .focused'; }

part2() {
start_app
# Part 1's Schedules (a full run): part 2 starts from none.
rade schedule list --all --json | jq -r '.schedules[] | "\(.workspace) \(.name)"' | while read -r w n; do
    rade schedule remove "$n" --workspace "$w" >/dev/null
done
echo "== part 2 setup: Projects shop and ops, a folder"
for r in shop ops; do
    repo "$T/src/$r"
    rade source add "$T/src/$r" >/dev/null
    rade project add "$r" --source "$r" >/dev/null
done
mkdir -p "$T/gone"
rade schedule add a1 --project shop --cron "0 9 * * 1-5" "hello" >/dev/null
rade schedule add a2 --project shop --cron "@daily" --off "hello" >/dev/null
rade schedule add b1 --project ops --cron "0 */2 * * *" "hello" >/dev/null
rade schedule add c1 --dir "$T/gone" --cron "@weekly" "hello" >/dev/null
rmdir "$T/gone"
rade schedule run c1 >/dev/null || true
check "R139-a c1's run failed" test "$(runs c1 | jq -r '.runs[0].outcome')" = failed

echo "== R139-a the page from the Statusline cell, grouped by Project then Scratch"
cells() { rpc debug.ui '{"op": "chrome"}' | jq -r '.chrome.statusline.cells | join("|")'; }
check "R139-a the Statusline cell says 3 on" eventually 5 bash -c "$(declare -f rpc cells); cells | grep -qF '⏱ 3'"
rpc debug.ui '{"op": "click", "widget": "status-schedules"}' >/dev/null
check "R139-a the page is open" eventually 5 is "ui | jq -r .open" true
eventually 5 is "ui | jq '.groups | length'" 3 || true
u=$(ui)
check "R139-a groups shop, ops, Scratch" test "$(jq -c '[.groups[].title]' <<<"$u")" = '["shop","ops","Scratch"]'
check "R139-a LEDs: a1 on, a2 off, b1 on, c1 failed" \
    test "$(jq -c '[.groups[].rows[] | [.name, .state]]' <<<"$u")" = '[["a1","on"],["a2","off"],["b1","on"],["c1","failed"]]'
check "R139-a when texts" test "$(jq -c '[.groups[].rows[].when]' <<<"$u")" = '["Weekdays 09:00","Daily 00:00","Every 2 h","Sundays 00:00"]'
check "R139-a next times on the on rows, none on off" test "$(jq -c '[.groups[].rows[] | (.next != null)]' <<<"$u")" = '[true,false,true,true]'
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    shot "m52-page-$th"
done
rade debug action color_scheme_dark >/dev/null
ui '"arg": "filter", "project": "shop"' >/dev/null
check "R139-a the Project filter leaves one group" eventually 5 is "ui | jq -c '[.groups[].title]'" '["shop"]'
ui '"arg": "filter", "project": null' >/dev/null

echo "== R139-b the form: Weekdays 09:00, codex, then a bad cron"
ui '"arg": "new"' >/dev/null
ui '"arg": "form_set", "field": "name", "value": "fromui"' >/dev/null
ui '"arg": "form_set", "field": "target", "value": "project"' >/dev/null
ui '"arg": "form_set", "field": "project", "value": "ops"' >/dev/null
ui '"arg": "form_set", "field": "preset", "value": "Weekdays 09:00"' >/dev/null
ui '"arg": "form_set", "field": "agent", "value": "codex"' >/dev/null
f=$(ui '"arg": "form_set", "field": "prompt", "value": "summarize yesterday"' | jq .form)
check "R139-b the next 3 times show" test "$(jq -r .preview <<<"$f" | grep -o '·' | wc -l)" = 2
check "R139-b Create enabled" test "$(jq -r .create_enabled <<<"$f")" = true
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    shot "m52-form-$th"
done
rade debug action color_scheme_dark >/dev/null
f=$(ui '"arg": "form_set", "field": "cron", "value": "61 * * * *"' | jq .form)
check "R139-b a bad cron shows its error" test "$(jq -r '.preview_error' <<<"$f")" = true
jq -r .preview <<<"$f" | grep -q minute && ok "R139-b the error names the field" || bad "R139-b preview: $(jq -r .preview <<<"$f")"
check "R139-b Create disabled" test "$(jq -r .create_enabled <<<"$f")" = false
shot m52-form-bad-cron
ui '"arg": "form_set", "field": "cron", "value": "0 9 * * 1-5"' >/dev/null
ui '"arg": "form_submit"' >/dev/null
check "R139-b Create wrote the Schedule" eventually 5 is "rade schedule show fromui --json | jq -r '[.project, .link, .cron, .agent, .prompt] | @tsv'" "$(printf 'ops\tops\t0 9 * * 1-5\tcodex\tsummarize yesterday')"
check "R139-b the form closed, the new row selected" eventually 5 is "ui | jq -c '[(.form == null), .selected[1]]'" '[true,"fromui"]'
ui '"arg": "edit", "name": "fromui"' >/dev/null
check "R139-b Edit fills the form" eventually 5 is "ui | jq -r '[.form.params.name, .form.cron, .form.params.agent] | @tsv'" "$(printf 'fromui\t0 9 * * 1-5\tcodex')"
ui '"arg": "form_cancel"' >/dev/null
rade schedule add oncedone --project ops --at "$(date -d tomorrow +%F) 10:00" "old prompt" >/dev/null
rade schedule run oncedone >/dev/null
ui '"arg": "edit", "name": "oncedone"' >/dev/null
check "R136-c Edit opens the done Schedule" eventually 5 is "ui | jq -r .form.params.name" oncedone
f=$(ui '"arg": "form_set", "field": "prompt", "value": "new prompt"' | jq .form)
check "R136-c a done Schedule's prompt edit: Save enabled, no at sent" test "$(jq -c '[.create_enabled, (.params | has("at")), (.params | has("account"))]' <<<"$f")" = '[true,false,false]'
ui '"arg": "form_submit"' >/dev/null
check "R136-c saved: new prompt, still done" eventually 5 is "rade schedule show oncedone --json | jq -c '[.prompt, .done]'" '["new prompt",true]'

echo "== R139-c Run now, then Resume of the older run"
ui '"arg": "select", "workspace": "default", "name": "a1"' >/dev/null
eventually 5 is "ui | jq -r .selected[1]" a1 || true
ui '"arg": "run_now"' >/dev/null
check "R139-c Run now started run 1" eventually 10 is "runs a1 | jq -r '.runs[0] | [.n, .outcome] | @tsv'" "$(printf '1\tstarted')"
eventually 10 is "runs a1 | jq -r '.runs[0].status'" done || true
P1=$(runs a1 | jq -r '.runs[0].pane'); S1=$(runs a1 | jq -r '.runs[0].session')
rade close "$P1" >/dev/null
ui '"arg": "run_now"' >/dev/null
check "R139-c run 2 started" eventually 10 is "runs a1 | jq -r '.runs[0].n'" 2
eventually 5 is "ui | jq '[.detail.runs[] | select(.n == 1 and .pane == null)] | length'" 1 || true
ui '"arg": "resume", "n": 1' >/dev/null
check "R139-c the old session resumed in the Schedule's Tab" eventually 10 grep -q -- "--resume $S1" "$FAKE_ARGV"
check "R139-c recorded as run 1's resumed Pane" eventually 5 is "runs a1 | jq -r '.runs[1].resumed | type'" string
R1=$(runs a1 | jq -r '.runs[1].resumed')
check "R139-e Resume closes the page" eventually 5 is "ui | jq -r .open" false
check "R139-e the resumed Pane is focused" eventually 5 is focused "$R1"
ui '"arg": "open", "name": "a1"' >/dev/null
eventually 5 is "ui | jq -r .open" true || true
P2=$(runs a1 | jq -r '.runs[0].pane')
ui '"arg": "resume", "n": 2' >/dev/null
check "R139-e Show closes the page" eventually 5 is "ui | jq -r .open" false
check "R139-e the open Run's Pane is focused" eventually 5 is focused "$P2"
ui '"arg": "open", "name": "a1"' >/dev/null
eventually 5 is "ui | jq -r .open" true || true
for th in dark light; do
    rade debug action "color_scheme_$th" >/dev/null; sleep 1
    shot "m52-detail-$th"
done
rade debug action color_scheme_dark >/dev/null

echo "== R139-d Delete… asks; Panes stay"
before=$(rade list --json | jq '[.panes[] | select((.name // "") | startswith("a1-"))] | length')
ui '"arg": "delete"' >/dev/null
check "R139-d Delete asks" eventually 5 is "ui | jq -r .delete_open" true
shot m52-delete
ui '"arg": "delete_respond", "response": "delete", "close_panes": false' >/dev/null
check "R139-d the Schedule is gone" eventually 5 is "rade schedule list --json | jq '[.schedules[] | select(.name == \"a1\")] | length'" 0
check "R139-d its Panes stay" test "$(rade list --json | jq '[.panes[] | select((.name // "") | startswith("a1-"))] | length')" = "$before"
ui '"arg": "close"' >/dev/null
check "R139-a Esc / ✕ closes the page" eventually 5 is "ui | jq -r .open" false
stop_app
}

case $PART in
1) part1 ;;
2) part2 ;;
*) part1; part2 ;;
esac
echo "M52: $PASS passed, $FAIL failed"
((FAIL == 0))
