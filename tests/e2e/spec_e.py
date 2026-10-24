#!/usr/bin/env python3
"""R110-d: measure the Usage page (scene E) against docs/internals/design/spec/e-usage.md
rows E.1–E.34, the same way as spec_a.py / spec_f.py: each row read with the
hidden `debug.ui geometry` op (test mode, ADR-0021) and `debug.ui usage` (what
the page shows), compared with the sheet's value and tolerance (spec README:
sizes ±1 px, font size ±0.5 px, family / weight / colors / texts exact).

Usage: spec_e.py <socket> <tokens.toml> <dark|light>

The page must be open on scene E (tests/e2e/usage-scene.py) with the filter
All workspaces, period 7 days and `acme-work` selected. Times that move with
the clock (`in 1h 12m`, `40s`, weekday names) are matched by pattern. E.35 is
docs/internals/design/contrast.py; E.36 is the golden. Prints one PASS / FAIL line per
row; exit 1 when any row fails.
"""
import json
import socket
import sys
import tomllib

SOCK, TOKENS, THEME = sys.argv[1:4]
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



import re


def ui_usage():
    return rpc("debug.ui", {"op": "usage"})["usage"]


def match(rid, what, got, pattern):
    ok = isinstance(got, str) and re.fullmatch(pattern, got) is not None
    row(rid).append((ok, f"{what} {got!r} ~ /{pattern}/"))


def main():
    u = ui_usage()
    true("E.1", "page shown", u.get("shown"))
    chrome = rpc("debug.ui", {"op": "chrome"})["chrome"]
    eq("E.1", "breadcrumb", chrome["breadcrumb"], "Agent usage")
    cc = geo("crumb-current")
    eq("E.1", "crumb label", cc["label"], "Agent usage")
    eq("E.1", "crumb color", cc["color"], tok("fg"))
    font("E.1", "crumb", cc, weight=500)
    g = geo("tb-button-usage")
    true("E.1", "gauge button has class on", "on" in g["classes"])
    eq("E.1", "gauge background", g["background"], tok("bg3"))
    eq("E.1", "gauge icon color", g["color"], tok("acc"))
    rv = geo("review-button")
    true("E.1", "no Review button", not rv.get("found") or not rv.get("visible", True))

    p = geo("usage-page")
    eq("E.2", "background", p["background"], tok("bg0"))
    pad("E.2", "page", p, [18, 24, 18, 24])

    ic = geo("usage-icon")
    near("E.3", "gauge size", ic["natural"]["width"], 24)
    eq("E.3", "gauge color", ic["color"], tok("acc"))
    font("E.3", "title", geo("usage-title"), INTER, 20, 600)
    eq("E.3", "title text", geo("usage-title")["label"], "Agent usage")
    sub = geo("usage-sub")
    font("E.3", "sub", sub, MONO, 12)
    eq("E.3", "sub color", sub["color"], tok("fg3"))
    eq("E.3", "sub text", sub["label"], "7 accounts · limits from each CLI · tokens from local session logs")

    pc = geo("usage-period")
    near("E.4", "height", pc["height"], 26)
    pad("E.4", "period", pc, [1, 1, 1, 1], "border")
    eq("E.4", "border color", pc["border_color"]["top"], tok("line2"))
    near("E.4", "radius", pc["radius"], 6)
    segs = geo("usage-period-seg", all_=True)
    eq("E.4", "segments", [s["label"] for s in segs], ["Today", "7 days", "30 days"])
    font("E.4", "segment", segs[0], MONO, 11.5)
    eq("E.4", "segment color", segs[0]["color"], tok("fg2"))
    near("E.4", "segment padding.left", segs[0]["padding"]["left"], 10)
    near("E.4", "segment padding.right", segs[0]["padding"]["right"], 10)
    near("E.4", "divider", segs[1]["border"]["left"], 1)
    eq("E.4", "divider color", segs[1]["border_color"]["left"], tok("line"))

    true("E.5", "7 days active", "active" in segs[1]["classes"] and "active" not in segs[0]["classes"])
    eq("E.5", "active fill", segs[1]["background"], tok("acc-dim"))
    eq("E.5", "active text", segs[1]["color"], tok("acc"))

    eq("E.6", "header buttons", u["header"]["buttons"], ["All workspaces", "Add account"])
    f = geo("usage-ws-filter")
    true("E.6", "filter is a secondary button", "secondary-button" in f["classes"])
    true("E.6", "Add account is a secondary button", "secondary-button" in geo("usage-add")["classes"])
    near("E.6", "caret 11 px", geo("btn-caret")["natural"]["width"], 11)

    eq("E.7", "primary buttons", len(geo("suggested-action", all_=True)), 0)

    acc, det, hd = geo("usage-accounts"), geo("usage-detail"), geo("usage-header")
    near("E.8", "list width", acc["width"], 668)
    near("E.8", "detail width", det["width"], 440)
    near("E.8", "gap", det["x"] - (acc["x"] + acc["width"]), 16)
    near("E.8", "16 px below the header", acc["y"] - (hd["y"] + hd["height"]), 16)
    near("E.8", "tops aligned", det["y"], acc["y"])

    card = geo("usage-accounts-card")
    pad("E.9", "card", card, [1, 1, 1, 1], "border")
    eq("E.9", "border color", card["border_color"]["top"], tok("line"))
    near("E.9", "radius", card["radius"], 8)
    eq("E.9", "background", card["background"], tok("bg1"))

    eq("E.10", "columns", u["columns"], ["ACCOUNT", "PLAN", "TIGHTEST LIMIT", "TOKENS 7D", "UPDATED"])
    col = geo("usage-col")
    font("E.10", "head", col, MONO, 10.5, 600)
    near("E.10", "letter spacing 0.1em", (col.get("font") or {}).get("letter_spacing"), 1.05, 0.05)
    eq("E.10", "head color", col["color"], tok("fg3"))
    th = geo("usage-table-head")
    near("E.10", "head padding.top", th["padding"]["top"], 6)
    near("E.10", "column padding.left", col["padding"]["left"], 10)
    tk = geo("usage-col", nth=3)
    true("E.10", "TOKENS 7D right-aligned", tk["label"] == "TOKENS 7D")

    rows = geo("usage-row", all_=True)
    for r in rows[:-1]:
        near("E.11", f"row {r.get('name') or ''} height", r["height"], 39)
    pad("E.11", "cell", geo("usage-cell"), [8, 10, 8, 10])
    near("E.11", "row border.bottom", rows[0]["border"]["bottom"], 1)
    eq("E.11", "row border color", rows[0]["border_color"]["bottom"], tok("line"))

    b = geo("usage-page agent-badge")
    near("E.12", "badge width", b["width"], 18)
    near("E.12", "badge height", b["height"], 18)
    near("E.12", "badge radius", b["radius"], 4)
    font("E.12", "badge", b, MONO, 10, 600)
    eq("E.12", "claude letter", b["label"], "C")
    eq("E.12", "claude color", b["color"], tok("agent-claude"))
    eq("E.12", "claude background", b["background"], tok("agent-claude-bg"))
    x = geo("usage-page agent-badge-codex")
    eq("E.12", "codex letter", x["label"], "X")
    eq("E.12", "codex color", x["color"], tok("agent-codex"))
    eq("E.12", "codex background", x["background"], tok("bg4"))

    n = geo("usage-account-name")
    font("E.13", "name", n, INTER, 12.5, 500)
    eq("E.13", "name color", n["color"], tok("fg"))
    chips = geo("usage-ws-chip", all_=True)
    c = chips[0]
    font("E.13", "chip", c, MONO, 10.5)
    near("E.13", "chip padding.left", c["padding"]["left"], 5)
    near("E.13", "chip padding.right", c["padding"]["right"], 5)
    near("E.13", "chip radius", c["radius"], 4)
    near("E.13", "chip border", c["border"]["top"], 1)
    eq("E.13", "chip border color", c["border_color"]["top"], tok("line2"))
    eq("E.13", "chip color", c["color"], tok("fg2"))
    near("E.13", "chip margin-left", c["margin"]["left"], 4)
    eq("E.13", "chip labels (default ✓)", [c["label"] for c in chips],
       ["personal ✓", "acme ✓", "acme ✓", "acme", "acme", "personal ✓", "personal"])

    m = geo("usage-page usage-meter", nth=2)
    near("E.14", "meter width", m["width"], 56)
    near("E.14", "meter height", m["height"], 5)
    near("E.14", "meter radius", m["radius"], 3)
    eq("E.14", "track", m["background"], tok("bg4"))
    eq("E.14", "fill", geo("usage-page usage-meter-fill", nth=2)["background"], tok("acc"))
    v = geo("usage-tightest-value", nth=2)
    font("E.14", "value", v, MONO, 12, 600)
    eq("E.14", "value color", v["color"], tok("fg"))
    bx = geo("usage-tightest-box", nth=2)
    true("E.14", f"value min width 34 ({bx['width']})", bx["width"] >= 33)
    nm = geo("usage-tightest-name", nth=2)
    font("E.14", "limit name", nm, INTER, 12)
    eq("E.14", "limit name color", nm["color"], tok("fg2"))
    near("E.14", "gap value → name", nm["x"] - (bx["x"] + bx["width"]), 8)
    near("E.14", "gap meter → value", bx["x"] - (m["x"] + m["width"]), 8)

    tl = geo("usage-tightest", all_=True)
    true("E.15", "personal and acme hot, acme-work not",
         "hot" in tl[0]["classes"] and "hot" in tl[1]["classes"] and "hot" not in tl[2]["classes"])
    for k in (0, 1):
        eq("E.15", f"hot value {k} color", geo("usage-tightest-value", nth=k)["color"], tok("wait"))
        eq("E.15", f"hot fill {k}", geo("usage-page usage-meter-fill", nth=k)["background"], tok("wait"))
    hi = geo("usage-hot-icon", all_=True)
    eq("E.15", "warning-diamond on the hot rows only", len(hi), 2)
    if hi:
        near("E.15", "icon size", hi[0]["natural"]["width"], 11)
        eq("E.15", "icon color", hi[0]["color"], tok("wait"))
    eq("E.15", "hot rows (≥ 80 %)", [r["name"] for r in u["rows"] if r["hot"]], ["personal", "acme"])

    near("E.16", "62 % → 35 px of 56", geo("usage-page usage-meter-fill", nth=2)["width"], 35)

    t = geo("usage-tokens")
    font("E.17", "tokens", t, MONO, 12)
    eq("E.17", "tokens color", t["color"], tok("fg2"))
    cell = geo("usage-tokens-cell")
    near("E.17", "tokens end at the cell's right padding", t["x"] + t["width"], cell["x"] + cell["width"] - 10)
    font("E.17", "updated", geo("usage-updated"), MONO, 12)
    eq("E.17", "acme-work tokens", u["rows"][2]["tokens"], "12.4M")
    match("E.17", "acme-work updated", u["rows"][2]["updated"], r"[0-9]{1,2}s")

    eq("E.18", "rows", [f"{r['name']}/{r['agent']} {r['tightest']}" for r in u["rows"]],
       ["personal/claude 88% weekly", "acme/codex 81% Spark 5-hour", "acme-work/claude 62% 5-hour",
        "acme-biz/codex 27% weekly", "acme-team/claude 12% weekly", "personal/codex 4% weekly",
        "oss/codex no data yet"])

    true("E.19", "acme-work selected", "selected" in rows[2]["classes"] and u["rows"][2]["selected"])
    eq("E.19", "selected background", rows[2]["background"], tok("acc-dim"))
    first = geo("usage-cell-account", nth=2)
    eq("E.19", "2 px acc inset bar on the first cell (drawn left edge)", first["drawn"]["edge"]["left"], tok("acc"))

    near("E.20", "oss opacity", rows[6]["opacity"], 0.6, 0.01)
    o = u["rows"][6]
    eq("E.20", "oss cells", [o["tightest"], o["tokens"], o["updated"]], ["no data yet", "0", "—"])
    eq("E.20", "no data color", geo("usage-tightest-none")["color"], tok("fg3"))

    rh = geo("usage-resets-header")
    near("E.21", "margin.top", rh["margin"]["top"], 16)
    near("E.21", "margin.bottom", rh["margin"]["bottom"], 8)
    eq("E.21", "label", geo("usage-resets-header section-label")["label"], "NEXT RESETS")
    rn = geo("usage-resets-note")
    eq("E.21", "note", rn["label"], "quota coming back")
    font("E.21", "note", rn, MONO, 11)
    eq("E.21", "note color", rn["color"], tok("fg3"))

    rr = geo("usage-reset-row", all_=True)
    for r in rr:
        near("E.22", "reset row height", r["height"], 32)
    pad("E.22", "reset row", rr[0], [0, 12, 0, 12])
    near("E.22", "border.bottom", rr[0]["border"]["bottom"], 1)
    near("E.22", "no border on the last", rr[-1]["border"]["bottom"], 0)
    w = geo("usage-reset-what")
    font("E.22", "row text", w, INTER, 12)
    eq("E.22", "row text color", w["color"], tok("fg2"))
    near("E.22", "time column 86", geo("usage-reset-time")["width"], 86)
    near("E.22", "account column 90", geo("usage-reset-account")["width"], 90)

    t0, t1 = geo("usage-reset-time", nth=0), geo("usage-reset-time", nth=1)
    font("E.23", "time", t0, MONO, 12)
    eq("E.23", "soonest time color", t0["color"], tok("acc"))
    eq("E.23", "other time color", t1["color"], tok("fg2"))
    a = geo("usage-reset-account")
    eq("E.23", "account color", a["color"], tok("fg"))
    font("E.23", "account", a, weight=500)
    fc = geo("usage-reset-forecast")
    font("E.23", "forecast", fc, MONO, 11)
    eq("E.23", "forecast color", fc["color"], tok("fg3"))

    rs = u["resets"]
    pats = [
        (r"in 1h 1[0-2]m", "acme-work", "5-hour · now 62%", r"forecast: hits 100% in ~(35|40|45)m"),
        (r"in 2h (29|30)m", "acme", "Spark 5-hour · now 81%", r"GPT-5\.3-Codex-Spark only"),
        (r"[A-Z][a-z]{2} 21:00", "personal", "weekly · now 88%", r"forecast: 9[6-8]% at reset"),
        (r"[A-Z][a-z]{2} 09:00", "acme-work", "weekly ×4 · max 58% Opus", r""),
    ]
    eq("E.24", "4 rows", len(rs), 4)
    for r, (wp, acct, what, note) in zip(rs, pats):
        match("E.24", "when", r["when"], wp)
        eq("E.24", "account", r["account"], acct)
        eq("E.24", "what", r["what"], what)
        match("E.24", "note", r["note"], note)

    pad("E.25", "detail", det, [14, 16, 10, 16])
    pad("E.25", "detail", det, [1, 1, 1, 1], "border")
    eq("E.25", "border color", det["border_color"]["top"], tok("line"))
    near("E.25", "radius", det["radius"], 8)
    eq("E.25", "background", det["background"], tok("bg1"))

    db = geo("usage-detail-badge")
    near("E.26", "badge width", db["width"], 24)
    near("E.26", "badge radius", db["radius"], 6)
    font("E.26", "badge", db, MONO, 12, 600)
    font("E.26", "name", geo("usage-detail-name"), INTER, 13, 600)
    ds = geo("usage-detail-sub")
    font("E.26", "sub", ds, MONO, 11)
    eq("E.26", "sub color", ds["color"], tok("fg3"))
    eq("E.26", "sub", ds["label"], "Claude Code · ~/.claude-work")
    true("E.26", "dots-three secondary button", "secondary-button" in geo("usage-detail-menu")["classes"])

    groups = geo("usage-limit-group", all_=True)
    eq("E.27", "group labels", [g["label"] for g in groups], ["SESSION", "WEEKLY", "EXTRA USAGE", "TOKENS PER DAY"])
    near("E.27", "margin.top", groups[0]["margin"]["top"], 12)
    near("E.27", "margin.bottom", groups[0]["margin"]["bottom"], 4)

    lr = geo("usage-limit-row")
    near("E.28", "row height", lr["height"], 24)
    ln = geo("usage-limit-name")
    font("E.28", "name", ln, INTER, 12)
    eq("E.28", "name color", ln["color"], tok("fg2"))
    near("E.28", "name column 88", ln["width"], 88)
    lv = geo("usage-limit-value")
    font("E.28", "value", lv, MONO, 12, 600)
    eq("E.28", "value color", lv["color"], tok("fg"))
    near("E.28", "value column 40", lv["width"], 40)
    lt = geo("usage-limit-reset")
    font("E.28", "reset", lt, MONO, 11)
    eq("E.28", "reset color", lt["color"], tok("fg3"))
    near("E.28", "reset column 92", lt["width"], 92)
    bar = geo("usage-limit-bar")
    near("E.28", "gap name → bar", bar["x"] - (ln["x"] + ln["width"]), 10)

    near("E.29", "bar height", bar["height"], 6)
    near("E.29", "bar radius", bar["radius"], 3)
    eq("E.29", "track", bar["background"], tok("bg4"))
    eq("E.29", "fill", geo("usage-limit-bar-fill")["background"], tok("acc"))

    d = u["detail"]
    eq("E.30", "limit rows", [[g["group"], [f"{r['name']} {r['value']}" for r in g["rows"]]] for g in d["groups"]],
       [["SESSION", ["5-hour 62%"]], ["WEEKLY", ["All models 41%", "Opus 58%", "Sonnet 22%", "OAuth apps 3%"]],
        ["EXTRA USAGE", ["Spend limit $12"]]])
    match("E.30", "5-hour reset", d["groups"][0]["rows"][0]["reset"], r"1h 1[0-2]m")
    for r in d["groups"][1]["rows"]:
        match("E.30", f"{r['name']} reset", r["reset"], r"[A-Z][a-z]{2} 09:00")

    sp = d["groups"][2]["rows"][0]
    eq("E.31", "spend value / reset", [sp["value"], sp["reset"]], ["$12", "of $50 /mo"])
    others = [r["value"] + r["reset"] for g in d["groups"] for r in g["rows"] if r["name"] != "Spend limit"]
    others += [r["tightest"] + r["tokens"] for r in u["rows"]] + [r["what"] + r["note"] for r in u["resets"]]
    true("E.31", "no other $ on the page", not any("$" in x for x in others))

    ch = geo("usage-chart")
    near("E.32", "chart height", ch["height"], 110)
    near("E.32", "padding.top", ch["padding"]["top"], 18)
    bars = geo("usage-chart-bar", all_=True)
    eq("E.32", "7 bars", len(d["chart"]["values"]), 7)
    eq("E.32", "today bar", geo("usage-chart-bar", nth=len(bars) - 1)["background"], tok("acc"))
    eq("E.32", "past bar", bars[0]["background"], tok("acc-bar"))
    near("E.32", "bar radius", bars[0]["radius"], 4)
    if len(bars) > 1:
        near("E.32", "gap", bars[1]["x"] - (bars[0]["x"] + bars[0]["width"]), 8)
    dl = geo("usage-chart-day")
    font("E.32", "day letter", dl, MONO, 10.5)
    eq("E.32", "day color", dl["color"], tok("fg3"))
    cv = geo("usage-chart-value")
    font("E.32", "value", cv, MONO, 10.5, 500)
    eq("E.32", "value color", cv["color"], tok("fg2"))
    eq("E.32", "max / today", [d["chart"]["max"], d["chart"]["today"]], ["3.1M", "1.5M"])
    eq("E.32", "note", geo("usage-chart-note")["label"], "cache reads excluded")

    ft = geo("usage-detail-footer")
    near("E.33", "border.top", ft["border"]["top"], 1)
    eq("E.33", "border color", ft["border_color"]["top"], tok("line"))
    near("E.33", "margin.top", ft["margin"]["top"], 10)
    near("E.33", "padding.top", ft["padding"]["top"], 8)
    fi = geo("usage-footer-item")
    font("E.33", "footer", fi, MONO, 11)
    eq("E.33", "footer color", fi["color"], tok("fg3"))
    eq("E.33", "footer texts", d["footer"]["projects"] + [d["footer"]["models"]],
       ["Shop 9.1M", "Billing 2.3M", "Scratch 1.0M", "opus 78% · sonnet 22%"])

    cells = chrome["statusline"]["cells"]
    true("E.34", f"Statusline ACME, usage cell, hint ({cells})",
         "ACME" in cells and "claude 5h 62% · wk 41%" in cells and "Ctrl ⇧ P commands" in cells)

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
