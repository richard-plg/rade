#!/usr/bin/env bash
# The narrated YouTube cut of a demo scene: Kokoro TTS reads
# narration/<scene>.tsv, the scene runs with NARRATE=1 (its beats last as
# long as their lines, marks note when each line starts), then cut.py adds
# voice, captions, sound effects, music and an end card.
#   scripts/demo/youtube.sh tour|grid   → target/demo/youtube/<scene>.mp4
# NO_RUN=1 reuses the last narrated recording; VOICE (af_heart) and SPEED (1.0)
# pick the Kokoro voice; MUSIC is a track to put under it (default: "Lofi Hip
# Hop Loop" by omfgdude, CC0, https://opengameart.org/content/lofi-hip-hop-loop;
# MUSIC= for none). Downloads the model (~350 MB) and the track into target/.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
scene=${1:?tour or grid}
case $scene in
    tour) recs=(workspaces overview files) ;;
    grid) recs=(grid) ;;
    *) echo "no narration for '$scene'" >&2; exit 2 ;;
esac
TTS=$ROOT/target/tts
mkdir -p "$TTS" "$ROOT/target/music"
for f in kokoro-v1.0.onnx voices-v1.0.bin; do
    [[ -s $TTS/$f ]] || curl -sSfL -o "$TTS/$f" "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/$f"
done
if [[ ! -x $TTS/venv/bin/python ]]; then
    uv venv -q --python 3.12 "$TTS/venv"
    VIRTUAL_ENV=$TTS/venv uv pip install -q kokoro-onnx soundfile numpy pillow cairosvg
fi
[[ -s $TTS/icon.png ]] || "$TTS/venv/bin/python" -c "import cairosvg, sys; cairosvg.svg2png(url=sys.argv[1], write_to=sys.argv[2], output_width=256)" \
    "$ROOT/packaging/dev.rade.App.svg" "$TTS/icon.png"
export MUSIC=${MUSIC-$ROOT/target/music/LofiLoop_1.ogg}
[[ -z $MUSIC || -s $MUSIC ]] || curl -sSfL -o "$MUSIC" https://opengameart.org/sites/default/files/LofiLoop_1.ogg

echo "== narration ($scene)"
TTS_MODELS=$TTS "$TTS/venv/bin/python" -I "$ROOT/scripts/demo/tts.py" "$ROOT/scripts/demo/narration/$scene.tsv" \
    "$ROOT/target/demo/narration/$scene" "${VOICE:-af_heart}" "${SPEED:-1.0}"
if [[ -z ${NO_RUN:-} ]]; then
    echo "== scene"
    NARRATE=1 NO_BUILD=${NO_BUILD:-1} "$ROOT/scripts/demo/$scene.sh" >"$ROOT/target/demo/$scene-narrated.log" 2>&1 ||
        { tail -20 "$ROOT/target/demo/$scene-narrated.log"; exit 1; }
fi
echo "== cut"
"$TTS/venv/bin/python" -I "$ROOT/scripts/demo/cut.py" "$scene" "${recs[@]}"
