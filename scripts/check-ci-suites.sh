#!/usr/bin/env bash
# Every E2E suite runs in CI (PRD R143.1). A suite = a tests/e2e/*.sh that sources
# lib.sh, except the parts `<suite>-<part>.sh` run by their `<suite>.sh`. CI = the
# `suites:` lists of .github/workflows/ci.yml and the `ci/e2e.sh <image> <suites>`
# lines of the other workflows. Exit 1 naming each suite outside CI.
#   scripts/check-ci-suites.sh [--root DIR] [--list]   (--list: print the suites CI runs)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
list=
while (($#)); do
    case $1 in
        --root) root=${2:?--root needs a value}; shift 2 ;;
        --list) list=1; shift ;;
        *) echo "usage: scripts/check-ci-suites.sh [--root DIR] [--list]" >&2; exit 2 ;;
    esac
done
ci_suites() {
    local f
    for f in "$root"/.github/workflows/*.yml; do
        sed -nE 's/^[[:space:]]*suites:[[:space:]]*(.*)$/\1/p' "$f"
        sed -nE 's/.*ci\/e2e\.sh[[:space:]]+"[^"]*"[[:space:]]+([^$]*)$/\1/p' "$f"
    done | tr -s ' \t' '\n' | grep -E '^[A-Za-z0-9_-]+$' | sort -u
}
if [[ -n $list ]]; then ci_suites; exit 0; fi
in_ci=$(ci_suites)
missing=0
for f in "$root"/tests/e2e/*.sh; do
    grep -qE '^[[:space:]]*(source|\.)[[:space:]].*lib\.sh' "$f" || continue
    s=$(basename "$f" .sh)
    [[ $s == *-* && -f "$root/tests/e2e/${s%%-*}.sh" ]] && continue # a part of its suite
    grep -qxF "$s" <<<"$in_ci" && continue
    echo "check-ci-suites: tests/e2e/$s.sh is in no CI shard (.github/workflows/ci.yml)" >&2
    missing=1
done
exit "$missing"
