#!/usr/bin/env bash
# M49 end-to-end tests: one Agent per Pane (m49-identity.sh, R128), Tab glyphs
# (m49-tabs.sh, R115.3) and Files actions (m49-files.sh, R129), each in its own
# private session → target/e2e-m49/<part>/.
# Usage: tests/e2e/m49.sh   (PARTS="identity files" to run some; NO_BUILD=1, KEEP=1)
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
[[ ${NO_BUILD:-} == 1 ]] || cargo build --release --manifest-path "$HERE/../../Cargo.toml" -q || exit 1
rc=0
for part in ${PARTS:-identity tabs files}; do
    echo "=== m49-$part"
    NO_BUILD=1 "$HERE/m49-$part.sh" || rc=1
done
exit $rc
