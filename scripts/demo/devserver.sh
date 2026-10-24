#!/usr/bin/env bash
# A dev server for the demo scenes: Vite's banner, then a static server on
# the given port (default 5173) serving the current folder.
port=${1:-5173}
printf '\n  \e[32m\e[1mVITE\e[0m \e[32mv6.3.1\e[0m  ready in \e[1m412\e[0m ms\n\n'
printf '  \e[32m➜\e[0m  \e[1mLocal\e[0m:   \e[36mhttp://localhost:%s/\e[0m\n' "$port"
printf '  \e[2m➜  Network: use --host to expose\e[0m\n\n'
exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1
