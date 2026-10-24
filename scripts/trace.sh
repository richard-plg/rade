#!/usr/bin/env bash
# AC → test traceability for the 1.0.0 requirements (R97 and later).
#
# Reads the Acceptance Criteria of every R97+ section of docs/PRD.md (sections
# marked "Removed (1.0.0)" or "Superseded (1.0.0)" are skipped), finds the tests
# whose names carry an AC ID, writes docs/checks/trace.md and gates CI.
#
# AC line format, one per line inside its R section:
#   - R97-a [M36] (unit): Given <state>. When <action>. Then <result>.
#   kind = unit | e2e | unit+e2e | live (optional); Given, When, Then in that order.
# A test proves an AC when
#   Rust   a fn definition under crates/ is named r97_a_<words> (several IDs:
#          r98_b_r97_d_<words>; r97_cd_… is R97-cd, not R97-c);
#   shell  a `check "R97-a <words>" …` call (single or double quotes) at the start
#          of a line in tests/e2e/*.sh, tests/live/*.sh, tests/acceptance/*.sh or
#          scripts/*_test.sh. Comments, strings and later arguments never count.
#
# Usage: scripts/trace.sh [--root DIR] [--prd FILE] [--no-write] [--strict]
#   --root DIR  repository root (default: this script's repo)
#   --prd FILE  read this PRD instead of <root>/docs/PRD.md
#   --no-write  print the matrix to stdout instead of writing docs/checks/trace.md
#   --strict    fail on any missing AC, not only on gated milestones
# Exit: 0 ok · 1 a gated milestone (one with docs/checks/M<n>.md) or --strict has
#       an AC without a test · 2 format error in the PRD or bad usage (nothing written).
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec python3 - "$here/.." "$@" <<'PY'
import os, re, sys

default_root, args = sys.argv[1], sys.argv[2:]
USAGE = "usage: scripts/trace.sh [--root DIR] [--prd FILE] [--no-write] [--strict]"
root, prd, write, strict = default_root, None, True, False
i = 0
while i < len(args):
    a = args[i]
    if a in ("--root", "--prd"):
        if i + 1 >= len(args):
            print(f"trace.sh: {a} needs a value\n{USAGE}", file=sys.stderr); sys.exit(2)
        if a == "--root": root = args[i + 1]
        else: prd = args[i + 1]
        i += 2; continue
    if a == "--no-write": write = False
    elif a == "--strict": strict = True
    elif a in ("-h", "--help"): print(USAGE); sys.exit(0)
    else:
        print(f"trace.sh: unknown option '{a}'\n{USAGE}", file=sys.stderr); sys.exit(2)
    i += 1
root = os.path.abspath(root)
prd_path = os.path.abspath(prd) if prd else os.path.join(root, "docs", "PRD.md")
if not os.path.isfile(prd_path):
    print(f"trace.sh: no PRD at {prd_path}", file=sys.stderr); sys.exit(2)

def rel(p):
    r = os.path.relpath(p, root)
    return p if r.startswith("..") else r

FIRST = 97
SECTION = re.compile(r"^##\s+R(\d+)\b(.*)$")
SKIP = re.compile(r"(Removed|Superseded) \(1\.0\.0\)")
AC_START = re.compile(r"^- R\d+-[a-z]+\b")
AC = re.compile(r"^- R(\d+)-([a-z]+) \[M(\d+)\](?: \(([^)]*)\))?: (.+)$")
KINDS = {"unit", "e2e", "unit+e2e", "live"}
GWT = re.compile(r"\bGiven\b.*?\bWhen\b.*?\bThen\b")

acs, errors = {}, []          # id → (r, letter, milestone, kind)
sec, sec_line, sec_count = None, 0, 0

def close_section():
    if sec is not None and sec_count == 0:
        errors.append(f"{rel(prd_path)}:{sec_line}: R{sec} has no AC in the 1.0.0 format")

with open(prd_path, encoding="utf-8") as f:
    for n, line in enumerate(f, 1):
        line = line.rstrip("\n")
        if re.match(r"^#{1,2}\s", line):
            close_section()
            sec = None
            m = SECTION.match(line)
            if m and int(m.group(1)) >= FIRST and not SKIP.search(m.group(2)):
                sec, sec_line, sec_count = int(m.group(1)), n, 0
            continue
        if sec is None or not AC_START.match(line):
            continue
        where = f"{rel(prd_path)}:{n}"
        m = AC.match(line)
        if not m:
            errors.append(f"{where}: AC line does not match '- R<n>-<letter> [M<n>] (<kind>): Given … When … Then …'"); continue
        r, letter, ms, kind, body = int(m.group(1)), m.group(2), int(m.group(3)), m.group(4), m.group(5)
        aid = f"R{r}-{letter}"
        if r != sec:
            errors.append(f"{where}: {aid} is inside the R{sec} section"); continue
        if kind is not None and kind not in KINDS:
            errors.append(f"{where}: {aid} kind '{kind}' is not one of unit, e2e, unit+e2e, live"); continue
        if not GWT.search(body):
            errors.append(f"{where}: {aid} needs the words Given, When, Then in that order"); continue
        if aid in acs:
            errors.append(f"{where}: {aid} is defined twice"); continue
        acs[aid] = (r, letter, ms, kind or "—")
        sec_count += 1
close_section()

# ── tests ────────────────────────────────────────────────────────────────────
hits = {}                    # id → [path:line]
RUST_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)")
SH_CHECK = re.compile(r"""^\s*check\s+(?:"([^"]*)"|'([^']*)')""")
SH_IDS = re.compile(r"^(R\d+-[a-z]+(?:[ ,/]+R\d+-[a-z]+)*)(?=[\s,/:]|$)")

def rust_ids(name):
    t, out, i = name.split("_"), [], 0
    while i + 1 < len(t) and re.fullmatch(r"r\d+", t[i]) and re.fullmatch(r"[a-z]+", t[i + 1]):
        out.append(f"R{t[i][1:]}-{t[i + 1]}"); i += 2
    return out

def shell_ids(name):
    m = SH_IDS.match(name)
    return re.findall(r"R\d+-[a-z]+", m.group(1)) if m else []

def scan(path, ids_of_line):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for n, line in enumerate(f, 1):
                for aid in ids_of_line(line):
                    hits.setdefault(aid, []).append(f"{rel(path)}:{n}")
    except OSError:
        pass

def rust_line(line):
    m = RUST_FN.match(line)
    return rust_ids(m.group(1)) if m else []

def shell_line(line):
    m = SH_CHECK.match(line)
    return shell_ids(m.group(1) if m.group(1) is not None else m.group(2)) if m else []

crates = os.path.join(root, "crates")
for d, dirs, files in os.walk(crates):
    dirs[:] = sorted(x for x in dirs if x not in ("target", ".git"))
    for fn in sorted(files):
        if fn.endswith(".rs"):
            scan(os.path.join(d, fn), rust_line)
for sub, pat in (("tests/e2e", r".*\.sh"), ("tests/live", r".*\.sh"),
                 ("tests/acceptance", r".*\.sh"), ("scripts", r".*_test\.sh")):
    d = os.path.join(root, sub)
    if os.path.isdir(d):
        for fn in sorted(os.listdir(d)):
            p = os.path.join(d, fn)
            if re.fullmatch(pat, fn) and os.path.isfile(p):
                scan(p, shell_line)

# ── report ───────────────────────────────────────────────────────────────────
order = sorted(acs, key=lambda a: (acs[a][0], len(acs[a][1]), acs[a][1]))
covered = [a for a in order if hits.get(a)]
missing = [a for a in order if not hits.get(a)]
rs = sorted({v[0] for v in acs.values()})
span = "" if not rs else (f" (R{rs[0]})" if len(rs) == 1 else f" (R{rs[0]}–R{rs[-1]})")
summary = (f"ACs: {len(order)}{span}, covered {len(covered)}, missing {len(missing)}, "
           f"format errors {len(errors)}")

def gated(ms):
    return os.path.isfile(os.path.join(root, "docs", "checks", f"M{ms}.md"))

out = ["# AC traceability (1.0.0)", "",
       "Generated by scripts/trace.sh from docs/PRD.md (R97 and later) — do not edit by hand.",
       "A milestone is gated once docs/checks/M<n>.md exists: every AC tagged with it then needs a test.",
       "", summary, "",
       "| Milestone | ACs | covered | missing | gated |", "|---|---|---|---|---|"]
for ms in sorted({v[2] for v in acs.values()}):
    mine = [a for a in order if acs[a][2] == ms]
    miss = sum(1 for a in mine if not hits.get(a))
    out.append(f"| M{ms} | {len(mine)} | {len(mine) - miss} | {miss} | {'yes' if gated(ms) else 'no'} |")
out += ["", "| AC | Milestone | Kind | Status | Tests |", "|---|---|---|---|---|"]
for a in order:
    _, _, ms, kind = acs[a]
    t = hits.get(a)
    out.append(f"| {a} | M{ms} | {kind} | {'covered' if t else 'missing'} | {', '.join(t) if t else '—'} |")
text = "\n".join(out) + "\n"

for e in errors:
    print(e, file=sys.stderr)
if errors:
    print(summary)
    print("trace.sh: format errors; nothing written", file=sys.stderr)
    sys.exit(2)
if write:
    dst = os.path.join(root, "docs", "checks", "trace.md")
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    tmp = dst + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(text)
    os.replace(tmp, dst)
    print(summary)
    print(f"wrote {rel(dst)}")
else:
    sys.stdout.write(text)

failing = [a for a in missing if strict or gated(acs[a][2])]
if failing:
    why = "--strict" if strict else "gated milestone"
    print(f"trace.sh: {len(failing)} AC(s) without a test ({why}): {' '.join(failing)}", file=sys.stderr)
    sys.exit(1)
PY
