#!/usr/bin/env bash
# M24 spike, the other half (docs/research/webkit-compat.md): the same
# public pages and probe as webkit-compat.sh, in `rade browser --engine
# chromium` (headless, temp profile, temp XDG dirs; no RADE app needed).
# Usage: tests/research/chromium-compat.sh [chrome]   → target/research-chromium/
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=$ROOT/target/research-chromium
CHROME=${1:-$(ls -d "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux*/chrome 2>/dev/null | tail -1)}
[[ -x $CHROME ]] || { echo "no Chromium (pass its path)"; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT"
T=$(mktemp -d /tmp/rade-crc.XXXXXX)
trap 'timeout 20 "$ROOT/target/release/rade" browser --engine chromium close </dev/null >/dev/null 2>&1; rm -rf "$T"' EXIT
mkdir -p "$T/run" "$T/config/rade" && chmod 700 "$T/run"
export XDG_RUNTIME_DIR=$T/run XDG_CONFIG_HOME=$T/config XDG_CACHE_HOME=$T/cache
unset RADE_SOCKET RADE_PANE_ID WAYLAND_DISPLAY DISPLAY
printf '[browser]\nchromium = "%s"\n' "$CHROME" >"$XDG_CONFIG_HOME/rade/config.toml"
cr() { timeout 60 "$ROOT/target/release/rade" browser --engine chromium "$@" </dev/null; }
PROBE=$(cat "$HERE/compat-probe.js")
while read -r name url wait; do
  [[ -z $name || $name == \#* ]] && continue
  [[ $# -gt 1 && " ${*:2} " != *" $name "* ]] && continue
  echo "== $name $url"
  cr open "$url" --json >"$OUT/$name.open.json" 2>"$OUT/$name.err" || echo "   open failed: $(tail -1 "$OUT/$name.err")"
  sleep "${wait:-5}"
  cr eval "$PROBE" --json >"$OUT/$name.probe.json" 2>>"$OUT/$name.err" || echo "   probe failed: $(tail -1 "$OUT/$name.err")"
  cr console >"$OUT/$name.console.txt" 2>&1
  cr screenshot "$OUT/$name.png" >/dev/null 2>&1
  jq -c '.result | del(.ua)' "$OUT/$name.probe.json" 2>/dev/null | cut -c1-900
done < "$HERE/compat-sites.txt"
jq -r .result.ua "$OUT"/*.probe.json 2>/dev/null | sort -u
