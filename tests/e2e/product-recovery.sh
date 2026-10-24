#!/usr/bin/env bash
# P1-01: actionable Session Restore faults without losing the original state.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(cd "$HERE/../.." && pwd)/target/e2e-product-recovery
source "$HERE/lib.sh"
ui() { rpc debug.ui "{\"op\":\"${1:-chrome}\"}"; }
warning() { ui | jq -e '.chrome.session.save_warning | length > 0' >/dev/null; }
warning_contains() { ui | jq -e --arg text "$1" ' .chrome.session.save_warning | contains($text) ' >/dev/null; }
clear_warning() { ui | jq -e '.chrome.session.save_warning == ""' >/dev/null; }
recovery_notice() { ui | jq -e '.chrome.session.recovery | contains("fresh session") and contains("Recovery copy:")' >/dev/null; }
STATE_DIR=$(dirname "$STATE")
# Keep the harness able to remove the isolated tree even if a check aborts.
trap 'chmod u+rwx "$STATE_DIR" 2>/dev/null || true; cleanup' EXIT
mkdir -p "$STATE_DIR"

# Repeated corrupt launches never replace an earlier recovery copy.
printf '{' > "$STATE"
printf 'earlier backup' > "$STATE.recovery-1.bak"
start_app
check "load failure explains fresh session and recovery" recovery_notice
check "old recovery copy survives" test "$(cat "$STATE.recovery-1.bak")" = 'earlier backup'
check "corrupt bytes preserved" test "$(cat "$STATE.recovery-2.bak")" = '{'
shot corrupt-session
stop_app

# Future-version bytes also survive even when fresh state is saved.
jq '.version = 99' "$STATE" > "$T/future-state.json"
cp "$T/future-state.json" "$STATE"
start_app
check "future version preserved" cmp -s "$STATE.recovery-3.bak" "$T/future-state.json"
stop_app

# A readable future session in an unwritable directory cannot be backed up.
# Existing state.lock allows startup; no .tmp file exists to mask EACCES.
cp "$T/future-state.json" "$STATE"
chmod u-w "$STATE_DIR"
start_app
check "future backup failure warns" warning
ui retry_save >/dev/null
check "blocked future state stays byte-identical" cmp -s "$STATE" "$T/future-state.json"
check "permission error explained" warning_contains "Permission denied"
shot blocked-future-session
chmod u+w "$STATE_DIR"
ui retry_save >/dev/null
check "Retry preserves future state after permissions fixed" cmp -s "$STATE.recovery-4.bak" "$T/future-state.json"
check "Retry clears blocked-future warning" clear_warning
stop_app

# A directory at the state path prevents both loading and backing up.
# Automatic and manual saves must refuse to replace it until backup succeeds.
rm "$STATE"
mkdir "$STATE"
printf 'keep' > "$STATE/original"
start_app
check "backup failure warns" eventually 8 warning
ui retry_save >/dev/null
check "Retry does not overwrite unreadable state" test "$(cat "$STATE/original")" = keep
shot blocked-recovery
# Simulate fixing the original path while the app is still running.
mv "$STATE" "$T/unreadable-state"
printf '{' > "$STATE"
ui retry_save >/dev/null
check "Retry preserves original before fresh save" test "$(cat "$STATE.recovery-5.bak")" = '{'
check "Retry clears save warning" clear_warning
check "recovery information remains" recovery_notice
stop_app

# Saving a valid session can fail after startup; Retry writes the current model.
start_app
cp "$STATE" "$T/before-denied.json"
chmod u-w "$STATE_DIR"
rade spawn --tab new >/dev/null
check "write failure produces persistent warning" eventually 8 warning
sleep 1
check "warning remains visible" warning
shot save-failure
check "permission-denied save explained" warning_contains "Permission denied"
check "failed save preserves previous bytes" cmp -s "$STATE" "$T/before-denied.json"
chmod u+w "$STATE_DIR"
ui retry_save >/dev/null
check "successful Retry clears warning" clear_warning
check "state was saved as current version" test "$(jq .version "$STATE")" = 3
shot saved-session
stop_app

# Inject the real ENOSPC errno only on writes to this isolated state's temp file.
# No mounts, disk filling, or interference with other app/agent writes.
cat > "$T/enospc.c" <<'C'
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
ssize_t write(int fd, const void *buf, size_t count) {
    ssize_t (*real_write)(int, const void *, size_t) = dlsym(RTLD_NEXT, "write");
    const char *target = getenv("RADE_TEST_ENOSPC_PATH");
    const char *marker = getenv("RADE_TEST_ENOSPC_MARKER");
    if (target && marker && access(marker, F_OK) == 0) {
        char link[64], path[4096];
        snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
        ssize_t n = readlink(link, path, sizeof(path) - 1);
        if (n >= 0) {
            path[n] = 0;
            if (strcmp(path, target) == 0) { errno = ENOSPC; return -1; }
        }
    }
    return real_write(fd, buf, count);
}
C
cc -shared -fPIC "$T/enospc.c" -ldl -o "$T/enospc.so"
env LD_PRELOAD="$T/enospc.so" RADE_TEST_ENOSPC_PATH="$STATE_DIR/.state.json.tmp" \
    RADE_TEST_ENOSPC_MARKER="$T/disk-full" "$BIN/rade-app" >>"$OUT/app.log" 2>&1 &
APP_PID=$!
check "fault-injection app started" eventually 15 rade list
# Let startup's normal save finish before turning the disk-full fault on.
sleep 4
cp "$STATE" "$T/before-full.json"
touch "$T/disk-full"
rade spawn --tab new >/dev/null
check "disk-full save produces warning" eventually 8 warning
check "ENOSPC is explained" warning_contains "No space left on device"
check "disk-full save preserves previous bytes" cmp -s "$STATE" "$T/before-full.json"
shot disk-full
rm "$T/disk-full"
ui retry_save >/dev/null
check "Retry after ENOSPC clears warning" clear_warning
stop_app

echo "$PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
