#!/usr/bin/env python3
"""Vendor Catppuccin's VS Code icons (MIT) for the file tree and palette (R8, ADR-0037).

    gh api repos/catppuccin/vscode-icons/tarball/v1.26.0 > ctp.tgz && tar xzf ctp.tgz
    python3 vendor.py catppuccin-vscode-icons-<sha>/

Reads src/defaults/{fileIcons,folderIcons}.ts (TypeScript object literals) and
writes map.json in the shape rade_core::filetree::Icons reads (names lowercased,
path-style names and languageIds dropped, `light` empty). Copies the mocha SVGs
it maps with every color made `currentColor`: build.rs paints them per theme
from tokens.toml (files fg2, folders fg3, an open folder acc).
"""
import ast, json, os, re, shutil, sys

src = sys.argv[1]
here = os.path.dirname(os.path.abspath(__file__))

def literal(path):
    t = open(os.path.join(src, "src/defaults", path)).read()
    t = re.sub(r"/\*.*?\*/", "", t, flags=re.S)
    t = re.sub(r"//[^\n]*", "", t)
    start = t.index("= {") + 2
    body = t[start : t.index("\n}\n", start) + 2]
    body = re.sub(r"(\b[A-Za-z]+):", r"'\1':", body)  # bare keys → quoted
    return ast.literal_eval(body)

files, folders = literal("fileIcons.ts"), literal("folderIcons.ts")
version = json.load(open(os.path.join(src, "package.json")))["version"]
out = {"version": version, "file": "_file", "folder": "_folder", "folderExpanded": "_folder_open",
       "fileNames": {}, "fileExtensions": {}, "folderNames": {}, "folderNamesExpanded": {},
       "light": {"fileNames": {}, "fileExtensions": {}, "folderNames": {}, "folderNamesExpanded": {}}}
for icon, m in files.items():
    for n in m.get("fileNames", []):
        if "/" not in n:
            out["fileNames"].setdefault(n.lower(), icon)
    for e in m.get("fileExtensions", []):
        out["fileExtensions"].setdefault(e.lower(), icon)
for icon, m in folders.items():
    for n in m.get("folderNames", []):
        if "/" not in n:
            out["folderNames"].setdefault(n.lower(), f"folder_{icon}")
            out["folderNamesExpanded"].setdefault(n.lower(), f"folder_{icon}_open")

used = {out["file"], out["folder"], out["folderExpanded"]}
for k in ("fileNames", "fileExtensions", "folderNames", "folderNamesExpanded"):
    used |= set(out[k].values())
icons = os.path.join(here, "icons")
shutil.rmtree(icons, ignore_errors=True)
os.makedirs(icons)
missing = []
for s in sorted(used):
    p = os.path.join(src, "icons/mocha", s + ".svg")
    if not os.path.exists(p):
        missing.append(s)
        continue
    svg = open(p).read()
    svg = re.sub(r'#[0-9a-fA-F]{8}\b|#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b', 'currentColor', svg)
    open(os.path.join(icons, s + ".svg"), "w").write(svg)
# Drop map entries whose SVG is missing (none expected).
for k in ("fileNames", "fileExtensions", "folderNames", "folderNamesExpanded"):
    out[k] = {n: s for n, s in out[k].items() if s not in missing}
shutil.copy(os.path.join(src, "LICENSE"), os.path.join(here, "LICENSE"))
json.dump(out, open(os.path.join(here, "map.json"), "w"), separators=(",", ":"), sort_keys=True)
print(f"catppuccin vscode-icons {version}: {len(used) - len(missing)} icons, missing {missing}")
