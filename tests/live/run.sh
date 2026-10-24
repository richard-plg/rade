#!/usr/bin/env bash
# Live E2E on the owner's real desktop (see tests/live/README.md).
# Usage: tests/live/run.sh [m1|…|m9|m11|m12|m13|m14|m14b|m15|m16|m24|m26|m28|m29|m30|m31|m40|m44|all]...   (NO_BUILD=1, KEEP=1)
set -uo pipefail # no -e: a failed check must not skip the rest (or cleanup)
source "$(dirname "$0")/lib.sh"
ms=("$@")
[[ ${#ms[@]} -eq 0 || ${ms[0]} == all ]] && ms=(m1 m2 m3 m4 m5 m6 m7 m8 m9 m11 m12 m13 m14 m14b m15 m16 m24 m26 m28 m29 m30 m31 m40 m44)
# Plain US layout while typing commands; tests that need Bamboo switch to it.
ib engine xkb:us::eng
start_app
for m in "${ms[@]}"; do
    f=$HERE/$m.sh
    [[ -f $m ]] && f=$m # or a script path (iterating on one check)
    [[ -f $f ]] || die "no such milestone: $m"
    echo "== ${m^^}"
    source "$f"
done
summary
