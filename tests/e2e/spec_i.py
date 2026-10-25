#!/usr/bin/env python3
"""R124-r: measure the Voice Composer (scenes I-a, I-c, I-e) against
docs/internals/design/spec/i-voice.md rows I.1–I.25, the same way as
spec_f.py / spec_h.py: each row read with the hidden `debug.ui geometry` op
(test mode, ADR-0021) and `debug.voice info`, compared with the sheet's value
and tolerance (spec README: sizes ±1 px, font size ±0.5 px, family / weight /
colors / texts exact). I.26 is docs/internals/design/contrast.py and I.27 is
m46.sh's 1280 px / Text scale 200 % pass (the m40 helpers); the glow, code
spans, changed marks, unsure mark and italic of the golden-only parts are
the goldens of m46.sh.

Usage: spec_i.py <socket> <tokens.toml> <style.css> <dark|light> <a|c|e> <primaries before>

The scene (`debug.voice {"op":"scene","arg":"<a|c|e>"}`) must be shown on the
Agent Pane `planner` (left of `tests`, Worktree checkout-v2). Prints one
PASS / FAIL line per row; exit 1 when any row fails.
"""
import json
import re
import socket
import sys
import tomllib

SOCK, TOKENS, STYLE, THEME, SCENE, PRIMARIES = sys.argv[1:7]
TOK = tomllib.load(open(TOKENS, "rb"))[THEME]
CSS = open(STYLE).read()
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


def true(rid, what, cond):
    row(rid).append((bool(cond), what))


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


def css_rule(selector):
    """The declarations of the first `selector { … }` rule in style.css."""
    m = re.search(r"(?m)^" + re.escape(selector) + r"\s*\{([^}]*)\}", CSS)
    return m.group(1) if m else ""


def main():
    info = rpc("debug.voice", {"op": "info"})
    a, c, e = SCENE == "a", SCENE == "c", SCENE == "e"
    true("I.1", f"scene {SCENE} open", info["open"])

    # I.1 box
    box = geo("voice-composer")
    pad("I.1", "box", box, [1, 1, 1, 1], "border")
    near("I.1", "radius", box["radius"], 8, 0.01)
    eq("I.1", "background", box["background"], tok("bg2"))
    true("I.1", f"shadow drawn and holds the token ({box['shadow']})",
         bool(box["shadow"]) and "0 20px 60px " + tok("shadow") in box["shadow"])
    if not a:
        for side in ("top", "right", "bottom", "left"):
            eq("I.1", f"border {side}", box["border_color"][side], tok("line2"))

    # I.2 place: inside the Pane's frame (its body is the frame minus the caption)
    frames = [f for f in geo("pane", all_=True)
              if f["x"] <= box["x"] and box["x"] + box["width"] <= f["x"] + f["width"]
              and f["y"] <= box["y"] and box["y"] + box["height"] <= f["y"] + f["height"]]
    true("I.2", "the composer sits inside a Pane frame", bool(frames))
    if frames:
        fr = min(frames, key=lambda f: f["width"] * f["height"])
        bd = fr.get("border") or {}
        bx, bw = fr["x"] + bd.get("left", 0), fr["width"] - bd.get("left", 0) - bd.get("right", 0)
        bbottom = fr["y"] + fr["height"] - bd.get("bottom", 0)
        near("I.2", "width = min(760, body − 20)", box["width"], min(760, bw - 20))
        near("I.2", "centred", box["x"] + box["width"] / 2, bx + bw / 2)
        near("I.2", "bottom 10 px above the body's bottom", bbottom - (box["y"] + box["height"]), 10)
        if bw - 20 <= 760:
            near("I.2", "left 10 px inside", box["x"] - bx, 10)

    # I.3 listening ring
    if a:
        for side in ("top", "right", "bottom", "left"):
            eq("I.3", f"border {side} acc-line", box["border_color"][side], tok("acc-line"))

    # I.4 header
    h = geo("voice-header")
    near("I.4", "height", h["height"], 34)
    pad("I.4", "header", h, [0, 6, 0, 12])
    near("I.4", "border.bottom", (h.get("border") or {}).get("bottom"), 1)
    eq("I.4", "border.bottom color", h["border_color"]["bottom"], tok("line"))
    dot, st = geo("voice-dot"), geo("voice-state")
    near("I.4", "gap dot → state 8", st["x"] - (dot["x"] + dot["width"]), 8)

    # I.5 dot
    near("I.5", "width", dot["width"], 8)
    near("I.5", "height", dot["height"], 8)
    near("I.5", "radius", dot["radius"], 4)
    eq("I.5", "background", dot["background"], tok("err" if a else "acc"))

    # I.6 state
    font("I.6", "state", st, INTER, 12, 600)
    eq("I.6", "color", st["color"], tok("fg"))
    eq("I.6", "label", st["label"], "Listening" if a else "Ready")

    # I.7 time
    t = geo("voice-time")
    font("I.7", "time", t, MONO, 11)
    eq("I.7", "color", t["color"], tok("fg3"))
    eq("I.7", "label", t["label"], {"a": "0:06", "c": "0:11 · 3.2 s", "e": "revision 2 · 1.9 s"}[SCENE])

    # I.8 meter
    m = geo("voice-meter")
    eq("I.8", "visible", m.get("visible"), a)
    if a:
        near("I.8", "width", m["width"], 110)
        near("I.8", "height", m["height"], 16)

    # I.9 target
    tg = geo("voice-target")
    font("I.9", "target", tg, MONO, 11)
    eq("I.9", "color", tg["color"], tok("fg3"))
    eq("I.9", "text", info.get("target"), "→ planner · checkout-v2")

    # I.10 close
    cl = geo("voice-close")
    near("I.10", "width", cl["width"], 22)
    near("I.10", "height", cl["height"], 22)
    eq("I.10", "color", cl["color"], tok("fg3"))

    # I.11 heard text
    hd, ht, q = geo("voice-heard"), geo("voice-heard-text"), geo("voice-quote")
    pad("I.11", "heard", hd, [8, 12, 8, 12])
    font("I.11", "heard", ht, INTER, 12.5)
    eq("I.11", "color", ht["color"], tok("fg2"))
    true("I.11", "italic (style.css .voice-heard)", "font-style: italic" in css_rule(".voice-heard"))
    near("I.11", "quote icon", q["width"], 14)
    eq("I.11", "quote color", q["color"], tok("fg3"))

    # I.12 heard folded (I-c)
    if c:
        near("I.12", "one line: text height ≤ 20", min(ht["height"], 20), ht["height"], 0.01)
        eq("I.12", "folded", info["heard_folded"], True)
        tt = geo("voice-heard-toggle")
        font("I.12", "toggle", tt, MONO, 11)
        eq("I.12", "toggle label", tt["label"], "show")
        eq("I.12", "toggle color", tt["color"], tok("fg3"))

    # I.13 draft box (the draft text is drawn into the same box)
    d = geo("voice-draft")
    eq("I.13", "background", d["background"], tok("bg0"))
    pad("I.13", "draft", d, [1, 1, 1, 1], "border")
    near("I.13", "radius", d["radius"], 6, 0.01)
    pad("I.13", "draft", d, [9, 11, 9, 11])
    pad("I.13", "draft", d, [0, 12, 0, 12], "margin")
    want_border = tok("acc-line") if info.get("focus") == "draft" else tok("line")
    for side in ("top", "right", "bottom", "left"):
        eq("I.13", f"border {side} ({info.get('focus')})", d["border_color"][side], want_border)
    if c:
        eq("I.13", "the draft has the focus in I-c", info.get("focus"), "draft")

    # I.14 draft text: Inter 13.5 px, line height 1.55 (20.9 px)
    df = info.get("draft_font") or {}
    true("I.14", f"family {df.get('family')!r}", (df.get("family") or "").split(",")[0] == INTER)
    near("I.14", "size", df.get("size"), 13.5, 0.5)
    if not a:
        pitches = df.get("line_pitches") or []
        near("I.14", f"line height 1.55 × 13.5 (least pitch of {pitches})", min(pitches, default=None), 13.5 * 1.55, 1.0)
    eq("I.14", "color (style.css)", "color: @fg;" in css_rule(".voice-draft"), True)

    # I.15 code spans
    tags = info["draft_tags"]
    ticks = info["draft"].count("`") // 2
    if a:
        eq("I.15", "no draft yet", info["draft"], "")
    else:
        true("I.15", f"{len(tags['code'])} code ranges = {ticks} backtick pairs", ticks > 0 and len(tags["code"]) == ticks)
        raw = info["draft"].encode()
        true("I.15", "each range spans `…`", all(raw[s:s + 1] == b"`" and raw[en - 1:en] == b"`" for s, en in tags["code"]))

    # I.16 changed marks, I.17 unsure mark (I-e)
    if e:
        raw = info["draft"].encode()
        words = [raw[s:en].decode() for s, en in tags["changed"]]
        eq("I.16", "two changed ranges", len(words), 2)
        true("I.16", f"the two bullets' words {words}",
             len(words) == 2 and words[0].startswith("Keep `applyPromo()`") and words[1].startswith("No tests in this change"))
        un = [raw[s:en].decode() for s, en in tags["unsure"]]
        eq("I.17", "one unsure range", un, ['[?] "agent tests"'])
    else:
        eq("I.16", "no changed marks", tags["changed"], [])

    # I.18 placeholder (I-a)
    ph = geo("voice-placeholder")
    if a:
        eq("I.18", "text", ph["label"], "The prompt appears here when you let go of F8.")
        eq("I.18", "color", ph["color"], tok("fg3"))
        eq("I.18", "visible", ph["visible"], True)
    else:
        eq("I.18", "hidden once there is a draft", ph["visible"], False)

    # I.19 activity (I-a)
    ac = geo("voice-activity")
    if a:
        pad("I.19", "activity", ac, [7, 12, 0, 12])
        font("I.19", "activity", ac, MONO, 11)
        eq("I.19", "color", ac["color"], tok("fg3"))
        near("I.19", "min height 24", ac["height"], 24)
        eq("I.19", "text", info["activity"], "Reading the session and checkout-v2 while you speak")

    # I.20 footer
    f = geo("voice-footer")
    pad("I.20", "footer", f, [8, 8, 8, 12])
    near("I.20", "border.top", (f.get("border") or {}).get("top"), 1)
    eq("I.20", "border.top color", f["border_color"]["top"], tok("line"))
    font("I.20", "footer", f, INTER, 11.5)
    eq("I.20", "color", f["color"], tok("fg3"))
    if a:
        eq("I.20", "hints", info["hints"], ["Release F8 to draft", "Esc cancel"])
    else:
        eq("I.20", "hints", info["hints"], ["Hold F8 to revise", "Esc discard"])

    # I.21 Send, I.22 Paste only, I.23 disabled
    s, p = geo("voice-send"), geo("voice-paste")
    eq("I.21", "background", s["background"], tok("acc"))
    eq("I.21", "color", s["color"], tok("acc-fg"))
    font("I.21", "send", s, INTER, 12, 600)
    near("I.21", "height", s["height"], 26)
    near("I.21", "radius", s["radius"], 6, 0.01)
    prim = geo("suggested-action", all_=True)
    true("I.21", f"the one .suggested-action on screen ({len(prim)} open, {PRIMARIES} closed)",
         len(prim) == 1 and int(PRIMARIES) == 0 and prim[0]["classes"].count("voice-send") == 1)
    near("I.22", "height", p["height"], 26)
    eq("I.22", "background", p["background"], tok("bg2"))
    pad("I.22", "paste", p, [1, 1, 1, 1], "border")
    eq("I.22", "border color", p["border_color"]["top"], tok("line2"))
    op = 0.45 if a else 1.0
    near("I.23", "Send opacity", s["opacity"], op, 0.01)
    near("I.23", "Paste only opacity", p["opacity"], op, 0.01)

    # I.24 mic button (planner's: the first)
    b = geo("voice-button", nth=0)
    near("I.24", "width", b["width"], 22)
    near("I.24", "height", b["height"], 22)
    if a:
        true("I.24", f"recording {b['classes']}", "recording" in b["classes"])
        eq("I.24", "color", b["color"], tok("err"))
        eq("I.24", "background", b["background"], tok("rec-dim"))
    else:
        true("I.24", f"open {b['classes']}", "open" in b["classes"] and "recording" not in b["classes"])
        eq("I.24", "color", b["color"], tok("acc"))
        eq("I.24", "background", b["background"], tok("acc-dim"))

    # I.25 REC cell (I-a)
    r = geo("status-voice")
    if a:
        eq("I.25", "visible", r["visible"], True)
        font("I.25", "REC", r, MONO, 11)
        eq("I.25", "color", r["color"], tok("err"))
        eq("I.25", "label", r["label"], "REC 0:06")
    else:
        eq("I.25", "gone when nothing records", r["visible"], False)

    fail = 0
    for rid in sorted(ROWS, key=lambda x: int(x.split(".")[1])):
        bad = [m for ok, m in ROWS[rid] if not ok]
        if bad:
            fail += 1
            print(f"FAIL {rid} [{THEME} {SCENE}]: " + "; ".join(bad))
        else:
            print(f"PASS {rid} [{THEME} {SCENE}] ({len(ROWS[rid])} checks)")
    sys.exit(1 if fail else 0)


main()
