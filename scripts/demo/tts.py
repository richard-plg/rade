"""Narration for the YouTube cuts (scripts/demo/youtube.sh): reads
narration/<scene>.tsv and writes one WAV per line with Kokoro TTS, plus
<label> <seconds> lines in durations.txt. Run with the venv youtube.sh makes.
Usage: tts.py <scene.tsv> <out dir> [voice] [speed]"""
import os
import sys

import soundfile as sf
from kokoro_onnx import Kokoro

tsv, out = sys.argv[1], sys.argv[2]
voice = sys.argv[3] if len(sys.argv) > 3 else "af_heart"
speed = float(sys.argv[4]) if len(sys.argv) > 4 else 1.0
models = os.environ["TTS_MODELS"]
k = Kokoro(os.path.join(models, "kokoro-v1.0.onnx"), os.path.join(models, "voices-v1.0.bin"))
os.makedirs(out, exist_ok=True)
with open(os.path.join(out, "durations.txt"), "w") as durs:
    for line in open(tsv):
        if not line.strip() or line.startswith("#"):
            continue
        label, _caption, text = line.rstrip("\n").split("\t")
        samples, rate = k.create(text, voice=voice, speed=speed, lang="en-us")
        sf.write(os.path.join(out, f"{label}.wav"), samples, rate)
        durs.write(f"{label} {len(samples) / rate:.2f}\n")
        print(f"  {label:9} {len(samples) / rate:5.2f}s  {text}")
