#!/usr/bin/env bash
# Run E2E suites, each in its own fresh container of an image built from
# ci/Dockerfile (target e2e or deb). Runs on the CI runner (needs only bash +
# docker) or locally to reproduce CI. Outputs (logs, screenshots) are copied
# to ci-out/<suite>/ for upload.
#   ci/e2e.sh <image> <suite>...      e.g. ci/e2e.sh rade-ci:e2e run m2 m3
# E2E_JOBS (default 4) suites run at once: llvmpipe is CPU-bound and some
# suites assert timings (m8, m11).
set -uo pipefail
IMAGE=${1:?usage: ci/e2e.sh <image> <suite>...}; shift
JOBS=${E2E_JOBS:-4}
OUTDIR=${OUTDIR:-ci-out}
mkdir -p "$OUTDIR"
TAG=$(tr -dc a-z0-9 </dev/urandom | head -c6)

one() {
    local s=$1 c="rade-e2e-$TAG-$1" t0=$SECONDS rc
    # --init: reap the app/weston children; shm: WebKit wants more than 64 MB.
    # E2E_SKIP_PERF passes through (tests/e2e/lib.sh perf_check).
    docker run --name "$c" --init --shm-size=1g -e "E2E_SKIP_PERF=${E2E_SKIP_PERF:-}" \
        "$IMAGE" "tests/e2e/$s.sh" >"$OUTDIR/$s.log" 2>&1
    rc=$?
    mkdir -p "$OUTDIR/$s"
    docker cp "$c:/src/target/." "$OUTDIR/$s/" >/dev/null 2>&1 || true
    rm -rf "$OUTDIR/$s/release" "$OUTDIR/$s/debian" "$OUTDIR/$s/deb-build"
    docker rm -f "$c" >/dev/null 2>&1
    printf '%-7s %s  %4ss  %s\n' "$s" "$([[ $rc == 0 ]] && echo PASS || echo FAIL)" $((SECONDS - t0)) \
        "$(grep -E 'passed, [0-9]+ failed' "$OUTDIR/$s.log" | tail -1)"
    return $rc
}
export -f one; export IMAGE OUTDIR TAG

printf '%s\n' "$@" | xargs -P "$JOBS" -I{} bash -c 'one {}' | tee "$OUTDIR/summary.txt"
status=0
for s in "$@"; do
    grep -q "^$s .*PASS" "$OUTDIR/summary.txt" && continue
    echo "::group::$s.log (failed)"; grep -E 'FAIL|^==' "$OUTDIR/$s.log"; tail -n 40 "$OUTDIR/$s.log"; echo "::endgroup::"
    # One serial retry, flagged: under parallel load GLib's freedesktop backend
    # can send a second banner instead of replacing one whose Notify reply
    # hasn't arrived yet (m6, docs/RELEASING.md "Flaky E2E").
    mv "$OUTDIR/$s.log" "$OUTDIR/$s.try1.log"
    if one "$s" | tee -a "$OUTDIR/summary.txt"; then
        echo "::warning::E2E $s failed under load, passed on a serial retry (see $s.try1.log)"
    else
        status=1
    fi
done
exit "$status"
