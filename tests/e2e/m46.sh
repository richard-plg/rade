#!/usr/bin/env bash
# E2E (M46, PRD R124): Voice prompts, driven by a fake microphone, a fake
# OpenAI-compatible STT server and a fake voice agent (stream-json) — no
# audio device, no network, no real claude (tests/e2e/fake-mic.sh,
# fake-stt.py, fake-voice-agent.py; `claude` on PATH is a wrapper around the
# fake so the app builds its real command line). Agent Panes `planner` (the
# stand-in Agent as `claude`, whose Claude transcript holds `handleSubmit`) and
# `tests` (a raw-mode sink that logs the bytes it receives) in Worktree
# checkout-v2 of Source shop; F8 is held with `debug.ui key press:F8` /
# `release:F8`.
#   R124-k  hold 3 s: Listening, live partials, REC, STT prompt vocabulary, first
#           message (session + git status), streamed draft with `Read …`, Ready
#   R124-l  hand edit, hold again: stale, REVISE carries the edit, changed marks,
#           revision 1, one voice agent pid
#   R124-m  Enter = bracketed paste + Enter on the sink, composer closed, agent
#           gone; Alt+Enter = paste only
#   R124-n  agent env without RADE_PANE_ID / RADE_SOCKET, Pane status unchanged,
#           no new file under $XDG_RUNTIME_DIR, $TMPDIR or the Claude config dir;
#           REAL=1: a real Haiku draft leaves no new .jsonl (skipped otherwise)
#   R124-o  timeout_secs = 10 → Timed out with the delta kept; Settings › Voice
#           Max steps 3 → `max_turns = 3` and the next agent's --max-turns 3
#   R124-p  no STT server, failing record_command, Set up voice… → Settings
#   R124-q  Shell Pane Toast, mic button on/off, one composer at a time
#   R124-r  spec I (tests/e2e/spec_i.py) scenes I-a / I-c / I-e dark + light at
#           1440 × 900, goldens tests/e2e/golden/m46/ (APPROVE=1 writes missing
#           ones), live-state screenshots; I.26 contrast; I.27 at 1280 px and
#           Text scale 200 % (the m40 helpers)
# Screenshots → target/e2e-m46/.
# Usage: tests/e2e/m46.sh   (NO_BUILD=1 skips the build, KEEP=1 keeps the temp dir,
#        APPROVE=1 writes missing goldens, REAL=1 adds the real-claude part of R124-n,
#        ONLY="q" runs only those sections: k l m n o p q r (l needs k)
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$HERE/../../target/e2e-m46}
export WESTON_WIDTH=1440
source "$HERE/lib.sh"

SOCK=$XDG_RUNTIME_DIR/rade/rade-dev.sock
TOKENS=$ROOT/crates/rade-app/src/tokens.toml
STYLE=$ROOT/crates/rade-app/src/style.css
PORT=18178 DEADPORT=18179
export TMPDIR=$T/tmp
mkdir -p "$TMPDIR" "$T/fakebin" "$XDG_CONFIG_HOME/rade" "$XDG_CONFIG_HOME/gtk-4.0"
# A steady caret: the goldens must not depend on the blink phase.
printf '[Settings]\ngtk-cursor-blink=false\n' >"$XDG_CONFIG_HOME/gtk-4.0/settings.ini"
export STT_LOG=$T/stt.log FAKE_STT_LOG=$T/stt.log FAKE_VOICE_LOG=$T/voice.log SINK_LOG=$T/sink.bin
export FAKE_STT_SCRIPT='["ừ thì cái phần promo", "ừ thì cái phần promo code á nó bị", "ừ thì cái phần promo code á nó bị trừ hai lần"]'
export FAKE_VOICE_DRAFTS='["Fix the promo discount being applied twice in `useTotals()`.\n- Apply it once, in `applyPromo()` (`src/checkout/promo.ts:31`).\n- Add a test for a percent promo.", "Fix the promo discount being applied twice in `useTotals()`.\n- Apply it once, in `applyPromo()` (`src/checkout/promo.ts:31`).\n- Keep the `applyPromo()` signature unchanged."]'
export FAKE_AGENT=claude
FIRST_DRAFT=$(jq -r '.[0]' <<<"$FAKE_VOICE_DRAFTS")

# `claude` on PATH = the fake voice agent (the app builds the real argv, D7);
# $T/hang makes it hang after one delta; $T/real-claude (REAL=1) makes it the
# real one on the real config dir.
cat >"$T/fakebin/claude" <<EOT
#!/usr/bin/env bash
if [[ -f "$T/real-claude" ]]; then
    export CLAUDE_CONFIG_DIR=\$(sed -n 2p "$T/real-claude")
    exec "\$(sed -n 1p "$T/real-claude")" "\$@"
fi
[[ -f "$T/hang" ]] && export FAKE_VOICE_HANG=1
exec python3 -I "$HERE/fake-voice-agent.py" "\$@"
EOT
# The sink Agent: bracketed paste on, raw input, every byte it gets → $SINK_LOG.
cat >"$T/fakebin/sink" <<'EOT'
#!/usr/bin/env bash
printf '\e[?2004h'
stty raw -echo
exec python3 -u -c '
import os
while True:
    b = os.read(0, 4096)
    if not b:
        break
    open(os.environ["SINK_LOG"], "ab").write(b)
'
EOT
chmod +x "$T/fakebin/claude" "$T/fakebin/sink"
ORIG_PATH=$PATH
export PATH=$T/fakebin:$PATH

python3 -I "$HERE/fake-stt.py" $PORT >/dev/null 2>&1 & STT_PID=$!
trap 'kill $STT_PID 2>/dev/null; cleanup' EXIT
conf() { # record_command
    cat >"$XDG_CONFIG_HOME/rade/config.toml" <<EOT
idle_seconds = 3600
[agents.claude]
command = "$HERE/fake-agent.sh --model opus"
[agents.opencode]
command = "$T/fakebin/sink"
[voice]
stt_url = "http://127.0.0.1:$PORT/v1/audio/transcriptions"
record_command = "${1:-$HERE/fake-mic.sh}"
EOT
}
conf

G() { git -c user.email=t@t -c user.name=t -c commit.gpgsign=false "$@"; }
R=$T/shop
mkdir -p "$R/src/checkout"
echo 'export const applyPromo = (t: number) => t;' >"$R/src/checkout/promo.ts"
echo 'export const useTotals = () => 0;' >"$R/src/checkout/useTotals.ts"
G -C "$R" init -q -b main && G -C "$R" add -A && G -C "$R" commit -qm "Add checkout"

# ---- helpers ---------------------------------------------------------------
vinfo() { rpc debug.voice '{"op":"info"}'; }
vq() { vinfo | jq -r "$1"; }
vop() { rpc debug.voice "{\"op\":\"$1\"${2:+,$2}}"; }
vkey() { rpc debug.ui "{\"op\":\"key\",\"keys\":\"$1\"}" >/dev/null; }
geo() { rpc debug.ui "{\"op\":\"geometry\",\"widget\":\"$1\"${2:+, $2}}"; }
ui() { rpc debug.ui "{\"op\":\"$1\"${2:+, \"arg\": \"$2\"}}"; }
pst() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | "\(.status) \(.status_since)"'; }
alive() { local s; s=$(ps -o stat= -p "$1" 2>/dev/null); [[ -n $s && $s != Z* ]]; }
voice_procs() { pgrep -f "fake-voice-agent.py" | wc -l; }
# take <seconds>: hold F8, let go, wait for the draft (state ready).
take() {
    vkey press:F8; sleep "${1:-2}"; vkey release:F8
    eventually 30 is "vq .state" ready
}
# close the composer whatever its state (Esc until it is gone).
close_composer() {
    local i
    for i in 1 2 3 4 5; do
        [[ $(vq .open) == true ]] || return 0
        vop escape >/dev/null; sleep 0.3
    done
    [[ $(vq .open) == false ]]
}
# The Toasts the app raised so far (`debug.ui chrome` → notes, the last 20).
notes() { ui chrome | jq -c '.chrome.notes'; }
last_note() { ui chrome | jq -r '.chrome.notes[-1] // ""'; }
snap() { find "$XDG_RUNTIME_DIR" "$TMPDIR" "$HOME/.claude" -mindepth 1 -printf '%p %y\n' 2>/dev/null | sort; }
vlog() { jq -c "$@" "$FAKE_VOICE_LOG"; }
export -f vinfo vq vop vkey geo ui pst alive voice_procs take close_composer notes last_note snap vlog rpc is eventually

want() { [[ -z ${ONLY:-} || " $ONLY " == *" $1 "* ]]; }
echo "== start"
start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
[[ $(ui chrome | jq -r .chrome.inspector.shown) == true ]] && vkey ctrl+shift+e
rade source add "$R" --name shop >/dev/null
rade worktree add shop checkout-v2 >/dev/null
planner=$(rade spawn --agent claude --source shop --worktree checkout-v2 --name planner "look at the checkout" --json | jq -r .pane)
tests=$(rade spawn --agent opencode --source shop --worktree checkout-v2 --name tests --json | jq -r .pane)
shell=$(rade list --json | jq -r '[.panes[] | select(.agent == null)][0].pane')
eventually 15 is "pst $planner | cut -d' ' -f1" done
CWD=$(rade list --json | jq -r --arg p "$planner" '.panes[] | select(.pane==$p) | .cwd')
SLUG=$(sed 's/[^A-Za-z0-9]/-/g' <<<"$CWD")
mkdir -p "$HOME/.claude/projects/$SLUG"
# RADE gives the Agent its own `--session-id`; the stand-in prints it.
SID=$(rade read "$planner" --scrollback | sed -n 's/^fake-agent session //p' | head -1)
echo "   session id $SID"
cat >"$HOME/.claude/projects/$SLUG/$SID.jsonl" <<'EOT'
{"type":"user","message":{"role":"user","content":"the promo applies twice when handleSubmit runs in CheckoutForm"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I found handleSubmit calling applyPromo and useTotals; I will make the promo apply once."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/secret/tool-input"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"TOOL-RESULT-NOT-FOR-THE-VOICE-AGENT"}]}}
EOT
echo "wip" >"$CWD/notes.txt"
echo "// wip" >>"$CWD/src/checkout/promo.ts"
rade focus "$planner" >/dev/null
sleep 1
echo "   planner=$planner tests=$tests shell=$shell cwd=$CWD"

# ======================================================================== k
if want k; then
echo "== R124-k hold F8 for 3 s"
: >"$FAKE_STT_LOG"; : >"$FAKE_VOICE_LOG"
vkey press:F8
sleep 3.1
vinfo >"$T/k-listening.json"
shot k-listening
check "R124-k Listening in the composer while F8 is held (hold mode, recorder running)" \
    bash -c "jq -e '.open and .state==\"listening\" and .label==\"Listening\" and .mode==\"hold\" and .recorder.bytes > 0' '$T/k-listening.json' >/dev/null"
check "R124-k at least 2 live partials while held (STT requests with a growing take; words shown)" \
    bash -c "[[ \$(jq '.stt_requests' '$T/k-listening.json') -ge 2 ]] && [[ \$(jq -s '[.[].wav.samples] | . == sort and (unique | length) >= 2' '$FAKE_STT_LOG') == true ]] && jq -e '(.heard + .tentative) | length > 0' '$T/k-listening.json' >/dev/null"
check "R124-k the Statusline shows REC while talking" \
    bash -c "[[ \$(geo status-voice | jq -r '.visible and (.label | test(\"^REC [0-9]+:[0-9]{2}\$\"))') == true ]]"
vkey release:F8
: >"$T/k-samples"
for _ in $(seq 120); do
    vinfo | jq -c '{state, activity, step, len: (.draft | length)}' >>"$T/k-samples"
    [[ $(tail -1 "$T/k-samples" | jq -r .state) == ready ]] && break
    sleep 0.1
done
shot k-ready
FINAL_LEN=$(vq '.draft | length')
check "R124-k the state is Ready with the draft" \
    bash -c "[[ \$(vq .state) == ready && \$(vq .label) == Ready ]] && [[ \$(vq '.draft | length') -gt 40 ]]"
check "R124-k the activity line showed 'Read …' while the agent worked" \
    bash -c "jq -e -s 'any(.[]; .state == \"drafting\" and (.activity | test(\"^Read src/checkout/\")))' '$T/k-samples' >/dev/null"
check "R124-k the draft streamed in (drafting samples with a part of the final text)" \
    bash -c "jq -e -s --argjson n $FINAL_LEN '[.[] | select(.state == \"drafting\" and .len > 0 and .len < \$n)] | length >= 2' '$T/k-samples' >/dev/null"
check "R124-k the STT prompt contained handleSubmit (and not the tool input or result)" \
    bash -c "jq -e -s 'any(.[]; .fields.prompt | test(\"handleSubmit\"))' '$FAKE_STT_LOG' >/dev/null && ! grep -q 'tool-input\|TOOL-RESULT' '$FAKE_STT_LOG'"
check "R124-k the voice agent's first message held the session text, git status and the speech" \
    bash -c "jq -e -s '[.[] | select(.turn != null)][0].message | test(\"promo applies twice when handleSubmit runs\") and test(\"git status --short\") and test(\"notes.txt\") and test(\"SPEECH\") and (test(\"TOOL-RESULT\") | not) and (test(\"tool-input\") | not)' '$FAKE_VOICE_LOG' >/dev/null"
check "R124-k the voice agent was started once, in the Pane's directory, by the built claude command" \
    bash -c "jq -e -s '[.[] | select(.start)] | length == 1 and .[0].cwd == \"$CWD\" and (.[0].argv | index(\"--no-session-persistence\") != null and index(\"--max-turns\") != null)' '$FAKE_VOICE_LOG' >/dev/null"
fi

# ======================================================================== l
if want l; then
echo "== R124-l revise"
rpc debug.voice '{"op":"type","text":" EDITED-BY-HAND"}' >/dev/null
check "R124-l a Ready draft edited by hand (draft_edited, text in the box)" \
    bash -c "vinfo | jq -e '.state==\"ready\" and .draft_edited and (.draft | test(\"EDITED-BY-HAND\"))' >/dev/null"
PID1=$(vq .agent.pid)
vkey press:F8
sleep 1.8
shot l-revising
check "R124-l while F8 is held: Revising and the draft dims (stale)" \
    bash -c "[[ \$(vq .label) == Revising && \$(vq .state) == listening ]] && geo voice-draft | jq -e '.classes | index(\"stale\") != null' >/dev/null"
vkey release:F8
eventually 30 is "vq '[.state, .revision] | join(\" \")'" "ready 1"
shot l-revised
check "R124-l the REVISE message carried the edited draft and the new words" \
    bash -c "jq -e -s '[.[] | select(.turn == 1)][0].message | startswith(\"REVISE\") and test(\"EDITED-BY-HAND\") and test(\"SPEECH\")' '$FAKE_VOICE_LOG' >/dev/null"
check "R124-l the new draft shows changed marks and the header says revision 1" \
    bash -c "vinfo | jq -e '(.draft_tags.changed | length) > 0 and (.time | startswith(\"revision 1\")) and (.draft | test(\"EDITED\") | not)' >/dev/null"
check "R124-l the same voice agent process answered (one pid, one start, two turns)" \
    bash -c "[[ \$(vq .agent.pid) == $PID1 ]] && [[ \$(vq .agent.turns) == 2 ]] && jq -e -s '([.[] | select(.start)] | length) == 1 and ([.[] | select(.turn != null) | .pid] | unique | length) == 1' '$FAKE_VOICE_LOG' >/dev/null"
close_composer
fi

# ======================================================================== m
if want m; then
echo "== R124-m send to the sink Agent Pane"
rade focus "$tests" >/dev/null; sleep 0.5
take 2
SINK_DRAFT=$(vq .draft)
AGENT_PID=$(vq .agent.pid)
shot m-ready
: >"$SINK_LOG"
vkey enter
eventually 10 is "vq .open" false
sleep 0.5
check "R124-m Enter pasted the draft in a bracketed paste followed by Enter" \
    python3 - "$SINK_LOG" "$SINK_DRAFT" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
draft = sys.argv[2]
body = data.removeprefix(b"\x1b[200~").removesuffix(b"\r").removesuffix(b"\x1b[201~")
norm = body.decode().replace("\r\n", "\n").replace("\r", "\n")
ok = data.startswith(b"\x1b[200~") and data.endswith(b"\x1b[201~\r") and norm == draft
if not ok:
    print("sink got", repr(data))
sys.exit(0 if ok else 1)
PY
check "R124-m the composer is closed and the voice agent process is gone" \
    bash -c "[[ \$(vq .open) == false ]] && ! alive $AGENT_PID && [[ \$(voice_procs) == 0 ]]"
take 2
: >"$SINK_LOG"
vkey alt+enter
eventually 10 is "vq .open" false
sleep 0.5
check "R124-m Alt+Enter on another draft pastes without Enter" \
    python3 - "$SINK_LOG" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
ok = data.startswith(b"\x1b[200~") and data.endswith(b"\x1b[201~")
if not ok:
    print("sink got", repr(data))
sys.exit(0 if ok else 1)
PY
fi

# ======================================================================== n
if want n; then
echo "== R124-n nothing kept, no Pane status"
rade focus "$planner" >/dev/null; sleep 0.5
close_composer
ST0=$(pst "$planner")
snap >"$T/snap-before"
: >"$FAKE_VOICE_LOG"
take 2
snap >"$T/snap-ready"
close_composer
snap >"$T/snap-closed"
echo "   status before: $ST0, after: $(pst "$planner"); files before: $(wc -l <"$T/snap-before")"
check "R124-n the voice agent's env has no RADE_PANE_ID / RADE_SOCKET; its 'rade status --hook' did nothing" \
    bash -c "jq -e -s '[.[] | select(.start)][0] | (.env | has(\"RADE_PANE_ID\") or has(\"RADE_SOCKET\") | not) and .status_hook == 0' '$FAKE_VOICE_LOG' >/dev/null"
check "R124-n the Pane's status (and its since-stamp) did not change across the draft" \
    test "$(pst "$planner")" = "$ST0"
check "R124-n no file or directory was created under \$XDG_RUNTIME_DIR, \$TMPDIR or the Claude config dir (while Ready and after close)" \
    bash -c "diff '$T/snap-before' '$T/snap-ready' && diff '$T/snap-before' '$T/snap-closed'"
if [[ ${REAL:-} == 1 ]] && REAL_CLAUDE=$(PATH=$ORIG_PATH command -v claude); then
    REAL_HOME=$(getent passwd "$(id -un)" | cut -d: -f6)
    REAL_CFG=${REAL_CLAUDE_CONFIG_DIR:-$REAL_HOME/.claude}
    jsonls() { find "$REAL_CFG/projects" -name '*.jsonl' 2>/dev/null | sort; }
    jsonls >"$T/real-before"
    printf '%s\n%s\n' "$REAL_CLAUDE" "$REAL_CFG" >"$T/real-claude"
    take_real() { vkey press:F8; sleep 4; vkey release:F8; eventually 120 is "vq .state" ready; }
    take_real
    echo "   real draft: $(vq .draft | head -c 160)"
    close_composer
    rm -f "$T/real-claude"
    jsonls >"$T/real-after"
    check "R124-n REAL: a real Haiku draft leaves no new .jsonl under $REAL_CFG/projects" \
        bash -c "[[ -n \$(cat '$T/real-before' '$T/real-after') || true ]] && diff '$T/real-before' '$T/real-after'"
else
    echo "  SKIP R124-n REAL part: ${REAL:+claude is not installed}${REAL:-set REAL=1 (and have claude installed) to run a real Haiku draft}"
fi
fi

# ======================================================================== o
if want o; then
echo "== R124-o timeout_secs = 10, Max steps = 3 from the Settings page"
ui app_settings_open >/dev/null
rpc debug.ui '{"op":"app_settings_set","key":"voice.timeout_secs","value":10}' >/dev/null
ui app_settings_close >/dev/null
touch "$T/hang"
: >"$FAKE_VOICE_LOG"
vkey press:F8; sleep 2; vkey release:F8
T0=$(date +%s.%N)
eventually 30 is "vq .label" "Timed out"
T1=$(date +%s.%N)
shot o-timed-out
echo "   timed out after $(python3 -c "print(round($T1 - $T0, 1))") s"
check "R124-o after 10 s (±2 s) the state is 'Timed out' with the delta kept as the draft" \
    bash -c "python3 -c 'import sys; sys.exit(0 if 8 <= $T1 - $T0 <= 12.5 else 1)' && [[ \$(vq .state) == error ]] && [[ \$(vq .draft) == '${FIRST_DRAFT:0:12}' ]] && vq .error | grep -q '10 s'"
close_composer
check "R124-o the hung voice agent was killed with the composer" eventually 5 is voice_procs 0
rm -f "$T/hang"
cp "$XDG_CONFIG_HOME/rade/config.toml" "$T/conf-before"
ui app_settings_open >/dev/null
rpc debug.ui '{"op":"app_settings_set","key":"voice.max_turns","value":3}' >/dev/null
ui app_settings_close >/dev/null
check "R124-o Settings › Voice › Max steps = 3 wrote max_turns = 3 under [voice], other lines unchanged" \
    bash -c "[[ \$(diff '$T/conf-before' '$XDG_CONFIG_HOME/rade/config.toml' | grep -c '^[<>]') == 1 ]] && diff '$T/conf-before' '$XDG_CONFIG_HOME/rade/config.toml' | grep -qx '> max_turns = 3' && python3 -c 'import sys, tomllib; c = tomllib.load(open(sys.argv[1], \"rb\")); sys.exit(0 if c[\"voice\"][\"max_turns\"] == 3 and c[\"voice\"][\"timeout_secs\"] == 10 else 1)' '$XDG_CONFIG_HOME/rade/config.toml'"
: >"$FAKE_VOICE_LOG"
vkey press:F8; sleep 1; vkey release:F8
eventually 30 is "vq .state" ready
check "R124-o the next voice agent's argv has --max-turns 3" \
    bash -c "jq -e -s '[.[] | select(.start)][-1].argv | . as \$a | (index(\"--max-turns\") | \$a[. + 1]) == \"3\"' '$FAKE_VOICE_LOG' >/dev/null && vq '.agent.argv | join(\" \")' | grep -q -- '--max-turns 3'"
close_composer
ui app_settings_open >/dev/null
rpc debug.ui '{"op":"app_settings_reset","key":"voice.max_turns"}' >/dev/null
rpc debug.ui '{"op":"app_settings_reset","key":"voice.timeout_secs"}' >/dev/null
ui app_settings_close >/dev/null
fi

# ======================================================================== p
if want p; then
echo "== R124-p errors"
kill $STT_PID; wait $STT_PID 2>/dev/null
vkey press:F8; sleep 1.5; vkey release:F8
eventually 15 is "vq .label" "Can't transcribe"
shot p-no-stt
check "R124-p no STT server → 'Can't transcribe' with 'No speech-to-text server at 127.0.0.1:$PORT'" \
    bash -c "[[ \$(vq .state) == error ]] && vq .error | grep -q 'No speech-to-text server at 127.0.0.1:$PORT'"
close_composer
python3 -I "$HERE/fake-stt.py" $PORT >/dev/null 2>&1 & STT_PID=$!
conf "FAKE_MIC_FAIL=1 $HERE/fake-mic.sh"
sleep 1
vkey press:F8; sleep 1.5; vkey release:F8
eventually 15 is "vq .label" "Can't record"
shot p-no-mic
check "R124-p a failing record_command → 'Can't record: pw-record: no source'" \
    bash -c "[[ \$(vq .state) == error ]] && [[ \$(vq .error) == \"Can't record: pw-record: no source\" ]]"
close_composer
conf
sleep 1
take 1.5
check "R124-p holding F8 again retries: a working record_command drafts" test "$(vq .state)" = ready
close_composer
# "Set up voice…" on a STT error.
kill $STT_PID; wait $STT_PID 2>/dev/null
vkey press:F8; sleep 1.2; vkey release:F8
eventually 15 is "vq .label" "Can't transcribe"
vop setup_link >/dev/null
sleep 0.5
check "R124-p 'Set up voice…' opens the Settings page filtered to 'voice'" \
    bash -c "ui app_settings | jq -e '.open and .query == \"voice\" and ([.rows[].category] | unique == [\"Voice\"])' >/dev/null"
ui app_settings_close >/dev/null
close_composer
python3 -I "$HERE/fake-stt.py" $PORT >/dev/null 2>&1 & STT_PID=$!
sleep 0.5
fi

# ======================================================================== q
if want q; then
echo "== R124-q Shell Pane, the mic button, one composer at a time"
rade focus "$shell" >/dev/null; sleep 0.5
vkey press:F8; sleep 0.4; vkey release:F8
sleep 0.5
check "R124-q F8 on a Shell Pane: Toast 'Voice works on Agent Panes', no composer" \
    bash -c "[[ \$(last_note) == 'Voice works on Agent Panes' && \$(vq .open) == false ]]"
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null
rade focus "$planner" >/dev/null; sleep 0.8
rpc debug.ui '{"op":"click","widget":"voice-button"}' >/dev/null
sleep 1.2
shot q-mic-on
check "R124-q the mic button's first click starts a take (toggle mode, recording)" \
    bash -c "vinfo | jq -e '.open and .state == \"listening\" and .mode == \"toggle\" and .recorder != null' >/dev/null && geo voice-button | jq -e '.classes | index(\"recording\") != null' >/dev/null"
rpc debug.ui '{"op":"click","widget":"voice-button"}' >/dev/null
eventually 30 is "vq .state" ready
check "R124-q the second click ends the take and the draft comes" \
    bash -c "[[ \$(vq .state) == ready && \$(vq .recorder) == null ]]"
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null
rade focus "$tests" >/dev/null; sleep 0.5
vkey press:F8; sleep 0.4; vkey release:F8
sleep 0.5
check "R124-q F8 while the composer is open on planner: ignored, Toast 'Voice is open on planner'" \
    bash -c "[[ \$(vq .pane) == planner && \$(vq .state) == ready && \$(vq .recorder) == null ]] && [[ \$(last_note) == 'Voice is open on planner' ]]"
close_composer
rade focus "$tests" >/dev/null; sleep 0.5
vkey press:F8; sleep 0.4; vkey release:F8
sleep 1
check "R124-q after planner's composer is closed, the next F8 opens it on the focused 'tests' Pane" \
    bash -c "vinfo | jq -e '.open and .pane == \"tests\"' >/dev/null"
close_composer
rpc debug.ui '{"op":"toasts_clear"}' >/dev/null
fi

# A Pane whose recorded folder is gone (a removed Worktree; an agent that
# never reports OSC 7): the voice agent starts in the next folder that exists
# instead of failing with "No such file or directory" (owner's run, 2026-10-26).
if want q; then
echo "== R124-k a Pane whose folder is gone"
rade worktree add shop gone >/dev/null
GONE=$(rade worktree list shop --json | jq -r '.worktrees[] | select(.worktree=="gone") | .path')
orphan=$(rade spawn --agent claude --source shop --worktree gone --name orphan "look around" --json | jq -r .pane)
sleep 2
rm -rf "$GONE"
rade focus "$orphan" >/dev/null; sleep 0.5
: >"$FAKE_VOICE_LOG"
take 2 || true
check "R124-k the voice agent of a Pane whose folder was deleted still drafts, started in \$HOME" \
    bash -c "[[ \$(vq .state) == ready ]] && jq -e -s '[.[] | select(.start)] | .[0].cwd == \"$HOME\"' '$FAKE_VOICE_LOG' >/dev/null"
close_composer
rade close "$orphan" --force >/dev/null 2>&1 || true
fi

# ======================================================================== r
if want r; then
echo "== R124-r spec I scenes, goldens, 1280 px and Text scale 200 %"
rade focus "$planner" >/dev/null
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null; sleep 1.2
masks() { # file
    local f=$1 args=() w
    for w in terminal glyph-running led-running nav-row-meta activity-time voice-caret; do args+=("$(geo "$w" '"all":true')"); done
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
spec_i() { python3 "$HERE/spec_i.py" "$SOCK" "$TOKENS" "$STYLE" "$1" "$2" "$3" >"$OUT/spec-i-$2-$1.txt" 2>&1; }
prim0=$(geo suggested-action '"all":true' | jq length)
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
notoasts() { rpc debug.ui '{"op":"toasts_clear"}' >/dev/null; sleep 0.6; }
notoasts
for th in dark light; do
    [[ $th == light ]] && light
    for s in a c e; do
        notoasts
        vop scene "\"arg\":\"$s\"" >/dev/null; sleep 1
        check "R124-r scene I-$s $th: spec I rows I.1–I.25 within tolerance" spec_i "$th" "$s" "$prim0"
        sed 's/^/    /' "$OUT/spec-i-$s-$th.txt" | grep -v '^    PASS' || true
        masks "$OUT/masks-$s-$th.json"
        shot "$s-$th"
    done
    vop scene '"arg":"off"' >/dev/null
    [[ $th == light ]] && dark
done
python3 "$ROOT/docs/internals/design/contrast.py" >"$OUT/contrast.txt" 2>&1
check "R124-r I.26 contrast: every pair of the sheet passes (fg2/bg2, fg3/bg0/bg2, wait/wait-dim, acc-fg/acc, err/bg1; both themes)" \
    bash -c "! grep -v '^OK' '$OUT/contrast.txt' | grep -q . && for t in dark light; do for p in 'fg2 on bg2' 'fg3 on bg0' 'fg3 on bg2' 'wait on wait-dim' 'acc-fg on acc' 'err on bg1'; do grep -q \"\$t \$p\" '$OUT/contrast.txt' || exit 1; done; done"
echo "== Goldens I (tests/e2e/golden/m46/)"
GOLD=$HERE/golden/m46
mkdir -p "$GOLD"
for s in a c e; do
    for th in dark light; do
        if [[ ! -s $GOLD/$s-$th.png && ${APPROVE:-} == 1 ]]; then
            cp "$OUT/$s-$th.png" "$GOLD/$s-$th.png" && echo "  approved (first run): $s-$th"
        fi
        check "R124-r golden I-$s $th within threshold" \
            python3 "$HERE/golden-diff.py" "$GOLD/$s-$th.png" "$OUT/$s-$th.png" --masks "$OUT/masks-$s-$th.json" --out "$OUT/$s-$th-diff.png"
    done
done

# Live drafting state, dark + light (the agent hangs after one delta).
touch "$T/hang"
for th in dark light; do
    [[ $th == light ]] && light
    notoasts
    vkey press:F8; sleep 1.5; vkey release:F8
    eventually 15 is "vq '.state + \" \" + (if (.draft | length) > 0 then \"text\" else \"empty\" end)'" "drafting text"
    sleep 0.3
    shot "live-drafting-$th"
    check "R124-r live drafting $th: composer shows the draft so far, Send and Paste only insensitive" \
        bash -c "vinfo | jq -e '.state == \"drafting\" and (.draft | length) > 0 and (.send_enabled | not) and (.paste_enabled | not)' >/dev/null"
    close_composer
    [[ $th == light ]] && dark
done
rm -f "$T/hang"

# I.27: 1280 px and Text scale 200 %, the m40 helpers.
noscroll() { local h; h=$(ui hscroll); [[ $(jq .count <<<"$h") == 0 && $(jq .window.width <<<"$h") == "$1" ]]; }
noclip() { ui clipped >"$T/clip.json"; [[ $(jq .count "$T/clip.json") == 0 ]] || { jq -c '.labels[:5][] | [.text, .classes, .allocation, .natural, .why]' "$T/clip.json" | sed 's/^/    clipped: /'; false; }; }
layout() { rpc debug.ui "{\"op\":\"layout\",\"width\":$1,\"height\":$2,\"text_scale\":${3:-1.0}}" >/dev/null; sleep 1.2; }
export T
export -f noscroll noclip
for mode in 1280 200; do
    if [[ $mode == 1280 ]]; then layout 1280 800; W=1280; else layout 1440 900 2.0; W=1440; fi
    for th in dark light; do
        [[ $th == light ]] && light
        rade focus "$planner" >/dev/null; sleep 0.5
        for s in a c e; do
            notoasts
            vop scene "\"arg\":\"$s\"" >/dev/null; sleep 1
            check "R124-r I.27 $mode $th I-$s: no horizontal scroll (window $W px), no label clipped" \
                bash -c "noscroll $W && noclip"
            shot "i27-$mode-$s-$th"
        done
        vop scene '"arg":"off"' >/dev/null
        [[ $th == light ]] && dark
    done
done
layout 1440 900
fi


# What the fakes saw (the start records hold the app's env: left out).
cp "$FAKE_STT_LOG" "$OUT/stt.log" 2>/dev/null
jq -c 'select(.turn != null)' "$FAKE_VOICE_LOG" >"$OUT/voice-turns.log" 2>/dev/null
xxd "$SINK_LOG" 2>/dev/null | tail -4 >"$OUT/sink-tail.txt"

echo
echo "m46: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
