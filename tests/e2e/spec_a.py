#!/usr/bin/env python3
"""R103-a: measure scene A's chrome against docs/internals/design/spec/a-workspace.md.

Usage: spec_a.py <socket> <tokens.toml> <dark|light> <planner pane> <tests pane>
                 [--rows A.17-A.38,A.58-A.69,A.81-A.86]

With `--rows` only those rows are measured and printed: the Navigator
(A.17–A.38, R104-b) and Changes (A.58–A.69 and A.81–A.86, R120-g) need the
M37 scene (tests/e2e/m37.sh); without it, the M35 rows below.

Rows A.2–A.16 (titlebar), A.39–A.57 (tabs, Panes, Inspector), A.70–A.73 and
A.75–A.77 (Statusline) are read with the hidden `debug.ui geometry` op
(test mode, ADR-0021) and compared with the sheet's value and tolerance
(spec README: sizes ±1 px, font size ±0.5 px, family / weight / colors /
texts exact). Colors are what GTK draws for the widget's own box; a
background of an opaque token is also checked on the window's pixels where
no child covers the widget (`drawn.background`). A.78 / A.79 are
`docs/internals/design/contrast.py` (m35.sh runs it).
Prints one PASS / FAIL line per row; exit 1 when any row fails.
"""
import json
import socket
import sys
import tomllib

SOCK, TOKENS, THEME, PLANNER, TESTS = sys.argv[1:6]
TOK = tomllib.load(open(TOKENS, "rb"))[THEME]
RANGES = []
if "--rows" in sys.argv:
    for part in sys.argv[sys.argv.index("--rows") + 1].split(","):
        lo, _, hi = part.partition("-")
        RANGES.append((int(lo.split(".")[1]), int((hi or lo).split(".")[1])))


def wanted(n):
    """Row A.<n> is measured and printed."""
    if RANGES:
        return any(lo <= n <= hi for lo, hi in RANGES)
    return 2 <= n <= 16 or 39 <= n <= 57 or 70 <= n <= 77


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


FAILS = []
ROWS = {}


def row(rid):
    ROWS.setdefault(rid, [])
    return ROWS[rid]


def near(rid, what, got, want, tol=1.0):
    ok = got is not None and abs(float(got) - float(want)) <= tol + 1e-6
    row(rid).append((ok, f"{what} {got} ≈ {want} ±{tol}"))


def eq(rid, what, got, want):
    if isinstance(want, str) and want.startswith("#") and isinstance(got, str) and got.startswith("#"):
        # Colors: ±1 per channel for rounding (spec README).
        g = [int(got[i:i + 2], 16) for i in range(1, 9, 2)]
        w = [int(want[i:i + 2], 16) for i in range(1, 9, 2)]
        ok = len(g) == len(w) and all(abs(a - b) <= 1 for a, b in zip(g, w))
    else:
        ok = got == want
    row(rid).append((ok, f"{what} {got!r} = {want!r}"))


def bg(rid, what, g, name):
    """The computed background, and for an opaque token also the window's
    pixel where no child covers the widget (`drawn.background`, R102.5)."""
    eq(rid, f"{what} background", g["background"], tok(name))
    d = (g.get("drawn") or {}).get("background")
    if d is not None and tok(name).endswith("ff"):
        eq(rid, f"{what} drawn background", d, tok(name))


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


def found(rid, g, name):
    if not g.get("found", True):
        row(rid).append((False, f"{name} not found"))
        return False
    return True


MONO, INTER = "JetBrains Mono", "Inter"

if not RANGES:
    pass
    # ---- Titlebar A.2–A.16 -----------------------------------------------------
    tb = geo("titlebar")
    near("A.2", "titlebar height", tb["height"], 40)
    bg("A.3", "titlebar", tb, "bg1")
    eq("A.3", "titlebar border.bottom", tb["border"]["bottom"], 1)
    eq("A.3", "titlebar border color", tb["border_color"]["bottom"], tok("line"))
    near("A.4", "titlebar padding.left", tb["padding"]["left"], 10)
    near("A.4", "titlebar padding.right", tb["padding"]["right"], 8)
    ws = geo("ws-button")
    cr = geo("crumbs")
    near("A.4", "gap ws-button → crumbs", cr["x"] - (ws["x"] + ws["width"]), 10)
    near("A.5", "ws-button height", ws["height"], 28)
    near("A.5", "ws-button radius", ws["radius"], 6)
    for got, want, side in zip(sides(ws, "padding"), [0, 8, 0, 4], "trbl"):
        near("A.5", f"ws-button padding.{side}", got, want)
    mk = geo("ws-mark")
    near("A.6", "ws-mark width", mk["width"], 20)
    near("A.6", "ws-mark height", mk["height"], 20)
    near("A.6", "ws-mark radius", mk["radius"], 5)
    bg("A.6", "ws-mark", mk, "acc")
    eq("A.6", "ws-mark color", mk["color"], tok("acc-fg"))
    font("A.6", "ws-mark", mk, MONO, 11, 600)
    eq("A.6", "ws-mark letter", mk["label"], "A")
    wn = geo("ws-name")
    font("A.7", "ws-name", wn, INTER, 13, 600)
    eq("A.7", "ws-name color", wn["color"], tok("fg"))
    eq("A.7", "ws-name label", wn["label"], "acme")
    font("A.8", "crumbs", cr, INTER, 12.5)
    eq("A.8", "crumbs color", cr["color"], tok("fg2"))
    eq("A.8", "crumb-sep color", geo("crumb-sep")["color"], tok("fg3"))
    cc = geo("crumb-current")
    eq("A.8", "crumb-current color", cc["color"], tok("fg"))
    font("A.8", "crumb-current", cc, weight=500)
    cb = geo("crumb-branch")
    font("A.9", "crumb-branch", cb, MONO, 11.5)
    eq("A.9", "crumb-branch color", cb["color"], tok("fg2"))
    eq("A.9", "crumb-branch label", cb["label"], "checkout-v2")
    bi = geo("crumb-branch-icon")
    eq("A.9", "git-branch icon color", bi["color"], tok("wt"))
    near("A.9", "git-branch icon size", bi["natural"]["width"], 13)
    ba = geo("crumb-base")
    font("A.10", "crumb-base", ba, MONO, 11)
    eq("A.10", "crumb-base color", ba["color"], tok("fg3"))
    eq("A.10", "crumb-base label", ba["label"], "← develop ↑3")
    cf = geo("command-field")
    near("A.11", "command-field width", cf["width"], 440)
    near("A.11", "command-field height", cf["height"], 26)
    near("A.11", "command-field radius", cf["radius"], 6)
    eq("A.11", "command-field border", cf["border"]["top"], 1)
    eq("A.11", "command-field border color", cf["border_color"]["top"], tok("line2"))
    bg("A.11", "command-field", cf, "bg0")
    btns = geo("tb-buttons")
    mid = (cr["x"] + cr["width"] + btns["x"]) / 2
    near("A.12", "command-field centre", cf["x"] + cf["width"] / 2, mid)
    ph = geo("command-placeholder")
    eq("A.12", "placeholder", ph["label"], "Search or run a command…")
    font("A.12", "placeholder", ph, INTER, 12.5)
    eq("A.12", "placeholder color", ph["color"], tok("fg3"))
    kb = geo("command-field kbd")
    font("A.13", "kbd", kb, MONO, 10.5, 500)
    eq("A.13", "kbd color", kb["color"], tok("fg2"))
    for got, want, side in zip(sides(kb, "padding"), [1, 5, 1, 5], "trbl"):
        near("A.13", f"kbd padding.{side}", got, want)
    for got, want, side in zip(sides(kb, "border"), [1, 1, 2, 1], "trbl"):
        near("A.13", f"kbd border.{side}", got, want, 0)
    eq("A.13", "kbd border color", kb["border_color"]["top"], tok("line2"))
    near("A.13", "kbd radius", kb["radius"], 4)
    bg("A.13", "kbd", kb, "bg2")
    tbb = geo("tb-buttons tb-button")
    near("A.14", "tb-button width", tbb["width"], 28)
    near("A.14", "tb-button height", tbb["height"], 28)
    near("A.14", "tb-button radius", tbb["radius"], 6)
    eq("A.14", "tb-button color", tbb["color"], tok("fg2"))
    near("A.14", "tb-button icon", geo("tb-buttons tb-button image")["natural"]["width"], 16)
    rb = geo("review-button")
    near("A.15", "review-button height", rb["height"], 26)
    near("A.15", "review-button padding.left", rb["padding"]["left"], 10)
    near("A.15", "review-button padding.right", rb["padding"]["right"], 10)
    near("A.15", "review-button radius", rb["radius"], 6)
    eq("A.15", "review-button border", rb["border"]["top"], 1)
    eq("A.15", "review-button border color", rb["border_color"]["top"], tok("line2"))
    bg("A.15", "review-button", rb, "bg2")
    rl = geo("review-label")
    eq("A.15", "review label", rl["label"], "Review")
    font("A.15", "review label", rl, INTER, 12.5)
    eq("A.15", "review label color", rl["color"], tok("fg"))
    rc = geo("review-count")
    eq("A.15", "review-count", rc["label"], "4")
    font("A.15", "review-count", rc, MONO, 11)
    eq("A.15", "review-count color", rc["color"], tok("fg3"))
    wc = geo("window-controls")
    eq("A.16", "window-controls border.left", wc["border"]["left"], 1)
    eq("A.16", "window-controls border color", wc["border_color"]["left"], tok("line"))
    near("A.16", "window-controls padding.left", wc["padding"]["left"], 8)
    near("A.16", "window-controls button width", geo("window-controls tb-button")["width"], 26)

    # ---- Tabs A.39–A.44 ----------------------------------------------------------
    ts = geo("tab-strip")
    near("A.39", "tab-strip height", ts["height"], 34)
    bg("A.39", "tab-strip", ts, "bg1")
    eq("A.39", "tab-strip border.bottom", ts["border"]["bottom"], 1)
    eq("A.39", "tab-strip border color", ts["border_color"]["bottom"], tok("line"))
    near("A.39", "tab-strip padding.left", ts["padding"]["left"], 4)
    tabs = geo("tab", all_=True)
    tabs = [t for t in tabs if "tab" in t["classes"]]
    idle = [t for t in tabs if "active" not in t["classes"]]
    act = [t for t in tabs if "active" in t["classes"]]
    if idle:
        t = idle[0]
        near("A.40", "tab padding.left", t["padding"]["left"], 12)
        near("A.40", "tab padding.right", t["padding"]["right"], 12)
        font("A.40", "tab", t, INTER, 12.5)
        eq("A.40", "tab color", t["color"], tok("fg2"))
        eq("A.40", "tab border.right", t["border"]["right"], 1)
        eq("A.40", "tab border color", t["border_color"]["right"], tok("line"))
    else:
        row("A.40").append((False, "no inactive tab"))
    if len(act) == 1:
        a = act[0]
        eq("A.41", "active tab label", a["label"], "agents")
        bg("A.41", "active tab", a, "bg0")
        eq("A.41", "active tab color", a["color"], tok("fg"))
        bars = [b for b in geo("tab-activebar", all_=True) if "active" in b["classes"]]
        if len(bars) == 1:
            near("A.41", "active bar height", bars[0]["height"], 2)
            near("A.41", "active bar width = tab width", bars[0]["width"], a["width"])
            near("A.41", "active bar x = tab x", bars[0]["x"], a["x"])
            bg("A.41", "active bar", bars[0], "acc")
        else:
            row("A.41").append((False, f"{len(bars)} active bars"))
    else:
        row("A.41").append((False, f"{len(act)} active tabs"))
    tc = geo("tab-count")
    if found("A.42", tc, "tab-count"):
        font("A.42", "tab-count", tc, MONO, 11)
        eq("A.42", "tab-count color", tc["color"], tok("fg3"))
        eq("A.42", "tab-count label", tc["label"], "2")
    cl = geo("tab-close")
    eq("A.42", "tab-close color", cl["color"], tok("fg3"))
    near("A.42", "tab-close icon", geo("tab-close image")["natural"]["width"], 12)
    tg = geo("tab-glyph")
    if found("A.43", tg, "tab-glyph"):
        near("A.43", "tab glyph width", tg["width"], 14, 0)
        # `agents 2`: its lead `planner` (claude), the Tab's worst status waiting.
        eq("A.43", "tab glyph is the lead's (claude)", "mark-claude" in tg["classes"], True)
        eq("A.43", "tab glyph color (waiting)", tg["color"], tok("wait"))
    ti = geo("tab-icon")
    if ti.get("found", True):
        near("A.43", "tab-icon width", ti["natural"]["width"], 13)
        eq("A.43", "tab-icon color", ti["color"], tok("fg3"))
    ta = geo("tab-add")
    near("A.44", "tab-add width", ta["width"], 34)
    eq("A.44", "tab-add color", ta["color"], tok("fg3"))
    near("A.44", "tab-add icon", geo("tab-add image")["natural"]["width"], 16)

    # ---- Panes A.45–A.52 ---------------------------------------------------------
    pn = geo("panes")
    for got, side in zip(sides(pn, "padding"), "trbl"):
        near("A.45", f"panes padding.{side}", got, 6)
    panes = sorted(geo("pane", all_=True), key=lambda p: p["x"])
    panes = [p for p in panes if "pane" in p["classes"]]
    if len(panes) == 2:
        near("A.45", "gap between Panes", panes[1]["x"] - (panes[0]["x"] + panes[0]["width"]), 6)
        plain = [p for p in panes if "focused" not in p["classes"]][0]
        foc = [p for p in panes if "focused" in p["classes"]]
        eq("A.46", "pane border", plain["border"]["top"], 1)
        eq("A.46", "pane border color", plain["border_color"]["top"], tok("line"))
        near("A.46", "pane radius", plain["radius"], 6)
        bg("A.46", "pane", plain, "bg0")
        eq("A.47", "focused Pane", [p["label"] for p in foc], ["tests"])
        if foc:
            eq("A.47", "focused border color", foc[0]["border_color"]["top"], tok("acc-line"))
            # Light has no outer glow (its `glow` is transparent: nothing drawn).
            glow = None if tok("glow").endswith("00") else f"0 0 18px {tok('glow')}"
            eq("A.47", "focused glow", foc[0]["shadow"], glow)
    else:
        row("A.45").append((False, f"{len(panes)} Panes shown"))
    ph = geo("pane-header")
    near("A.48", "pane-header height", ph["height"], 26)
    near("A.48", "pane-header padding.left", ph["padding"]["left"], 8)
    near("A.48", "pane-header padding.right", ph["padding"]["right"], 8)
    bg("A.48", "pane-header", ph, "bg1")
    eq("A.48", "pane-header border.bottom", ph["border"]["bottom"], 1)
    eq("A.48", "pane-header border color", ph["border_color"]["bottom"], tok("line"))
    font("A.49", "pane-header", ph, MONO, 11.5)
    eq("A.49", "pane-header color", ph["color"], tok("fg3"))
    pnm = geo("pane-name")
    eq("A.49", "pane-name color", pnm["color"], tok("fg"))
    font("A.49", "pane-name", pnm, weight=500)
    st = [s for s in geo("pane-status", all_=True) if "pane-status-waiting" in s["classes"]]
    eq("A.49", "waiting status color", st[0]["color"] if st else None, tok("wait"))
    hdrs = sorted(rpc("debug.ui", {"op": "geometry", "widget": "pane-header", "all": True}), key=lambda h: h["x"])


    def header_text(pid):
        return rpc("debug.ui", {"op": "pane", "pane": pid})["pane"]["header"]


    eq("A.50", "planner header", header_text(PLANNER), "planner · claude · opus · done")
    eq("A.50", "tests header", header_text(TESTS), "tests · codex · gpt-5 · needs input")
    term = geo("terminal", all_=True)
    term = sorted([t for t in term if "terminal" in t["classes"]], key=lambda t: t["x"])
    if panes and term:
        p0, t0 = panes[0], term[0]
        h0 = [h for h in hdrs if abs(h["x"] - p0["x"]) < 3]
        near("A.51", "terminal x − pane x", t0["x"] - p0["x"], 12)
        if h0:
            near("A.51", "terminal y − header bottom", t0["y"] - (h0[0]["y"] + h0[0]["height"]), 10)
        eq("A.52", "terminal background", t0["background"], tok("bg0"))

    # ---- Inspector A.53–A.57 -----------------------------------------------------
    ins = geo("inspector")
    near("A.53", "inspector width", ins["width"], 340)
    bg("A.53", "inspector", ins, "bg1")
    eq("A.53", "inspector border.left", ins["border"]["left"], 1)
    eq("A.53", "inspector border color", ins["border_color"]["left"], tok("line"))
    it = geo("inspector-tabs")
    near("A.54", "inspector-tabs height", it["height"], 34)
    ch = rpc("debug.ui", {"op": "chrome"})["chrome"]["inspector"]
    eq("A.54", "inspector labels", ch["labels"][1:], ["Files", "Search"])
    eq("A.54", "Changes label", ch["labels"][0].split(" ")[0], "Changes")
    eq("A.54", "Activity icon tab", ch["activity"], True)
    acts = geo("inspector-activity")
    eq("A.54", "Activity right-aligned", round(acts["x"] + acts["width"]), round(ins["x"] + ins["width"]))
    itabs = [t for t in geo("inspector-tab", all_=True) if "inspector-activity" not in t["classes"]]
    idle = [t for t in itabs if "active" not in t["classes"]]
    on = [t for t in itabs if "active" in t["classes"]]
    if idle:
        t = idle[0]
        font("A.55", "inspector-tab", t, MONO, 11, 500)
        near("A.55", "inspector-tab letter-spacing", t["font"]["letter_spacing"], 0.66, 0.1)
        eq("A.55", "inspector-tab color", t["color"], tok("fg3"))
        near("A.55", "inspector-tab padding.left", t["padding"]["left"], 12)
        near("A.55", "inspector-tab padding.right", t["padding"]["right"], 12)
    ic = geo("inspector-tab-icon")
    near("A.55", "inspector-tab icon", ic["natural"]["width"], 14)
    if len(on) == 1:
        eq("A.56", "active inspector-tab color", on[0]["color"], tok("fg"))
        bar = [b for b in geo("inspector-tab-bar", all_=True) if "active" in b["classes"]]
        if len(bar) == 1:
            near("A.56", "bar height", bar[0]["height"], 2)
            near("A.56", "bar x = tab x + 10", bar[0]["x"], on[0]["x"] + 10)
            near("A.56", "bar width = tab width − 20", bar[0]["width"], on[0]["width"] - 20)
            near("A.56", "bar at the bottom edge", bar[0]["y"] + 2, on[0]["y"] + on[0]["height"])
            bg("A.56", "bar", bar[0], "acc")
        else:
            row("A.56").append((False, f"{len(bar)} active bars"))
    else:
        row("A.56").append((False, f"{len(on)} active Inspector tabs"))
    cnt = geo("inspector-tab-count")
    if found("A.57", cnt, "inspector-tab-count"):
        font("A.57", "inspector-tab-count", cnt, MONO, 10)
        eq("A.57", "inspector-tab-count color", cnt["color"], tok("acc"))

    # ---- Statusline A.70–A.77 ----------------------------------------------------
    sl = geo("statusline")
    near("A.70", "statusline height", sl["height"], 26)
    bg("A.70", "statusline", sl, "bg1")
    eq("A.70", "statusline border.top", sl["border"]["top"], 1)
    eq("A.70", "statusline border color", sl["border_color"]["top"], tok("line"))
    font("A.70", "statusline", sl, MONO, 11)
    eq("A.70", "statusline color", sl["color"], tok("fg2"))
    sc = geo("status-cell-project")
    near("A.71", "status-cell padding.left", sc["padding"]["left"], 10)
    near("A.71", "status-cell padding.right", sc["padding"]["right"], 10)
    eq("A.71", "status-cell border.right", sc["border"]["right"], 1)
    eq("A.71", "status-cell border color", sc["border_color"]["right"], tok("line"))
    near("A.71", "status-cell icon", geo("status-cell-project image")["natural"]["width"], 12)
    wsc = geo("status-cell-ws")
    bg("A.72", "ws cell", wsc, "acc")
    eq("A.72", "ws cell color", wsc["color"], tok("acc-fg"))
    font("A.72", "ws cell", wsc, weight=600)
    near("A.72", "ws cell radius", wsc["radius"], 0, 0)
    eq("A.72", "ws cell label", wsc["label"], "ACME")
    counts = geo("status-count", all_=True)
    want = [("1 waiting", "wait"), ("1 running", "run"), ("2 done", "ok")]
    eq("A.73", "counts", [c["label"] for c in counts], [w[0] for w in want])
    for c, (_, t) in zip(counts, want):
        eq("A.73", f"{c['label']} color", c["color"], tok(t))
    cells = rpc("debug.ui", {"op": "chrome"})["chrome"]["statusline"]["cells"]
    eq("A.75", "identity cell", "richard@acme.io" in cells, True)
    hc = geo("status-cell-hints")
    near("A.76", "hints right edge", hc["x"] + hc["width"], 1440)
    eq("A.76", "hints border.left", hc["border"]["left"], 1)
    eq("A.76", "hints border.right", hc["border"]["right"], 0)
    eq("A.76", "hint texts", cells[-2:], ["Ctrl ⇧ P commands", "Ctrl ⇧ G next waiting"])
    eq("A.77", "cell order", cells[:5], ["ACME", "Shop", "checkout-v2 ↑3 ↓0", "1 waiting · 1 running · 2 done", ":5173"])



# ---- Navigator A.17–A.38 (R104-b) ------------------------------------------
def navigator():
    nv = geo("navigator")
    near("A.17", "navigator width", nv["width"], 268)
    bg("A.17", "navigator", nv, "bg1")
    eq("A.17", "navigator border.right", nv["border"]["right"], 1)
    eq("A.17", "navigator border color", nv["border_color"]["right"], tok("line"))
    nh = geo("nav-header")
    for got, want, side in zip(sides(nh, "padding"), [12, 12, 6, 14], "trbl"):
        near("A.18", f"nav-header padding.{side}", got, want)
    sl = geo("nav-header section-label")
    font("A.18", "PROJECTS label", sl, MONO, 10.5, 600)
    eq("A.18", "PROJECTS label color", sl["color"], tok("fg3"))
    eq("A.18", "PROJECTS label text", (sl["label"] or "").upper(), "PROJECTS")
    hb = geo("nav-header tb-button")
    near("A.19", "header button width", hb["width"], 22)
    near("A.19", "header button height", hb["height"], 22)
    eq("A.19", "header button color", hb["color"], tok("fg3"))
    nt = geo("nav-tree")
    near("A.20", "nav-tree padding.left", nt["padding"]["left"], 6)
    near("A.20", "nav-tree padding.right", nt["padding"]["right"], 6)
    rows = [r for r in geo("nav-row", all_=True) if "nav-row" in r["classes"]]
    bad_h = [r["height"] for r in rows if abs(r["height"] - 26) > 1]
    eq("A.21", f"{len(rows)} rows 26 px high (off: {bad_h})", bad_h, [])
    selr = [r for r in rows if "selected" in r["classes"]] or rows
    near("A.22", "row radius (the selected row draws it)", selr[0]["radius"] if selr else None, 5)
    inner = geo("nav-row-inner", all_=True)
    near("A.22", "row padding.right (inner)", inner[0]["padding"]["right"] if inner else None, 8)
    for d, want in [(0, 6), (1, 20), (2, 34), (3, 48)]:
        xs = [i for i in inner if f"depth-{d}" in i["classes"]]
        near("A.23", f"depth-{d} padding.left", xs[0]["padding"]["left"] if xs else None, want)
    titles = geo("nav-row-title", all_=True)
    t = [x for x in titles if x["label"] == "shop-web"] or titles
    font("A.24", "row title", t[0], INTER, 13)
    eq("A.24", "row title color", t[0]["color"], tok("fg"))
    shop = [x for x in titles if x["label"] == "Shop"]
    font("A.24", "Project title", shop[0] if shop else {}, weight=600)
    metas = geo("nav-row-meta", all_=True)
    plain = [m for m in metas if "warn" not in m["classes"]]
    if plain:
        m = plain[0]
        font("A.25", "row meta", m, MONO, 11)
        eq("A.25", "row meta color", m["color"], tok("fg3"))
        owner = [r for r in rows if r["y"] <= m["y"] + 1 and m["y"] + m["height"] <= r["y"] + r["height"] + 1]
        if owner:
            near("A.25", "meta right = row right − 8", m["x"] + m["width"], owner[0]["x"] + owner[0]["width"] - 8)
    ch = geo("nav-chevron")
    near("A.26", "chevron column width", ch["width"], 12)
    cb = geo("nav-chevron-button")
    if cb.get("found", True):
        eq("A.26", "chevron color", cb["color"], tok("fg3"))
        near("A.26", "chevron icon", geo("nav-chevron-button image")["natural"]["width"], 11)
    icons = geo("nav-row-icon", all_=True)
    near("A.27", "row icon size", icons[0]["natural"]["width"] if icons else None, 15)
    wt = [i for i in icons if "nav-wt" in i["classes"]]
    eq("A.27", "Worktree icon color", wt[0]["color"] if wt else None, tok("wt"))
    p1 = [i for i in icons if "proj-1" in i["classes"]]
    eq("A.27", "Shop cube color", p1[0]["color"] if p1 else None, tok("proj-1"))
    p3 = [i for i in icons if "proj-3" in i["classes"]]
    eq("A.27", "Docs cube color", p3[0]["color"] if p3 else None, tok("proj-3"))
    plainicon = [i for i in icons if not any(c in i["classes"] for c in ("nav-wt", "proj-1", "proj-2", "proj-3"))]
    eq("A.27", "link icon color", plainicon[0]["color"] if plainicon else None, tok("fg3"))
    sel = [r for r in rows if "selected" in r["classes"]]
    nav = rpc("debug.ui", {"op": "sidebar"})["sidebar"]
    selrow = [r for r in nav if r.get("selected")]
    eq("A.28", "selected row (the focused Agent, R104.6)", [r["title"] for r in selrow], ["tests"])
    if len(sel) == 1:
        bg("A.28", "selected row", sel[0], "acc-dim")
        bars = [b for b in geo("nav-row-selbar", all_=True)]
        if len(bars) == 1:
            near("A.28", "bar width", bars[0]["width"], 2)
            near("A.28", "bar height", bars[0]["height"], 16)
            near("A.28", "bar radius", bars[0]["radius"], 2)
            near("A.28", "bar x = row x", bars[0]["x"], sel[0]["x"])
            near("A.28", "bar inset top", bars[0]["y"] - sel[0]["y"], 5)
            bg("A.28", "bar", bars[0], "acc")
        else:
            row("A.28").append((False, f"{len(bars)} selection bars"))
    else:
        row("A.28").append((False, f"{len(sel)} selected rows"))
    # R115 (ADR-0038): Agent rows show their agent's glyph in the LED's colours.
    glyphs = geo("nav-tree agent-glyph", all_=True)
    if glyphs:
        near("A.29", "glyph width", glyphs[0]["width"], 14, 0)
        near("A.29", "glyph height", glyphs[0]["height"], 14, 0)
    else:
        row("A.29").append((False, "no agent glyph"))
    nav = rpc("debug.ui", {"op": "sidebar"})["sidebar"]
    by_title = {r["title"]: r for r in nav if r["kind"] == "agent"}
    for title, mark in [("planner", "claude"), ("tests", "codex"), ("dev-server", "claude")]:
        eq("A.29", f"{title} glyph", by_title.get(title, {}).get("glyph"), mark)
    for status, t in [("running", "run"), ("waiting", "wait"), ("done", "ok")]:
        xs = [g for g in glyphs if f"glyph-{status}" in g["classes"]]
        eq("A.30", f"{status} glyph", xs[0]["color"] if xs else None, tok(t))
    run_g = [g for g in glyphs if "glyph-running" in g["classes"]]
    eq("A.31", "running glyph has the pulse class", bool(run_g), True)
    # A.32: the waiting badge shows on a collapsed parent (R104-d): fold Shop.
    rpc("debug.ui", {"op": "nav_toggle", "kind": "project", "title": "Shop"})
    import time
    time.sleep(0.6)
    bd = geo("nav-badge")
    if found("A.32", bd, "nav-badge"):
        font("A.32", "badge", bd, MONO, 10.5, 600)
        eq("A.32", "badge color", bd["color"], tok("wait"))
        bg("A.32", "badge", bd, "wait-dim")
        near("A.32", "badge padding.left", bd["padding"]["left"], 5)
        near("A.32", "badge radius", bd["radius"], 4)
        near("A.32", "badge height", bd["height"], 16)
        eq("A.32", "badge text", bd["label"], "1")
    rpc("debug.ui", {"op": "nav_toggle", "kind": "project", "title": "Shop"})
    time.sleep(0.6)
    api = [r for r in rpc("debug.ui", {"op": "sidebar"})["sidebar"] if r["title"] == "shop-api"]
    eq("A.33", "shop-api drift", api[0]["drift"] if api else None, True)
    warn = [m for m in geo("nav-row-meta", all_=True) if "warn" in m["classes"] and m["label"] == "feat/checkout-v2"]
    eq("A.33", "drift meta color", warn[0]["color"] if warn else None, tok("wait"))
    eq("A.33", "drift meta text", warn[0]["label"] if warn else None, "feat/checkout-v2")
    dm = geo("nav-drift")
    if found("A.33", dm, "nav-drift"):
        eq("A.33", "drift marker color", dm["color"], tok("wait"))
        near("A.33", "drift marker size", dm["natural"]["width"], 12)
    ai = geo("nav-also-in")
    if found("A.34", ai, "nav-also-in"):
        eq("A.34", "also-in color", ai["color"], tok("run"))
        near("A.34", "also-in size", ai["natural"]["width"], 12)
    ui = [r for r in rpc("debug.ui", {"op": "sidebar"})["sidebar"] if r["title"] == "shared-ui"]
    eq("A.34", "tooltip", ui[0]["tooltip"] if ui else None, "Also in Billing @ release/2.4")
    sp = geo("nav-sep")
    if found("A.35", sp, "nav-sep"):
        near("A.35", "separator height", sp["height"], 1)
        near("A.35", "separator margin.top", sp["margin"]["top"], 8)
        near("A.35", "separator margin.left", sp["margin"]["left"], 12)
        bg("A.35", "separator", sp, "line")
    nh1 = geo("nav-header", nth=1)
    near("A.36", "Scratch header padding.top", nh1["padding"]["top"], 2)
    secs = [r for r in nav if r["kind"] == "section"]
    eq("A.36", "Scratch section", [r["title"] for r in secs][-1:], ["Scratch"])
    scr = [r for r in nav if r["kind"] == "scratch" and r["title"] == "~/workspaces/acme"]
    eq("A.36", "~/workspaces/acme meta", scr[0]["meta"] if scr else None, "2")
    ft = geo("nav-footer")
    eq("A.37", "footer border.top", ft["border"]["top"], 1)
    eq("A.37", "footer border color", ft["border_color"]["top"], tok("line"))
    for got, want, side in zip(sides(ft, "padding"), [8, 10, 8, 10], "trbl"):
        near("A.37", f"footer padding.{side}", got, want)
    pl = geo("inbox-pill")
    if found("A.38", pl, "inbox-pill"):
        near("A.38", "pill height", pl["height"], 28)
        near("A.38", "pill padding.left", pl["padding"]["left"], 8)
        near("A.38", "pill padding.right", pl["padding"]["right"], 8)
        near("A.38", "pill radius", pl["radius"], 6)
        bg("A.38", "pill", pl, "wait-dim")
        font("A.38", "pill", pl, INTER, 12)
        eq("A.38", "pill color", pl["color"], tok("wait"))
        eq("A.38", "Agent name color", geo("inbox-pill-name")["color"], tok("fg"))


# ---- Changes A.58–A.69, A.81–A.86 (R120-g, M45) ----------------------------
def scm_info():
    return rpc("debug.scm", {"op": "info"})


def file_rows():
    """The page's file rows in order: (card source, group kind, row dict)."""
    out = []
    for c in scm_info()["cards"]:
        for g in c["groups"]:
            out += [(c["source"], g["kind"], r) for r in g["rows"] if r["kind"] == "file"]
    return out


def changes():
    if wanted(58):
        hd = geo("changes-header")
        for got, want, side in zip(sides(hd, "padding"), [0, 12, 8, 12], "trbl"):
            near("A.58", f"changes-header padding.{side}", got, want)
        t = geo("changes-title")
        font("A.58", "header label", t, MONO, 10.5, 600)
        eq("A.58", "header text", t["label"], "PROJECT · SHOP")
        ad = geo("changes-total-add")
        eq("A.58", "+ total color", ad["color"], tok("add"))
        font("A.58", "+ total", ad, MONO, 11)
        eq("A.58", "− total color", geo("changes-total-del")["color"], tok("del"))
        tools = geo("changes-header changes-tool", all_=True)
        eq("A.58", "3 tools", len(tools), 3)
        for i, tl in enumerate(tools):
            near("A.58", f"tool {i} width", tl["width"], 22)
            near("A.58", f"tool {i} height", tl["height"], 22)
    if wanted(59):
        gs = geo("changes-group", all_=True)
        gs = [g for g in gs if "changes-group" in g["classes"]]
        if gs:
            g = gs[0]
            for got, want, side in zip(sides(g, "margin"), [0, 8, 10, 8], "trbl"):
                near("A.59", f"group margin.{side}", got, want)
            eq("A.59", "group border", g["border"]["top"], 1)
            eq("A.59", "group border color", g["border_color"]["top"], tok("line"))
            near("A.59", "group radius", g["radius"], 6)
            bg("A.59", "group", g, "bg2")
    if wanted(60):
        gh = geo("changes-group-header")
        near("A.60", "group header padding.top", gh["padding"]["top"], 7)
        near("A.60", "group header padding.left", gh["padding"]["left"], 10)
        eq("A.60", "group header border.bottom", gh["border"]["bottom"], 1)
        eq("A.60", "group header border color", gh["border_color"]["bottom"], tok("line"))
        gt = geo("changes-group-title")
        font("A.60", "group title", gt, INTER, 12, 600)
        gm = geo("changes-group-meta")
        font("A.60", "group meta", gm, MONO, 11)
        eq("A.60", "group meta color", gm["color"], tok("fg3"))
    if wanted(61):
        fr = geo("file-row")
        near("A.61", "file row height", fr["height"], 24)
        near("A.61", "file row padding.left", fr["padding"]["left"], 10)
        near("A.61", "file row padding.right", fr["padding"]["right"], 10)
        font("A.61", "file row name", geo("file-row-name"), MONO, 12)
        eq("A.61", "file row dir color", geo("file-row-dir")["color"], tok("fg3"))
        eq("A.61", "file row name color", geo("file-row-name")["color"], tok("fg"))
    if wanted(62):
        sts = geo("file-status", all_=True)
        near("A.62", "status column", sts[0]["width"] if sts else None, 12)
        for letter, tk in [("M", "wait"), ("A", "add"), ("U", "add"), ("D", "del"), ("R", "run")]:
            xs = [x for x in sts if x["label"] == letter]
            if xs:
                eq("A.62", f"{letter} color", xs[0]["color"], tok(tk))
                font("A.62", f"{letter}", xs[0], weight=600)
        if not any(x["label"] == "M" for x in sts):
            row("A.62").append((False, "no M row"))
        rows_all = geo("file-row", all_=True)
        for x in sts[:3]:
            r = [r for r in rows_all if r["y"] <= x["y"] <= r["y"] + r["height"]]
            if r:
                near("A.62", "status right = row right − 10", x["x"] + x["width"], r[0]["x"] + r[0]["width"] - 10)
    if wanted(63):
        sa = geo("file-stat-add")
        if found("A.63", sa, "file-stat-add"):
            font("A.63", "+ count", sa, MONO, 11)
            eq("A.63", "+ count color", sa["color"], tok("add"))
            st = geo("file-stat")
            stat_row = [x for x in sts if x["y"] <= st["y"] + 1 and st["y"] <= x["y"] + x["height"]]
            if stat_row:
                near("A.63", "stat right = status left − 8", st["x"] + st["width"], stat_row[0]["x"] - 8)
    if wanted(64):
        cr = geo("commit-row")
        near("A.64", "commit row padding.top", cr["padding"]["top"], 8)
        near("A.64", "commit row padding.left", cr["padding"]["left"], 10)
        eq("A.64", "commit row border.bottom", cr["border"]["bottom"], 1)
        eq("A.64", "commit row border color", cr["border_color"]["bottom"], tok("line"))
        ce = geo("commit-entry")
        near("A.64", "commit entry height", ce["height"], 26)
        near("A.64", "commit entry radius", ce["radius"], 5)
        eq("A.64", "commit entry border", ce["border"]["top"], 1)
        eq("A.64", "commit entry border color", ce["border_color"]["top"], tok("line2"))
        bg("A.64", "commit entry", ce, "bg0")
    if wanted(65):
        prim = [b for b in geo("suggested-action", all_=True)]
        eq("A.65", "visible primary buttons", [b["label"] for b in prim], ["Commit"])
        cbs = [b for b in geo("commit-button", all_=True) if "suggested-action" in b["classes"]]
        if cbs:
            c = cbs[0]
            # Insensitive (nothing staged) at 0.45 in scene A (M35 decision):
            # the computed fill is `acc`, the drawn one is dimmed.
            eq("A.65", "Commit background", c["background"], tok("acc"))
            eq("A.65", "Commit color", c["color"], tok("acc-fg"))
            font("A.65", "Commit", c, weight=600)
            near("A.65", "Commit height", c["height"], 26)
            near("A.65", "Commit radius", c["radius"], 6)
        # The arrow part: accent-filled only while Commit is the primary button.
        mores = geo("commit-more", all_=True)
        split_primary = [m for m in geo("commit-split", all_=True) if "primary" in m["classes"]]
        eq("A.65", "one primary commit-split (the focused card's)", len(split_primary), 1)
        if mores:
            near("A.65", "commit-more width", mores[0]["width"], 22)
            near("A.65", "commit-more height", mores[0]["height"], 26)
    if wanted(66):
        dr = geo("draft-button")
        near("A.66", "Draft height", dr["height"], 26)
        bg("A.66", "Draft", dr, "bg2")
        eq("A.66", "Draft tooltip", dr["tooltip"], "Draft with agent")
    if wanted(67):
        dc = geo("drift-card")
        if found("A.67", dc, "drift-card"):
            for got, want, side in zip(sides(dc, "margin"), [0, 8, 10, 8], "trbl"):
                near("A.67", f"drift card margin.{side}", got, want)
            near("A.67", "drift card padding.top", dc["padding"]["top"], 8)
            near("A.67", "drift card padding.left", dc["padding"]["left"], 10)
            eq("A.67", "drift card border color", dc["border_color"]["top"], tok("wait-line"))
            eq("A.67", "drift card background", dc["background"], tok("wait-dim"))
            near("A.67", "drift card radius", dc["radius"], 6)
            font("A.67", "drift text", geo("drift-text"), INTER, 12)
            eq("A.67", "drift icon color", geo("drift-icon")["color"], tok("wait"))
            near("A.67", "drift icon size", geo("drift-icon")["natural"]["width"], 15)
            ds = geo("drift-sub")
            font("A.68", "drift sub", ds, INTER, 11.5)
            eq("A.68", "drift sub color", ds["color"], tok("fg2"))
            btns = geo("drift-card btn", all_=True)
            eq("A.68", "buttons", [b["label"] for b in btns], ["Keep, show in Shop", "Move to a Worktree…"])
            for b in btns:
                near("A.68", f"{b['label']} height", b["height"], 22)
    if wanted(69):
        cards = scm_info()["cards"]
        clean = [c for c in cards if c["source"] == "shared-ui"]
        eq("A.69", "shared-ui card clean, 0 rows, no action button",
           [(c["clean"], sum(g["count"] for g in c["groups"]), c["label"], c["action"]) for c in clean],
           [(True, 0, "develop", None)])
        gs = [g for g in geo("changes-group", all_=True) if "changes-group" in g["classes"]]
        if len(gs) >= 3:
            near("A.69", "clean card opacity", gs[2]["opacity"], 0.75, 0.01)

    if wanted(81):
        # A.81 section rows and their count pills.
        sec = geo("scm-section")
        if found("A.81", sec, "scm-section"):
            near("A.81", "section height", sec["height"], 22)
            near("A.81", "section padding.left", sec["padding"]["left"], 10)
            near("A.81", "section padding.right", sec["padding"]["right"], 10)
        if sec.get("found", True):
            font("A.81", "section label", sec, MONO, 10.5, 600)
            eq("A.81", "section label color", sec["color"], tok("fg2"))
        titles = [g["title"] for c in scm_info()["cards"] for g in c["groups"]]
        eq("A.81", "section titles uppercase", titles, [t.upper() for t in titles])
        cp = geo("scm-count")
        if found("A.81", cp, "scm-count"):
            font("A.81", "count pill", cp, MONO, 10)
            bg("A.81", "count pill", cp, "bg3")
            near("A.81", "count pill radius", cp["radius"], 8)
            near("A.81", "count pill padding.left", cp["padding"]["left"], 5)
            near("A.81", "count pill padding.right", cp["padding"]["right"], 5)
    if wanted(82):
        # A.82 / A.83 need a selected row (m37 selects one first).
        rows = file_rows()
        sel = [i for i, (_, _, r) in enumerate(rows) if r["selected"]]
        if sel:
            acts = [a for a in geo("scm-act", all_=True) if a.get("visible")]
            if acts:
                for i, a in enumerate(acts):
                    near("A.82", f"scm-act {i} width", a["width"], 20)
                    near("A.82", f"scm-act {i} height", a["height"], 20)
            else:
                row("A.82").append((False, "no visible scm-act"))
            selrow = geo("file-row", nth=sel[0])
            bg("A.83", "selected row", selrow, "acc-dim")
        else:
            row("A.82").append((False, "no selected row to measure"))
            row("A.83").append((False, "no selected row to measure"))
    if wanted(84):
        # A.84 – A.86: states scene A lacks (a Publish / Sync card, a long subject).
        abs_ = [a for a in geo("scm-action-button", all_=True) if a.get("visible")]
        ab = abs_[0] if abs_ else {"found": False}
        if found("A.84", ab, "scm-action-button (visible)"):
            near("A.84", "action button height", ab["height"], 26)
            near("A.84", "action button margin.top", ab["margin"]["top"], 8)
            near("A.84", "action button margin.left", ab["margin"]["left"], 10)
            font("A.84", "action button", ab, INTER, 12, 600)
        cws = [a for a in geo("commit-warning", all_=True) if a.get("visible")]
        cw = cws[0] if cws else {"found": False}
        if found("A.85", cw, "commit-warning (visible)"):
            font("A.85", "subject warning", cw, INTER, 11)
            eq("A.85", "subject warning color", cw["color"], tok("wait"))
            near("A.85", "subject warning margin.top", cw["margin"]["top"], 2)
        sys_ = [a for a in geo("scm-sync", all_=True) if a.get("visible")]
        sy = sys_[0] if sys_ else {"found": False}
        if found("A.86", sy, "scm-sync (visible)"):
            near("A.86", "sync height", sy["height"], 22)
            font("A.86", "sync label", geo("scm-sync label"), MONO, 11)


if wanted(17):
    navigator()
if wanted(58) or wanted(81):
    changes()

bad = 0
for rid in sorted(ROWS, key=lambda r: int(r.split(".")[1])):
    if not wanted(int(rid.split(".")[1])):
        continue
    fails = [m for ok, m in ROWS[rid] if not ok]
    if fails:
        bad += 1
        print(f"FAIL {THEME} {rid}: " + "; ".join(fails))
    else:
        print(f"PASS {THEME} {rid} ({len(ROWS[rid])} checks)")
sys.exit(1 if bad else 0)
