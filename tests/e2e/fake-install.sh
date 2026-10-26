#!/usr/bin/env bash
# Stand-in for `pkexec apt-get install -y --no-remove <deb>` in the M48 suite (R127):
# RADE_UPDATE_INSTALL points at this script (Devel builds only). Files in $RADE_E2E_T:
#   install.log      gets "<path> <sha256 of the file>"
#   install-sleep    seconds to wait (default 0)
#   install-stderr   copied to stderr when it exists
#   install-exit     the exit code (default 0)
#   install-version  copied to dpkg-version on exit 0 (what fake-dpkg-query.sh reports)
deb=$1
echo "$deb $(sha256sum "$deb" | cut -d' ' -f1)" >>"$RADE_E2E_T/install.log"
sleep "$(cat "$RADE_E2E_T/install-sleep" 2>/dev/null || echo 0)"
[[ -f $RADE_E2E_T/install-stderr ]] && cat "$RADE_E2E_T/install-stderr" >&2
code=$(cat "$RADE_E2E_T/install-exit" 2>/dev/null || echo 0)
if [[ $code == 0 && -f $RADE_E2E_T/install-version ]]; then
    cp "$RADE_E2E_T/install-version" "$RADE_E2E_T/dpkg-version"
fi
exit "$code"
