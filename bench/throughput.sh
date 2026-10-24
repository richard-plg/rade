#!/usr/bin/env bash
# R4 throughput: time `cat` of a 100 MB text file inside (a) standalone Ghostty
# and (b) rade-app (a right split opened with `rade spawn --cmd`).
# Timing is taken inside the terminal, around `cat` only, so app startup
# is excluded. Needs a live Wayland session (or WAYLAND_DISPLAY pointing at
# a compositor). Usage: bench/throughput.sh [runs=3] [size_mb=100]
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUNS=${1:-3}
SIZE_MB=${2:-100}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

DATA=$WORK/data.txt
head -c "${SIZE_MB}M" < <(base64 -w 100 /dev/urandom) > "$DATA"

# Writes start/end timestamps (ns) around cat, then exits.
INNER="date +%s%N > $WORK/start; cat $DATA; date +%s%N > $WORK/end"

wait_for_end() { # pid
    for _ in $(seq 1 1200); do
        [[ -s $WORK/end ]] && return 0
        kill -0 "$1" 2>/dev/null || { [[ -s $WORK/end ]]; return; }
        sleep 0.1
    done
    return 1
}

run_one() { # label, command...
    local label=$1; shift
    rm -f "$WORK/start" "$WORK/end"
    "$@" >/dev/null 2>&1 &
    local pid=$!
    if wait_for_end "$pid"; then
        local ms=$(( ($(cat "$WORK/end") - $(cat "$WORK/start")) / 1000000 ))
        printf '%-10s %6d ms  %6.1f MB/s\n' "$label" "$ms" "$(echo "$SIZE_MB * 1000 / $ms" | bc -l)"
    else
        echo "$label TIMEOUT"
    fi
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

# rade-app with throwaway state; the command runs in a half-width split.
rade_app_run() {
    local d=$WORK/rade
    rm -rf "$d" && mkdir -p "$d"
    export XDG_DATA_HOME=$d XDG_CONFIG_HOME=$d RADE_SOCKET=$d/rade.sock
    "$ROOT/target/release/rade-app" &
    local app=$!
    trap 'kill $app 2>/dev/null' TERM
    for _ in $(seq 100); do "$ROOT/target/release/rade" list >/dev/null 2>&1 && break; sleep 0.1; done
    "$ROOT/target/release/rade" spawn --split right --cmd "sh -c '$INNER'" >/dev/null
    wait "$app"
}

echo "size=${SIZE_MB}MB runs=$RUNS ghostty=$(ghostty --version | head -1)"
for i in $(seq 1 "$RUNS"); do
    run_one ghostty ghostty --wait-after-command=false -e sh -c "$INNER"
    run_one rade-app rade_app_run
done
