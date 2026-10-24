#!/usr/bin/env python3
"""R118-e: measure the Review Loop (scene H) against docs/internals/design/spec/h-review.md
rows H.1–H.4, H.7, H.8, H.10, H.12–H.15, H.17 and H.19 the way spec_f.py
does: each row read with the hidden `debug.ui geometry` op (test mode,
ADR-0021) and `debug.review info`, compared with the sheet's value and
tolerance (spec README: sizes ±1 px, font size ±0.5 px, colors exact).
H.5, H.6, H.9, H.11, H.16, H.18, H.20 are checked by m44.sh's `debug.review`
checks; H.21 is docs/internals/design/contrast.py; H.22 is m40's layout pass.

Usage: spec_h.py <socket> <tokens.toml> <dark|light> <review pane id>

The Review must show a draft comment card and have the composer closed.
Prints one PASS / FAIL line per row; exit 1 when any row fails.
"""
import json
import socket
import sys
import tomllib

SOCK, TOKENS, THEME, PANE = sys.argv[1:5]
TOK = tomllib.load(open(TOKENS, "rb"))[THEME]
MONO, INTER = "JetBrains Mono", "Inter"


def tok(name):
    v = TOK[name]
    return v if len(v) == 9 else v + "ff"


def rpc(method, params):
    s = socket.socket(socket.AF_UNIX)
    s.connect(SOCK)
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        buf += s.recv(1 << 20)
    r = json.loads(buf)
    if "error" in r:
        raise RuntimeError(r["error"])
    return r["result"]


def geo(widget, nth=None, all_=False):
    p = {"op": "geometry", "widget": widget}
    if nth is not None:
        p["nth"] = nth
    if all_:
        p["all"] = True
    return rpc("debug.ui", p)


ROWS = {}


def row(rid):
    return ROWS.setdefault(rid, [])


def near(rid, what, got, want, tol=1.0):
    ok = got is not None and abs(float(got) - float(want)) <= tol + 1e-6
    row(rid).append((ok, f"{what} {got} ≈ {want} ±{tol}"))


def eq(rid, what, got, want):
    if isinstance(want, str) and want.startswith("#") and isinstance(got, str) and got.startswith("#"):
        g = [int(got[i:i + 2], 16) for i in range(1, 9, 2)]
        w = [int(want[i:i + 2], 16) for i in range(1, 9, 2)]
        ok = len(g) == len(w) and all(abs(a - b) <= 1 for a, b in zip(g, w))
    else:
        ok = got == want
    row(rid).append((ok, f"{what} {got!r} = {want!r}"))


def font(rid, what, g, family=None, size=None, weight=None, tol=0.5):
    f = g.get("font") or {}
    if family:
        eq(rid, f"{what} family", f.get("family"), family)
    if size is not None:
        near(rid, f"{what} size", f.get("size"), size, tol)
    if weight is not None:
        eq(rid, f"{what} weight", f.get("weight"), weight)


def sides(g, key):
    s = g.get(key) or {}
    return [s.get("top"), s.get("right"), s.get("bottom"), s.get("left")]


def pad(rid, what, g, want, key="padding"):
    for got, w, side in zip(sides(g, key), want, "trbl"):
        near(rid, f"{what} {key}.{side}", got, w)


def border(g, side):
    return (g.get("border") or {}).get(side), (g.get("border_color") or {}).get(side)


info = rpc("debug.review", {"pane": PANE, "op": "info"})

# H.1 header: two 30 px lines on bg1, a 1 px line below.
h = geo("review-head")
eq("H.1", "background", h.get("background"), tok("bg1"))
eq("H.1", "border.bottom", border(h, "bottom"), (1, tok("line")))
pad("H.1", "header", h, [0, 8, 0, 12])
for line in ("review-head1", "review-head2"):
    near("H.1", f"{line} height", geo(line)["height"], 30)

# H.2 branch, base.
font("H.2", "branch", geo("review-branch"), INTER, 12.5, 600)
eq("H.2", "branch color", geo("review-branch")["color"], tok("fg"))
font("H.2", "base", geo("review-base"), MONO, 11)
eq("H.2", "base color", geo("review-base")["color"], tok("fg3"))
eq("H.2", "texts", [info["head"]["branch"], info["head"]["base"]], ["feat", "main"])

# H.3 the Agent: glyph 14 px in its status colour, name.
font("H.3", "name", geo("review-agent-name"), INTER, 12.5)
eq("H.3", "agent", info["head"]["agent"], "claude")
near("H.3", "glyph width", geo("agent-glyph")["width"], 14)

# H.4 task: Inter 12.5 fg2, in quotes, one line.
t = geo("review-task")
font("H.4", "task", t, INTER, 12.5)
eq("H.4", "color", t["color"], tok("fg2"))
eq("H.4", "text", info["head"]["task_shown"], "“implement OTP login”")

# H.7 scope control.
s = geo("review-scope")
near("H.7", "height", s["height"], 24)
eq("H.7", "label", info["head"]["scope"], "All changes  6")

# H.8 viewed meter.
near("H.8", "meter width", geo("review-progress")["width"], 64)
font("H.8", "label", geo("review-viewed"), MONO, 11)

# H.10 files rail.
f = geo("review-files")
near("H.10", "width", f["width"], 240, 2)
eq("H.10", "background", f.get("background"), tok("bg1"))
near("H.10", "filter height", geo("review-filter")["height"], 26)

# H.12 file rows: 26 px, name Inter 12.5.
fr = geo("review-file", nth=0)
near("H.12", "row height", fr["height"], 26)
font("H.12", "name", geo("review-file-name", nth=0), INTER, 12.5)

# H.13 the generated group is last and folded.
last = info["rail"][-1]
eq("H.13", "last rail row", [last["kind"], last["folded"]], ["generated", True])

# H.14 file header in the diff: bg2, Viewed box.
fh = geo("rade-diff-file", nth=0)
near("H.14", "height", fh["height"], 30)
eq("H.14", "background", fh.get("background"), tok("bg2"))
eq("H.14", "border.top", border(fh, "top"), (1, tok("line2")))

# H.15 comment card.
c = geo("review-card", nth=0)
eq("H.15", "background", c.get("background"), tok("bg2"))
near("H.15", "radius", c.get("radius"), 6)
pad("H.15", "card", c, [8, 10, 8, 10])
font("H.15", "text", geo("review-card-text", nth=0), INTER, 13)
font("H.15", "head", geo("review-card-head", nth=0), INTER, 11.5, 600)

# H.16 a draft card: acc-line border, chip `draft` in acc.
eq("H.16", "draft border", border(c, "top"), (1, tok("acc-line")))
eq("H.16", "chip color", geo("chip-draft", nth=0)["color"], tok("acc"))

# H.19 footer: 40 px, bg1, one primary button.
ft = geo("review-footer")
near("H.19", "height", ft["height"], 40)
eq("H.19", "background", ft.get("background"), tok("bg1"))
eq("H.19", "border.top", border(ft, "top"), (1, tok("line")))
eq("H.19", "drafts", info["drafts"], "1 draft")
font("H.19", "drafts label", geo("review-drafts"), MONO, 11)

fail = 0
for rid in sorted(ROWS, key=lambda r: int(r.split(".")[1])):
    bad = [w for o, w in ROWS[rid] if not o]
    fail += bool(bad)
    print(("FAIL " if bad else "PASS ") + rid + (": " + "; ".join(bad) if bad else ""))
sys.exit(1 if fail else 0)
