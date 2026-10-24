# Shared setup for the docs' demo scenes (scripts/demo/*.sh): the E2E harness
# (tests/e2e/lib.sh: temp HOME, private D-Bus, headless Weston, the real app
# in test mode) plus demo Agents, a demo identity and a frame recorder.
# Scene files set NAME and source this; output → target/demo/<NAME>/.
DEMO=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=${OUT:-$DEMO/../../target/demo/$NAME}
export WESTON_WIDTH=${WESTON_WIDTH:-1440}
# tests/e2e/lib.sh finds the repository two levels above the calling script.
source "$DEMO/../../tests/e2e/lib.sh"
AGENT=$DEMO/agent.sh

# A believable prompt and git identity (fake, like every name in the scenes).
cat >"$HOME/.bashrc" <<'RC'
PS1='\[\e[36m\]\W\[\e[0m\] \[\e[2m\]$\[\e[0m\] '
alias ls='ls --color=auto'
RC
export GIT_AUTHOR_NAME="Sam Rivera" GIT_AUTHOR_EMAIL=sam@acme.dev
export GIT_COMMITTER_NAME="Sam Rivera" GIT_COMMITTER_EMAIL=sam@acme.dev
git config --global user.name "Sam Rivera"
git config --global user.email sam@acme.dev
git config --global init.defaultBranch main
git config --global commit.gpgsign false
git config --global advice.detachedHead false

mkdir -p "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' $AGENT"
[agents.codex]
command = "env DEMO_AGENT=codex DEMO_TITLE='Codex (demo)' $AGENT"
C

id() { jq -r .pane; }
ui() { rpc debug.ui "{\"op\": \"$1\"${2:+, \"arg\": \"$2\"}}"; }
pane_status() { rade list --json | jq -r --arg p "$1" '.panes[] | select(.pane==$p) | .status'; }
export -f ui rpc pane_status
light() { rade debug action color_scheme_light >/dev/null; sleep 1; }
dark() { rade debug action color_scheme_dark >/dev/null; sleep 1; }
notoasts() { rpc debug.ui '{"op":"toasts_clear"}' >/dev/null; sleep 0.6; }
# `still <name>`: dark and light screenshots → $OUT/<name>-dark.png, -light.png.
still() {
    dark; shot "$1-dark"
    light; shot "$1-light"
    dark
}
# Types text into a Pane a few characters at a time (for recordings).
typein() { # pane, text, [--enter]
    local p=$1 s=$2 i
    for ((i = 0; i < ${#s}; i += 3)); do rade send "$p" "${s:i:3}" >/dev/null; sleep 0.05; done
    [[ ${3:-} == --enter ]] && { sleep 0.3; rade send "$p" "" --enter >/dev/null; }
}

# ---- Recorder: screenshots in a loop → MP4 + GIF ------------------------------
# rec_start <name>; …drive the app…; rec_stop  → $OUT/<name>.mp4, <name>.gif
REC= REC_PID=
rec_start() {
    REC=$OUT/rec-$1
    rm -rf "$REC" && mkdir -p "$REC" && touch "$REC/.on"
    ( while [[ -e $REC/.on ]]; do
          f=$REC/$(date +%s%N).png
          rade debug screenshot "$f" >/dev/null 2>&1 || rm -f "$f"
      done ) &
    REC_PID=$!
    sleep 0.3
}
rec_stop() {
    local name=${REC##*/rec-}
    sleep 0.5
    rm -f "$REC/.on"; wait "$REC_PID" 2>/dev/null
    python3 -I - "$REC" >"$REC/list.txt" <<'PY'
import os, sys
d = sys.argv[1]
fs = sorted(f for f in os.listdir(d) if f.endswith(".png"))
for a, b in zip(fs, fs[1:] + [None]):
    dur = (int(b[:-4]) - int(a[:-4])) / 1e9 if b else 0.5
    print(f"file '{os.path.join(d, a)}'\nduration {dur:.3f}")
print(f"file '{os.path.join(d, fs[-1])}'")
PY
    ffmpeg -loglevel error -y -f concat -safe 0 -i "$REC/list.txt" \
        -vf "fps=30,format=yuv420p" -c:v libx264 -crf 22 -preset slow -movflags +faststart "$OUT/$name.mp4"
    ffmpeg -loglevel error -y -i "$OUT/$name.mp4" \
        -vf "fps=8,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff:max_colors=128[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
        "$OUT/$name.gif"
    echo "  rec ${OUT#$ROOT/}/$name.mp4 ($(ls "$REC"/*.png | wc -l) frames) + .gif"
}
