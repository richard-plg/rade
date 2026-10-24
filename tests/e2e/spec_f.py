#!/usr/bin/env python3
"""R108-i: measure the `+` menu (scene F) against docs/design/spec/f-new-agent.md
rows F.1–F.7, F.11–F.14 and F.21, the same way as spec_a.py / spec_bcd.py:
each row read with the hidden `debug.ui geometry` op (test mode, ADR-0021)
and `debug.ui new_menu` (the rows), compared with the sheet's value and
tolerance (spec README: sizes ±1 px, font size ±0.5 px, family / weight /
colors / texts exact).

Usage: spec_f.py <socket> <tokens.toml> <dark|light> <header text> <primaries before> [m39]

The menu must be open (`debug.ui {"op":"new_menu","arg":"open"}`). With
`m39` (R110-i, tests/e2e/m39.sh: scene F's usage figures in the store and
the forecast Toast shown) the usage rows F.8–F.10 and the Toast rows
F.15–F.19 are measured too, and F.14 expects m39's accounts. F.20 is checked
by m39.sh; F.22 is docs/design/contrast.py; F.23 is a golden. Prints one
PASS / FAIL line per row; exit 1 when any row fails.
"""
import json
import re
import socket
import sys
import tomllib

SOCK, TOKENS, THEME, HEADER, PRIMARIES = sys.argv[1:6]
M39 = len(sys.argv) > 6 and sys.argv[6] == "m39"
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


def usage_rows(rows, idx, items):
    """F.8–F.10: the account rows' usage figures (R110-i)."""
    detail_widgets = geo("new-menu-item-detail", all_=True)
    with_detail = [i for i, r in enumerate(rows) if r.get("detail")]

    def detail_geo(i):
        return detail_widgets[with_detail.index(i)] if i in with_detail else {}

    w = idx.get(("acme-work", "claude"))
    if w is None:
        row("F.8").append((False, "no acme-work row"))
    else:
        r = rows[w]
        eq("F.8", "detail", r["detail"], "5h 62% · wk 41%")
        true("F.8", "default ✓ and keyboard-selected", r["default"] and "selected" in items[w]["classes"])
        eq("F.8", "selected fill", items[w]["background"], tok("acc-dim"))
        eq("F.8", "2 px acc inset bar (drawn left edge)", (items[w].get("drawn") or {}).get("edge", {}).get("left"), tok("acc"))
        eq("F.8", "check color", geo("new-menu-default")["color"], tok("acc"))
        eq("F.8", "detail color", detail_geo(w).get("color"), tok("fg3"))
    t = idx.get(("acme-team", "claude"))
    if t is None:
        row("F.9").append((False, "no acme-team row"))
    else:
        eq("F.9", "detail / no check", [rows[t]["detail"], rows[t]["default"], rows[t]["hot"]],
           ["5h 8% · wk 12%", False, False])
    x = idx.get(("acme", "codex"))
    if x is None:
        row("F.10").append((False, "no acme codex row"))
    else:
        eq("F.10", "detail / default / hot", [rows[x]["detail"], rows[x]["default"], rows[x]["hot"]],
           ["Spark 5h 81%", True, True])
        eq("F.10", "detail color wait", detail_geo(x).get("color"), tok("wait"))
        hot = geo("new-menu-hot", all_=True)
        eq("F.10", "one warning-diamond (the acme row)", len(hot), 1)
        if hot:
            near("F.10", "icon 11 px", hot[0]["natural"]["width"], 11)
            eq("F.10", "icon color", hot[0]["color"], tok("wait"))
            g = items[x]
            true("F.10", "icon inside the acme row", g["y"] <= hot[0]["y"] <= g["y"] + g["height"])


def toast_rows():
    """F.15–F.19: the forecast Toast (R110-g, R103.6)."""
    t = geo("toast")
    if not t.get("found") or not t.get("visible", True):
        for rid in ("F.15", "F.16", "F.17", "F.18", "F.19"):
            row(rid).append((False, "no Toast shown"))
        return
    over = geo("toastoverlay")
    sl = geo("statusline")
    centre = over["x"] + over["width"] / 2
    near("F.15", f"centred in the Tabs column (centre {centre})", t["x"] + t["width"] / 2, centre)
    near("F.15", "bottom 18 px above the Statusline", sl["y"] - (t["y"] + t["height"]), 18)
    near("F.15", "bottom 44 px above the window bottom", sl["y"] + sl["height"] - (t["y"] + t["height"]), 44)
    pad("F.16", "toast", t, [10, 14, 10, 14])
    pad("F.16", "toast", t, [1, 1, 1, 1], "border")
    eq("F.16", "border color", t["border_color"]["top"], tok("line2"))
    near("F.16", "radius", t["radius"], 8)
    eq("F.16", "background", t["background"], tok("bg2"))
    true("F.16", f"shadow drawn ({t['shadow']})", bool(t["shadow"]))
    title = geo("toast-title")
    font("F.16", "title", title, INTER, 12.5)
    eq("F.16", "title color", title["color"], tok("fg"))
    ch = rpc("debug.ui", {"op": "chrome"})["chrome"]
    ft = (ch.get("forecast_toasts") or [{}])[0]
    ok = re.fullmatch(r"acme-work will hit its 5-hour limit in ~(35|40|45)m at this pace", ft.get("title") or "")
    true("F.17", f"title {ft.get('title')!r}", ok)
    true("F.17", f"sub {ft.get('sub')!r}",
         re.fullmatch(r"62% now · resets in 1h 1[0-2]m · acme-team is at 8%", ft.get("sub") or ""))
    true("F.17", "name in bold", (title.get("markup") or "<b>acme-work</b>").startswith("<b>acme-work</b>"))
    ic = geo("toast-icon")
    near("F.17", "warning-diamond 16 px", ic["natural"]["width"], 16)
    eq("F.17", "icon color", ic["color"], tok("wait"))
    sub = geo("toast-sub")
    font("F.17", "sub", sub, MONO, 11)
    eq("F.17", "sub color", sub["color"], tok("fg3"))
    near("F.17", "sub 2 px below the title", sub["y"] - (title["y"] + title["height"]), 2)
    eq("F.18", "action", ft.get("button"), "Next agents use acme-team")
    btns = [b for b in geo("toast button", all_=True)]
    labels = [b["label"] for b in btns]
    true("F.18", f"action button shown ({labels})", "Next agents use acme-team" in labels)
    act = next((b for b in btns if b["label"] == "Next agents use acme-team"), {})
    eq("F.18", "action background (secondary)", act.get("background"), tok("bg2"))
    eq("F.18", "action border color", (act.get("border_color") or {}).get("top"), tok("line2"))
    close = [b for b in btns if "circular" in b["classes"]]
    true("F.18", "close x", bool(close))
    if close:
        eq("F.18", "close color", close[0]["color"], tok("fg3"))
    hits = []
    for b in geo("button", all_=True):
        if b["y"] >= t["y"] and b["y"] + b["height"] <= t["y"] + t["height"] and \
                b["x"] >= t["x"] and b["x"] + b["width"] <= t["x"] + t["width"]:
            continue  # its own
        if b["width"] <= 0 or b["height"] <= 0 or b.get("opacity", 1) == 0:
            continue
        if b["x"] < t["x"] + t["width"] and t["x"] < b["x"] + b["width"] and \
                b["y"] < t["y"] + t["height"] and t["y"] < b["y"] + b["height"]:
            hits.append(b.get("label") or ",".join(b["classes"][:3]))
    true("F.19", f"covers no other button ({hits})", not hits)


def main():
    menu = rpc("debug.ui", {"op": "new_menu"})["new_menu"]
    true("F.1", "menu open", menu["open"])
    rows = menu["rows"]
    idx = {(r["name"], r.get("agent")): i for i, r in enumerate(rows)}

    m = geo("new-menu")
    near("F.1", "width", m["width"], 340)
    near("F.1", "top y", m["y"], 40, 2)
    eq("F.1", "titlebar bottom", geo("titlebar")["height"], 40)

    b = geo("tb-button-new")
    centre = b["x"] + b["width"] / 2
    win = geo("titlebar")["width"]
    true("F.2", f"contains the + centre {centre} in [{m['x']}, {m['x'] + m['width']}]",
         m["x"] <= centre <= m["x"] + m["width"])
    true("F.2", f"right edge {m['x'] + m['width']} ≤ window {win} − 8", m["x"] + m["width"] <= win - 8)

    pad("F.3", "box", m, [1, 1, 1, 1], "border")
    for side in ("top", "right", "bottom", "left"):
        eq("F.3", f"border {side}", m["border_color"][side], tok("line2"))
    near("F.3", "radius", m["radius"], 8)
    eq("F.3", "background", m["background"], tok("bg2"))
    true("F.3", f"shadow drawn ({m['shadow']})", bool(m["shadow"]))
    pad("F.3", "box", m, [6, 0, 6, 0])

    h = geo("new-menu-header")
    eq("F.4", "text", h["label"], HEADER)
    eq("F.4", "debug header", menu["header"], HEADER)
    font("F.4", "header", h, MONO, 11)
    eq("F.4", "color", h["color"], tok("fg3"))
    pad("F.4", "header", h, [4, 14, 8, 14])
    near("F.4", "border.bottom", h["border"]["bottom"], 1)
    eq("F.4", "border.bottom color", h["border_color"]["bottom"], tok("line"))
    near("F.4", "margin.bottom", h["margin"]["bottom"], 4)

    secs = geo("new-menu-section", all_=True)
    eq("F.5", "labels", [s["label"] for s in secs], ["CLAUDE CODE", "CODEX", "OTHER"])
    for s in secs[:1]:
        pad("F.5", "section", s, [8, 14, 2, 14])
    sl = geo("new-menu-section section-label")
    font("F.5", "label (A.18)", sl, MONO, 10.5, 600)
    near("F.5", "letter spacing", (sl.get("font") or {}).get("letter_spacing"), 1.26, 0.05)
    eq("F.5", "label color", sl["color"], tok("fg3"))

    items = geo("new-menu-item", all_=True)
    eq("F.6", "one widget per row", len(items), len(rows))
    shell = items[0]
    near("F.6", "height", shell["height"], 30)
    for it in items:
        if abs(it["height"] - 30) > 1:
            row("F.6").append((False, f"row {it['label']!r} height {it['height']}"))
    pad("F.6", "item", shell, [0, 14, 0, 14])
    font("F.6", "item", shell, INTER, 13)
    eq("F.6", "item color", geo("new-menu-item-name")["color"], tok("fg"))
    ic, nm = geo("new-menu-icon"), geo("new-menu-item-name")
    near("F.6", "gap icon → name", nm["x"] - (ic["x"] + ic["width"]), 10)
    d = geo("new-menu-item-detail")
    font("F.6", "detail", d, MONO, 11.5)
    eq("F.6", "detail color", d["color"], tok("fg3"))
    k = geo("new-menu-item kbd")
    near("F.6", "key hint right edge = item right − 14", k["x"] + k["width"], shell["x"] + shell["width"] - 14)

    # R115 (ADR-0038): the agent's glyph replaces the lettered badge.
    gl = geo("new-menu-glyph", all_=True)
    if gl:
        near("F.7", "glyph column width", gl[0]["width"], 18)
        cl = [g for g in gl if "mark-claude" in g["classes"]]
        cx = [g for g in gl if "mark-codex" in g["classes"]]
        eq("F.7", "claude rows show the Claude logo", bool(cl), True)
        eq("F.7", "codex rows show the Codex logo", bool(cx), True)
        colors = sorted({g["color"] for g in gl})
        eq("F.7", "glyph colors fg2 (acc on the selected row)",
           all(c in (tok("fg2"), tok("acc")) for c in colors), True)
    else:
        row("F.7").append((False, "no new-menu-glyph"))

    pi = idx.get(("personal", "claude"))
    if pi is None:
        row("F.11").append((False, "no locked personal row"))
    else:
        p = rows[pi]
        eq("F.11", "personal detail", p["detail"], "other workspace")
        true("F.11", "personal locked, not sensitive", p["locked"] and not p["sensitive"])
        true("F.11", "tooltip names Workspace personal and ADR-0015",
             (p.get("tooltip") or "").startswith("Declared in Workspace personal;") and "ADR-0015" in p["tooltip"])
        g = items[pi]
        near("F.11", "opacity", g["opacity"], 0.45, 0.01)
        locks = geo("new-menu-lock", all_=True)
        true("F.11", "one lock-simple, inside the personal row",
             len(locks) >= 1 and any(g["y"] <= lk["y"] <= g["y"] + g["height"] for lk in locks))
        if locks:
            near("F.11", "lock size", locks[0]["natural"]["width"], 16)
        r = rpc("debug.ui", {"op": "new_menu", "arg": "activate", "name": "personal", "agent": "claude"})
        eq("F.11", "activating it does nothing", r["activated"], False)
        true("F.11", "the menu stays open", r["new_menu"]["open"])

    s = rows[0]
    eq("F.12", "Shell row", [s["kind"], s["name"], s["icon"], s["keys"]], ["shell", "Shell", "terminal-window", "Ctrl ⇧ T"])
    fo = rows[idx[("Fan-out…", None)]] if ("Fan-out…", None) in idx else {}
    br = rows[idx[("Browser", None)]] if ("Browser", None) in idx else {}
    eq("F.13", "Fan-out row", [fo.get("icon"), fo.get("name"), fo.get("detail")], ["users-three", "Fan-out…", "one prompt, several agents"])
    eq("F.13", "Browser row", [br.get("icon"), br.get("name"), br.get("keys")], ["globe-simple", "Browser", "Ctrl ⇧ O"])

    order = [f"{r['section']}:{r['name']}" + ("*" if r["default"] else "") + ("!" if r["locked"] else "") for r in rows]
    want = [":Shell", "CLAUDE CODE:acme-work*", "CLAUDE CODE:acme-team", "CLAUDE CODE:personal!",
            "CODEX:acme*", "CODEX:personal!", "OTHER:Fan-out…", "OTHER:Browser"]
    if M39:
        want = [":Shell", "CLAUDE CODE:acme-work*", "CLAUDE CODE:acme-team", "CLAUDE CODE:personal!",
                "CODEX:acme*", "CODEX:acme-biz", "CODEX:oss!", "CODEX:personal!", "OTHER:Fan-out…", "OTHER:Browser"]
    eq("F.14", "row order", order, want)
    defaults = geo("new-menu-default", all_=True)
    eq("F.14", "check marks (one per agent default)", len(defaults), 2)
    if defaults:
        eq("F.14", "check color", defaults[0]["color"], tok("acc"))

    if M39:
        usage_rows(rows, idx, items)
        toast_rows()

    prim = [g["label"] for g in geo("suggested-action", all_=True)]
    true("F.21", f"the menu adds no .suggested-action ({len(prim)} open, {PRIMARIES} closed, ≤ 1)",
         len(prim) == int(PRIMARIES) and len(prim) <= 1)

    fail = 0
    for rid in sorted(ROWS, key=lambda r: (r.split(".")[0], int(r.split(".")[1]))):
        bad = [m for ok, m in ROWS[rid] if not ok]
        if bad:
            fail += 1
            print(f"FAIL {rid} [{THEME}]: " + "; ".join(bad))
        else:
            print(f"PASS {rid} [{THEME}] ({len(ROWS[rid])} checks)")
    sys.exit(1 if fail else 0)


main()
