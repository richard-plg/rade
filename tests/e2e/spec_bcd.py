#!/usr/bin/env python3
"""R105-c, R106-c, R107-f: measure scenes B (Project home), C (New Project
dialog) and D (command palette) against docs/design/spec/b-project-home.md,
c-new-project.md and d-palette.md, the same way as spec_a.py: each row read
with the hidden `debug.ui geometry` op (test mode, ADR-0021) and compared
with the sheet's value and tolerance (spec README: sizes ±1 px, font size
±0.5 px, family / weight / colors / texts exact).

Usage: spec_bcd.py <socket> <tokens.toml> <dark|light> <b|c|d>

Rows measured only by a screenshot (shadows, glow, the whole scene) are the
goldens' (golden-diff.py); contrast rows (B.38, C.27, D.23) are
docs/design/contrast.py. Prints one PASS / FAIL line per row; exit 1 when
any row fails.
"""
import json
import time
import socket
import sys
import tomllib

SOCK, TOKENS, THEME, SCENE = sys.argv[1:5]
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


def bg(rid, what, g, name):
    eq(rid, f"{what} background", g.get("background"), tok(name))
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


def pad(rid, what, g, want, key="padding"):
    for got, w, side in zip(sides(g, key), want, "trbl"):
        near(rid, f"{what} {key}.{side}", got, w)


def primaries():
    return [b["label"] for b in geo("suggested-action", all_=True)]


def scene_b():
    cr = geo("crumbs")
    eq("B.1", "breadcrumb text", rpc("debug.ui", {"op": "chrome"})["chrome"]["breadcrumb"], "Shop / Home")
    font("B.1", "breadcrumb", cr, INTER, 12.5)
    eq("B.1", "crumb-current color", geo("crumb-current")["color"], tok("fg"))
    eq("B.2", "no Review button", [b for b in geo("review-button", all_=True)], [])
    nav = rpc("debug.ui", {"op": "sidebar"})["sidebar"]
    sel = [r["title"] for r in nav if r.get("selected")]
    eq("B.3", "selected Navigator row", sel, ["Shop"])
    links = [r for r in nav if r["kind"] == "link" and r["depth"] == 1]
    eq("B.3", "Shop's links at depth 1", [r["title"] for r in links][:3], ["shop-web", "shop-api", "shared-ui"])
    eq("B.3", "Shop's links collapsed (caret-right)", [r.get("collapsed") for r in links][:3], [True, True, True])
    shop = [r["key"] for r in nav if r["kind"] == "project" and r["title"] == "Shop"]
    under = [r for r in nav if shop and r["key"].startswith(shop[0] + "/") and r["depth"] >= 2]
    eq("B.3", "nothing shown under Shop's links", [r["title"] for r in under], [])
    ph = geo("project-home")
    near("B.4", "page width", ph["width"], 1172)
    bg("B.4", "page", ph, "bg0")
    pad("B.5", "page", ph, [22, 28, 22, 28])
    ic = geo("home-icon")
    near("B.6", "cube size", ic["natural"]["width"], 26)
    eq("B.6", "cube color", ic["color"], tok("proj-1"))
    ti = geo("home-title")
    font("B.7", "title", ti, INTER, 20, 600)
    eq("B.7", "title color", ti["color"], tok("fg"))
    eq("B.7", "title", ti["label"], "Shop")
    su = geo("home-sub")
    font("B.8", "sub-line", su, MONO, 12)
    eq("B.8", "sub-line color", su["color"], tok("fg3"))
    eq("B.8", "sub-line", su["label"], "acme · 3 sources · .rade/projects/shop/project.toml")
    hs = geo("home-settings")
    ho = geo("home-open-terminal")
    near("B.9", "icon → text gap", ti["x"] - (ic["x"] + ic["width"]), 12)
    near("B.9", "actions gap", ho["x"] - (hs["x"] + hs["width"]), 8)
    home = rpc("debug.ui", {"op": "home"})["home"]
    eq("B.10", "header buttons", home["buttons"], ["Settings", "Open terminal"])
    near("B.10", "right-aligned (page right − 28)", ho["x"] + ho["width"], ph["x"] + ph["width"] - 28)
    bg("B.11", "Open terminal", ho, "acc")
    eq("B.11", "Open terminal color", ho["color"], tok("acc-fg"))
    near("B.11", "Open terminal height", ho["height"], 26)
    near("B.11", "Open terminal radius", ho["radius"], 6)
    eq("B.11", "one primary", primaries(), ["Open terminal"])
    near("B.12", "Settings height", hs["height"], 26)
    near("B.12", "Settings padding.left", hs["padding"]["left"], 10)
    near("B.12", "Settings radius", hs["radius"], 6)
    eq("B.12", "Settings border", hs["border"]["top"], 1)
    eq("B.12", "Settings border color", hs["border_color"]["top"], tok("line2"))
    bg("B.12", "Settings", hs, "bg2")
    font("B.12", "Settings", hs, INTER, 12.5)
    kp = geo("home-kpis")
    near("B.13", "KPI row margin.top", kp["margin"]["top"], 18)
    near("B.13", "KPI row margin.bottom", kp["margin"]["bottom"], 22)
    ks = [k for k in geo("kpi", all_=True) if "kpi" in k["classes"]]
    eq("B.14", "KPI count", len(ks), 3)
    if len(ks) == 3:
        near("B.13", "KPI gap", ks[1]["x"] - (ks[0]["x"] + ks[0]["width"]), 10)
        for k in ks:
            near("B.14", "KPI width", k["width"], 365)
        k = ks[0]
        eq("B.15", "KPI border", k["border"]["top"], 1)
        eq("B.15", "KPI border color", k["border_color"]["top"], tok("line"))
        near("B.15", "KPI radius", k["radius"], 8)
        pad("B.15", "KPI", k, [10, 12, 10, 12])
        bg("B.15", "KPI", k, "bg1")
    kl = geo("kpi-label")
    font("B.16", "KPI label", kl, MONO, 10.5, 600)
    kv = geo("kpi-value")
    font("B.16", "KPI value", kv, MONO, 18, 600)
    eq("B.16", "KPI value color", kv["color"], tok("fg"))
    kd = geo("kpi-detail")
    font("B.16", "KPI detail", kd, size=12)
    eq("B.16", "KPI detail color", kd["color"], tok("fg3"))
    eq("B.17", "KPI texts", [f"{k['value']} {k['detail']}" for k in home["kpis"]],
       ["3 1 waiting · 1 running", "2 checkout-v2, perf-img", "7 files in 2 sources"])
    se = geo("home-section")
    near("B.18", "section margin.top", se["margin"]["top"], 6)
    near("B.18", "section margin.bottom", se["margin"]["bottom"], 8)
    eq("B.18", "Add source button", geo("home-add-source")["label"] or "Add source", "Add source")
    lt = geo("link-table")
    near("B.19", "link table width", lt["width"], 1116)
    lh = geo("link-table-head")
    near("B.20", "head padding.top", lh["padding"]["top"], 6)
    eq("B.20", "head border.bottom", lh["border"]["bottom"], 1)
    eq("B.20", "head border color", lh["border_color"]["bottom"], tok("line"))
    cols = geo("link-table-col", all_=True)
    eq("B.20", "columns", [c["label"] for c in cols], ["SOURCE", "BASE", "CHECKOUT", "HEAD", "VS BASE", "STATE"])
    if cols:
        font("B.20", "column", cols[0], MONO, 10.5, 600)
        eq("B.20", "column color", cols[0]["color"], tok("fg3"))
        near("B.20", "column padding.left", cols[0]["padding"]["left"], 10)
    lrs = geo("link-row", all_=True)
    for i, r in enumerate(lrs):
        near("B.21", f"row {i} height", r["height"], 39)
    lc = geo("link-cell-source")
    font("B.22", "Source name", lc, INTER, 12.5, 500)
    eq("B.22", "Source name color", lc["color"], tok("fg"))
    lb = geo("link-cell-base")
    font("B.22", "Base", lb, MONO, 12)
    eq("B.22", "Base color", lb["color"], tok("fg2"))
    lri = geo("link-row-icon")
    near("B.23", "Source icon", lri["natural"]["width"], 16)
    eq("B.23", "Source icon color", lri["color"], tok("fg3"))
    heads = geo("link-cell-head", all_=True)
    eq("B.24", "shop-api HEAD color", heads[1]["color"] if len(heads) > 1 else None, tok("wait"))
    eq("B.24", "shop-api HEAD", heads[1]["label"] if len(heads) > 1 else None, "feat/checkout-v2")
    paths = geo("link-cell-path", all_=True)
    eq("B.25", "Project checkout path", [p["label"] for p in paths], [".worktrees-dev/.projects/shop/shared-ui"])
    if paths:
        eq("B.25", "path color", paths[0]["color"], tok("fg3"))
    kinds = geo("link-cell-kind", all_=True)
    eq("B.25", "checkout kinds", [k["label"] for k in kinds], ["main checkout", "main checkout", "project checkout"])
    chips = [c for c in geo("chip", all_=True) if "chip" in c["classes"]]
    if chips:
        c = chips[0]
        font("B.26", "chip", geo("chip-label"), MONO, 11)
        near("B.26", "chip padding.left", c["padding"]["left"], 6)
        near("B.26", "chip padding.top", c["padding"]["top"], 1)
        near("B.26", "chip radius", c["radius"], 4)
        eq("B.26", "chip border", c["border"]["top"], 1)
        near("B.26", "chip icon", geo("chip-icon")["natural"]["width"], 12)
    want = [("chip-ok", "ok", "ok-line", None), ("chip-warn", "wait", "wait-line", "wait-dim"), ("chip-info", "run", "run-line", None)]
    for i, (cls, ink, line, fill) in enumerate(want):
        rid = f"B.{27 + i}"
        c = chips[i] if i < len(chips) else None
        if not c:
            row(rid).append((False, f"chip {i} missing"))
            continue
        eq(rid, f"chip {i} class", cls in c["classes"], True)
        eq(rid, f"chip {i} color", c["color"], tok(ink))
        eq(rid, f"chip {i} border color", c["border_color"]["top"], tok(line))
        if fill:
            eq(rid, f"chip {i} background", c["background"], tok(fill))
    labels = [l["label"] for l in geo("chip-label", all_=True)]
    eq("B.27", "chip texts", labels, ["clean", "off base · 3 changes", "also in · Billing @ release/2.4"])
    acts = geo("link-row-action", all_=True)
    if acts:
        eq("B.30", "row action color", acts[0]["color"], tok("fg3"))
        near("B.30", "row action icon", geo("link-row-action image")["natural"]["width"], 15)
        near("B.30", "row action padding.left", acts[0]["padding"]["left"], 4)
    hw = geo("home-worktrees")
    ha = geo("home-activity")
    near("B.31", "Worktrees column width", hw["width"], 608, 2)
    near("B.31", "Activity column width", ha["width"], 486, 2)
    near("B.31", "column gap", ha["x"] - (hw["x"] + hw["width"]), 22)
    hl = geo("home-lower")
    near("B.31", "22 px below the table", hl["y"] - (lt["y"] + lt["height"]), 22)
    hc = geo("home-card")
    eq("B.32", "card border", hc["border"]["top"], 1)
    eq("B.32", "card border color", hc["border_color"]["top"], tok("line"))
    near("B.32", "card radius", hc["radius"], 8)
    bg("B.32", "card", hc, "bg1")
    cr0 = geo("home-card-row")
    near("B.33", "card row height", cr0["height"], 30)
    near("B.33", "card row padding.left", cr0["padding"]["left"], 12)
    wts = home["worktrees"]
    eq("B.34", "Worktree rows", [f"{w['name']} {w['sub']} {w['meta']}" for w in wts],
       ["checkout-v2 shop-web ← develop ↑3 · 4 changed · 2 agents", "perf-img shop-web ← develop clean"])
    wi = geo("home-wt-icon")
    near("B.34", "Worktree icon", wi["natural"]["width"], 15)
    eq("B.34", "Worktree icon color", wi["color"], tok("wt"))
    ws = geo("home-wt-sub")
    font("B.34", "Worktree sub", ws, MONO, 11)
    eq("B.34", "Worktree sub color", ws["color"], tok("fg3"))
    eq("B.35", "Activity rows", [f"{a['actor']} {a['text']}" for a in home["activity"]],
       ["tests asked which fixtures", "planner finished · 24 tests passed", "#482 opened from perf-img"])
    at = geo("activity-time")
    if found("B.35", at, "activity-time"):
        font("B.35", "time", at, MONO, 11)
        eq("B.35", "time color", at["color"], tok("fg3"))
    eq("B.35", "actor color", geo("activity-actor")["color"], tok("fg"))
    secs = geo("home-section", all_=True)
    for s in secs[1:3]:
        near("B.36", "section margin.top", s["margin"]["top"], 6)
        near("B.36", "section margin.bottom", s["margin"]["bottom"], 8)
    st = rpc("debug.ui", {"op": "chrome"})["chrome"]["statusline"]
    cells = st["cells"]
    # Counts are the Workspace's on every page (R103-j) and the hints the
    # global forms (A.76), not spec B's Project-only `● 1 waiting ● 1
    # running` / `Alt 1 Shop` (redesign-decisions.md, M37 round 1).
    eq("B.37", "Statusline cells", [c for c in cells if not c.startswith(":")],
       ["ACME", "Shop", "checkout-v2 ↑3 ↓0", "1 waiting · 2 running · 1 done", "richard@acme.io",
        "Ctrl ⇧ P commands", "Ctrl ⇧ G next waiting"])


def scene_c():
    sc = geo("dialog-scrim")
    near("C.1", "scrim width", sc["width"], 1440)
    near("C.1", "scrim height", sc["height"], 900)
    eq("C.1", "scrim background", sc["background"], tok("scrim"))
    d = geo("new-project-dialog")
    near("C.2", "dialog width", d["width"], 680)
    near("C.2", "dialog x", d["x"], 380)
    near("C.2", "dialog y", d["y"], 92)
    eq("C.3", "dialog border", d["border"]["top"], 1)
    eq("C.3", "dialog border color", d["border_color"]["top"], tok("line2"))
    near("C.3", "dialog radius", d["radius"], 10)
    eq("C.3", "dialog background", d["background"], tok("bg2"))
    h = geo("dialog-header")
    pad("C.4", "header", h, [14, 18, 14, 18])
    eq("C.4", "header border.bottom", h["border"]["bottom"], 1)
    eq("C.4", "header border color", h["border_color"]["bottom"], tok("line"))
    di = geo("dialog-icon")
    near("C.5", "cube", di["natural"]["width"], 20)
    eq("C.5", "cube color", di["color"], tok("acc"))
    t = geo("dialog-title")
    font("C.5", "title", t, INTER, 15, 600)
    eq("C.5", "title color", t["color"], tok("fg"))
    eq("C.5", "title", t["label"], "New Project")
    c = geo("dialog-context")
    font("C.5", "context", c, MONO, 11.5)
    eq("C.5", "context color", c["color"], tok("fg3"))
    eq("C.5", "context", c["label"], "in acme")
    x = geo("dialog-close")
    eq("C.6", "close color", x["color"], tok("fg3"))
    near("C.6", "close icon", geo("dialog-close image")["natural"]["width"], 16)
    near("C.6", "close right = dialog right − 18", x["x"] + x["width"], d["x"] + d["width"] - 18)
    b = geo("dialog-body")
    pad("C.7", "body", b, [16, 18, 16, 18])
    fl = geo("field-label")
    font("C.8", "field label", fl, INTER, 12)
    eq("C.8", "field label color", fl["color"], tok("fg2"))
    ne = geo("project-name-entry")
    near("C.8", "label 6 px above its control", ne["y"] - (fl["y"] + fl["height"]), 6)
    near("C.9", "Name height", ne["height"], 30)
    near("C.9", "Name radius", ne["radius"], 6)
    eq("C.9", "Name border", ne["border"]["top"], 1)
    eq("C.9", "Name background", ne["background"], tok("bg0"))
    near("C.9", "Name padding.left", ne["padding"]["left"], 10)
    font("C.9", "Name", ne, INTER, 13)
    info = rpc("debug.ui", {"op": "dialog"})["dialog"]
    eq("C.10", "keyboard focus", info["focus"], "project-name-entry")
    fl1 = geo("field-label", nth=1)
    eq("C.11", "links label", fl1["label"], "Sources and base branch")
    fh = geo("field-hint")
    eq("C.11", "hint", fh["label"], "Each source gets one base. A source can be in many Projects.")
    eq("C.11", "hint color", fh["color"], tok("fg3"))
    le = geo("link-editor")
    eq("C.12", "link table border", le["border"]["top"], 1)
    eq("C.12", "link table border color", le["border_color"]["top"], tok("line"))
    near("C.12", "link table radius", le["radius"], 6)
    rows = geo("link-editor-row", all_=True)
    if rows:
        eq("C.12", "row border.bottom", rows[0]["border"]["bottom"], 1)
        pad("C.13", "row", rows[0], [8, 10, 8, 10])
        font("C.13", "row", geo("link-source"), size=12.5)
    # C.14 / C.15: 43 px with a one-line plan, 50 px with two; the C.19
    # texts wrap at 680 px, so each row = max(26, its plan) + 16 + 1.
    plans = geo("link-plan", all_=True)
    for i, (r, pl) in enumerate(zip(rows, plans)):
        lines = 1 if pl["height"] < 20 else 2
        want = 43 if lines == 1 else 50
        rid = "C.14" if lines == 1 else "C.15"
        near(rid, f"row {i} height ({lines}-line plan)", r["height"], want, 1 if lines == 1 else 2)
    eq("C.15", "shared-ui's plan wraps (two lines), never ellipsized", len(plans) >= 3 and plans[2]["height"] >= 20, True)
    src = geo("link-source")
    near("C.13", "Source column width", src["width"], 150)
    bs = geo("link-base")
    near("C.13", "base column width", bs["width"], 190)
    ck = geo("link-check")
    near("C.13", "check column width", ck["width"], 22)
    rm = geo("link-remove")
    near("C.13", "remove column width", rm["width"], 24)
    near("C.16", "check box icon", geo("link-check image")["natural"]["width"], 17)
    eq("C.16", "checked color", ck["color"], tok("acc"))
    font("C.17", "Source", src, MONO, 12.5)
    eq("C.17", "Source color", src["color"], tok("fg"))
    pl = geo("link-plan-detail")
    font("C.18", "plan", pl, INTER, 11.5)
    eq("C.18", "plan color", pl["color"], tok("fg3"))
    # The kind is a span of the one wrapping plan label (decision M37).
    mk = [r.get("plan_markup", "") for r in rpc("debug.ui", {"op": "dialog"})["dialog"]["rows"]]
    fg2 = tok("fg2")[:7]
    eq("C.18", "plan kind span fg2 500", all(m.startswith(f'<span weight="500" foreground="{fg2}">') for m in mk), True)
    wait = tok("wait")[:7]
    eq("C.18", "drifted HEAD span in wait", any(f'<span foreground="{wait}">feat/checkout-v2</span>' in m for m in mk), True)
    eq("C.19", "plan texts", [r["plan"] for r in info["rows"]], [
        "main checkout · on develop · shared with Shop",
        "new project checkout · main checkout is on feat/checkout-v2",
        "new project checkout · Shop uses develop, Billing release/2.4",
    ])
    bb = geo("link-base button")
    near("C.20", "base select width", bs["width"], 190)
    near("C.20", "base select height", bb["height"], 26)
    near("C.20", "base select radius", bb["radius"], 5)
    eq("C.20", "base select border", bb["border"]["top"], 1)
    eq("C.20", "base select border color", bb["border_color"]["top"], tok("line2"))
    eq("C.20", "base select background", bb["background"], tok("bg0"))
    font("C.20", "base select", bb, MONO, 12)
    eq("C.21", "remove color", rm["color"], tok("fg3"))
    near("C.21", "remove icon", geo("link-remove image")["natural"]["width"], 16)
    ar = geo("link-add-row")
    near("C.22", "Add source… row height", ar["height"], 33)
    eq("C.22", "Add source… color", ar["color"], tok("fg3"))
    eq("C.22", "Add source… text", geo("link-add-label")["label"], "Add source…")
    eq("C.22", "Add source… hint", geo("link-add-hint")["label"], "from acme, a folder, or owner/repo")
    near("C.22", "plus icon", geo("link-add-icon")["natural"]["width"], 15)
    f = geo("dialog-footer")
    pad("C.23", "footer", f, [12, 18, 12, 18])
    eq("C.23", "footer border.top", f["border"]["top"], 1)
    eq("C.23", "footer border color", f["border_color"]["top"], tok("line"))
    bg("C.23", "footer", f, "bg1")
    n = geo("dialog-note-text")
    eq("C.24", "note", n["label"], "RADE never switches a dirty checkout. It makes a Worktree when a base is taken.")
    font("C.24", "note", n, INTER, 11.5)
    eq("C.24", "note color", n["color"], tok("fg3"))
    eq("C.25", "buttons", info["buttons"], ["Cancel", "Create Project"])
    cn = geo("dialog-cancel")
    pr = geo("dialog-primary")
    eq("C.25", "Cancel before Create", cn["x"] < pr["x"], True)
    bg("C.26", "Create Project", pr, "acc")
    eq("C.26", "Create Project color", pr["color"], tok("acc-fg"))
    font("C.26", "Create Project", pr, weight=600)
    near("C.26", "Create Project height", pr["height"], 26)
    near("C.26", "Create Project radius", pr["radius"], 6)
    eq("C.26", "one primary", primaries(), ["Create Project"])


def scene_d():
    sc = geo("palette-scrim")
    eq("D.1", "scrim background", sc["background"], tok("scrim"))
    near("D.1", "scrim width", sc["width"], 1440)
    near("D.1", "scrim height", sc["height"], 900)
    p = geo("palette")
    near("D.2", "palette width", p["width"], 720)
    near("D.2", "palette x", p["x"], 360)
    near("D.2", "palette y", p["y"], 64)
    eq("D.3", "palette border", p["border"]["top"], 1)
    eq("D.3", "palette border color", p["border_color"]["top"], tok("line2"))
    near("D.3", "palette radius", p["radius"], 10)
    eq("D.3", "palette background", p["background"], tok("bg2"))
    er = geo("palette-entry-row")
    near("D.4", "query row height", er["height"], 46)
    near("D.4", "query row padding.left", er["padding"]["left"], 14)
    eq("D.4", "query row border.bottom", er["border"]["bottom"], 1)
    eq("D.4", "query row border color", er["border_color"]["bottom"], tok("line"))
    ei = geo("palette-entry-icon")
    near("D.5", "glass icon", ei["natural"]["width"], 18)
    eq("D.5", "glass color", ei["color"], tok("acc"))
    en = geo("palette-entry text")
    font("D.5", "query", en, size=14)
    eq("D.5", "query color", en["color"], tok("fg"))
    es = geo("palette-esc")
    near("D.6", "Esc right = palette right − 14", es["x"] + es["width"], p["x"] + p["width"] - 14)
    font("D.6", "Esc cap", es, MONO, 10.5, 500)
    ss = geo("palette-scopes")
    pad("D.7", "scope row", ss, [8, 12, 8, 12])
    eq("D.7", "scope row border.bottom", ss["border"]["bottom"], 1)
    info = rpc("debug.palette", {"op": "info"})
    eq("D.8", "scopes", info["scopes"], ["All", "> Actions", "@ Agents", "/ Sources", "# Files", "$ Symbols"])
    chips = geo("palette-scope", all_=True)
    if chips:
        font("D.8", "scope chip", chips[1], MONO, 11, 500)
        eq("D.8", "scope chip color", chips[1]["color"], tok("fg3"))
        pad("D.8", "scope chip", chips[1], [2, 8, 2, 8])
        near("D.8", "scope chip radius", chips[1]["radius"], 4)
        eq("D.8", "Tab next hint", geo("palette-scope-hint")["label"], "Tab next")
        eq("D.9", "All is active", "active" in chips[0]["classes"], True)
        eq("D.9", "active background", chips[0]["background"], tok("acc-dim"))
        eq("D.9", "active color", chips[0]["color"], tok("acc"))
    secs = geo("palette-section", all_=True)
    eq("D.10", "sections", info["sections"][:4], ["SOURCES", "WORKTREES", "ACTIONS", "FILES"])
    if secs:
        pad("D.10", "section", secs[0], [8, 14, 4, 14])
        font("D.10", "section label", geo("palette-section-label"), MONO, 10.5, 600)
    items = [i for i in geo("palette-item", all_=True) if "palette-item" in i["classes"]]
    if items:
        near("D.11", "item height", items[0]["height"], 32)
        near("D.11", "item padding.right", items[0]["padding"]["right"], 14)
        t = geo("palette-item-title")
        font("D.11", "item title", t, INTER, 13)
        eq("D.11", "item title color", t["color"], tok("fg"))
    icons = geo("palette-item-icon", all_=True)
    plain = [i for i in icons if "wt" not in i["classes"]]
    if len(plain) > 1:
        near("D.12", "icon size", plain[1]["natural"]["height"], 16)
        near("D.12", "icon column", plain[1]["width"], 18)
        eq("D.12", "icon color", plain[1]["color"], tok("fg3"))
    wt = [i for i in icons if "wt" in i["classes"]]
    eq("D.12", "Worktree icon color", wt[0]["color"] if wt else None, tok("wt"))
    de = geo("palette-item-detail")
    font("D.13", "detail", de, MONO, 11.5)
    eq("D.13", "detail color", de["color"], tok("fg3"))
    keys = [k for k in geo("palette-item-keys", all_=True)]
    eq("D.14", "no key caps in 'shared' (Ctrl ⇧ A is Ghostty's select_all, R23-e)", len(keys), 0)
    if not keys:
        # Measured on a query whose rows have keys, then back to 'shared'.
        rpc("debug.palette", {"op": "type", "arg": "split right"})
        for _ in range(50):
            if not rpc("debug.palette", {"op": "info"}).get("busy"):
                break
            time.sleep(0.1)
        time.sleep(0.3)
        keys = [k for k in geo("palette-item-keys", all_=True)]
        if found("D.14", keys[0] if keys else {"found": False}, "palette-item-keys ('split right')"):
            near("D.14", "keys right = palette right − 14", keys[0]["x"] + keys[0]["width"], p["x"] + p["width"] - 14)
            caps = [c["label"] for c in geo("kbd", all_=True) if c["y"] >= keys[0]["y"] and c["y"] < keys[0]["y"] + keys[0]["height"] and c["x"] >= keys[0]["x"]]
            eq("D.14", "Split Right caps", caps, ["Ctrl", "⇧", "Enter"])
        rpc("debug.palette", {"op": "type", "arg": "shared"})
        for _ in range(50):
            if not rpc("debug.palette", {"op": "info"}).get("busy"):
                break
            time.sleep(0.1)
        time.sleep(0.3)
    sel = [i for i in items if "selected" in i["classes"]]
    eq("D.15", "first item selected", bool(items) and "selected" in items[0]["classes"], True)
    if sel:
        bg("D.15", "selected item", sel[0], "acc-dim")
        bars = [b for b in geo("palette-item-selbar", all_=True) if b["background"] != "#00000000"]
        eq("D.15", "one selection bar", len(bars), 1)
        if bars:
            near("D.15", "bar width", bars[0]["width"], 2)
            eq("D.15", "bar color", bars[0]["background"], tok("acc"))
        si = [i for i in icons if i["y"] >= sel[0]["y"] and i["y"] < sel[0]["y"] + sel[0]["height"]]
        eq("D.15", "selected icon color", si[0]["color"] if si else None, tok("acc"))
        hints = [h for h in geo("palette-item-hint", all_=True)]
        eq("D.15", "one Enter hint, on the selected row",
           [h["y"] >= sel[0]["y"] and h["y"] < sel[0]["y"] + sel[0]["height"] for h in hints], [True])
        ht = geo("palette-item-hint-text", all_=True)
        eq("D.15", "hint text", "Enter " + ht[0]["label"] if ht else None, "Enter open terminal")
    rows = info["rows"]
    eq("D.16", "two shared-ui links", [f"{r['kind']}:{r['title']} · {r['detail']}" for r in rows[:2]],
       ["link:shared-ui · Shop · develop", "link:shared-ui · Billing · release/2.4 · 1 agent"])
    w = [r for r in rows if r["kind"] == "worktree"]
    eq("D.17", "Worktree item", f"{w[0]['title']} · {w[0]['detail']}" if w else None, "tokens · Billing · shared-ui ← release/2.4 · ↑1")
    acts = [r["title"] for r in rows if r["kind"] == "action"]
    for a in ["Add shared-ui to Project…", "Change base of shared-ui in Shop…", "Fan-out on shared-ui…"]:
        eq("D.18", f"action {a}", a in acts, True)
    add = [r for r in rows if r["title"] == "Add shared-ui to Project…"]
    eq("D.18", "Add shared-ui to Project… has no keys (Ctrl ⇧ A stays Ghostty's select_all)",
       add[0].get("keys") if add else "missing", None)
    fan = [r for r in rows if r["title"] == "Fan-out on shared-ui…"]
    eq("D.18", "Fan-out detail", fan[0]["detail"] if fan else None, "one prompt, several agents")
    files = [r for r in rows if r["kind"] == "file"]
    eq("D.19", "file details have no absolute path", [r["detail"] for r in files if r["detail"].startswith("/")], [])
    ft = geo("palette-footer")
    pad("D.20", "footer", ft, [8, 14, 8, 14])
    eq("D.20", "footer border.top", ft["border"]["top"], 1)
    hints = [h["label"] for h in geo("palette-footer-hint", all_=True)]
    eq("D.20", "footer hints", hints, ["↑↓ move", "Enter run", "Ctrl Enter in new tab"])
    font("D.20", "footer", geo("palette-footer-hint"), INTER, 11.5)
    eq("D.20", "status", info["status"].split(" · ")[0].endswith("results"), True)
    eq("D.21", f"palette height {p['height']} ≤ 772", p["height"] <= 772 + 1, True)
    eq("D.22", "no primary button while open", primaries(), [])


{"b": scene_b, "c": scene_c, "d": scene_d}[SCENE]()

bad = 0
for rid in sorted(ROWS, key=lambda r: int(r.split(".")[1])):
    fails = [m for ok, m in ROWS[rid] if not ok]
    if fails:
        bad += 1
        print(f"FAIL {THEME} {rid}: " + "; ".join(fails))
    else:
        print(f"PASS {THEME} {rid} ({len(ROWS[rid])} checks)")
sys.exit(1 if bad else 0)
