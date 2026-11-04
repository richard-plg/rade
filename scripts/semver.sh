#!/usr/bin/env bash
# SemVer 2.0.0 precedence (PRD R145.1), the same rules as rade_core::update::Version:
#   scripts/semver.sh gt A B   → exit 0 when A is above B, else 1 (2 on a bad version)
# A pre-release is below its release (1.6.0-rc.1 < 1.6.0); numeric identifiers
# compare as numbers, others as ASCII, numeric below alphanumeric, more fields above fewer.
set -euo pipefail
[[ ${1:-} == gt && $# == 3 ]] || { echo "usage: scripts/semver.sh gt A B" >&2; exit 2; }
exec python3 -I - "$2" "$3" <<'PY'
import re, sys
R = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$")
def key(v):
    m = R.match(v)
    if not m:
        print(f"semver: '{v}' is not SemVer", file=sys.stderr); sys.exit(2)
    core = tuple(int(x) for x in m.groups()[:3])
    pre = m.group(4)
    if pre is None:
        return core, (1,)
    ids = tuple((0, int(i), "") if i.isdigit() else (1, 0, i) for i in pre.split("."))
    return core, (0, ids)
sys.exit(0 if key(sys.argv[1]) > key(sys.argv[2]) else 1)
PY
