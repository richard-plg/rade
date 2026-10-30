#!/usr/bin/env bash
# M39 end-to-end tests (ADR-0006, ADR-0035): Usage (PRD R109, R110).
# A temp HOME with scene E's 7 accounts in two Workspaces: `acme` (acme-work
# claude default ~/.claude-work with Detailed limits on, acme-team claude
# ~/.claude-team, acme codex default ~/.codex, acme-biz codex ~/.codex-biz)
# and `personal` (personal claude ~/.claude, personal codex ~/.codex-personal,
# oss codex ~/.codex-oss). The fake Agent (tests/e2e/fake-agent.sh) is the
# `claude` and `codex` Presets' command: it writes Codex rollouts / Claude
# transcripts and calls `rade status --statusline`; Detailed limits ask
# tests/e2e/fake-usage-server.py (RADE_CLAUDE_USAGE_URL, test mode only).
# Part 1 (Group 2): R109-b, R109-i, R110-h, R109-h, R110-j, R109-j.
# Part 2 (Group 3): scene E / F data written into the
# store (tests/e2e/usage-scene.py): R110-f, R110-i, R110-g, R110-d, R110-e with
# spec E / F geometry (tests/e2e/spec_e.py, spec_f.py) in dark and light, goldens
# E and F (tests/e2e/golden/m39/, APPROVE=1 writes missing ones).
# Usage: tests/e2e/m39.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir,
#        PART=2 to run part 2 only)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m39
export WESTON_WIDTH=1440
source "$HERE/lib.sh"

export XDG_STATE_HOME=$T/state
mkdir -p "$XDG_STATE_HOME"
USAGE=$XDG_STATE_HOME/rade-dev/usage
FAKE=$HERE/fake-agent.sh
export FAKE_AGENT=claude
mkdir -p "$XDG_CONFIG_HOME/rade"
printf 'idle_seconds = 1\n\n[agents.claude]\ncommand = "%s"\n\n[agents.codex]\ncommand = "%s"\n' "$FAKE" "$FAKE" \
    >"$XDG_CONFIG_HOME/rade/config.toml"
FIX=$ROOT/crates/rade-core/tests/fixtures/usage
FAKE_TOKEN=sk-ant-oat01-M39-FAKE-E2E-TOKEN-7f3a9c

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
code() { set +e; "$@" >"$T/out" 2>"$T/err"; local c=$?; set -e; echo $c; }
sha8() { printf %s "$(realpath "$1")" | sha256sum | cut -c1-8; }
activity() { rpc debug.ui '{"op":"activity"}' | jq -r '.activity[].line'; }
# Lines of the Activity tab equal to $1.
nact() { activity | grep -cxF -- "$1" || true; }
# A limit of an account (usage.get): `<used_percent>` of `<limit_id>/<name>`.
used() { rade account usage "$1" --agent "$2" --workspace "$3" --json | jq -r --arg k "$4" '.limits[] | select("\(.limit_id)/\(.name)" == $k) | .used_percent + 0'; }
export -f rpc used

# ---- The scene ----------------------------------------------------------------
A=$HOME/workspaces/acme P=$HOME/workspaces/personal
for r in "$A/api" "$P/notes"; do
    mkdir -p "$r" && G -C "$r" init -q -b main && echo base >"$r/README.md"
    G -C "$r" add -A && G -C "$r" commit -qm init
done
for d in .claude-work .claude-team .codex .codex-biz .claude .codex-personal .codex-oss; do mkdir -p "$HOME/$d"; done
# The fake login of acme-work (read by Detailed limits at call time).
printf '{"claudeAiOauth":{"accessToken":"%s","refreshToken":"x","expiresAt":9999999999999}}\n' "$FAKE_TOKEN" \
    >"$HOME/.claude-work/.credentials.json"
mkdir -p "$A/.rade" "$P/.rade"
cat >"$A/.rade/workspace.toml" <<'EOF'
name = "acme"

[[account]]
agent = "claude"
name = "acme-work"
config_dir = "~/.claude-work"
default = true
detailed_limits = true

[[account]]
agent = "claude"
name = "acme-team"
config_dir = "~/.claude-team"

[[account]]
agent = "codex"
name = "acme"
config_dir = "~/.codex"
default = true

[[account]]
agent = "codex"
name = "acme-biz"
config_dir = "~/.codex-biz"
EOF
cat >"$P/.rade/workspace.toml" <<'EOF'
name = "personal"

[[account]]
agent = "claude"
name = "personal"
config_dir = "~/.claude"

[[account]]
agent = "codex"
name = "personal"
config_dir = "~/.codex-personal"

[[account]]
agent = "codex"
name = "oss"
config_dir = "~/.codex-oss"
EOF

# The statusline JSON of R109-a with resets after now (5-hour in 1h 12m).
NOW=$(date +%s)
jq -c --argjson f $((NOW + 4320)) --argjson w $((NOW + 3 * 86400)) --argjson s $((NOW + 9 * 86400)) \
    '.rate_limits.five_hour.resets_at = $f | .rate_limits.seven_day.resets_at = $w | .rate_limits.spend_limit.resets_at = $s' \
    "$FIX/statusline-2.1.309.json" >"$T/statusline.json"

# The fake usage endpoint: good once, then an unexpected shape.
SRV_LOG=$T/usage-server.log
: >"$SRV_LOG"
python3 "$HERE/fake-usage-server.py" "$T/usage-port" "$SRV_LOG" 1 &
SRV_PID=$!
trap 'kill $SRV_PID 2>/dev/null || true; cleanup' EXIT
eventually 5 test -s "$T/usage-port"
export RADE_CLAUDE_USAGE_URL=http://127.0.0.1:$(cat "$T/usage-port")/api/oauth/usage
nreq() { wc -l <"$SRV_LOG"; }

start_app
rade workspace add "$A" >/dev/null
rade workspace add "$P" >/dev/null
rade source add "$A/api" --workspace acme >/dev/null
rade source add "$P/notes" --workspace personal >/dev/null
rade workspace switch acme >/dev/null

if [[ ${PART:-} != 2 ]]; then # ---- Part 1 (Group 2) ----------------------------------
echo "== R109-b rade status --statusline --then (app running)"
TEAM_STORE=$USAGE/claude-$(sha8 "$HOME/.claude-team").jsonl
rc=$(code env CLAUDE_CONFIG_DIR="$HOME/.claude-team" rade status --statusline --then "printf prev" <"$T/statusline.json")
check "R109-b stdout is exactly 'prev', exit 0, nothing on stderr" \
    bash -c "[[ $rc == 0 && \$(cat '$T/out') == prev && ! -s '$T/err' ]]"
check "R109-b 1 sample (one batch, one t) appended to claude-<sha8>.jsonl: 5-hour 62, weekly 41, spend 12 of 50" \
    bash -c "[[ \$(jq -s '[.[].t] | unique | length' '$TEAM_STORE') == 1 ]] && [[ \$(jq -sc '[.[] | \"\(.limit_id):\(.used_percent + 0):\(.account)\"]' '$TEAM_STORE') == '[\"five_hour:62:acme-team\",\"seven_day:41:acme-team\",\"spend_limit:24:acme-team\"]' ]] && [[ \$(jq -s '.[2].used_usd + 0, .[2].limit_usd + 0' '$TEAM_STORE' | tr '\n' ' ') == '12 50 ' ]]"
rc=$(code env CLAUDE_CONFIG_DIR="$HOME/.claude-team" rade status --statusline <"$T/statusline.json")
check "R109-b without --then: prints nothing, exit 0; an unchanged sample adds no line" \
    bash -c "[[ $rc == 0 && ! -s '$T/out' && \$(wc -l <'$TEAM_STORE') == 3 ]]"

echo "== R109-i / R110-h a fake Codex Agent on account acme writes its rollout"
RESET=$((NOW + 9000))
cx=$(rade spawn --agent codex --account acme --source api --tab new --name cx)
eventually 10 screen_has "$cx" 'fake-agent session' || true
step() { # <prompt> <expected used %> — the turn ends, its account is refreshed
    rade send "$cx" "$1" --enter >/dev/null
    eventually 10 is "used acme codex acme codex_bengalfox/5-hour" "$2"
}
step "codexlimit 79 $RESET" 79 || true
t0=$(date +%s.%N)
rade send "$cx" "codexlimit 81 $RESET" --enter >/dev/null
check "R109-i rade account usage acme --json lists codex_bengalfox GPT-5.3-Codex-Spark 5-hour at 81 within 5 s of the turn end" \
    eventually 6 is "rade account usage acme --json | jq -r '.limits[] | select(.limit_id == \"codex_bengalfox\" and .name == \"5-hour\") | \"\(.model) \(.used_percent + 0)\"'" "GPT-5.3-Codex-Spark 81"
echo "    seen after $(echo "$(date +%s.%N) - $t0" | bc | cut -c1-4) s"
rade account usage acme --json >"$OUT/usage-acme.json"
step "codexlimit 81 $RESET blocked" 81 || true
eventually 10 is "rade account usage acme --json | jq -r '.limits[] | select(.limit_id == \"codex_bengalfox\" and .name == \"5-hour\") | .blocked'" true || true
step "codexlimit 3 $((RESET + 18000))" 3 || true
sleep 0.5
activity >"$OUT/activity-r110h.txt"
check "R110-h Activity has 'account acme: Spark 5-hour over 80%' once" is "nact 'account acme: Spark 5-hour over 80%'" 1
check "R110-h Activity has 'account acme: Spark 5-hour blocked' once" is "nact 'account acme: Spark 5-hour blocked'" 1
check "R110-h Activity has 'account acme: Spark 5-hour reset' once" is "nact 'account acme: Spark 5-hour reset'" 1

echo "== R109-h Detailed limits for acme-work (fake endpoint, test mode)"
WORK_STORE=$USAGE/claude-$(sha8 "$HOME/.claude-work").jsonl
for _ in 1 2 3; do rpc usage.refresh '{"account":"acme-work","workspace":"acme"}' >>"$T/refresh.json"; done
check "R109-h 3 refreshes within 5 minutes → the server got 1 request (with a Bearer header)" \
    bash -c "[[ \$(wc -l <'$SRV_LOG') == 1 ]] && grep -q 'auth=yes' '$SRV_LOG'"
check "R109-h Opus 58 % and Sonnet 22 % are stored and in usage.get" \
    bash -c "[[ \$(used acme-work claude acme seven_day_opus/Opus) == 58 && \$(used acme-work claude acme seven_day_sonnet/Sonnet) == 22 ]] && grep -q '\"source\":\"detailed\"' '$WORK_STORE'"
leaks() { grep -rlF -- "$FAKE_TOKEN" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$OUT/app.log" 2>/dev/null | wc -l; }
check "R109-h the fake token appears 0 times under the XDG state, data and config dirs (and the app log)" is leaks 0

echo "== R109-b the wrapper with no app"
stop_app
t0=$(date +%s.%N)
rc=$(code env CLAUDE_CONFIG_DIR="$HOME/.claude-team" rade status --statusline --then "printf prev" <"$T/statusline.json")
dt=$(echo "$(date +%s.%N) - $t0" | bc)
echo "    took $dt s"
check "R109-b no app: stdout exactly 'prev', exit 0, within 2 s, no RADE error printed" \
    bash -c "[[ $rc == 0 && \$(cat '$T/out') == prev && ! -s '$T/err' ]] && (( \$(echo '$dt < 2' | bc) ))"

echo "== R109-h an unexpected response turns Detailed limits off"
start_app
cp "$A/.rade/workspace.toml" "$T/acme-before.toml"
rpc usage.refresh '{"account":"acme-work","workspace":"acme"}' >>"$T/refresh.json"
check "R109-h the server got its 2nd request (a new app) and answered an unexpected shape" is nreq 2
check "R109-h workspace.toml: detailed_limits = false, every other byte kept" \
    bash -c "[[ \$(diff '$T/acme-before.toml' '$A/.rade/workspace.toml' | grep '^[<>]' | tr '\n' '|') == '< detailed_limits = true|> detailed_limits = false|' ]]"
eventually 5 is "activity | grep -c 'account acme-work: Detailed limits off'" 1 || true
activity >"$OUT/activity-r109h.txt"
check "R109-h one Activity line says the option turned off" is "activity | grep -c '^account acme-work: Detailed limits off'" 1
for _ in 1 2; do rpc usage.refresh '{"account":"acme-work","workspace":"acme"}' >/dev/null; done
check "R109-h off: later refreshes send nothing" is nreq 2
check "R109-h still no fake token under state / data / config" is leaks 0

echo "== R110-j rade account usage"
cl=$(rade spawn --agent claude --account acme-work --source api --tab new --name cl)
eventually 10 screen_has "$cl" 'fake-agent session' || true
rade send "$cl" "statusline $T/statusline.json" --enter >/dev/null
eventually 10 is "used acme-work claude acme five_hour/5-hour" 62 || true
rade send "$cl" "transcript 100 200" --enter >/dev/null
eventually 10 is "rade account usage acme-work --json | jq '.tokens.by_model[0].tokens'" 300 || true
rade account usage >"$T/lines" 2>&1 || true
cp "$T/lines" "$OUT/account-usage.txt"
cat "$T/lines" | sed 's/^/    /'
check "R110-j one line per account of the caller's Workspace (acme: 4), sorted by Tightest limit (62 = 62 by reset, then listed order)" \
    test "$(awk '{print $1 ":" $2}' "$T/lines" | tr '\n' ' ')" = "acme-work:claude acme-team:claude acme:codex acme-biz:codex "
check "R110-j acme-work's line: '62% 5-hour  resets in 1h 1xm'" \
    bash -c "grep -qE '^acme-work  claude  62% 5-hour  resets in 1h 1[0-2]m$' '$T/lines'"
check "R110-j acme's line names Spark 5-hour and its reset; acme-biz says no usage data" \
    bash -c "grep -qE '^acme  codex  33% Spark weekly  resets ' '$T/lines' && grep -qxF 'acme-biz  codex  no usage data' '$T/lines'"
rade account usage acme-work --json >"$OUT/usage-acme-work.json"
check "R110-j --json for acme-work: limits grouped as spec E.30" \
    test "$(jq -c '[.groups[] | {g: .group, l: [.limits[] | "\(.name) \(.used_percent + 0)"]}]' "$OUT/usage-acme-work.json")" \
    = '[{"g":"Session","l":["5-hour 62"]},{"g":"Weekly","l":["All models 41","Opus 58","Sonnet 22","OAuth apps 3"]},{"g":"Extra usage","l":["Spend limit 24"]}]'
check "R110-j the spend limit is \$12 of \$50 /month; tokens 300 with cache reads excluded" \
    test "$(jq -r '"\(.groups[2].limits[0].used_usd + 0) \(.groups[2].limits[0].limit_usd + 0) \(.groups[2].limits[0].period) \(.tokens.by_project[0].project) \(.tokens.by_project[0].tokens)"' "$OUT/usage-acme-work.json")" = "12 50 month Scratch 300"
check "R110-j usage.list (all) lists 7 accounts; oss last with no data" \
    test "$(rpc usage.list '{"all":true}' | jq -r '"\(.accounts | length) \(.accounts[-1].state)"')" = "7 no-data"

echo "== R109-j rade setup --agent claude twice in a temp HOME"
SH=$T/setup-home
mkdir -p "$SH/.claude-work" "$SH/.claude-team"
printf '{\n  "statusLine": {"type": "command", "command": "bash -c \\"echo '"'"'hi'"'"'\\"", "padding": 0},\n  "theme": "dark"\n}\n' \
    >"$SH/.claude-work/settings.json"
printf '{}\n' >"$SH/.claude-team/settings.json"
OLD=$(jq -r .statusLine.command "$SH/.claude-work/settings.json")
setup() { env HOME="$SH" XDG_CONFIG_HOME="$SH/.config" XDG_DATA_HOME="$SH/.local/share" XDG_STATE_HOME="$SH/.local/state" \
    rade setup --agent claude; }
setup >"$OUT/setup-1.txt" 2>&1
W=$(jq -r .statusLine.command "$SH/.claude-work/settings.json")
echo "    work: $W"
check "R109-j ~/.claude-work's command is rade status --statusline --then '<old, sh-quoted>'" \
    test "$W" = "rade status --statusline --then 'bash -c \"echo '\\''hi'\\''\"'"
check "R109-j the --then argument as sh parses it equals the old command byte for byte" \
    test "$(sh -c 'eval "set -- $1"; printf %s "$5"' x "$W")" = "$OLD"
check "R109-j padding is still 0; other keys kept" \
    test "$(jq -c '[.statusLine.padding, .statusLine.type, .theme]' "$SH/.claude-work/settings.json")" = '[0,"command","dark"]'
check "R109-j ~/.claude-team's command is 'rade status --statusline' (no --then)" \
    test "$(jq -r .statusLine.command "$SH/.claude-team/settings.json")" = "rade status --statusline"
sums() { md5sum "$SH/.claude-work/settings.json" "$SH/.claude-team/settings.json"; }
sums >"$T/sums-1"; nb1=$(find "$SH" -name '*.rade-bak-*' | wc -l)
sleep 1 # a new backup would get a new name
setup >"$OUT/setup-2.txt" 2>&1
check "R109-j the second run leaves both files byte-identical and writes no new backup ($nb1 backups)" \
    bash -c "sums() { md5sum '$SH/.claude-work/settings.json' '$SH/.claude-team/settings.json'; }; sums | cmp -s - '$T/sums-1' && [[ \$(find '$SH' -name '*.rade-bak-*' | wc -l) == $nb1 ]]"

# The limit events in the Inspector's Activity tab.
[[ $(rpc debug.ui '{"op":"chrome"}' | jq -r .chrome.inspector.shown) == true ]] ||
    rpc debug.ui '{"op":"key","keys":"ctrl+shift+e"}' >/dev/null || true
rpc debug.ui '{"op":"right_page","arg":"activity"}' >/dev/null || true
sleep 0.5
shot m39-part1-activity

fi # part 1

# ---- Part 2 (Group 3): the usage UI on scene E / F ------------------------------
echo "== Part 2: scene E / F written into the store (tests/e2e/usage-scene.py)"
SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
TOKENS=$ROOT/crates/rade-app/src/tokens.toml
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+, $2}}"; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
usage() { ui usage "${1:-}" | jq -c .usage; }
nm() { rpc debug.ui "{\"op\":\"new_menu\"${1:+, $1}}"; }
key() { rpc debug.ui "{\"op\":\"key\",\"keys\":\"$1\"}"; }
chrome() { ui chrome | jq -c .chrome; }
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
notoasts() { rpc debug.ui '{"op":"toasts_clear"}' >/dev/null; sleep 0.6; }
primaries() { geo .suggested-action '"all":true' | jq -r '[.[] | .label] | join(",")'; }
loaded() { [[ $(usage | jq -r '"\(.shown) \(.loading)"') == "true false" ]]; }
export -f loaded
penv() { python3 - "$1" "$2" <<'PY'
import os, sys
pane, var = sys.argv[1], sys.argv[2]
for pid in sorted(p for p in os.listdir("/proc") if p.isdigit()):
    try:
        raw = open(f"/proc/{pid}/environ", "rb").read().split(b"\0")
    except OSError:
        continue
    env = dict(e.decode(errors="replace").split("=", 1) for e in raw if b"=" in e)
    # This run's Panes only (another suite on the machine reuses Pane ids).
    if env.get("RADE_PANE_ID") == pane and env.get("XDG_RUNTIME_DIR") == os.environ.get("XDG_RUNTIME_DIR"):
        print(env.get(var, "<unset>"))
        break
PY
}
export -f rpc geo ui usage nm chrome penv
# Golden masks: Terminal contents, LEDs, ages and reset times.
masks() { # file, extra selectors…
    local f=$1; shift
    local args=()
    for w in terminal glyph-running led-running nav-row-meta activity-time "$@"; do args+=("$(geo "$w" '"all":true')"); done
    python3 - "${args[@]}" <<'PY' >"$f"
import json, sys
out = []
for arg in sys.argv[1:]:
    for g in json.loads(arg):
        if not g.get("visible", True):
            continue
        out.append({"x": g["x"] - 2, "y": g["y"] - 2, "w": g["width"] + 4, "h": g["height"] + 4,
                    "why": ",".join(c for c in g["classes"] if not c.startswith("st-"))[:40]})
print(json.dumps(out, indent=1))
PY
}
# The scene, re-centred on now (forecast ~40m, reset in 1h 12m, updated 40s).
scene() { python3 "$HERE/usage-scene.py" "$USAGE" "$HOME" >"$T/scene.json"; }

# No Detailed limits endpoint from here on (a failed request keeps the option on).
kill "$SRV_PID" 2>/dev/null || true
stop_app
rm -rf "$USAGE" "$HOME"/.codex*/sessions "$HOME"/.claude*/projects
sed -i 's/^detailed_limits = false$/detailed_limits = true/' "$A/.rade/workspace.toml"
scene
start_app
rade workspace switch acme >/dev/null
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
# Part 1's Panes go: scenes E and F start from the same Navigator either way.
for p in $(rade list --all --json | jq -r '.panes[].pane'); do rade close --force "$p" >/dev/null 2>&1 || true; done
# Scenes E and F have no Inspector (part 1 opened it for its Activity shot).
[[ $(chrome | jq -r .inspector.shown) == true ]] && key ctrl+shift+e >/dev/null
rpc usage.refresh '{"workspace":"acme"}' >/dev/null

echo "== R110-g the forecast Toast (spec F.17, F.18)"
ftoast() { chrome | jq -r '.forecast_toasts[0] | "\(.title)|\(.sub)|\(.button)"'; }
eventually 10 bash -c "[[ \$(rpc debug.ui '{\"op\":\"chrome\"}' | jq '.chrome.forecast_toasts | length') -ge 1 ]]" || true
ftoast >"$T/toast"
echo "    $(cat "$T/toast")"
check "R110-g Toast 'acme-work will hit its 5-hour limit in ~40m at this pace'" \
    test "$(cut -d'|' -f1 "$T/toast")" = "acme-work will hit its 5-hour limit in ~40m at this pace"
check "R110-g sub-line '62% now · resets in 1h 1xm · acme-team is at 8%'" \
    bash -c "cut -d'|' -f2 '$T/toast' | grep -qxE '62% now · resets in 1h 1[0-2]m · acme-team is at 8%'"
check "R110-g action 'Next agents use acme-team'" test "$(cut -d'|' -f3 "$T/toast")" = "Next agents use acme-team"
check "R110-g the Toast is recorded in Activity" \
    eventually 5 is "rpc debug.ui '{\"op\":\"activity\"}' | jq -r '.activity[].line' | grep -cE '^account acme-work: will hit its 5-hour limit in ~[0-9]+m at this pace\$'" 1
rpc usage.refresh '{"workspace":"acme"}' >/dev/null
sleep 0.5
check "R110-g at most one forecast Toast per limit per window (a second refresh raises none)" \
    is "chrome | jq '.forecast_toasts | length'" 1
notoasts

echo "== R110-f the Statusline usage cell (A.74)"
planner=$(rade spawn --agent claude --account acme-work --source api --tab new --name planner)
tests=$(rade spawn --agent codex --account acme --source api --name tests)
eventually 10 screen_has "$planner" 'fake-agent session' || true
rade focus "$planner" >/dev/null
cell() { chrome | jq -r '[.statusline.cells[] | select(startswith("claude ") or startswith("codex "))][0] // "none"'; }
check "R110-f planner (acme-work) focused → cell 'claude 5h 62% · wk 41%'" eventually 5 is cell "claude 5h 62% · wk 41%"
order_ok() { rpc debug.ui '{"op":"chrome"}' | jq -e '.chrome.statusline.kinds as $k | ($k | index("usage")) as $u
    | $u != null and ([$k[] | select(. == "ws" or . == "project" or . == "worktree" or . == "counts" or . == "ports")] | length) == $u
    and ($k[$u + 1:] | all(. == "identity" or . == "hint"))' >/dev/null; }
check "R110-f the cell sits after Workspace / Project / Worktree / counts / ports, before identity and hints (A.77)" order_ok
check "R110-f meter 36 × 5 px, fill 22 ± 1 px" \
    bash -c "m=\$(geo 'status-cell-usage usage-meter'); f=\$(geo 'status-cell-usage usage-meter-fill'); [[ \$(jq .width <<<\"\$m\") == 36 && \$(jq .height <<<\"\$m\") == 5 ]] && (( \$(jq '.width - 22 | fabs <= 1' <<<\"\$f\" | grep -c true) == 1 ))"
check "R110-f tooltip 'Claude · acme-work'" test "$(chrome | jq -r .statusline.usage.tooltip)" = "Claude · acme-work"
rade focus "$tests" >/dev/null
check "R110-f tests (acme codex) focused → 'codex Spark 5h 81%'" eventually 5 is cell "codex Spark 5h 81%"
sh1=$(rade spawn --source api --name shell)
rade focus "$sh1" >/dev/null
check "R110-f a Pane without an account → the cell is hidden" eventually 5 is "chrome | jq -r '.statusline.kinds | index(\"usage\") // \"none\"'" none
rade close --force "$sh1" >/dev/null
rade focus "$planner" >/dev/null
sleep 0.5

echo "== R110-i the + menu with usage figures (spec F.8–F.10)"
prim0=$(geo suggested-action '"all":true' | jq length)
nm '"arg":"open"' >/dev/null
mrow() { nm | jq -r --arg n "$1" --arg a "$2" '.new_menu.rows[] | select(.name == $n and .agent == $a) | "\(.detail)|\(.default)|\(.hot)"'; }
check "R110-i acme-work (default ✓): '5h 62% · wk 41%'" is "mrow acme-work claude" "5h 62% · wk 41%|true|false"
check "R110-i acme-team: '5h 8% · wk 12%', no check" is "mrow acme-team claude" "5h 8% · wk 12%|false|false"
check "R110-i acme (codex, default ✓): 'Spark 5h 81%', hot (wait + warning-diamond)" is "mrow acme codex" "Spark 5h 81%|true|true"
check "R110-i acme-biz: its Tightest limit 'wk 27%'" is "mrow acme-biz codex" "wk 27%|false|false"
check "R110-i oss (Workspace personal): locked 'other workspace', no figure" is "mrow oss codex" "other workspace|false|false"
nm '"arg":"close"' >/dev/null

echo "== Goldens F (+ menu and forecast Toast over the Workspace) and spec F rows F.8–F.10, F.15–F.19"
spec_f() { python3 "$HERE/spec_f.py" "$SOCK" "$TOKENS" "$1" "New in api" "$prim0" m39 >"$OUT/spec-f-$1.txt" 2>&1; }
for th in dark light; do
    [[ $th == light ]] && light
    scene
    rpc debug.ui '{"op":"forecast_check","arg":"reset"}' >/dev/null
    eventually 10 bash -c "[[ \$(geo toast | jq -r .visible) == true ]]" || true
    nm '"arg":"open"' >/dev/null
    sleep 1.2 # the Toast's reveal and one lift tick
    check "R110-i $th: spec F rows F.8–F.10, F.15–F.19 within tolerance" spec_f "$th"
    { grep -E '^FAIL' "$OUT/spec-f-$th.txt" || true; } | sed 's/^/    /'
    masks "$OUT/masks-f-$th.json" usage-reset-time toast-sub status-cell-usage
    shot "f-$th"
    nm '"arg":"close"' >/dev/null
done
dark

echo "== R110-g the Toast's action changes only the default for Agents opened later"
before_toml=$(md5sum <"$A/.rade/workspace.toml")
scene
rpc debug.ui '{"op":"forecast_check","arg":"reset"}' >/dev/null
eventually 10 bash -c "[[ \$(rpc debug.ui '{\"op\":\"chrome\"}' | jq '.chrome.forecast_toasts | length') -ge 2 ]]" || true
rpc debug.ui '{"op":"forecast_action"}' >/dev/null
check "R110-g after the action the + menu's Claude default is acme-team" \
    eventually 5 is "nm | jq -r '[.new_menu.rows[] | select(.agent == \"claude\" and .default) | .name] | join(\",\")'" acme-team
before=$(rade list --all --json | jq -r '[.panes[].pane] | sort | join(" ")')
nm '"arg":"open"' >/dev/null
nm '"arg":"activate","name":"acme-team","agent":"claude"' >/dev/null
sleep 0.5
new=$(rade list --all --json | jq -r --arg b " $before " '[.panes[].pane | . as $id | select(($b | contains(" " + $id + " ")) | not)][0]')
check "R110-g the next + → Claude opens with acme-team (RADE_ACCOUNT=acme-team)" \
    eventually 10 is "penv $new RADE_ACCOUNT" acme-team
plain=$(rade spawn --agent claude --source api --name later)
check "R110-g rade spawn --agent claude (no --account) uses acme-team too" eventually 10 is "penv $plain RADE_ACCOUNT" acme-team
check "R110-g the running planner keeps RADE_ACCOUNT=acme-work" is "penv $planner RADE_ACCOUNT" acme-work
check "R110-g workspace.toml unchanged; account list default still acme-work" \
    bash -c "[[ \$(md5sum <'$A/.rade/workspace.toml') == '$before_toml' && \$(rade account list --workspace acme --json | jq -r '[.accounts[] | select(.agent == \"claude\" and .default) | .name] | join(\",\")') == acme-work ]]"
check "R110-g the choice is not saved in state.json" bash -c "! grep -q next_defaults '$STATE'"
rade close --force "$new" >/dev/null; rade close --force "$plain" >/dev/null
rade focus "$planner" >/dev/null
notoasts

echo "== R110-d the Usage page from its three entry points (spec E)"
scene
rpc usage.refresh '{"workspace":"acme"}' >/dev/null
notoasts
rade focus "$planner" >/dev/null
rpc debug.ui '{"op":"click","widget":"status-usage"}' >/dev/null
check "R110-d the Statusline usage cell opens it (breadcrumb 'Agent usage')" \
    eventually 5 bash -c "[[ \$(usage | jq -r .shown) == true && \$(chrome | jq -r .breadcrumb) == 'Agent usage' ]]"
ui usage close >/dev/null
check "R110-d closed again" eventually 3 is "usage | jq -r .shown" false
rpc debug.ui '{"op":"click","widget":"tb-button-usage"}' >/dev/null
check "R110-d the titlebar gauge button opens it" eventually 5 is "usage | jq -r .shown" true
ui usage close >/dev/null
eventually 3 is "usage | jq -r .shown" false || true
key ctrl+shift+p >/dev/null
eventually 3 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .open) == true ]]" || true
rade --json debug palette --ui type '> usage' >/dev/null
eventually 5 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .busy) == false ]]" || true
rade --json debug palette --ui key enter >/dev/null || true
check "R110-d palette '> usage' + Enter opens it" eventually 5 is "usage | jq -r .shown" true
ui usage "filter all" >/dev/null
ui usage "select claude acme-work acme" >/dev/null
eventually 10 loaded || true
sleep 0.3
eventually 5 loaded || true
usage >"$OUT/usage-all.json"
check "R110-d rows in E.18 order: personal 88 · acme 81 · acme-work 62 · acme-biz 27 · acme-team 12 · personal 4 · oss no data" \
    test "$(jq -r '[.rows[] | "\(.name)/\(.agent) \(.tightest)"] | join(" | ")' "$OUT/usage-all.json")" \
    = "personal/claude 88% weekly | acme/codex 81% Spark 5-hour | acme-work/claude 62% 5-hour | acme-biz/codex 27% weekly | acme-team/claude 12% weekly | personal/codex 4% weekly | oss/codex no data yet"
resets_ok() { jq -e '[.resets[] | "\(.when)|\(.account)|\(.what)|\(.note)"] | join("#")
    | test("^in 1h 1[0-2]m[|]acme-work[|]5-hour · now 62%[|]forecast: hits 100% in ~(35|40|45)m#in 2h (29|30)m[|]acme[|]Spark 5-hour · now 81%[|]GPT-5[.]3-Codex-Spark only#[A-Z][a-z]{2} 21:00[|]personal[|]weekly · now 88%[|]forecast: 9[6-8]% at reset#[A-Z][a-z]{2} 09:00[|]acme-work[|]weekly ×4 · max 58% Opus[|]$")' "$OUT/usage-all.json" >/dev/null; }
check "R110-d Next resets (E.24): acme-work 5-hour, acme Spark, personal weekly, acme-work weekly ×4, in time order" resets_ok
check "R110-d detail of acme-work (E.26, E.30, E.31)" \
    test "$(jq -c '[.detail.sub, [.detail.groups[] | {g: .group, r: [.rows[] | "\(.name) \(.value)"]}]]' "$OUT/usage-all.json")" \
    = '["Claude Code · ~/.claude-work",[{"g":"SESSION","r":["5-hour 62%"]},{"g":"WEEKLY","r":["All models 41%","Opus 58%","Sonnet 22%","OAuth apps 3%"]},{"g":"EXTRA USAGE","r":["Spend limit $12"]}]]'
# The weekly reset is 3 days ahead at 09:00 (usage-scene.py `mon`): its weekday
# follows the run date (`Mon` on the mockup's Friday).
WK=$(LC_ALL=C date -d '+3 days' +%a)
check "R110-d reset columns: 5-hour '1h 1xm', weekly '$WK 09:00', spend 'of \$50 /mo'; no other \$ on the page" \
    bash -c "jq -r '[.detail.groups[].rows[] | .reset] | join(\"|\")' '$OUT/usage-all.json' | grep -qE '^1h 1[0-2]m\|($WK 09:00\|){4}of \\\$50 /mo\$' && [[ \$(jq -r '[.rows[] | .tightest, .tokens], [.resets[] | .what, .note], [.detail.groups[].rows[] | select(.name != \"Spend limit\") | .value, .reset] | .[]' '$OUT/usage-all.json' | grep -c '\\\$') == 0 ]]"
check "R110-d chart: 7 bars, max 3.1M, today 1.5M; footer Shop 9.1M · Billing 2.3M · Scratch 1.0M · opus 78% · sonnet 22%" \
    test "$(jq -c '[(.detail.chart.values | length), .detail.chart.max, .detail.chart.today, .detail.footer.projects, .detail.footer.models]' "$OUT/usage-all.json")" \
    = '[7,"3.1M","1.5M",["Shop 9.1M","Billing 2.3M","Scratch 1.0M"],"opus 78% · sonnet 22%"]'
check "R110-d header: sub-line, period 7 days, filter All workspaces, Add account; columns (E.3, E.6, E.10)" \
    test "$(jq -c '[.header.sub, .period, .header.buttons, .columns]' "$OUT/usage-all.json")" \
    = '["7 accounts · limits from each CLI · tokens from local session logs","7d",["All workspaces","Add account"],["ACCOUNT","PLAN","TIGHTEST LIMIT","TOKENS 7D","UPDATED"]]'
check "R110-d hot rows (≥ 80 %): personal and acme only (E.15); oss row: no data yet, 0, — (E.20)" \
    test "$(jq -c '[[.rows[] | select(.hot) | .name], (.rows[-1] | [.tightest, .tokens, .updated])]' "$OUT/usage-all.json")" = '[["personal","acme"],["no data yet","0","—"]]'
check "R110-d tokens / updated of scene E (E.17; acme-work updated 40 s ago at writing)" \
    bash -c "jq -r '[.rows[] | \"\\(.name) \\(.tokens) \\(.updated)\"] | join(\"|\")' '$OUT/usage-all.json' | grep -qxE 'personal 6.8M 1d\|acme 900k 5m\|acme-work 12.4M [1-9][0-9]?s\|acme-biz 2.6M 1h\|acme-team 1.1M 2h\|personal 200k 3d\|oss 0 —'"

ui usage "select claude acme-work acme" >/dev/null
spec_e() { python3 "$HERE/spec_e.py" "$SOCK" "$TOKENS" "$1" >"$OUT/spec-e-$1.txt" 2>&1; }
for th in dark light; do
    [[ $th == light ]] && light
    scene
    rpc usage.refresh '{"workspace":"acme"}' >/dev/null
    notoasts
    ui usage "period 7d" >/dev/null
    eventually 10 loaded || true
    sleep 0.5
    check "R110-d $th: every spec E Measurements row within tolerance" spec_e "$th"
    { grep -E '^FAIL' "$OUT/spec-e-$th.txt" || true; } | sed 's/^/    /'
    masks "$OUT/masks-e-$th.json" usage-updated usage-reset-time usage-reset-forecast usage-limit-reset toast
    shot "e-$th"
done
dark

echo "== R110-e filter and period"
ui usage "filter acme" >/dev/null
check "R110-e filter acme → 4 accounts" eventually 5 bash -c "loaded && [[ \$(usage | jq -r '\"\(.filter) \(.rows | length)\"') == 'acme 4' ]]"
ui usage "filter all" >/dev/null
check "R110-e All workspaces → 7 accounts" eventually 5 bash -c "loaded && [[ \$(usage | jq -r '\"\(.filter) \(.rows | length)\"') == 'All workspaces 7' ]]"
toks() { usage | jq -r '[.rows[] | select(.name == "acme-work") | .tokens][0]'; }
ui usage "period today" >/dev/null
eventually 5 bash -c "loaded && [[ \$(usage | jq -r .period) == today ]]" || true
t_today=$(toks); h_today=$(usage | jq -r '.columns[3]')
ui usage "period 30d" >/dev/null
eventually 5 bash -c "loaded && [[ \$(usage | jq -r .period) == 30d ]]" || true
t_month=$(toks); h_month=$(usage | jq -r '.columns[3]')
echo "    acme-work tokens: today $t_today, 30 days $t_month"
check "R110-e the token cells change with the period (Today 1.5M → 30 days 22.8M; head TOKENS TODAY → TOKENS 30D)" \
    test "$t_today $t_month $h_today $h_month" = "1.5M 22.8M TOKENS TODAY TOKENS 30D"
ui usage "period 7d" >/dev/null
ui usage "filter personal" >/dev/null
eventually 5 loaded || true
check "R110-e the empty filter: 'No Agent Accounts in …' never happens with accounts; personal → 3" is "usage | jq '.rows | length'" 3
ui usage "filter all" >/dev/null
eventually 5 loaded || true
check "R110-d ↓ moves the selection (acme-work → acme-biz)" \
    bash -c "ui usage 'select claude acme-work acme' >/dev/null; sleep 0.5; ui usage 'key down' >/dev/null; sleep 0.8; [[ \$(usage | jq -r '[.rows[] | select(.selected) | .name][0]') == acme-biz ]]"

echo "== R110-l the Usage page closes with its ✕ and with Esc (M51)"
check "R110-l the header has a close button 'Close (Esc)'" is "usage | jq -r .header.close" "Close (Esc)"
check "R110-l …a flat icon button at the header's right end" \
    bash -c "geo usage-close | jq -e '(.classes | index(\"flat\")) and .label == null' >/dev/null"
rade focus "$planner" >/dev/null
ui usage open >/dev/null
eventually 3 is "usage | jq -r .shown" true || true
ui usage click_close >/dev/null
check "R110-l ✕ closes it and the Tabs are back (breadcrumb not 'Agent usage')" \
    eventually 3 bash -c "[[ \$(usage | jq -r .shown) == false && \$(chrome | jq -r .breadcrumb) != 'Agent usage' ]]"
ui usage open >/dev/null
eventually 3 is "usage | jq -r .shown" true || true
sleep 0.3
# Esc with the Add account dialog open is the dialog's, never the page's.
rpc debug.ui '{"op":"click","widget":"usage-add"}' >/dev/null
sleep 0.5
check "R110-l Esc over the Add account dialog leaves the page open" \
    bash -c "[[ \$(key escape | jq -r .handled) != rade ]] && sleep 0.3 && [[ \$(usage | jq -r .shown) == true ]]"
ui usage close_dialog >/dev/null
sleep 0.5
key escape >"$OUT/usage-esc.json"
check "R110-l Esc closes it" eventually 3 is "usage | jq -r .shown" false
check "R110-l …and the keyboard is back in the Pane it came from" eventually 3 is "rpc debug.window '{}' | jq -r '.focus.pane'" "$planner"

echo "== Goldens E and F (tests/e2e/golden/m39/)"
GOLD=$HERE/golden/m39
mkdir -p "$GOLD"
for s in e f; do
    for th in dark light; do
        if [[ ! -s $GOLD/$s-$th.png && ${APPROVE:-} == 1 ]]; then
            cp "$OUT/$s-$th.png" "$GOLD/$s-$th.png" && echo "  approved (first run): $s-$th"
        fi
        check "R115-c golden ${s^^} $th (with the Agent glyphs) within threshold" python3 "$HERE/golden-diff.py" "$GOLD/$s-$th.png" "$OUT/$s-$th.png" --masks "$OUT/masks-$s-$th.json" --out "$OUT/$s-$th-diff.png"
    done
done
check "design-diff page (mockup · GTK · golden) generated" \
    bash -c "OUT='$OUT' SCENES='e f' GOLDEN='$GOLD' '$HERE/design-diff.sh' >/dev/null && [[ -s '$ROOT/target/design-diff/index.html' ]]"
[[ -n ${PAUSE:-} ]] && { echo "paused: RADE_SOCKET=$SOCK"; sleep "$PAUSE"; }

echo
echo "m39: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
