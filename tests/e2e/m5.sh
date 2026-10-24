#!/usr/bin/env bash
# M5 end-to-end tests (ADR-0006): image paste as a file path (R5, ADR-0004).
# Headless Weston has no seat, so wl-copy can't reach it: `rade debug
# clipboard` puts the data on RADE's own GDK clipboard, then `rade debug
# action paste` runs Ghostty's paste binding on the focused Terminal — the
# code path of Ctrl+Shift+V. The Terminal runs a sink that enables bracketed
# paste and echoes its raw input through `cat -v`.
# Usage: tests/e2e/m5.sh   (NO_BUILD=1 to skip the build, KEEP=1 to keep the temp dir)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-m5
source "$HERE/lib.sh"

PASTE=$XDG_CACHE_HOME/rade/paste
F=$T/files
mkdir -p "$F"
python3 - "$F/shot.png" <<'PY'
import struct, sys, zlib
w = h = 32
raw = b"".join(b"\0" + bytes([200, 60, 40, 255] * w) for _ in range(h))
chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
cp "$F/shot.png" "$F/My Shot.png"
cp "$F/shot.png" "$F/it's.png"
printf 'hello from the clipboard' > "$F/text.txt"
printf '# copied in a file manager\r\nfile://%s\r\nfile://%s\r\nhttps://example.com/x.png\r\n' \
    "${F// /%20}/My%20Shot.png" "$F/it%27s.png" > "$F/uris"
printf 'https://example.com/x.png\r\n' > "$F/web-uris"
cat > "$T/sink.sh" <<'SH'
stty raw -echo
printf 'SINK READY\r\n\033[?2004h'
exec cat -v
SH

# The last bracketed paste on a sink's screen (cat -v shows ESC as ^[).
pasted() { rade read "$1" --scrollback | tr -d '\n' | sed -n 's/.*\^\[\[200~\(.*\)\^\[\[201~.*/\1/p'; }
has_paste() { [[ -n $(pasted "$1") ]]; }
# Pasted text split into words by a shell, one per line.
words() { eval "printf '%s\n' $(pasted "$1")"; }
sink() { # name → a focused Terminal running the sink
    local p
    p=$(rade spawn --cmd "bash $T/sink.sh" --name "$1" --json | jq -r .pane)
    rade focus "$p"
    eventually 10 screen_has "$p" 'SINK READY' || true
    echo "$p"
}
paste_into() { # pane, MIME=FILE… → paste
    local p=$1; shift
    rade debug clipboard "$@" >/dev/null
    rade debug action paste >/dev/null
    eventually 5 has_paste "$p" || true
}
is_png() { [[ $(head -c 8 "$1" | od -An -tx1 | tr -d ' \n') == 89504e470d0a1a0a ]]; }
mode() { stat -c %a "$1"; }

echo "== R5.4 setup: an 8-day-old and a 6-day-old paste file"
mkdir -p "$PASTE"
echo old > "$PASTE/20200101-000000-000.png"
touch -d '8 days ago' "$PASTE/20200101-000000-000.png"
echo recent > "$PASTE/recent.png"
touch -d '6 days ago' "$PASTE/recent.png"

echo "== start"
start_app
check "R5.4 8-day-old paste file deleted on startup" test ! -e "$PASTE/20200101-000000-000.png"
check "R5.4 6-day-old paste file kept" test -e "$PASTE/recent.png"
rm "$PASTE/recent.png"

echo "== R5.1 image-only clipboard → PNG saved, path pasted"
a=$(sink img)
paste_into "$a" "image/png=$F/shot.png"
img=$(pasted "$a")
echo "  pasted: $img"
check "bracketed paste of a path in the paste dir" bash -c "[[ '$img' == '$PASTE/'*.png ]]"
check "name is <yyyymmdd-hhmmss-ms>.png" bash -c "[[ \$(basename '$img') =~ ^[0-9]{8}-[0-9]{6}-[0-9]{3}\\.png\$ ]]"
check "file exists and is a PNG" is_png "$img"
check "file is 0600" test "$(mode "$img")" = 600
check "paste dir is 0700" test "$(mode "$PASTE")" = 700
check "image survives the round trip (32x32)" test "$(python3 -c "import struct,sys; d=open(sys.argv[1],'rb').read(24); print(*struct.unpack('>II', d[16:24]))" "$img")" = "32 32"
rade close "$a" --force >/dev/null

echo "== R5.1 text clipboard → text pasted"
a=$(sink text)
paste_into "$a" "text/plain=$F/text.txt"
check "text pasted as is" test "$(pasted "$a")" = 'hello from the clipboard'
rade close "$a" --force >/dev/null

echo "== R5.1 text + image → text wins"
a=$(sink both)
n_before=$(ls "$PASTE" | wc -l)
paste_into "$a" "image/png=$F/shot.png" "text/plain=$F/text.txt"
check "text pasted" test "$(pasted "$a")" = 'hello from the clipboard'
check "no image saved" test "$(ls "$PASTE" | wc -l)" = "$n_before"
rade close "$a" --force >/dev/null

echo "== R5.1 copied files (text/uri-list) → shell-escaped paths"
a=$(sink uris)
paste_into "$a" "text/uri-list=$F/uris" "text/plain=$F/text.txt"
echo "  pasted: $(pasted "$a")"
check "two local paths, escaped, web URL skipped" test "$(words "$a")" = "$F/My Shot.png
$F/it's.png"
rade close "$a" --force >/dev/null

echo "== R5.1 uri-list with no local files falls back to text"
a=$(sink weburis)
paste_into "$a" "text/uri-list=$F/web-uris" "text/plain=$F/text.txt"
check "text pasted" test "$(pasted "$a")" = 'hello from the clipboard'
rade close "$a" --force >/dev/null

echo "== R5.2 drop files → escaped paths"
a=$(sink drop)
rade debug drop "$a" "$F/My Shot.png" "$F/shot.png" >/dev/null
eventually 5 has_paste "$a" || true
check "dropped paths pasted (bracketed), escaped" test "$(words "$a")" = "$F/My Shot.png
$F/shot.png"
rade close "$a" --force >/dev/null

echo "== R5.2 drop image data (a browser) → saved PNG path"
a=$(sink dropimg)
rade debug drop "$a" --image "$F/shot.png" >/dev/null
eventually 5 has_paste "$a" || true
dimg=$(pasted "$a")
check "path in the paste dir, a PNG" bash -c "[[ '$dimg' == '$PASTE/'*.png ]] && [[ '$dimg' != '$img' ]]"
check "dropped image file is a PNG" is_png "$dimg"
rade close "$a" --force >/dev/null

stop_app

echo
echo "M5 E2E: $PASS passed, $FAIL failed (logs in ${OUT#$ROOT/})"
[[ $FAIL == 0 ]]
