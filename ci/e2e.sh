#!/usr/bin/env bash
# Run E2E suites, each in its own fresh container of an image built from
# ci/Dockerfile (target e2e or deb). Runs on the CI runner (needs only bash +
# docker) or locally to reproduce CI. Outputs (logs, screenshots) are copied
# to ci-out/<suite>/ for upload.
#   ci/e2e.sh <image> <suite>...      e.g. ci/e2e.sh rade-ci:e2e run m2 m3
# E2E_JOBS (default 4) suites run at once: llvmpipe is CPU-bound and some
# suites assert timings (m8, m11). Each suite gets E2E_SUITE_LIMIT (default 20m,
# PRD R140.4). A failed suite is retried once, serially, only when ci/flaky.txt
# names it (R143.2); otherwise the failure stands.
set -uo pipefail
IMAGE=${1:?usage: ci/e2e.sh <image> <suite>...}; shift
JOBS=${E2E_JOBS:-4}
LIMIT=${E2E_SUITE_LIMIT:-20m}
FLAKY=${E2E_FLAKY:-$(dirname "$0")/flaky.txt}
OUTDIR=${OUTDIR:-ci-out}
mkdir -p "$OUTDIR"
TAG=$(tr -dc a-z0-9 </dev/urandom | head -c6)

one() {
    local s=$1 c="rade-e2e-$TAG-$1" t0=$SECONDS rc
    # --init: reap the app/weston children; shm: WebKit wants more than 64 MB.
    # E2E_SKIP_PERF passes through (tests/e2e/lib.sh perf_check).
    timeout --foreground "$LIMIT" \
        docker run --name "$c" --init --shm-size=1g -e "E2E_SKIP_PERF=${E2E_SKIP_PERF:-}" \
        "$IMAGE" "tests/e2e/$s.sh" >"$OUTDIR/$s.log" 2>&1
    rc=$?
    [[ $rc == 124 ]] && echo "e2e: $s stopped after $LIMIT (E2E_SUITE_LIMIT)" >>"$OUTDIR/$s.log"
    docker stop -t 5 "$c" >/dev/null 2>&1 # a timed-out container still runs
    mkdir -p "$OUTDIR/$s"
    docker cp "$c:/src/target/." "$OUTDIR/$s/" >/dev/null 2>&1 || true
    rm -rf "$OUTDIR/$s/release" "$OUTDIR/$s/debian" "$OUTDIR/$s/deb-build"
    docker rm -f "$c" >/dev/null 2>&1
    printf '%-7s %s  %4ss  %s\n' "$s" "$([[ $rc == 0 ]] && echo PASS || echo FAIL)" $((SECONDS - t0)) \
        "$(grep -E 'passed, [0-9]+ failed' "$OUTDIR/$s.log" | tail -1)"
    return $rc
}
# Whether a failed suite may be retried: named in the flaky list (R143.2).
retry_allowed() { grep -qE "^$1([[:space:]]|\$)" "$FLAKY" 2>/dev/null; }
export -f one; export IMAGE OUTDIR TAG LIMIT

printf '%s\n' "$@" | xargs -P "$JOBS" -I{} bash -c 'one {}' | tee "$OUTDIR/summary.txt"
status=0
for s in "$@"; do
    grep -q "^$s .*PASS" "$OUTDIR/summary.txt" && continue
    echo "::group::$s.log (failed)"; grep -E 'FAIL|^==' "$OUTDIR/$s.log"; tail -n 40 "$OUTDIR/$s.log"; echo "::endgroup::"
    if ! retry_allowed "$s"; then
        echo "::error::E2E $s failed (not in ci/flaky.txt: no retry)"
        status=1
        continue
    fi
    # One serial retry, flagged, for a suite with a known flake (ci/flaky.txt).
    mv "$OUTDIR/$s.log" "$OUTDIR/$s.try1.log"
    if one "$s" | tee -a "$OUTDIR/summary.txt"; then
        echo "::warning::E2E $s failed under load, passed on a serial retry (ci/flaky.txt; see $s.try1.log)"
    else
        status=1
    fi
done
exit "$status"
