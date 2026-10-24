#!/usr/bin/env bash
# Scripted stand-in coding Agent for the docs' screenshots and recordings
# (scripts/demo/). It looks like a coding agent at work, edits files for real
# and reports Agent Status the way installed hooks do — but every turn is a
# "play" from scripts/demo/plays/, chosen by the prompt. Never used by tests.
#
# A play is a text file; its first line `match: <bash regex>` picks it for a
# prompt (first match in file-name order; no match → plays/default.play).
# Directives, one per line (blank lines and `#` comments skipped):
#   say <text>          ● <text>                     (the agent talking)
#   tool <Name> <arg>   ● Name(arg)                  (a tool call)
#   out <text>            └ <text>                   (its result; more `out`s indent under it)
#   add <text>          + <text> in green            (a diff line shown in a result)
#   del <text>          - <text> in red
#   think <seconds>     a spinner "Thinking…" for that long
#   pause <seconds>
#   sh <command>        runs in the Pane's directory (edit files, commit…), output hidden
#   ask <question>      prints it, status `waiting`, waits for one line of input
#   done                status `done` (the turn ends; the prompt comes back)
#   notify <text>       OSC 9 desktop notification text (like Codex's turn end)
# Env: DEMO_AGENT (the Preset the start hook names, default claude), DEMO_SPEED
# (multiplies every delay, default 1), DEMO_PLAYS (the plays directory).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
PLAYS=${DEMO_PLAYS:-$HERE/plays}
SPEED=${DEMO_SPEED:-1}
sid=demo-$$ prompt=
while (($#)); do
    case $1 in
        --session-id|--resume) sid=$2; shift 2 ;;
        --model) shift 2 ;;
        -*) shift ;;
        *) prompt+=${prompt:+ }$1; shift ;;
    esac
done
hook() { printf '{"session_id":"%s"}' "$sid" | rade status "$@" --hook 2>/dev/null; }
hook --agent "${DEMO_AGENT:-claude}"

B=$'\e[1m' D=$'\e[2m' R=$'\e[0m' G=$'\e[32m' RED=$'\e[31m' C=$'\e[36m' O=$'\e[38;5;173m'
nap() { sleep "$(awk -v s="$1" -v k="$SPEED" 'BEGIN { printf "%.3f", s * k }')"; }
# Types a line out quickly, like a streamed answer.
stream() {
    local s=$1 i
    for ((i = 0; i < ${#s}; i += 6)); do printf '%s' "${s:i:6}"; nap 0.012; done
    printf '\n'
}
spinner() {
    local n=$(awk -v s="$1" -v k="$SPEED" 'BEGIN { printf "%d", s * k * 10 }') i f='✻✶✳✢·✢✳✶'
    for ((i = 0; i < n; i++)); do printf '\r%s%s Thinking…%s ' "$O" "${f:i%8:1}" "$R"; sleep 0.1; done
    printf '\r\e[K'
}
banner() {
    printf '%s✻%s %s%s%s\n' "$O" "$R" "$B" "${DEMO_TITLE:-Demo agent}" "$R"
    printf '  %s%s · %s%s\n' "$D" "$(basename -s .git "$(git remote get-url origin 2>/dev/null || echo "$PWD")")" "$(git branch --show-current 2>/dev/null || echo -)" "$R"
}
pick() { # prompt → play file
    local f re
    for f in "$PLAYS"/*.play; do
        re=$(sed -n '1s/^match: //p' "$f")
        [[ -n $re && $1 =~ $re ]] && { echo "$f"; return; }
    done
    echo "$PLAYS/default.play"
}
play() { # file
    local line cmd arg
    while IFS= read -r -u 3 line || [[ -n $line ]]; do
        [[ -z $line || $line == \#* || $line == match:* ]] && continue
        cmd=${line%% *} arg=${line#"$cmd"}; arg=${arg# }
        case $cmd in
            say) printf '%s●%s ' "$B" "$R"; stream "$arg"; nap 0.25 ;;
            tool) printf '%s●%s %s%s%s(%s)\n' "$G" "$R" "$B" "${arg%% *}" "$R" "${arg#* }"; nap 0.35 ;;
            out) printf '  %s└%s  %s\n' "$D" "$R" "$arg"; nap 0.08 ;;
            add) printf '      %s+ %s%s\n' "$G" "$arg" "$R"; nap 0.05 ;;
            del) printf '      %s- %s%s\n' "$RED" "$arg" "$R"; nap 0.05 ;;
            think) spinner "$arg" ;;
            pause) nap "$arg" ;;
            sh) (eval "$arg") >/dev/null 2>&1 ;;
            ask) printf '\n%s●%s %s\n' "$O" "$R" "$arg"; hook waiting
                 printf '%s❯%s ' "$C" "$R"
                 while IFS= read -r -t 0.05 _; do :; done   # drop input typed before the question
                 IFS= read -r _ || exit 0; hook running; printf '\n' ;;
            notify) printf '\e]9;%s\a' "$arg" ;;
            done) hook done ;;
        esac
    done 3<"$1"
}
banner
while :; do
    if [[ -z $prompt ]]; then
        printf '\n%s❯%s ' "$C" "$R"
        IFS= read -r prompt || exit 0
        while IFS= read -r -t 0.3 more; do prompt+=" $more"; done
    else
        printf '%s❯%s %s\n' "$C" "$R" "$prompt"
    fi
    printf '\n'
    hook running
    play "$(pick "$prompt")"
    prompt=
done
