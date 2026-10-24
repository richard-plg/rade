#!/usr/bin/env bash
# Stand-in coding Agent for E2E (deterministic, no network): takes its prompt
# as arguments (Preset prompt = arg) or reads it from the Terminal (prompt =
# type), replies in upper case, and reports Agent Status the way installed
# hooks do (`rade status … --hook`, session id on stdin).
#   "ask …"  → prints a question and reports `waiting`
#   "quit"   → exits with code 3
#   "osc <t>"      → done, then its own OSC 9 <t> (like Codex's turn-end notification)
#   "oscfirst <t>" → OSC 777 <t> first, then `waiting` (like Claude's permission prompt)
#   "spin"   → busy forever with a spinner (screen keeps changing, no hook)
#   "add …"  → appends "<prompt> by $RADE_PANE_ID" to README.md in its cwd (a Fan-out Agent)
#   "run:<script>" → runs the script (the prompt's first line) in its cwd (M29)
#   a Fan-out Judge prompt → prints RANKING: $FAKE_RANKING and WINNER: $FAKE_WINNER
#   "codexlimit <used> <resets_at> [blocked]" → appends a Codex rollout `token_count` line (Spark 5-hour
#        at <used> %, weekly 33 %) to $CODEX_HOME/sessions/…/rollout-fake-<sid>.jsonl, then `done` (M39)
#   "transcript <input> <output>" → appends a Claude transcript line (message.usage, cwd = $PWD) to
#        $CLAUDE_CONFIG_DIR/projects/<cwd slug>/<sid>.jsonl, then `done` (M39)
#   "statusline <file>" → pipes the statusline JSON in <file> into `rade status --statusline`, then `done` (M39)
#   $FAKE_PROMPTS set: every prompt is appended there as "<pane> <prompt>"
# $FAKE_ENV (a file) is sourced before each prompt: settings a test changes later.
# FAKE_NO_HOOKS=1: reports nothing (an Agent without hooks, R10.2 fallbacks).
#   --resume <id> → prints "resumed <id>" and waits for a prompt (Session Restore)
# Lines pasted together (arriving within 0.3 s) are one prompt.
#   --model <m>, other --flags → echoed with $FAKE_CONFIG_DIR (recorded-invocation restore)
# $FAKE_AGENT: the Preset its session-start hook names (default `fake`; M38 runs it as `claude`).
# $FAKE_ARGV set: each start appends "<pane> <args>" there (M38: a refused invocation never ran).
[[ -n ${FAKE_ARGV:-} ]] && echo "${RADE_PANE_ID:-} $*" >>"$FAKE_ARGV"
sid=fake-$$ flags=() prompt=
while (($#)); do
    case $1 in
        --session-id) sid=$2; shift 2 ;;
        --resume) sid=$2; shift 2; echo "resumed $sid" ;;
        --model) flags+=("$1" "$2"); shift 2 ;;
        -*) flags+=("$1"); shift ;;
        *) prompt+=${prompt:+ }$1; shift ;;
    esac
done
echo "fake-agent session $sid"
echo "fake-agent config=${FAKE_CONFIG_DIR:-none} flags=${flags[*]}"
hook() { [[ -n ${FAKE_NO_HOOKS:-} ]] || printf '{"session_id":"%s"}' "$sid" | rade status "$@" --hook; }
hook --agent "${FAKE_AGENT:-fake}" # like a SessionStart hook
# Usage logs the way codex-cli 0.163.0 / Claude Code 2.1.309 write them (M39).
toks=0
codex_line() { # <used> <resets_at> [blocked]
    local dir=${CODEX_HOME:-$HOME/.codex}/sessions/$(date -u +%Y/%m/%d) now reached=null
    local f=$dir/rollout-fake-$sid.jsonl
    now=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
    [[ ${3:-} == blocked ]] && reached='"primary"'
    mkdir -p "$dir"
    [[ -f $f ]] || printf '{"timestamp":"%s","type":"session_meta","payload":{"cli_version":"0.163.0","id":"%s","cwd":"%s"}}\n{"timestamp":"%s","type":"turn_context","payload":{"cwd":"%s","model":"gpt-5.3-codex-spark"}}\n' \
        "$now" "$sid" "$PWD" "$now" "$PWD" >>"$f"
    toks=$((toks + 1000))
    printf '{"timestamp":"%s","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":%d,"cached_input_tokens":0,"output_tokens":100,"total_tokens":%d}},"rate_limits":{"limit_id":"codex_bengalfox","limit_name":"GPT-5.3-Codex-Spark","primary":{"used_percent":%s,"window_minutes":300,"resets_at":%s},"secondary":{"used_percent":33.0,"window_minutes":10080,"resets_at":%s},"credits":null,"plan_type":"prolite","rate_limit_reached_type":%s}}}\n' \
        "$now" "$toks" "$((toks + 100))" "$1" "$2" "$(($2 + 500000))" "$reached" >>"$f"
}
claude_line() { # <input> <output>
    local slug=${PWD//\//-} now
    local dir=${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects/$slug
    now=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
    mkdir -p "$dir"
    printf '{"type":"assistant","cwd":"%s","sessionId":"%s","version":"2.1.309","timestamp":"%s","message":{"model":"claude-opus-5-5","id":"msg_%s_%s","role":"assistant","usage":{"input_tokens":%d,"cache_creation_input_tokens":0,"cache_read_input_tokens":50000,"output_tokens":%d}}}\n' \
        "$PWD" "$sid" "$now" "$sid" "$RANDOM$RANDOM" "$1" "$2" >>"$dir/$sid.jsonl"
}
while :; do
    if [[ -z $prompt ]]; then
        printf '> '
        IFS= read -r prompt || exit 0
        # A pasted multi-line prompt is one prompt (like a real TUI).
        while IFS= read -r -t 0.3 more; do prompt+=" $more"; done
    fi
    hook running
    # Settings a test changes while the app runs.
    [[ -n ${FAKE_ENV:-} && -f $FAKE_ENV ]] && source "$FAKE_ENV"
    echo "thinking about: $prompt"
    [[ -n ${FAKE_PROMPTS:-} ]] && echo "${RADE_PANE_ID:-} $prompt" >>"$FAKE_PROMPTS"
    sleep 0.5
    case $prompt in
        ask*) echo "QUESTION: which file should I use"; hook waiting ;;
        quit) exit 3 ;;
        osc\ *) echo "REPLY: ${prompt#osc }"; hook done; printf '\e]9;%s\a' "${prompt#osc }" ;;
        oscfirst\ *) printf '\e]777;notify;Fake;%s\a' "${prompt#oscfirst }"; sleep 0.3; hook waiting ;;
        *"WINNER: <"*) echo "Both work; one is tidier."; echo "RANKING: ${FAKE_RANKING:-}"
            echo "WINNER: ${FAKE_WINNER:-none}"; hook done ;;
        add*) echo "$prompt by $RADE_PANE_ID" >>README.md; echo "REPLY: ADDED"; hook done ;;
        run:*) "$(head -1 <<<"${prompt#run:}")"; echo "REPLY: RAN"; hook done ;;
        codexlimit\ *) codex_line ${prompt#codexlimit }; echo "REPLY: LOGGED"; hook done ;;
        transcript\ *) claude_line ${prompt#transcript }; echo "REPLY: LOGGED"; hook done ;;
        statusline\ *) rade status --statusline <"${prompt#statusline }"; echo "REPLY: STATUSLINE"; hook done ;;
        spin) i=0; while :; do printf '\rworking %d' $((i++)); sleep 0.3; done ;;
        *) echo "REPLY: ${prompt^^}"; hook done ;;
    esac
    prompt=
done
