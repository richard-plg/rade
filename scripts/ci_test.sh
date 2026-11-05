#!/usr/bin/env bash
# Tests of the CI scripts (PRD R143): scripts/check-ci-suites.sh and ci/e2e.sh's
# retry rule, run against a fake `docker` and a temp repo layout.
# Usage: scripts/ci_test.sh   (exit 1 if any check fails)
set -uo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root="$here/.."
work_base=${TMPDIR:-"$root/target/tmp"}
mkdir -p "$work_base"
work=$(mktemp -d "$work_base/ci_test.XXXXXX")
trap 'rm -rf "$work"' EXIT
pass=0 fail=0
check() { # name, command...
    local name=$1; shift
    if "$@"; then pass=$((pass + 1)); echo "ok   $name"
    else fail=$((fail + 1)); echo "FAIL $name"; fi
}

# A temp repo: two suites + a part, a workflow listing some of them.
layout() { # suites listed in ci.yml...
    rm -rf "$work/repo"; mkdir -p "$work/repo/tests/e2e" "$work/repo/.github/workflows"
    local s
    for s in ma mb mb-part; do printf '#!/bin/bash\nsource "$(dirname "$0")/lib.sh"\n' >"$work/repo/tests/e2e/$s.sh"; done
    printf '#!/bin/bash\n' >"$work/repo/tests/e2e/lib.sh" # a helper, not a suite
    printf 'jobs:\n  e2e:\n    strategy:\n      matrix:\n        include:\n          - shard: a\n            suites: %s\n' "$*" \
        >"$work/repo/.github/workflows/ci.yml"
}
all_listed_passes() { layout ma mb; "$root/scripts/check-ci-suites.sh" --root "$work/repo" 2>/dev/null; }
missing_is_named() {
    layout ma; local out rc=0
    out=$("$root/scripts/check-ci-suites.sh" --root "$work/repo" 2>&1) || rc=$?
    [ "$rc" = 1 ] && grep -q 'tests/e2e/mb.sh is in no CI shard' <<<"$out" && ! grep -q 'mb-part' <<<"$out"
}
real_repo_passes() { "$root/scripts/check-ci-suites.sh" >/dev/null; }
check "R143-a every suite listed in a shard passes" all_listed_passes
check "R143-a a suite in no shard fails, named (parts and lib.sh are not suites)" missing_is_named
check "R143-a this repo's suites are all in CI" real_repo_passes

# A fake docker: `run … tests/e2e/<s>.sh` fails on the first run of <s>, passes after.
mkdir -p "$work/bin"
cat >"$work/bin/docker" <<'SH'
#!/usr/bin/env bash
[[ $1 == run ]] || exit 0
s=$(basename "${@: -1}" .sh); n="$FAKE_DIR/$s.runs"
c=$(( $(cat "$n" 2>/dev/null || echo 0) + 1 )); echo "$c" >"$n"
if [[ $c == 1 ]]; then echo "1 passed, 1 failed"; exit 1; fi
echo "2 passed, 0 failed"; exit 0
SH
chmod +x "$work/bin/docker"
e2e() { # flaky-list contents → exit code of ci/e2e.sh on suite "mx"
    rm -rf "$work/fake" "$work/out"; mkdir -p "$work/fake"
    printf '%s\n' "$1" >"$work/flaky.txt"
    PATH="$work/bin:$PATH" FAKE_DIR="$work/fake" E2E_FLAKY="$work/flaky.txt" OUTDIR="$work/out" E2E_JOBS=1 \
        "$root/ci/e2e.sh" fake-image mx >"$work/e2e.log" 2>&1
}
unlisted_fails() { local rc=0; e2e "# nothing" || rc=$?; [ "$rc" = 1 ] && [ "$(cat "$work/fake/mx.runs")" = 1 ] && grep -q 'not in ci/flaky.txt' "$work/e2e.log"; }
listed_retries() { e2e "mx  a known flake" && [ "$(cat "$work/fake/mx.runs")" = 2 ] && grep -q 'passed on a serial retry' "$work/e2e.log"; }
prefix_not_listed() { local rc=0; e2e "mxy  another suite" || rc=$?; [ "$rc" = 1 ]; }
check "R143-c a suite that fails then would pass is a failure when not in ci/flaky.txt" unlisted_fails
check "R143-c a suite in ci/flaky.txt is retried once and passes" listed_retries
check "R143-c a flaky entry for another suite (mxy) does not cover mx" prefix_not_listed

# trace.sh --strict: an AC proven only by a suite outside CI fails (R143-b).
trace_root() { # suites in ci.yml...
    rm -rf "$work/tr"; cp -r "$root/scripts/testdata/trace/basic" "$work/tr"
    mkdir -p "$work/tr/.github/workflows"
    printf 'jobs:\n  e2e:\n    strategy:\n      matrix:\n        include:\n          - shard: a\n            suites: %s\n' "$*" \
        >"$work/tr/.github/workflows/ci.yml"
    "$root/scripts/trace.sh" --root "$work/tr" --no-write --strict 2>&1 >/dev/null
}
outside_named() { local out; out=$(trace_root other) || true; grep -q 'proven only by suites outside CI (--strict): R97-b (mX)' <<<"$out"; }
inside_quiet() { local out; out=$(trace_root mX) || true; ! grep -q 'outside CI' <<<"$out"; }
check "R143-b trace --strict names an AC whose only suite is outside CI" outside_named
check "R143-b trace --strict is quiet once that suite is in a shard" inside_quiet

echo "ci_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
