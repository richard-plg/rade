#!/usr/bin/env bash
# M34 end-to-end tests (PRD R101, ADR-0033): the engines removed in 1.0.0.
# The 7 removed commands exit 2 naming themselves (R101-a); the removed
# socket methods answer the contract error with data.removed, an unknown
# one without it (R101-b); Task / Batch / journal files on disk keep their
# bytes and mtime through start, spawn, worktree add and quit (R101-e);
# scripts/check-removed.sh passes on this tree and names a planted
# `rade task add` (R101-f); an app restored from the 0.7.1 fixture keeps the
# former Task Run Tab and its Panes have no RADE_TASK* / RADE_INITIATIVE*
# (R101-g); `rade setup --agent claude` in a temp HOME removes the 5 old
# skills (R101-i); the palette and the Tab / Project menus name none of the
# removed things (R101-j). Fake Agents only; a temp HOME for everything.
# Usage: tests/e2e/m34.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m34
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
FIXTURE=$ROOT/crates/rade-core/tests/fixtures/state-0.7.1.json
CONF=$XDG_CONFIG_HOME/rade/config.toml
mkdir -p "$XDG_CONFIG_HOME/rade" "$T/bin"
# The fixture's Panes run `claude` and `codex`: the fake Agent, never a real one.
ln -s "$FAKE" "$T/bin/claude"
ln -s "$FAKE" "$T/bin/codex"
export PATH=$T/bin:$PATH
cat >"$CONF" <<EOF
idle_seconds = 1
[agents.claude]
command = "$FAKE"
EOF

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
# `code <cmd…>` → its exit code; stdout in $T/out, stderr in $T/err.
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }

echo "== R101-a the 7 removed commands"
for c in "task list" "batch status" "initiative list" "digest" "report list" "flow" "lessons"; do
    read -ra argv <<<"$c"
    check "R101-a rade $c exits 2" test "$(code rade "${argv[@]}")" = 2
    check "R101-a … stderr: rade: '${argv[0]}' was removed in 1.0.0 (ADR-0033)" \
        test "$(cat "$T/err")" = "rade: '${argv[0]}' was removed in 1.0.0 (ADR-0033)"
done

echo "== the 0.7.1 tree: Workspace acme with Project shop, its Task / Batch / journal files"
# The fixture's @ROOT@, and HOME = its @ROOT@/home (the default Workspace's
# root is HOME): a short path outside the repository, because the palette
# also matches a Workspace's root path and ">report" would match
# ".../personal/RADE/target/..." (R101-j counts results). Digits only.
R=/tmp/rade-m34-$$
rm -rf "$R" && mkdir -p "$R/home"
trap 'rm -rf "$R"; cleanup' EXIT
export HOME=$R/home
printf 'PS1="$ "\n' >"$HOME/.bashrc"
A=$R/acme
mkdir -p "$A/.rade/projects/shop/tasks" "$A/.rade/projects/shop/batches" "$A/.rade/events" \
    "$R/home/notes" "$R/home/tools"
printf 'name = "Acme"\n' >"$A/.rade/workspace.toml"
printf 'name = "Shop"\n\n[[source]]\nid = "shop-api"\n\n[[source]]\nid = "shop-web"\n' \
    >"$A/.rade/projects/shop/project.toml"
for r in shop-api shop-web; do
    mkdir -p "$A/$r" && G -C "$A/$r" init -q -b main && echo "# $r" >"$A/$r/README.md" &&
        G -C "$A/$r" add -A && G -C "$A/$r" commit -qm init
done
G -C "$A/shop-api" worktree add -q -b otp "$A/.worktrees/shop-api/otp"
G -C "$A/shop-api" worktree add -q -b cart "$A/.worktrees/shop-api/cart"
G -C "$A/shop-web" worktree add -q -b checkout "$A/.worktrees/shop-web/checkout"
KEPT=("$A/.rade/projects/shop/tasks/T-1-otp.md" "$A/.rade/projects/shop/batches/B-1-x.md" "$A/.rade/events/2026-10-20.jsonl")
printf -- '---\nid: 1\ntitle: OTP\nstatus: running\n---\n\n## Goal\nOTP login.\n\n## Log\n- 2026-10-20 10:00 run started\n' >"${KEPT[0]}"
printf -- '---\nid: 1\ntitle: x\nstatus: paused\ntasks:\n  - T-1\n---\n' >"${KEPT[1]}"
printf '{"ts":"2026-10-20T10:00:00+07:00","ev":"task.status","task":"shop/T-1","status":"running"}\n' >"${KEPT[2]}"
touch -d '2026-10-20 10:00:00' "${KEPT[@]}"
fingerprint() { local f; for f in "${KEPT[@]}"; do echo "$(sha256sum <"$f" | cut -c1-64) $(stat -c %y "$f") ${f#"$A"/}"; done; }
BEFORE=$(fingerprint)
mkdir -p "$(dirname "$STATE")"
sed "s#@ROOT@#$R#g" "$FIXTURE" >"$STATE"
check "the fixture is a version 2 state.json with a Task Run Tab" test "$(jq -r '"\(.version) \([.. | objects | select(.layout? != null and .task? == "shop/T-1") | .id] | join(","))"' "$STATE")" = "2 t_11"

start_app

echo "== R101-b the removed socket methods"
for m in task.list batch.status initiative.list digest.get report.list flow.get lessons.draft debug.batch; do
    check "R101-b $m answers code 2, '$m' was removed in RADE 1.0.0 (ADR-0033), data {kind: USAGE, removed: 1.0.0}" \
        test "$(rpc "$m" '{}' | jq -c '[.code, .message, .data]')" = "[2,\"'$m' was removed in RADE 1.0.0 (ADR-0033)\",{\"kind\":\"USAGE\",\"removed\":\"1.0.0\"}]"
done
check "R101-b foo.bar answers code 2 without removed" test "$(rpc foo.bar '{}' | jq -c '[.code, (.data.removed // "none")]')" = '[2,"none"]'

echo "== R101-g the app restored from the 0.7.1 fixture"
check "state.json is version 3 now, with state.json.v2.bak" eventually 5 bash -c "[[ \$(jq -r .version '$STATE') == 3 && -s '$STATE.v2.bak' ]]"
check "every Tab of the fixture is there" test "$(rade debug state | jq '[.. | objects | select(.layout? != null) | .id] | contains(["t_11","t_15","t_18","t_21","t_23"])')" = true
check "R101-g the restored Task Run Tab still exists" test "$(rade debug state | jq '[.. | objects | select(.id? == "t_11" and .layout? != null)] | length')" = 1
for p in p_13 p_14; do
    check "$p (a restored Pane) is running" eventually 15 bash -c "rade list --all --json | jq -e '.panes[] | select(.pane == \"$p\")' >/dev/null"
    eventually 10 screen_has "$p" '$' || true
    rade send "$p" "env >$T/env-$p.txt" --enter >/dev/null
    check "… env ran in it" eventually 10 grep -q "^RADE_PANE_ID=$p\$" "$T/env-$p.txt"
done
check "R101-g none of RADE_TASK, RADE_TASK_FILE, RADE_TASK_WORKTREES, RADE_INITIATIVE, RADE_INITIATIVE_DIR is set in the Task Run Tab's Pane" \
    bash -c "! grep -qE '^(RADE_TASK|RADE_TASK_FILE|RADE_TASK_WORKTREES|RADE_INITIATIVE|RADE_INITIATIVE_DIR)=' '$T/env-p_13.txt'"
check "R101-g … nor in the former Initiative Tab's Pane" \
    bash -c "! grep -qE '^(RADE_TASK|RADE_TASK_FILE|RADE_TASK_WORKTREES|RADE_INITIATIVE|RADE_INITIATIVE_DIR)=' '$T/env-p_14.txt'"

echo "== R101-j the palette and the Tab / Project menus"
rade workspace switch acme >/dev/null
for q in task batch report; do
    PR=$(rpc debug.palette "{\"query\":\">$q\"}" | jq -r '[.results[] | "\(.title) — \(.detail)"] | join("|")')
    [[ -z $PR ]] || echo "    >$q: $PR"
    check "R101-j the palette gives 0 results for >$q" test -z "$PR"
done
TABM=$(rade debug menu tab t_11 --json | jq -r '[.items[].label] | join("|")')
PROJM=$(rade debug menu project shop --json | jq -r '[.items[].label] | join("|")')
echo "    tab menu: $TABM"
echo "    project menu: $PROJM"
check "the Tab and Project menus have items" bash -c "[[ -n '$TABM' && -n '$PROJM' ]]"
check "R101-j no Tab menu item contains Task, Batch, Initiative, Report or Start here" bash -c "! grep -qE 'Task|Batch|Initiative|Report|Start here' <<<'$TABM'"
check "R101-j no Project menu item contains Task, Batch, Initiative, Report or Start here" bash -c "! grep -qE 'Task|Batch|Initiative|Report|Start here' <<<'$PROJM'"
sleep 0.5
shot restored-light
rade debug action color_scheme_dark >/dev/null
sleep 0.5
shot restored-dark
rade debug action color_scheme_light >/dev/null

echo "== R101-e start, spawn an Agent, add a Worktree, quit: the files are untouched"
AG=$(rade spawn --source shop-api --agent claude --json | jq -r .pane)
check "an Agent spawned" eventually 10 screen_has "$AG" "fake-agent session"
check "a Worktree added" test -d "$(rade worktree add shop-api extra)"
stop_app
AFTER=$(fingerprint)
echo "$AFTER" | sed 's/^/    /'
check "R101-e each of the 3 files has its original bytes and mtime" test "$AFTER" = "$BEFORE"
check "… and RADE wrote nothing else under .rade/projects/shop or .rade/events" \
    test "$(find "$A/.rade/projects/shop" "$A/.rade/events" -type f | sort | sed "s#$A/##" | tr '\n' ' ')" = ".rade/events/2026-10-20.jsonl .rade/projects/shop/batches/B-1-x.md .rade/projects/shop/project.toml .rade/projects/shop/tasks/T-1-otp.md "

echo "== R101-f scripts/check-removed.sh"
check "R101-f the 1.0 tree: exit 0" test "$(code "$ROOT/scripts/check-removed.sh")" = 0
TREE=$T/tree
mkdir -p "$TREE"
cp -r "$ROOT/crates" "$ROOT/skills" "$ROOT/scripts" "$TREE/"
sed -i '5a Add one with `rade task add "OTP"`.' "$TREE/skills/rade/SKILL.md"
check "R101-f a copy with rade task add in skills/rade/SKILL.md: exit 1" \
    test "$(code "$TREE/scripts/check-removed.sh" --bin "$BIN/rade")" = 1
check "R101-f … naming skills/rade/SKILL.md and the line" grep -qx 'skills/rade/SKILL.md:6:Add one with `rade task add "OTP"`.' "$T/out"

echo "== R101-i rade setup --agent claude over 0.7.1's skills (temp HOME)"
H=$T/setup-home
for s in rade rade-config rade-task rade-batch rade-initiative rade-design rade-report; do
    mkdir -p "$H/.claude/skills/$s"
    printf -- '---\nname: %s\n---\n0.7.1 skill\n' "$s" >"$H/.claude/skills/$s/SKILL.md"
done
check "rade setup --agent claude runs" test "$(code env HOME="$H" XDG_CONFIG_HOME="$H/.config" rade setup --agent claude)" = 0
check "R101-i the 5 removed skill folders are gone" \
    bash -c "for s in rade-task rade-batch rade-initiative rade-design rade-report; do [[ ! -e '$H/.claude/skills/'\$s ]] || exit 1; done"
check "R101-i rade, rade-workspace, rade-config are installed" \
    test "$(cd "$H/.claude/skills" && ls -1 */SKILL.md | LC_ALL=C sort | tr '\n' ' ')" = "rade-config/SKILL.md rade-workspace/SKILL.md rade/SKILL.md "
check "… with 1.0's text" cmp -s "$H/.claude/skills/rade/SKILL.md" "$ROOT/skills/rade/SKILL.md"

echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
