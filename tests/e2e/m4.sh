#!/usr/bin/env bash
# M4 end-to-end tests (ADR-0006): open files from Terminals (Ctrl+click on
# paths / URLs), the Viewer (code, Markdown with Mermaid + relative images,
# md→md links + Back, live reload, Esc, restore), `rade open`, and the file
# tree (gitignore, toggle, git colors, live updates). Driven only through
# `rade`; Ctrl+click is a real Ghostty mouse event (`rade debug click`).
# Usage: tests/e2e/m4.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m4
source "$HERE/lib.sh"

state() { rade debug state; }
exit_code() { set +e; "$@" >/dev/null 2>"$T/err"; local c=$?; set -e; echo $c; }
viewers() { state | jq -c '[.. | objects | select(.kind?.type? == "viewer") | {id, path: .kind.path, line: .kind.line, back: (.kind.back // [])}]'; }
viewer_id() { viewers | jq -r '.[0].id // empty'; }
viewer_field() { viewers | jq -r ".[0].$1 // empty"; }
vinfo() { rade debug viewer "$1" info; }
vjs() { rade debug viewer "$1" js "$2" | jq -r .result; }
tree() { rade debug tree "$@"; }
row() { tree rows | jq -c --arg p "$1" '.rows[] | select(.path==$p)'; }
focused() { state | jq -r '. as $s | $s.sources[] | select(.id==$s.active_source) | . as $p | .worktrees[] | select(.id==$p.active_worktree) | . as $w | .tabs[] | select(.id==$w.active_tab) | .focused'; }

# ---- fixtures: a git repo with code, Markdown, an image, ignored files ----
X=$T/x
mkdir -p "$X/src" "$X/crates/rade-core/src" "$X/docs" "$X/img" "$X/target"
for i in $(seq 1 80); do echo "// line $i"; done > "$X/src/main.rs"
sed -i '42s/.*/fn line_42_target() {}/' "$X/src/main.rs"
for i in $(seq 1 200); do echo "// notify line $i"; done > "$X/crates/rade-core/src/notify.rs"
printf '# PRD\n\nRequirements.\n' > "$X/docs/PRD.md"
python3 - "$X/img/logo.png" <<'PY'
import struct, sys, zlib
w = h = 48
raw = b"".join(b"\0" + bytes([40, 110, 220, 255] * w) for _ in range(h))
chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
cat > "$X/README.md" <<'MD'
# Source X

A **sample** README with `inline code`, a [guide](docs/guide.md), an
[anchor](#tasks) and a [web link](https://example.com/docs).

![logo](img/logo.png)

## Architecture

```mermaid
graph LR
  CLI[rade CLI] --> Socket
  Socket --> App[RADE app]
  App --> Viewer
```

## Code

```rust
fn main() {
    let answer: u32 = 42; // the answer
    println!("{answer}");
}
```

| Feature | Status |
|---|---|
| Viewer | ~~planned~~ done |
| Tree | done |

## Tasks

- [x] render Markdown
- [ ] ship it

> [!NOTE]
> Rendered offline: no CDN.

Footnote[^1].

[^1]: Via comrak.
MD
cat > "$X/docs/guide.md" <<'MD'
# Guide

Back to the [README](../README.md).
MD
printf 'target/\n*.log\n' > "$X/.gitignore"
printf '[package]\nname = "x"\n' > "$X/Cargo.toml"
echo out > "$X/target/out.txt"
echo log > "$X/debug.log"
git -C "$X" init -q
git -C "$X" -c user.name=t -c user.email=t@t -c commit.gpgsign=false add -A
git -C "$X" -c user.name=t -c user.email=t@t -c commit.gpgsign=false commit -qm init
echo "Changed." >> "$X/docs/PRD.md"   # modified

# A stand-in browser: records the URLs GIO hands to the default http handler.
BROWSER_LOG=$T/browser.log
mkdir -p "$XDG_DATA_HOME/applications"
cat > "$XDG_DATA_HOME/applications/fakebrowser.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Fake Browser
Exec=sh -c 'echo "\$1" >> $BROWSER_LOG' sh %u
MimeType=x-scheme-handler/http;x-scheme-handler/https;
EOF
printf '[Default Applications]\nx-scheme-handler/http=fakebrowser.desktop\nx-scheme-handler/https=fakebrowser.desktop\n' \
    > "$XDG_CONFIG_HOME/mimeapps.list"

# A stand-in VS Code CLI (`[viewer] editor`): records its argv, one per line.
CODE_ARGV=$T/code.argv
mkdir -p "$T/bin" "$XDG_CONFIG_HOME/rade"
printf '#!/bin/sh\nprintf "%%s\\n" "$@" > %s\n' "$CODE_ARGV" > "$T/bin/code"
chmod +x "$T/bin/code"
printf '[viewer]\neditor = "%s"\n' "$T/bin/code" > "$XDG_CONFIG_HOME/rade/config.toml"
# …and its icon (VS Code's .deb installs /usr/share/pixmaps/vscode.png), so
# the check doesn't depend on VS Code being installed (CI image).
mkdir -p "$XDG_DATA_HOME/icons"
printf '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#007acc"/></svg>\n' \
    > "$XDG_DATA_HOME/icons/vscode.svg"

echo "== start"
# Light: since 1.0.0 RADE is dark unless the system asks for light (R102).
EDITOR=fake-editor ADW_DEBUG_COLOR_SCHEME=prefer-light start_app
rade source add "$X" >/dev/null
a=$(rade spawn --cmd bash --source x --name a --json | jq -r .pane)
rade focus "$a"
eventually 10 screen_has "$a" '$' || true
check "Terminal cwd = Worktree root" test "$(rade list --json | jq -r --arg p "$a" '.panes[] | select(.pane==$p) | .cwd')" = "$X"

echo "== R7-a: Ctrl+click on agent-style paths"
rade send "$a" "clear; printf 'edit src/main.rs:42 then crates/rade-core/src/notify.rs:167:5, see (docs/PRD.md).\\n'" --enter
eventually 5 screen_has "$a" "see (docs/PRD.md)."
rade debug click "$a" "src/main.rs:42" >/dev/null
check "Viewer opened" eventually 5 is "viewer_id | wc -l" 1
v=$(viewer_id)
check "at src/main.rs line 42" test "$(viewers | jq -c '.[0] | [.path, .line]')" = "[\"$X/src/main.rs\",42]"
tab_a=$(rade list --json | jq -r --arg p "$a" '.panes[] | select(.pane==$p) | .tab')
check "split right of the Terminal, same Tab" test "$(state | jq -c --arg t "$tab_a" '[.. | objects | select(.id?==$t) | .layout | .type, .dir, .a.pane.id, .b.pane.id]')" = "[\"split\",\"right\",\"$a\",\"$v\"]"
check "Viewer focused" test "$(focused)" = "$v"
check "code view, rust, cursor on 42" eventually 5 bash -c "rade debug viewer $v info | jq -e '.view==\"text\" and .language==\"rust\" and .cursor_line==42'"
check "scrolled so line 42 is on screen" eventually 5 bash -c "rade debug viewer $v info | jq -e '.top_line > 1 and .top_line <= 42'"
shot viewer-code

rade debug click "$a" "notify.rs:167:5" >/dev/null
check "path:line:col → same Viewer, line 167" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0] | \"\\(.kind.path):\\(.kind.line)\"') == '$X/crates/rade-core/src/notify.rs:167' ]]"
check "one Viewer in the Tab (reused)" test "$(viewers | jq length)" = 1
check "previous file on the Back list, with its line (R7.3)" test "$(viewer_field 'back[0] | "\(.path):\(.line)"')" = "$X/src/main.rs:42"
rade debug click "$a" "docs/PRD.md" >/dev/null
check "(docs/PRD.md). → PRD.md, no line" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0].kind.path') == '$X/docs/PRD.md' ]]"
check "Markdown renders in the WebView" eventually 10 bash -c "rade debug viewer $v info | jq -e '.view==\"markdown\" and .headings>=1'"

echo "== R7.1: cwd-relative, ~/, missing, http"
rade send "$a" "cd src; clear; printf 'rel main.rs:10 miss nope.rs:3\\n'" --enter
eventually 5 screen_has "$a" "miss nope.rs:3"
eventually 5 bash -c "rade list --json | jq -e --arg p $a '.panes[] | select(.pane==\$p) | .cwd == \"$X/src\"'" || true
rade debug click "$a" "main.rs:10" >/dev/null
check "relative to the Terminal's cwd (OSC 7)" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0] | \"\\(.kind.path):\\(.kind.line)\"') == '$X/src/main.rs:10' ]]"
before=$(viewers)
rade debug click "$a" "nope.rs:3" >/dev/null; sleep 0.5
check "missing file opens nothing" test "$(viewers)" = "$before"
mkdir -p "$HOME/notes"; echo "# Home note" > "$HOME/notes/n.md"
rade send "$a" "clear; printf 'home ~/notes/n.md web https://example.com/rade?q=1\\n'" --enter
eventually 5 screen_has "$a" "web https://example.com/rade?q=1"
rade debug click "$a" "~/notes/n.md" >/dev/null
check "~/ expands to HOME" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0].kind.path') == '$HOME/notes/n.md' ]]"
rade debug click "$a" "https://example.com/rade" >/dev/null
check "http(s) → system browser" eventually 5 grep -qx "https://example.com/rade?q=1" "$BROWSER_LOG"
cat > "$T/osc8.sh" <<EOF
printf '\\e]8;;file://$X/docs/guide.md\\e\\\\osc8-guide\\e]8;;\\e\\\\\\n'
EOF
rade send "$a" "clear; bash $T/osc8.sh" --enter
eventually 5 screen_has "$a" "osc8-guide"
rade debug click "$a" "osc8-guide" >/dev/null
check "OSC 8 file:// link" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0].kind.path') == '$X/docs/guide.md' ]]"

echo "== R7.2: Esc closes"
rade debug viewer "$v" escape >/dev/null
check "Viewer closed, split collapsed" eventually 5 is "viewer_id | wc -l" 0
check "focus back on the Terminal" test "$(focused)" = "$a"

echo "== R7 API: rade open"
check "missing → exit 4" test "$(cd "$X" && exit_code rade open nope.md)" = 4
check "folder → exit 2" test "$(cd "$X" && exit_code rade open docs)" = 2
v=$(cd "$X" && rade open README.md --pane a)
check "rade open → Viewer beside a" test "$(viewers | jq -c '.[0] | [.id, .path]')" = "[\"$v\",\"$X/README.md\"]"
check "focus stays on the Terminal" test "$(focused)" = "$a"
check "rade open path:line" test "$(cd "$X/src" && rade open main.rs:7 --pane a --json | jq -c '[.pane, .line]')" = "[\"$v\",7]"
rade debug viewer "$v" back >/dev/null
check "Back → README" eventually 5 is 'viewer_field path' "$X/README.md"

echo "== R7-b: README with Mermaid and a relative image"
info() { vinfo "$v"; }
check "Mermaid rendered to SVG" eventually 15 bash -c "rade debug viewer $v info | jq -e '.mermaid==\"done\" and .mermaid_svgs==1'"
check "relative image loaded" eventually 5 bash -c "rade debug viewer $v info | jq -e '.images==[{\"src\":\"img/logo.png\",\"loaded\":true}]'"
check "code highlighted (syntect spans)" test "$(info | jq '.code_spans > 5')" = true
check "table, task list" test "$(info | jq -c '[.tables, .tasks]')" = "[1,2]"
check "light appearance" test "$(info | jq -r .scheme)" = light
check "remote image blocked (CSP)" test "$(vjs "$v" "(function(){var i=new Image();i.src='https://example.com/x.png';document.body.appendChild(i);return String(i.complete && i.naturalWidth>0)})()")" = false
check "no page script runs (markup JS off)" test "$(vjs "$v" "typeof window.__x")" = undefined
sleep 1; shot md-light

echo "== R7.3: md → md link, Back, anchors, web links"
vjs "$v" "document.querySelector('a[href=\"#tasks\"]').click(); 'ok'" >/dev/null
sleep 0.3
check "#anchor scrolls in place" test "$(viewer_field path)" = "$X/README.md"
check "…to #tasks" test "$(vjs "$v" "location.hash")" = "#tasks"
vjs "$v" "document.querySelector('a[href=\"docs/guide.md\"]').click(); 'ok'" >/dev/null
check "md link opens in the same Viewer" eventually 5 is 'viewer_field path' "$X/docs/guide.md"
check "…with README on the Back list" test "$(viewer_field 'back[-1].path')" = "$X/README.md"
check "guide rendered" eventually 5 bash -c "rade debug viewer $v info | jq -e '.title==\"guide.md\"'"
vjs "$v" "document.querySelector('a[href=\"../README.md\"]').click(); 'ok'" >/dev/null
check "../README.md link" eventually 5 is 'viewer_field path' "$X/README.md"
rade debug viewer "$v" back >/dev/null
check "Back → guide" eventually 5 is 'viewer_field path' "$X/docs/guide.md"
rade debug viewer "$v" back >/dev/null
check "Back → README" eventually 5 is 'viewer_field path' "$X/README.md"
check "README re-rendered" eventually 10 bash -c "rade debug viewer $v info | jq -e '.title==\"README.md\" and .mermaid==\"done\"'"
n=$(wc -l < "$BROWSER_LOG")
vjs "$v" "document.querySelector('a[href^=\"https:\"]').click(); 'ok'" >/dev/null
check "web link → system browser, Viewer stays" eventually 5 grep -qx "https://example.com/docs" "$BROWSER_LOG"
check "…Viewer unchanged" test "$(viewer_field path)" = "$X/README.md"

echo "== R7.2: live reload"
printf '\nRELOAD-MARKER\n' >> "$X/README.md"
check "Markdown reloads on change" eventually 5 bash -c "[[ \$(rade debug viewer $v js 'String(document.body.innerText.includes(\"RELOAD-MARKER\"))' | jq -r .result) == true ]]"
rade open "$X/src/main.rs:5" --pane a >/dev/null
check "code view" eventually 5 bash -c "rade debug viewer $v info | jq -e '.view==\"text\" and .lines==81'"
echo "// appended" >> "$X/src/main.rs"
check "code reloads on change" eventually 5 bash -c "rade debug viewer $v info | jq -e '.lines==82'"
check "header: VS Code button" test "$(vinfo "$v" | jq -c .open_action)" = '{"icon":"vscode","tooltip":"Open in VS Code"}'
shot viewer-vscode
rade debug viewer "$v" open >/dev/null
check "VS Code button runs <editor> <worktree> --goto <file>:<line>" eventually 5 is 'cat "$CODE_ARGV" 2>/dev/null | paste -sd" "' "$X --goto $X/src/main.rs:5"
check "…and opens no Terminal" test "$(state | jq '[.. | objects | select(.kind?.command? != null and (.kind.command | startswith("fake-editor")))] | length')" = 0
printf '[viewer]\neditor = "rade-no-such-editor"\n' > "$XDG_CONFIG_HOME/rade/config.toml"
check "editor not on PATH → the old \$EDITOR button" eventually 5 is 'rade open "$X/src/main.rs:5" --pane a >/dev/null; vinfo "$v" | jq -c .open_action' '{"icon":"ph-arrow-square-out-symbolic","tooltip":"Open in fake-editor"}'
rade debug viewer "$v" open >/dev/null
check "Open in \$EDITOR → Terminal below running it at the line" eventually 5 bash -c "rade debug state | jq -e '[.. | objects | select(.kind?.command? == \"fake-editor +5 $X/src/main.rs\")] | length == 1'"
ed=$(state | jq -r '.. | objects | select(.kind?.command? != null and (.kind.command | startswith("fake-editor"))) | .id')
rade close "$ed" --force >/dev/null
head -c 6000000 /dev/zero > "$X/big.log"
rade open "$X/big.log" --pane a >/dev/null
check "large file → message" eventually 5 bash -c "rade debug viewer $v info | jq -e '.view==\"message\" and .message==\"File too large to show\"'"
printf 'ELF\0\0\1' > "$X/blob.bin"
rade open "$X/blob.bin" --pane a >/dev/null
check "binary file → message" eventually 5 bash -c "rade debug viewer $v info | jq -e '.message==\"Binary file\"'"

echo "== R114: HTML and PDF rendered"
cat > "$X/report.html" <<'HTML'
<!doctype html><html><head><title>Report</title><style>h1{color:rgb(1,2,3)}</style></head>
<body><h1>RENDERED-REPORT</h1><img src="img/logo.png"><p id="js">no script</p>
<p><a href="docs/guide.md">guide</a></p>
<script>document.getElementById('js').textContent = 'SCRIPT-RAN';</script></body></html>
HTML
python3 - "$X/spec.pdf" <<'PY'
import sys
objs = [b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        None, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
text = b"BT /F1 36 Tf 72 700 Td (RADE PDF MARKER) Tj ET"
objs[3] = b"<< /Length %d >>\nstream\n" % len(text) + text + b"\nendstream"
out, offs = bytearray(b"%PDF-1.4\n"), []
for i, o in enumerate(objs, 1):
    offs.append(len(out)); out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
x = len(out)
out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1) + b"".join(b"%010d 00000 n \n" % o for o in offs)
out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, x)
open(sys.argv[1], "wb").write(out)
PY
rade open "$X/report.html" --pane a >/dev/null
check "R114-b report.html → rendered (view doc, its <h1> on the page)" eventually 10 bash -c "rade debug viewer $v info | jq -e '.view==\"doc\" and (.doc.text | contains(\"RENDERED-REPORT\"))'"
check "R114-b …its relative image loads" eventually 5 bash -c "rade debug viewer $v info | jq -e '.doc.images==[true]'"
check "R114-b …its script ran ([viewer] html_scripts defaults on)" eventually 5 bash -c "rade debug viewer $v info | jq -e '.doc.text | contains(\"SCRIPT-RAN\")'"
sleep 0.5; shot html-rendered
rade debug viewer "$v" source >/dev/null
check "R114-b Source toggle → the markup, highlighted as HTML" eventually 5 bash -c "rade debug viewer $v info | jq -e '.view==\"text\" and .language==\"html\"'"
shot html-source
rade debug viewer "$v" source >/dev/null
check "R114-b …and back to rendered" eventually 5 bash -c "rade debug viewer $v info | jq -e '.view==\"doc\"'"
rade open "$X/spec.pdf" --pane a >/dev/null
check "R114-c spec.pdf → rendered in the PDF viewer (view doc, its file loaded)" eventually 10 bash -c "rade debug viewer $v info | jq -e '.view==\"doc\" and (.doc.uri | endswith(\"/spec.pdf\")) and .doc.loading==false'"
sleep 2; shot pdf-light
rade debug action color_scheme_dark >/dev/null; sleep 1; shot pdf-dark
rade debug action color_scheme_light >/dev/null
rm -f "$X/report.html" "$X/spec.pdf"
rade open "$X/README.md" --pane a >/dev/null
eventually 10 bash -c "rade debug viewer $v info | jq -e '.mermaid==\"done\"'" || true

echo "== R8: file tree"
check "hidden by default" test "$(state | jq -r '.file_tree // false')" = false
rade debug action toggle_file_tree
check "Ctrl+Shift+E shows it (state.json)" test "$(state | jq -r .file_tree)" = true
check "rooted at the active Worktree" eventually 5 is 'tree rows | jq -r .root' "$X"
top() { tree rows | jq -c '[.rows[] | select(.depth==0) | .path | split("/")[-1]]'; }
check "folders first, .gitignore respected, .git hidden" eventually 5 is top '["crates","docs","img","src",".gitignore","blob.bin","Cargo.toml","README.md"]'
check "git: docs modified (roll-up)" eventually 5 is 'row "$X/docs" | jq -r .git' modified
check "git: untracked blob.bin" test "$(row "$X/blob.bin" | jq -r .git)" = untracked
t0=$(date +%s%N)
echo new > "$X/agent-new.txt"
check "R8-a: new file appears within 1 s" eventually 1 bash -c "rade debug tree rows | jq -e --arg p '$X/agent-new.txt' '.rows[] | select(.path==\$p and .git==\"untracked\")'"
echo "  (appeared, untracked, after $(( ($(date +%s%N) - t0) / 1000000 )) ms)"
tree expand "$X/src" >/dev/null
check "expand src (lazy)" test "$(row "$X/src/main.rs" | jq -r .depth)" = 1
echo x > "$X/src/new_mod.rs"
check "live update inside an expanded folder" eventually 2 bash -c "rade debug tree rows | jq -e --arg p '$X/src/new_mod.rs' '.rows[] | select(.path==\$p and .git==\"untracked\")'"
rm "$X/agent-new.txt"
check "deleted file disappears" eventually 2 bash -c "! rade debug tree rows | jq -e --arg p '$X/agent-new.txt' '.rows[] | select(.path==\$p)'"
check "ignored target/, *.log hidden" test "$(tree rows | jq '[.rows[] | select(.path|test("/(target|debug.log)$"))] | length')" = 0
tree toggle_ignored >/dev/null
ignored_names() { tree rows | jq -c '[.rows[] | select(.ignored) | .path | split("/")[-1]]'; }
check "toggle shows ignored, marked" eventually 2 is ignored_names '["target","big.log","debug.log"]'
tree toggle_ignored >/dev/null
tree expand "$X/src" >/dev/null
tree activate "$X/src/main.rs" >/dev/null
check "click file → Viewer, focused" eventually 5 bash -c "[[ \$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == \"viewer\")][0].kind.path') == '$X/src/main.rs' ]]"
check "…same Viewer reused" test "$(viewer_id)" = "$v"
tree activate "$X/docs" >/dev/null
check "click folder → expands" test "$(row "$X/docs" | jq -r .expanded)" = true
icons() { tree rows | jq -c '[.rows[] | select(.depth==0 or (.path|endswith("/src/main.rs"))) | [(.path | split("/")[-1]), .icon]] | map(join("=")) | join(" ")'; }
check "Catppuccin icons: names, extensions, folders open/closed" is icons '"crates=folder_packages docs=folder_docs_open img=folder_images src=folder_src_open main.rs=rust .gitignore=git blob.bin=binary Cargo.toml=cargo README.md=readme"'
tree collapse "$X/src" >/dev/null
check "collapsed folder → closed icon" test "$(row "$X/src" | jq -r .icon)" = folder_src
tree expand "$X/src" >/dev/null
rade open "$X/README.md" --pane a >/dev/null
eventually 10 bash -c "rade debug viewer $v info | jq -e '.mermaid==\"done\"'" || true
sleep 1; shot tree-light
rade source add "$T" --name other >/dev/null 2>&1 || true
rade focus "$a"

echo "== R11: Viewer and tree survive a restart (dark appearance)"
stop_app
check "state.json has the Viewer" test "$(jq -r '[.. | objects | select(.kind?.type? == "viewer")][0].kind.path' "$STATE")" = "$X/README.md"
EDITOR=fake-editor ADW_DEBUG_COLOR_SCHEME=prefer-dark start_app
check "Viewer restored" test "$(viewer_id)" = "$v"
check "tree still shown" eventually 5 is 'tree rows | jq -r .root' "$X"
tree expand "$X/src" >/dev/null
tree expand "$X/docs" >/dev/null
check "dark: Cargo.toml keeps its cargo icon, painted for dark" eventually 5 is 'row "$X/Cargo.toml" | jq -r .icon' cargo
check "dark appearance follows the system" eventually 10 bash -c "rade debug viewer $v info | jq -e '.scheme==\"dark\" and .mermaid==\"done\" and (.images[0].loaded)'"
sleep 1; shot md-dark
rade open "$X/src/main.rs:42" --pane a >/dev/null
check "dark code scheme" eventually 5 bash -c "rade debug viewer $v info | jq -e '.scheme | test(\"dark\")'"
shot viewer-code-dark
shot tree-dark

echo "== R113: several Viewers per Tab, header zoom, content zoom"
nviewers() { viewers | jq length; }
check "R113-b one Viewer before" test "$(nviewers)" = 1
v2=$(rade open "$X/docs/PRD.md" --pane a --side --json | jq -r .pane)
check "R113-b rade open --side → a second Viewer (the first keeps main.rs)" \
    bash -c "[[ \$(rade debug state | jq '[.. | objects | select(.kind?.type? == \"viewer\")] | length') == 2 && '$v2' != '$v' ]]"
check "R113-b …the first still shows main.rs" test "$(vinfo "$v" | jq -r .path)" = "$X/src/main.rs"
rade debug viewer "$v2" zoom >/dev/null
check "R113-b the header Zoom button zooms the Viewer (the Tab shows only it)" \
    eventually 5 bash -c "[[ \$(rade debug viewer $v2 info | jq -r .zoomed) == true ]]"
shot viewer-zoomed
rade debug viewer "$v2" key Escape >/dev/null
check "R113-b Esc while zoomed restores the Layout, the Viewer stays" \
    eventually 5 bash -c "[[ \$(rade debug viewer $v2 info | jq -r .zoomed) == false ]]"
check "R113-b …still 2 Viewers" test "$(nviewers)" = 2
rade debug viewer "$v2" key Escape >/dev/null
check "R113-b a second Esc closes it" eventually 5 is nviewers 1
tree activate_side "$X/README.md" >/dev/null
check "R113-c the Files tree's Open to the Side (middle-click) → a new Viewer on README.md" \
    eventually 5 bash -c "[[ \$(rade debug state | jq '[.. | objects | select(.kind?.type? == \"viewer\")] | length') == 2 ]]"
check "R113-c …the first Viewer kept main.rs" test "$(vinfo "$v" | jq -r .path)" = "$X/src/main.rs"
echo "== R113.5: content zoom"
check "R113-e code view: zoom 100% shown" eventually 5 bash -c "[[ \$(rade debug viewer $v info | jq -r .content_zoom) == '100%' ]]"
rade debug viewer "$v" content_zoom in >/dev/null; rade debug viewer "$v" content_zoom in >/dev/null
check "R113-e two steps in → 125%" eventually 3 bash -c "[[ \$(rade debug viewer $v info | jq -r .content_zoom) == '125%' ]]"
shot viewer-code-125
rade debug viewer "$v" key ctrl+0 >/dev/null
check "R113-e Ctrl+0 → 100%" eventually 3 bash -c "[[ \$(rade debug viewer $v info | jq -r .content_zoom) == '100%' ]]"
rade open "$X/img/logo.png" --pane "$v" >/dev/null
check "R113-e an image opens fitted" eventually 5 bash -c "[[ \$(rade debug viewer $v info | jq -r .content_zoom) == Fit ]]"
rade debug viewer "$v" content_zoom in >/dev/null
check "R113-e zoom in from Fit → 110% of the 48 px image (53 px)" \
    eventually 3 bash -c "[[ \$(rade debug viewer $v info | jq -c '[.content_zoom, .image_size]') == '[\"110%\",[52,52]]' ]]"
for i in 1 2 3 4 5 6; do rade debug viewer "$v" key ctrl+plus >/dev/null; done
check "R113-e Ctrl++ six more → 400% (192 px)" eventually 3 bash -c "[[ \$(rade debug viewer $v info | jq -c '[.content_zoom, .image_size]') == '[\"400%\",[192,192]]' ]]"
shot image-400
rade open "$X/README.md" --pane "$v" >/dev/null
check "R113-e Markdown: the zoom applies to the page" eventually 8 bash -c "[[ \$(rade debug viewer $v info | jq -r .content_zoom) == '400%' ]]"
rade debug viewer "$v" key ctrl+0 >/dev/null
stop_app

echo "== R7.1: Ctrl+click after the app's terminal closed (0.5.1 crash: eprintln! EIO → abort)"
EDITOR=fake-editor python3 "$HERE/ptyrun.py" "$T/hup" "$T/hup.rc" "$BIN/rade-app" &
APP_PID=$!
eventually 15 rade list
rade send "$a" "clear; printf 'hup https://example.com/hup\\n'" --enter
eventually 5 screen_has "$a" "hup https://example.com/hup"
touch "$T/hup"; sleep 0.5   # the terminal window closes: stderr now gives EIO
rade debug click "$a" "https://example.com/hup" >/dev/null || true
check "web link opens, app keeps running" eventually 5 grep -qx "https://example.com/hup" "$BROWSER_LOG"
sleep 0.5; check "app alive" eval "rade list >/dev/null"
stop_app
check "clean exit" test "$(cat "$T/hup.rc")" = 0

echo
echo "M4 E2E: $PASS passed, $FAIL failed (screenshots in ${OUT#$ROOT/})"
[[ $FAIL == 0 ]]
