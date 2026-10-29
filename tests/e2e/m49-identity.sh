#!/usr/bin/env bash
# M49 end-to-end tests (ADR-0006): one Agent per Pane (R128) — a Claude Pane
# whose Agent runs a one-shot `codex exec` (in the foreground, in the
# background, orphaned) keeps its Agent, session and glyph, and Session Restore
# after kill -9 resumes Claude, never the Codex run.
# `claude` and `codex` are copies of tests/e2e/fake-agent.sh under those names
# (the built-in Presets' programs), so the CLI's process-tree guard and the
# recorded invocation see what they see with the real CLIs.
# Usage: tests/e2e/m49-identity.sh (or tests/e2e/m49.sh for all of M49)   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m49/identity
source "$HERE/lib.sh"

FAKE=$HERE/fake-agent.sh
mkdir -p "$T/bin" "$T/repo" "$XDG_CONFIG_HOME/rade"
for a in claude codex; do
    {
        echo '#!/usr/bin/env bash'
        echo "FAKE_AGENT=$a"
        tail -n +2 "$FAKE" | sed "s/echo \"\${RADE_PANE_ID:-} \$\*\"/echo \"\${RADE_PANE_ID:-} $a \$*\"/"
    } >"$T/bin/$a"
    chmod +x "$T/bin/$a"
done
export PATH=$T/bin:$PATH FAKE_ARGV=$T/argv.log
: >"$FAKE_ARGV"
git -C "$T/repo" init -q -b main
git -C "$T/repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
printf 'idle_seconds = 60\n' >"$XDG_CONFIG_HOME/rade/config.toml"

# The one-shot image jobs a Claude skill runs (livestream tools/img.sh).
cat >"$T/img-fg.sh" <<'EOF'
#!/usr/bin/env bash
codex --no-daemon exec --skip-git-repo-check -s workspace-write 'draw a kitchen' </dev/null >/dev/null 2>&1
EOF
cat >"$T/img-bg.sh" <<'EOF'
#!/usr/bin/env bash
codex --no-daemon exec --skip-git-repo-check -s workspace-write 'draw a kitchen' </dev/null >/dev/null 2>&1 &
wait
EOF
cat >"$T/img-orphan.sh" <<'EOF'
#!/usr/bin/env bash
setsid -f codex --no-daemon exec --skip-git-repo-check -s workspace-write 'draw a kitchen' </dev/null >/dev/null 2>&1
sleep 2
EOF
chmod +x "$T"/img-*.sh

field() { rade list --json | jq -r --arg p "$1" ".panes[] | select(.pane==\$p) | .$2"; }
pane_state() { rade debug state | jq -c --arg p "$1" '[.. | objects | select(.id? == $p and has("kind"))][0]'; }
sid_of() { pane_state "$1" | jq -r '.session_id // ""'; }
agent_of() { pane_state "$1" | jq -r '.agent // ""'; }
inv0() { pane_state "$1" | jq -r '(.invocation.argv // [""])[0] | split("/")[-1]'; }
ignored_lines() { rpc debug.ui '{"op":"activity"}' | jq --arg n "$1" '[.activity[]? | select(.kind == "ignored" and .actor == $n and .text == "ignored a codex session (started by its Agent)")] | length'; }

start_app
rade source add "$T/repo" >/dev/null
echo "== R128: a Claude Pane running codex exec keeps its Agent"
declare -A PANE SID
for mode in fg bg orphan; do
    PANE[$mode]=$(rade spawn --agent claude --name "c-$mode" --source repo --tab new --json | jq -r .pane)
done
for mode in fg bg orphan; do
    p=${PANE[$mode]}
    eventually 10 screen_has "$p" "> " || true
    eventually 10 is "agent_of $p" claude || true
    SID[$mode]=$(sid_of "$p")
    check "R128-e ($mode) starts as claude with a session" test "$(agent_of "$p")" = claude -a -n "${SID[$mode]}"
    rade send "$p" "run:$T/img-$mode.sh" --enter >/dev/null
    eventually 10 screen_has "$p" "REPLY: RAN" || true
    sleep 1 # the orphaned job reports after its parent has gone
done
for mode in fg bg orphan; do
    p=${PANE[$mode]}
    check "R128-e ($mode) keeps agent claude" test "$(agent_of "$p")" = claude
    check "R128-e ($mode) keeps its Claude session" test "$(sid_of "$p")" = "${SID[$mode]}"
    check "R128-e ($mode) keeps a claude invocation" bash -c "[[ '$(inv0 "$p")' == claude* ]]"
    check "R128-e ($mode) glyph is claude" \
        test "$(rpc debug.ui "{\"op\":\"pane\",\"pane\":\"$p\"}" | jq -r '.pane.glyph | if type == "object" then .mark // .glyph else . end')" = claude
done
check "R128-e an Ignored line for the orphaned run only" \
    test "$(ignored_lines c-orphan)" = 1 -a "$(ignored_lines c-fg)" = 0 -a "$(ignored_lines c-bg)" = 0
shot identity-glyphs

echo "== R128-f: kill -9 and relaunch resumes Claude, not the Codex run"
: >"$FAKE_ARGV"
kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=
start_app
for mode in fg bg orphan; do
    p=${PANE[$mode]}
    eventually 10 grep -q "^$p claude .*--resume ${SID[$mode]}" "$FAKE_ARGV" || true
    check "R128-f ($mode) resumes claude --resume <its session>" grep -q "^$p claude .*--resume ${SID[$mode]}" "$FAKE_ARGV"
    check "R128-f ($mode) runs no codex" bash -c "! grep -q '^$p codex' '$FAKE_ARGV'"
done

echo
echo "m49: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
