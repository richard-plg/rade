#!/usr/bin/env bash
# M24 spike (docs/internals/research/webkit-compat.md): open public pages in a Browser
# Pane (headless Weston, temp dirs like the E2E suites, nobody logged in),
# probe web features with compat-probe.js, keep console + screenshot. Each
# site gets a fresh app (a crash on one must not hide the others).
# Usage: tests/research/webkit-compat.sh [name…]   → target/research-webkit/
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/research-webkit
source "$HERE/../e2e/lib.sh"
PROBE=$(cat "$HERE/compat-probe.js")
mkdir -p "$T/p"; git -C "$T/p" init -q
while read -r name url wait; do
  [[ -z $name || $name == \#* ]] && continue
  [[ $# -gt 0 && " $* " != *" $name "* ]] && continue
  echo "== $name $url"
  rm -rf "$XDG_DATA_HOME/rade-dev"
  start_app
  rade source add "$T/p" >/dev/null; rade spawn --cmd bash --source p --name a >/dev/null
  timeout 60 rade browser open "$url" --json >"$OUT/$name.open.json" 2>"$OUT/$name.err" </dev/null || echo "   open failed: $(tail -1 "$OUT/$name.err")"
  sleep "${wait:-5}"
  if ! rade list >/dev/null 2>&1; then
    echo "   CRASHED: rade-app died (see app.log)"; wait "$APP_PID" 2>/dev/null; APP_PID=; continue
  fi
  timeout 40 rade browser eval "$PROBE" --json </dev/null >"$OUT/$name.probe.json" 2>>"$OUT/$name.err" || echo "   probe failed: $(tail -1 "$OUT/$name.err")"
  timeout 20 rade browser console </dev/null >"$OUT/$name.console.txt" 2>&1
  timeout 20 rade browser screenshot "$OUT/$name.png" </dev/null >/dev/null 2>&1
  jq -c '.result | del(.ua)' "$OUT/$name.probe.json" 2>/dev/null | cut -c1-900
  rade list >/dev/null 2>&1 || echo "   CRASHED after the probe"
  stop_app
done < "$HERE/compat-sites.txt"
jq -r .result.ua "$OUT"/*.probe.json 2>/dev/null | sort -u
