#!/usr/bin/env bash
# Test of scripts/semver.sh and the release version check (PRD R145-a).
# Usage: scripts/semver_test.sh   (exit 1 if any check fails)
set -uo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
pass=0 fail=0
check() { # name, command...
    local name=$1; shift
    if "$@"; then pass=$((pass + 1)); echo "ok   $name"
    else fail=$((fail + 1)); echo "FAIL $name"; fi
}
gt() { "$here/semver.sh" gt "$1" "$2"; }
not_gt() { ! "$here/semver.sh" gt "$1" "$2"; }
release_after_rc() { gt 1.6.0 1.6.0-rc.1 && not_gt 1.5.9 1.6.0-rc.1 && not_gt 1.6.0-rc.1 1.6.0; }
precedence() {
    gt 1.10.0 1.9.0 && gt 1.0.0-rc.10 1.0.0-rc.2 && gt 1.0.0-rc.1 1.0.0-beta \
        && gt 1.0.0-alpha.1 1.0.0-alpha && gt 1.0.0-alpha.beta 1.0.0-alpha.1 && not_gt 1.2.3 1.2.3
}
bad_version() { "$here/semver.sh" gt 1.2 1.0.0 2>/dev/null; [ $? = 2 ]; }
check "R145-a 1.6.0 is above 1.6.0-rc.1, 1.5.9 is not" release_after_rc
check "R145-a SemVer precedence (numeric ids, pre-release fields)" precedence
check "R145-a a version that is not SemVer exits 2" bad_version
echo "semver_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
