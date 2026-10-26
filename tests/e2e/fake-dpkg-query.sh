#!/usr/bin/env bash
# Stand-in for `dpkg-query -W -f='${db:Status-Abbrev}${Version}' rade` in the M48 suite (R127):
# RADE_UPDATE_DPKG_QUERY points at this script (Devel builds only). Prints `ii <version>` when
# $RADE_E2E_T/dpkg-version holds one, nothing otherwise (= not installed).
v=$(cat "$RADE_E2E_T/dpkg-version" 2>/dev/null)
[[ -n $v ]] && printf 'ii %s' "$v"
exit 0
