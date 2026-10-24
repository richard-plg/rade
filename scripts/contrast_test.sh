#!/usr/bin/env bash
# Test of docs/internals/design/contrast.py against crates/rade-app/src/tokens.toml (PRD R102-c).
# Usage: scripts/contrast_test.sh   (exit 1 if any check fails)
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root="$here/.."
tokens="$root/crates/rade-app/src/tokens.toml"
work_base=${TMPDIR:-"$root/target/tmp"}
mkdir -p "$work_base"
work=$(mktemp -d "$work_base/contrast_test.XXXXXX")
trap 'rm -rf "$work"' EXIT

pass=0 fail=0
check() { # name, command...
    local name=$1; shift
    if "$@"; then pass=$((pass + 1)); echo "ok   $name"
    else fail=$((fail + 1)); echo "FAIL $name"; fi
}
run() { rc=0; out=$(python3 "$root/docs/internals/design/contrast.py" "$@" 2>&1) || rc=$?; }

real_tokens_pass() {
    run "$tokens"
    [ "$rc" = 0 ] && ! grep -q '^LOW' <<<"$out" \
        && grep -q 'OK .* dark fg3 on bg3' <<<"$out" && grep -q 'OK .* light acc-fg on acc' <<<"$out"
}
mockup_fg3_fails() {
    # The mockup's dark fg3 (#5e6773) on bg3 is 2.95 : 1 (spec README, AA column).
    sed '/^\[dark\]/,/^\[light\]/ s/^fg3 = .*/fg3 = "#5e6773"/' "$tokens" >"$work/low.toml"
    run "$work/low.toml"
    [ "$rc" = 1 ] && grep -qE '^LOW +2\.9[0-9] \(need 4\.5\) dark fg3 on bg3$' <<<"$out" \
        && ! grep -q 'LOW .* light fg3' <<<"$out"
}
malformed_is_2() {
    printf '[dark]\nfg = "#fff"\n[light]\nfg = "#000000"\n[contrast]\ntext = ["fg on bg0"]\n' >"$work/bad.toml"
    run "$work/bad.toml"
    [ "$rc" = 2 ]
}

check "R102-c contrast.py passes every pair of tokens.toml in both themes" real_tokens_pass
check "R102-c dark fg3 = #5e6773 on bg3 exits 1 naming that pair" mockup_fg3_fails
check "contrast.py exits 2 on a malformed tokens file" malformed_is_2

echo "contrast_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
