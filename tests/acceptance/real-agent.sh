#!/usr/bin/env bash
# Opt-in paid/authenticated acceptance in an isolated RADE, repo and Claude config.
# RADE_REAL_AGENT_CONFIG=/absolute/authenticated/config NO_BUILD=1 tests/acceptance/real-agent.sh
# Only credential/onboarding files are copied; setup never writes the original config.
set -euo pipefail
if [[ ${1:-} == --self-check ]]; then
    bash -n "$0"
    python3 - "$0" <<'CHECK'
import pathlib, subprocess, sys, tempfile
script = pathlib.Path(sys.argv[1]).read_text()
with tempfile.TemporaryDirectory() as directory:
    repo = pathlib.Path(directory)
    for name in ('verify_bug.py', 'verify_ui.py'):
        marker = 'cat >"$T/work/app/' + name + "\" <<'CHECK'\n"
        code = script.split(marker, 1)[1].split('\nCHECK', 1)[0]
        (repo / name).write_text(code)
    def passes(name):
        return subprocess.run([sys.executable, str(repo / name)], cwd=repo, capture_output=True).returncode == 0
    (repo / 'bug.txt').write_text('broken\n')
    assert not passes('verify_bug.py')
    (repo / 'bug.txt').write_text('fixed\n')
    assert passes('verify_bug.py')
    assert not passes('verify_ui.py')
    (repo / 'index.html').write_text('<button aria-expanded="false" onclick="toggle()">Show details</button><p hidden>Details ready</p><script>const label="Hide details"</script>')
    assert passes('verify_ui.py')
    # An early setup failure must scrub copied auth/account files even before diagnostics exists.
    (repo / '.credentials.json').write_text('mock credential')
    (repo / '.claude.json').write_text('mock account')
    early = script.split('\nearly_cleanup() {\n', 1)[1].split('\ntrap early_cleanup EXIT', 1)[0]
    early = 'early_cleanup() {\n' + early
    import os
    result = subprocess.run(['bash', '-c', 'cleanup() { :; }; rade() { :; }; ' + early + '\ntrap early_cleanup EXIT; exit 7'], env={**os.environ, 'CLAUDE_CONFIG_DIR': str(repo)})
    assert result.returncode == 7
    assert not (repo / '.credentials.json').exists() and not (repo / '.claude.json').exists()
    capture = script.split("\ncat >\"$T/capture-bash.py\" <<'CAPTURE'\n", 1)[1].split("\nCAPTURE", 1)[0]
    (repo/'capture.py').write_text(capture)
    event = {'session_id':'mock','cwd':str(repo),'tool_name':'Bash','tool_input':{'command':'touch should-not-exist'}}
    import json
    trace = subprocess.run([sys.executable,str(repo/'capture.py'),str(repo/'trace.jsonl'),str(repo)],input=json.dumps(event),capture_output=True,text=True)
    assert trace.returncode == 0 and trace.stdout == '' and trace.stderr == ''
    assert not (repo/'should-not-exist').exists()
    record = json.loads((repo/'trace.jsonl').read_text())
    assert record == {'session_id':'mock','cwd':str(repo),'command':'touch should-not-exist'}
    assert (repo/'trace.jsonl').stat().st_mode & 0o777 == 0o600
print('PASS: script syntax and real fixture checks reject broken/missing results and accept expected content')
CHECK
    exit 0
fi
: "${RADE_REAL_AGENT_CONFIG:?Point to an authenticated Claude config directory}"
[[ $RADE_REAL_AGENT_CONFIG == /* ]] || { echo 'Use an absolute config directory'; exit 2; }
SCENARIO=${RADE_REAL_AGENT_SCENARIO:-all}
case "$SCENARIO" in all|ui|bug|resume) ;; *) echo 'Use RADE_REAL_AGENT_SCENARIO=all|ui|bug|resume'; exit 2 ;; esac
if [[ $SCENARIO == ui ]]; then
    TOTAL=${RADE_REAL_AGENT_TIMEOUT:-1800}
    DESIGN_WAIT=${RADE_REAL_AGENT_DESIGN_TIMEOUT:-900}
else
    TOTAL=${RADE_REAL_AGENT_TIMEOUT:-1200}
    DESIGN_WAIT=${RADE_REAL_AGENT_DESIGN_TIMEOUT:-600}
fi
[[ $TOTAL =~ ^[1-9][0-9]{1,3}$ && $TOTAL -ge 60 && $TOTAL -le 1800 ]] || { echo 'Total timeout must be 60..1800 seconds'; exit 2; }
[[ $DESIGN_WAIT =~ ^[1-9][0-9]{1,2}$ && $DESIGN_WAIT -ge 30 && $DESIGN_WAIT -le 900 ]] || { echo 'Design timeout must be 30..900 seconds'; exit 2; }
if [[ ${RADE_REAL_AGENT_BOUNDED:-} != 1 ]]; then
    exec timeout --signal=TERM --kill-after=30s "${TOTAL}s" env RADE_REAL_AGENT_BOUNDED=1 "$0" "$@"
fi
HERE=$(cd "$(dirname "$0")/../e2e" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/acceptance-real-agent
[[ $SCENARIO == all ]] || OUT+=-$SCENARIO
source "$HERE/lib.sh"
export CLAUDE_CONFIG_DIR=$T/claude
early_cleanup() {
    local status=$?
    trap - EXIT
    rm -f "$CLAUDE_CONFIG_DIR/.credentials.json" "$CLAUDE_CONFIG_DIR/.claude.json"
    RADE_SOCKET="$XDG_RUNTIME_DIR/rade/rade-dev.sock" rade browser --engine chromium close >/dev/null 2>&1 || true
    cleanup
    exit "$status"
}
trap early_cleanup EXIT
trap 'exit 124' TERM
unset CLAUDECODE
mkdir -p "$CLAUDE_CONFIG_DIR" "$XDG_CONFIG_HOME/rade" "$T/work/app"
python3 - "$RADE_REAL_AGENT_CONFIG" "$CLAUDE_CONFIG_DIR" "$T" <<'PY'
import json, os, pathlib, shutil, sys
source, target, root = map(pathlib.Path, sys.argv[1:])
credentials = source / '.credentials.json'
if not credentials.is_file():
    raise SystemExit('No credential file in RADE_REAL_AGENT_CONFIG; authenticate there first.')
shutil.copyfile(credentials, target / '.credentials.json')
os.chmod(target / '.credentials.json', 0o600)
original = json.loads((source / '.claude.json').read_text())
config = {key: original[key] for key in ('oauthAccount', 'userID', 'hasCompletedOnboarding', 'lastOnboardingVersion') if key in original}
config['hasCompletedOnboarding'] = True
config['theme'] = 'dark'
config['hasSeenAutoDefaultNotice'] = True
config['hasSeenAutoModeEntryWarning'] = True
config['hasSeenAutoDefaultNudge'] = True
config['hasResetAutoModeOptInForDefaultOffer'] = True
config['projects'] = {str(p): {'hasTrustDialogAccepted': True, 'hasCompletedProjectOnboarding': True} for p in (root, root/'work', root/'work/app')}
(target / '.claude.json').write_text(json.dumps(config))
(target / 'settings.json').write_text(json.dumps({'permissions': {'defaultMode': 'acceptEdits', 'additionalDirectories': [str(root)], 'allow': ['Read', 'Edit', 'Write', 'Glob', 'Grep', 'Bash(python3 *)', 'Bash(git *)', 'Bash(rade *)', 'Bash(sleep *)', 'Bash(ls *)', 'Bash(cat *)', 'Bash(mkdir *)', 'Bash(cd *)', 'Bash(head *)']}, 'autoUpdatesChannel': 'stable'}))
PY
cat >"$CLAUDE_CONFIG_DIR/CLAUDE.md" <<'MEMORY'
# Isolated RADE acceptance fixture instructions
Use Read, Glob and Grep tools to inspect files and directories. Do not use Bash for cat/ls/head/find reading, and do not combine cd with relative shell paths: the provider's conservative path checking can misinterpret these as reads outside the fixture.
Use absolute paths with tools. For Git use git -C /absolute/fixture/path directly, never cd && git. Bash may run the fixture's Python checks, rade commands and the requested sleep. Do not install packages or access external repositories/services. Keep every project edit under the throwaway fixture workspace.
Follow RADE's current phase instructions, owner feedback and checks; preserve all verify scripts. Do not change permission mode or disable safeguards. If a required tool needs a normal permission decision, stop at that prompt for the owner; do not switch to auto mode.
MEMORY
# User memory is documented under ~/.claude; cover both isolated config locations.
mkdir -p "$HOME/.claude"
cp "$CLAUDE_CONFIG_DIR/CLAUDE.md" "$HOME/.claude/CLAUDE.md"
rade setup --agent claude >"$OUT/setup.log"
cat >"$T/capture-bash.py" <<'CAPTURE'
import json, os, pathlib, sys
try:
    event = json.load(sys.stdin)
    cwd = event.get('cwd', '')
    command = event.get('tool_input', {}).get('command')
    if event.get('tool_name') == 'Bash' and isinstance(command, str) and pathlib.Path(cwd).resolve().is_relative_to(pathlib.Path(sys.argv[2]).resolve()):
        record = {'session_id': event.get('session_id', ''), 'cwd': cwd, 'command': command}
        fd = os.open(sys.argv[1], os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
        try:
            os.write(fd, (json.dumps(record) + '\n').encode())
        finally:
            os.close(fd)
except (OSError, ValueError, TypeError, KeyError, IndexError):
    pass
# No stdout, permission decision or execution; normal provider checks are unchanged.
CAPTURE
python3 - "$CLAUDE_CONFIG_DIR/settings.json" "$T" <<'HOOK'
import json, pathlib, shlex, sys
file, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
settings = json.loads(file.read_text())
command = 'python3 ' + ' '.join(shlex.quote(str(p)) for p in (root/'capture-bash.py', root/'bash-commands.jsonl', root))
settings.setdefault('hooks', {}).setdefault('PreToolUse', []).append({'matcher':'Bash','hooks':[{'type':'command','command':command}]})
file.write_text(json.dumps(settings))
HOOK

cat >"$XDG_CONFIG_HOME/rade/config.toml" <<'EOF'
idle_seconds = 3
[task]
verify_rounds = 2
[batch]
parallel = 1
review = "agent-judge"
max_rounds = 2
[agents.claude]
command = "claude --model sonnet --effort low --permission-mode acceptEdits --append-system-prompt 'Use Edit or Write for ALL file modifications and Read Glob Grep for inspection. Bash commands must be individual commands with literal absolute paths. Never use shell variables, heredocs, compound commands, pipes, loops or functions. No scratch files outside the fixture workspace. Use git -C with an absolute fixture path. Run screenshot commands separately with explicit paths. Keep this a tiny one-toggle fixture and reuse supplied real drafts; do not invent extra product features or extra screenshot matrices. Do not change permission mode.'"
resume = "claude --model sonnet --effort low --permission-mode acceptEdits --resume {session_id}"
EOF
START=$SECONDS
DEADLINE=$TOTAL
STAGE=setup
sanitize() {
    python3 -c 'import re,sys; text=sys.stdin.read(); text=re.sub(r"(?:sk-ant-|sk-)[A-Za-z0-9_-]+", "[REDACTED]", text); text=re.sub(r"(?i)(accessToken|refreshToken|api[_-]?key|authorization)([\s\"=:]+)[^\s\",}]+", r"\1\2[REDACTED]", text); sys.stdout.write(text)'
}
diagnostics() {
    local status=$?
    trap - EXIT
    printf '%s: exit %s; elapsed %s seconds\n' "$STAGE" "$status" "$((SECONDS-START))" >"$OUT/outcome.txt"
    if rade list --json >"$T/panes.json" 2>/dev/null; then
        jq -r '.panes[] | select(.agent != null) | .pane' "$T/panes.json" | while read -r pane; do
            rade read "$pane" --scrollback 2>/dev/null | sanitize >"$OUT/$pane-transcript.txt" || true
        done
    fi
    if [[ -d $T/work/.rade/projects ]]; then
        cp -R "$T/work/.rade/projects" "$OUT/projects"
    fi
    # Preserve user-facing diagnostics, never copied auth/settings or provider session files.
    rm -f "$CLAUDE_CONFIG_DIR/.credentials.json" "$CLAUDE_CONFIG_DIR/.claude.json"
    RADE_SOCKET="$XDG_RUNTIME_DIR/rade/rade-dev.sock" rade browser --engine chromium close >/dev/null 2>&1 || true
    cleanup
    exit "$status"
}
trap diagnostics EXIT
trap 'exit 124' TERM
await() {
    local label=$1 timeout=$2; shift 2
    local began=$SECONDS
    echo "  Waiting: $label (up to $timeout s)"
    while (( SECONDS-began < timeout && SECONDS-START < DEADLINE )); do
        if "$@" >"$T/poll.out" 2>&1; then ok "$label"; return 0; fi
        sleep 1
    done
    sanitize <"$T/poll.out" >"$OUT/last-poll.txt"
    echo "FAIL: $STAGE — $label timed out; inspect $OUT diagnostics."
    return 1
}
must() { local label=$1; shift; if "$@"; then ok "$label"; else bad "$label"; return 1; fi; }
ui() { rpc debug.ui "$(jq -nc --arg op "$1" --arg arg "${2:-}" '{op:$op,arg:$arg}')"; }
pg() { rpc debug.initiative "$1"; }
click() { pg "$(jq -nc --arg initiative "$1" --arg label "$2" '{op:"click",initiative:$initiative,label:$label}')" >/dev/null; }
settle_initiative() {
    local pane
    pane=$(rade initiative show "$I" --json | jq -r .agent)
    pane_done() { rade list --json | jq -e --arg pane "$pane" '.panes[] | select(.pane==$pane) | .status=="done" and .status_source=="hook"' >/dev/null; }
    await 'actual Initiative Stop hook completed' 90 pane_done
    sleep 2 # document submission can precede the CLI's final render/first-turn notices
    if rade read "$pane" --scrollback | tail -8 | grep -q 'auto mode on'; then
        echo 'FAIL: provider switched away from the explicitly requested acceptEdits mode; inspect first-use notice handling.'
        return 1
    fi
}
respond() { rpc debug.dialog "$(jq -nc --arg response "$1" '{op:"respond",response:$response}')" >/dev/null; }
status_is() { [[ $(rade initiative show "$1" --json | jq -r .status) == "$2" ]]; }
status_matches() { rade initiative show "$1" --json | jq -er --arg pattern "$2" '.status | test($pattern)' >/dev/null; }
set_verify() { rpc debug.ui "$(jq -nc --arg value "$1" '{op:"settings_set",arg:"acceptance",key:"task.verify",value:$value}')" >/dev/null; }
G() { git -c user.name=Acceptance -c user.email=acceptance@example.test -c commit.gpgsign=false "$@"; }
# No package manager or network dependency: Agent implementations run these checks unchanged.
cat >"$T/work/app/verify_ui.py" <<'CHECK'
from pathlib import Path
text = Path('index.html').read_text()
assert 'Show details' in text and 'Hide details' in text, 'toggle labels missing'
assert 'Details ready' in text and 'aria-expanded' in text, 'details/accessibility missing'
assert 'onclick' in text or 'addEventListener' in text, 'toggle behaviour missing'
print('PASS: required labels, details, accessible state and click behaviour source')
CHECK
cat >"$T/work/app/verify_bug.py" <<'CHECK'
from pathlib import Path
assert Path('bug.txt').read_text() == 'fixed\n', 'bug.txt must contain fixed, not broken'
print('PASS: bug.txt is fixed')
CHECK
cat >"$T/work/app/README.md" <<'DOC'
# Acceptance fixture
One plain index.html, no dependencies/build step. Python 3 checks are deliberately tiny.
Never change verify_ui.py or verify_bug.py. No commits or pushes to any external repository.
Git identity is configured for this isolated test repo.
DOC
git -C "$T/work/app" init -q -b main
G -C "$T/work/app" config user.name Acceptance
G -C "$T/work/app" config user.email acceptance@example.test
G -C "$T/work/app" add -A
G -C "$T/work/app" commit -qm init
start_app
rade workspace add "$T/work" --name acceptance >/dev/null
rade workspace switch acceptance >/dev/null
rade source add "$T/work/app" --name app >/dev/null
rade project add acceptance --source app >/dev/null
STAGE=smoke
P=$(rade spawn --agent claude --source app --name acceptance-smoke "Write a file named smoke.txt containing exactly RADE_REAL_AGENT_OK and a newline. Use the Write tool. Do not change any other file. Then stop." --json | jq -r .pane)
if ! eventually 180 test -f "$T/work/app/smoke.txt"; then
    rade read "$P" --scrollback | sanitize >"$OUT/smoke-transcript.txt"
    echo 'FAIL: real Agent smoke did not create the file; inspect smoke-transcript.txt'
    exit 1
fi
must "real Claude wrote the requested content" grep -qx RADE_REAL_AGENT_OK "$T/work/app/smoke.txt"
must "real Stop hook reports done" eventually 30 is "rade list --json | jq -r '.panes[] | select(.pane==\"$P\") | .status'" done
rade read "$P" --scrollback | sanitize >"$OUT/smoke-transcript.txt"
shot real-agent-smoke

if [[ $SCENARIO == all || $SCENARIO == ui ]]; then
STAGE=ui-initiative
echo '== Real UI Initiative: spec, design feedback, plan, verify/review/acceptance'
set_verify 'python3 verify_ui.py'
BRIEF='Build one static HTML detail-toggle feature in index.html; no dependencies, server, package installs or external resources. Initial button Show details, hidden text Details ready. Clicking shows details and button Hide details; clicking again hides it. Include aria-expanded and keyboard-native button. One spec section S1 with observable criteria for both toggle states. One milestone M1 only, covering S1; one Ready Task. Preserve verify_ui.py unchanged; verify command python3 verify_ui.py. Use tiny documents and one self-contained prototype button with data-testid="toggle". The implementation should match the approved prototype. No external push/deploy. This fully specifies the feature; proceed with the RADE phase prompt and ask only if a required decision is missing.'
INPUT_ARGS=()
if [[ -n ${RADE_REAL_AGENT_INPUTS:-} ]]; then
    for file in spec.md design.md prototype.html design-review.txt; do
        [[ -f $RADE_REAL_AGENT_INPUTS/$file ]] || { echo "Missing preserved real input: $file"; exit 2; }
        INPUT_ARGS+=(--input "$RADE_REAL_AGENT_INPUTS/$file")
    done
    BRIEF+=' Begin THIS new run with the original blue accent from prototype.html; a later owner feedback turn requests green, so do not anticipate that change. The attached inputs are REAL draft/spec/prototype/reviewer artifacts from an earlier interrupted acceptance attempt; they are not owner approvals. Reuse them to avoid rebuilding the same design. Submit a real spec, then design with required checks, each for normal owner review. Keep screenshot work to the required light/dark390/1280 matrix; no extra catalog features. Resolve recorded reviewer findings with Edit/Write.'
fi
I=$(rade initiative add 'Detail toggle' --project acceptance --kind ui --brief "$BRIEF" "${INPUT_ARGS[@]}" --json | jq -r .initiative)
DIR=$(rade initiative show "$I" --json | jq -r .dir)
rade initiative start "$I" >/dev/null
await 'real spec submitted' 240 status_is "$I" 'spec 1 to review'
settle_initiative
click "$I" 'Approve…'; respond approve
await 'real design prototype submitted' "$DESIGN_WAIT" status_is "$I" 'design 1 to review'
settle_initiative
click "$I" 'Open prototype'
prototype_pane() { rade debug state | jq -r --arg key "$I" '[.sources[].worktrees[].tabs[] | select(.initiative==$key) | .. | objects | select(.kind?.type?=="browser") | .id][0] // empty'; }
B=$(prototype_pane)
must 'prototype Browser exists' test -n "$B"
prototype_loaded() { rade browser snapshot --pane "$B" | grep -q 'button "Show details"'; }
await 'real prototype is usable in Browser' 30 prototype_loaded
must 'initial prototype is blue before owner feedback' test "$(rade browser eval '(()=>{const rgb=getComputedStyle(document.querySelector("button")).backgroundColor.match(/\d+/g).map(Number);return rgb[2]>rgb[0]&&rgb[2]>rgb[1]})()' --pane "$B")" = true
REF=$(rade browser snapshot --pane "$B" | sed -nE '/button "Show details"/s/.*\[ref=(e[0-9]+)\].*/\1/p' | head -1)
must 'prototype button has a ref' test -n "$REF"
rpc debug.browser "$(jq -nc --arg pane "$B" '{pane:$pane,op:"toggle"}')" >/dev/null
rpc debug.browser "$(jq -nc --arg pane "$B" --arg ref "$REF" '{pane:$pane,op:"pick",ref:$ref,comment:"Make the toggle button green; keep both toggle states and accessible state."}')" >/dev/null
must 'real Browser annotation recorded' bash -c 'rade browser annotations --pane "$1" --json | jq -e ".annotations | length > 0" >/dev/null' _ "$B"
rpc debug.browser "$(jq -nc --arg pane "$B" '{pane:$pane,op:"send"}')" >/dev/null
# Multiple real Agents exist: complete the actual recipient chooser, then
# require a NEW running/Stop-hook turn so an old done state cannot pass.
ANNOTATION_AGENT=$(rade initiative show "$I" --json | jq -r .agent)
CHOOSER=$(rpc debug.dialog '{"op":"info"}')
must 'annotation recipient chooser names Initiative agent' jq -e --arg pane "$ANNOTATION_AGENT" '.open and .heading=="Send to which Agent?" and (.responses | index($pane)!=null)' <<<"$CHOOSER"
respond "$ANNOTATION_AGENT"
must 'annotation recipient chooser closes' jq -e '.open==false' <<<"$(rpc debug.dialog '{"op":"info"}')"
annotation_delivered() { rpc debug.browser "$(jq -nc --arg pane "$B" '{pane:$pane,op:"state"}')" | jq -e --arg pane "$ANNOTATION_AGENT" '.note | contains("("+$pane+")")' >/dev/null; }
await 'annotation delivered to actual Initiative agent' 15 annotation_delivered
annotation_running() { rade list --json | jq -e --arg pane "$ANNOTATION_AGENT" '.panes[] | select(.pane==$pane) | .status=="running" and .status_source=="hook"' >/dev/null; }
await 'NEW annotation feedback turn starts' 30 annotation_running
settle_initiative
rade read "$ANNOTATION_AGENT" --scrollback | sanitize >"$OUT/annotation-feedback-transcript.txt"
rpc debug.dialog '{"op":"info"}' >"$OUT/annotation-dialog-closed.json"
# Request changes is the owner gate; the sent annotation is context, never approval.
click "$I" 'Request changes…'
rpc debug.dialog '{"op":"set","field":"note","value":"Apply the Browser annotation: make the toggle green. In design.md explicitly record the green toggle revision. Keep one M1 and no added scope."}' >/dev/null
respond send
await 'real design responds to annotation and changes' 240 status_is "$I" 'design 2 to review'
must 'revision is recorded in real design evidence' grep -qi green "$DIR/design/design.md"
shot real-ui-design-feedback
settle_initiative
click "$I" 'Approve…'; respond approve
await 'real one-milestone plan submitted' 240 status_is "$I" 'plan 1 to review'
must 'one M1 plan' bash -c 'test "$(grep -cE "^## M[0-9]+ " "$1")" = 1 && grep -qE "^## M1 " "$1"' _ "$DIR/plan.md"
settle_initiative
click "$I" 'Approve…'; respond approve
await 'real build, trusted verify, Reviewer and independent acceptance finish' 420 status_is "$I" 'M1 to review'
must 'independent checks record PASS' grep -q '^Result: PASS' "$DIR/checks/M1.md"
rade initiative show "$I" --json >"$OUT/ui-initiative.json"
real_batch_done() { rade batch status --json | jq -e 'any(.batches[]; .status=="done" and .policy.review=="agent-judge" and any(.items[]; .round >= 1))' >/dev/null; }
must 'real Batch is done with Reviewer rounds' real_batch_done
rade batch status --json >"$OUT/ui-batches.json"
click "$I" 'Continue…'; respond continue
await 'owner finishes the real Initiative' 30 status_is "$I" done
UIROOT=$(rade worktree list app --json | jq -r '.worktrees[] | select(.worktree | startswith("initiative/")) | .path' | head -1)
cp "$UIROOT/index.html" "$OUT/implemented-index.html"
G -C "$UIROOT" rev-parse HEAD >"$OUT/implemented-commit.txt"
must 'real implemented feature passes repo check' bash -c 'cd "$1" && python3 verify_ui.py' _ "$UIROOT"
rpc debug.browser "$(jq -nc --arg pane "$B" '{pane:$pane,op:"toggle"}')" >/dev/null
rade browser open "file://$UIROOT/index.html" --pane "$B" >/dev/null
await 'real implementation loads in Browser' 30 prototype_loaded
REF=$(rade browser snapshot --pane "$B" | sed -nE '/button "Show details"/s/.*\[ref=(e[0-9]+)\].*/\1/p' | head -1)
rade browser click "$REF" --pane "$B" >/dev/null
expanded() { rade browser snapshot --pane "$B" | grep -q 'button "Hide details"'; }
await 'actual click shows details and Hide label' 15 expanded
must 'owner green-button feedback appears in actual implementation' test "$(rade browser eval '(()=>{const rgb=getComputedStyle(document.querySelector("button")).backgroundColor.match(/\d+/g).map(Number);return rgb[1]>rgb[0]&&rgb[1]>rgb[2]})()' --pane "$B")" = true
must 'actual expanded accessibility state' test "$(rade browser eval 'document.querySelector("button").getAttribute("aria-expanded")' --pane "$B")" = true
REF=$(rade browser snapshot --pane "$B" | sed -nE '/button "Hide details"/s/.*\[ref=(e[0-9]+)\].*/\1/p' | head -1)
rade browser click "$REF" --pane "$B" >/dev/null
await 'actual second click hides details and resets label' 15 prototype_loaded
must 'actual collapsed accessibility state' test "$(rade browser eval 'document.querySelector("button").getAttribute("aria-expanded")' --pane "$B")" = false
# The checks themselves must survive unchanged, not be weakened by an Agent.
must 'UI check was preserved' cmp "$T/work/app/verify_ui.py" "$UIROOT/verify_ui.py"
shot real-ui-finished

fi

if [[ $SCENARIO == all || $SCENARIO == bug ]]; then
STAGE=bug-verify-fix
echo '== Real bug Task: fail trusted Verify, fix, pass'
set_verify 'python3 verify_bug.py'
cat >"$T/bug-body.md" <<'TASK'
## Goal
Exercise a real verification/fix round: bug.txt must ultimately contain exactly fixed and newline.
## Constraints
Preserve verify_bug.py. No other source changes, no package/network operations.
For the FIRST turn only, deliberately write bug.txt with broken and newline, summarize and stop without fixing it. RADE will run the trusted check and send the failure. On that follow-up correct it to fixed and newline, then stop.
## Done when
- [ ] bug.txt is fixed plus newline after the RADE failure/fix follow-up.
## Verify
- `python3 verify_bug.py` exits 0.
TASK
BUG=$(rade task add 'Real verify failure and fix' --project acceptance --status ready --body-file "$T/bug-body.md" --json | jq -r .task)
rade task start "$BUG" >/dev/null
bug_fixed() { rade task show "$BUG" --json | jq -e '.status=="review" and .verify.state=="passed"' >/dev/null; }
await 'real bug Agent fixes verification failure' 240 bug_fixed
BUGFILE=$(rade task show "$BUG" --json | jq -r .file)
must 'actual failed and passed checks are both logged' bash -c 'grep -q "verify failed, sent back" "$1" && grep -q "verify passed" "$1"' _ "$BUGFILE"
rade task show "$BUG" --json >"$OUT/bug-task.json"
BUGBRANCH=$(rade task show "$BUG" --json | jq -r .branch)
BUGROOT=$(rade worktree list app --json | jq -r --arg branch "$BUGBRANCH" '.worktrees[] | select(.worktree==$branch) | .path')
must 'bug Task Worktree exists' test -n "$BUGROOT"
python3 - "$BUGROOT/bug.txt" <<'ASSERT'
from pathlib import Path
import sys
assert Path(sys.argv[1]).read_bytes() == b'fixed\n', 'actual bug Task bytes are not fixed + newline'
ASSERT
ok 'actual bug.txt bytes are fixed + newline'
G -C "$T/work/app" show main:verify_bug.py >"$T/original-bug-check.py"
must 'bug verifier was not weakened' cmp "$T/original-bug-check.py" "$BUGROOT/verify_bug.py"
must 'trusted original checker passes in actual bug Worktree' bash -c 'cd "$1" && python3 "$2"' _ "$BUGROOT" "$T/original-bug-check.py"
ui tasks_open acceptance >/dev/null; ui tasks_select "$BUG" >/dev/null
shot real-bug-verified

fi

if [[ $SCENARIO == all || $SCENARIO == resume ]]; then
STAGE=interrupted-session
echo '== Real session interruption and restart'
P=$(rade spawn --agent claude --source app --name interrupted-real-agent 'Use Write to create before-restart.txt containing started and newline. Then call Bash sleep 120. Only AFTER it finishes use Write to create after-restart.txt containing completed and newline. Then stop. If interrupted and resumed, do not repeat the long sleep; write after-restart.txt and stop.' --json | jq -r .pane)
await 'real Agent begins the interruptible run' 150 test -f "$T/work/app/before-restart.txt"
session_ready() { rade debug state | jq -e --arg pane "$P" '[.. | objects | select(.id?==$pane) | .session_id | select(.!=null)] | length==1' >/dev/null; }
await 'real resumable session recorded' 15 session_ready
SID=$(rade debug state | jq -r --arg pane "$P" '.. | objects | select(.id?==$pane) | .session_id')
must 'run is not already finished' test ! -f "$T/work/app/after-restart.txt"
sleep 2 # debounced state write before deliberately crashing only the isolated app
kill -KILL "$APP_PID"; wait "$APP_PID" 2>/dev/null || true; APP_PID=
start_app
same_session() { rade debug state | jq -e --arg pane "$P" --arg session "$SID" '[.. | objects | select(.id?==$pane and .session_id?==$session)] | length==1' >/dev/null; }
await 'restart keeps the same real session' 30 same_session
await 'restart marks the interrupted turn' 30 is "rade list --json | jq -r '.panes[] | select(.pane==\"$P\") | .status'" interrupted
resumed_prompt_ready() { rade read "$P" --scrollback | tail -8 | grep -q 'accept edits on'; }
await 'restored interactive CLI prompt is ready' 60 resumed_prompt_ready
sleep 2 # don't paste into the provider's resume/startup render
rade send "$P" 'Continue the interrupted request now. Do not repeat sleep. Write after-restart.txt containing completed and newline using Write, then stop.' --enter >/dev/null
await 'restored real session finishes the requested work' 150 test -f "$T/work/app/after-restart.txt"
python3 - "$T/work/app/after-restart.txt" <<'ASSERT'
from pathlib import Path
import sys
assert Path(sys.argv[1]).read_bytes() == b'completed\n', 'actual resumed output is not completed + newline'
ASSERT
ok 'resumed content is exactly completed + newline'
await 'resumed real Stop hook reports turn finished' 30 is "rade list --json | jq -r '.panes[] | select(.pane==\"$P\") | .status'" done
shot real-session-restored
fi
STAGE=complete
echo "== $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
