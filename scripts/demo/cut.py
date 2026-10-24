"""The YouTube cut of a narrated scene (run by scripts/demo/youtube.sh): the
scene's recordings in order, the narration lines placed at the scene's marks
(never overlapping: a line waits for the one before it), on-screen captions,
a few sound effects, background music ducked under the voice, an end card.
Usage: cut.py <scene> <recording>…   (with the venv youtube.sh makes)
Reads target/demo/<scene>-narrated/ (recordings + rec-*/marks.txt) and
target/demo/narration/<scene>/ (WAVs); writes target/demo/youtube/<scene>.mp4."""
import os
import subprocess
import sys

import numpy as np
import soundfile as sf
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEMO = os.path.join(ROOT, "scripts", "demo")
FONTS = os.path.join(ROOT, "crates", "rade-app", "assets", "fonts")
RATE = 48000
END = 4.5  # end card, seconds
SFX = {"needs": "ding", "ask": "ding", "open": "whoosh", "answer": "typing"}

scene, recordings = sys.argv[1], sys.argv[2:]
src = os.path.join(ROOT, "target", "demo", f"{scene}-narrated")
narr = os.path.join(ROOT, "target", "demo", "narration", scene)
out = os.path.join(ROOT, "target", "demo", "youtube")
os.makedirs(out, exist_ok=True)
music_path = os.environ.get("MUSIC", "")


def probe(path):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
                       capture_output=True, text=True, check=True)
    return float(r.stdout)


lines = []
for line in open(os.path.join(DEMO, "narration", f"{scene}.tsv")):
    if line.strip() and not line.startswith("#"):
        label, caption, text = line.rstrip("\n").split("\t")
        lines.append((label, caption, text))
durs = dict((l.split()[0], float(l.split()[1])) for l in open(os.path.join(narr, "durations.txt")))

# Marks → seconds on the cut's timeline (0 = the first recording's first frame).
marks, offset = {}, 0.0
for rec in recordings:
    d = os.path.join(src, f"rec-{rec}")
    t0 = min(int(f[:-4]) for f in os.listdir(d) if f.endswith(".png"))
    for m in open(os.path.join(d, "marks.txt")) if os.path.exists(os.path.join(d, "marks.txt")) else []:
        ns, label = m.split()
        marks.setdefault(label, offset + (int(ns) - t0) / 1e9)
    offset += probe(os.path.join(src, f"{rec}.mp4"))
video = offset

place, t = {}, 0.0
for label, _, _ in lines:
    if label == "outro":
        continue
    if label not in marks:
        print(f"  no mark for '{label}': line dropped")
        continue
    place[label] = max(marks[label], t + 0.25)
    t = place[label] + durs[label]
    late = place[label] - marks[label]
    print(f"  {label:9} mark {marks[label]:6.2f}  voice {place[label]:6.2f}–{t:6.2f}" + (f"  (+{late:.1f}s late)" if late > 0.5 else ""))
hold = max(0.0, t + 0.8 - video)
card = video + hold
place["outro"] = card + 0.7
total = card + END
print(f"  video {video:.1f}s, hold {hold:.1f}s, total {total:.1f}s")


# ---- Audio -----------------------------------------------------------------------
def load(path):
    a, r = sf.read(path, dtype="float32", always_2d=True)
    a = a.mean(axis=1)
    if r != RATE:
        x = np.arange(int(len(a) * RATE / r)) * r / RATE
        a = np.interp(x, np.arange(len(a)), a).astype(np.float32)
    return a


def put(track, clip, at, gain=1.0):
    i = int(at * RATE)
    n = min(len(clip), len(track) - i)
    if n > 0:
        track[i:i + n] += clip[:n] * gain


def env(n, attack, decay):
    t = np.arange(n) / RATE
    return np.minimum(1, t / attack) * np.exp(-t / decay)


def ding():
    t = np.arange(int(0.9 * RATE)) / RATE
    tone = 0.6 * np.sin(2 * np.pi * 1318.5 * t) + 0.4 * np.sin(2 * np.pi * 1975.5 * t)
    return (tone * env(len(t), 0.004, 0.22)).astype(np.float32) * 0.22


def whoosh():
    n = int(0.55 * RATE)
    noise = np.random.default_rng(7).standard_normal(n)
    out, y = np.zeros(n), 0.0
    for i in range(n):  # a one-pole low-pass whose cutoff rises and falls
        k = 0.02 + 0.25 * np.sin(np.pi * i / n) ** 2
        y += k * (noise[i] - y)
        out[i] = y
    return (out * np.sin(np.pi * np.arange(n) / n) ** 1.5).astype(np.float32) * 0.5


def typing(keys=5):
    rng = np.random.default_rng(3)
    out = np.zeros(int((0.13 * keys + 0.1) * RATE), dtype=np.float32)
    for k in range(keys):
        n = int(0.025 * RATE)
        click = rng.standard_normal(n) * env(n, 0.0005, 0.004)
        click = np.diff(click, prepend=0)  # brighter
        put(out, click.astype(np.float32), 0.13 * k + rng.uniform(0, 0.03), 0.25)
    return out


n = int(total * RATE)
voice, fx = np.zeros(n, np.float32), np.zeros(n, np.float32)
for label, at in place.items():
    put(voice, load(os.path.join(narr, f"{label}.wav")), at)
for label, kind in SFX.items():
    if label in marks:
        put(fx, {"ding": ding, "whoosh": whoosh, "typing": typing}[kind](), marks[label])

mix = voice + fx
if music_path:
    m = load(music_path)
    m = np.tile(m, int(np.ceil(n / len(m))))[:n]
    # Duck under the voice, with 0.25 s ramps.
    talking = np.convolve((np.abs(voice) > 0.01).astype(np.float32), np.ones(int(0.25 * RATE)), "same") > 0
    gain = np.where(talking, 0.06, 0.16).astype(np.float32)
    k = int(0.25 * RATE)
    gain = np.convolve(gain, np.ones(k) / k, "same").astype(np.float32)
    fade = np.ones(n, np.float32)
    fade[: int(1.5 * RATE)] = np.linspace(0, 1, int(1.5 * RATE))
    fade[-int(3 * RATE):] = np.linspace(1, 0, int(3 * RATE))
    mix += m / (np.abs(m).max() + 1e-9) * gain * fade
mix /= max(1.0, np.abs(mix).max() / 0.89)
wav = os.path.join(out, f"{scene}.wav")
sf.write(wav, mix, RATE)


# ---- Captions (ASS) and the end card ------------------------------------------
def ts(s):
    return f"{int(s // 3600)}:{int(s % 3600 // 60):02}:{s % 60:05.2f}"


caps = [(place[l], c) for l, c, _ in lines if c and l in place and l != "outro"]
ass = os.path.join(out, f"{scene}.ass")
with open(ass, "w") as f:
    f.write("[Script Info]\nScriptType: v4.00+\nPlayResX: 1440\nPlayResY: 900\n\n"
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, "
            "Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, "
            "Alignment, MarginL, MarginR, MarginV, Encoding\n"
            "Style: Cap,Inter SemiBold,36,&H00FFFFFF,&H00FFFFFF,&H28141A1F,&H28141A1F,0,0,0,0,100,100,0.5,0,3,16,0,2,40,40,130,1\n\n"
            "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n")
    for i, (at, text) in enumerate(caps):
        label = next(l for l, c, _ in lines if c == text)
        end = min(caps[i + 1][0] - 0.1 if i + 1 < len(caps) else card, at + durs[label] + 1.2)
        f.write(f"Dialogue: 0,{ts(at)},{ts(end)},Cap,,0,0,0,,{{\\fad(180,180)}}{text}\n")

img = Image.new("RGB", (1440, 900), (11, 15, 20))
d = ImageDraw.Draw(img)
icon = Image.open(os.path.join(ROOT, "target", "tts", "icon.png")).convert("RGBA").resize((168, 168))
img.paste(icon, ((1440 - 168) // 2, 190), icon)
for text, font, size, y, color in [
    ("RADE", "Inter-SemiBold.ttf", 104, 400, (240, 244, 248)),
    ("Agentic Development Environment for Linux", "Inter-Regular.ttf", 40, 530, (170, 180, 190)),
    ("github.com/richard-plg/rade", "JetBrainsMono-Variable.ttf", 36, 620, (45, 212, 191)),
]:
    ft = ImageFont.truetype(os.path.join(FONTS, font), size)
    w = d.textlength(text, font=ft)
    d.text(((1440 - w) / 2, y), text, font=ft, fill=color)
endcard = os.path.join(out, f"{scene}-end.png")
img.save(endcard)

# ---- Video -----------------------------------------------------------------------
cmd = ["ffmpeg", "-loglevel", "error", "-y"]
for rec in recordings:
    cmd += ["-i", os.path.join(src, f"{rec}.mp4")]
cmd += ["-loop", "1", "-t", str(END), "-framerate", "30", "-i", endcard, "-i", wav]
k = len(recordings)
graph = "".join(f"[{i}:v]setsar=1[s{i}];" for i in range(k))
graph += "".join(f"[s{i}]" for i in range(k)) + f"concat=n={k}:v=1:a=0[c];"
graph += f"[c]tpad=stop_mode=clone:stop_duration={hold:.2f}[h];"
graph += f"[{k}:v]format=yuv420p,setsar=1,fade=in:0:12[e];"
graph += f"[h][e]concat=n=2:v=1:a=0[j];[j]ass={ass}:fontsdir={FONTS}[v];"
graph += f"[{k + 1}:a]loudnorm=I=-14:TP=-1.5:LRA=11[a]"
dest = os.path.join(out, f"{scene}.mp4")
cmd += ["-filter_complex", graph, "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-crf", "18", "-preset", "slow",
        "-pix_fmt", "yuv420p", "-r", "30", "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-movflags", "+faststart",
        "-t", f"{total:.2f}", dest]
subprocess.run(cmd, check=True)
print(f"  → {os.path.relpath(dest, ROOT)} ({probe(dest):.1f}s)")

# ---- Thumbnail for the README: a frame, a play button, the length ----------------
THUMB = {"tour": 36.5, "grid": 24.0}
if scene in THUMB:
    frame = os.path.join(out, f"{scene}-frame.png")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-ss", str(THUMB[scene]), "-i", dest, "-frames:v", "1", frame],
                   check=True)
    im = Image.open(frame).convert("RGBA")
    shade = Image.new("RGBA", im.size, (0, 0, 0, 70))
    im = Image.alpha_composite(im, shade)
    d = ImageDraw.Draw(im)
    cx, cy, r = 720, 450, 64
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(204, 0, 0, 235))
    d.polygon([(cx - 20, cy - 30), (cx - 20, cy + 30), (cx + 32, cy)], fill=(255, 255, 255, 255))
    secs = round(probe(dest))
    label = f"Watch on YouTube · {secs // 60}:{secs % 60:02} · narrated"
    ft = ImageFont.truetype(os.path.join(FONTS, "Inter-SemiBold.ttf"), 30)
    w = d.textlength(label, font=ft)
    d.rounded_rectangle((cx - w / 2 - 22, cy + r + 26, cx + w / 2 + 22, cy + r + 82), 12, fill=(11, 15, 20, 220))
    d.text((cx - w / 2, cy + r + 36), label, font=ft, fill=(240, 244, 248, 255))
    im.convert("RGB").save(os.path.join(out, f"youtube-{scene}.png"))
    os.remove(frame)
    print(f"  → target/demo/youtube/youtube-{scene}.png")
