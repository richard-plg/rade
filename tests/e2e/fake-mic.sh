#!/usr/bin/env bash
# Stand-in `record_command` for the voice E2E (R124): 16 kHz mono s16le on
# stdout at real-time pace until killed — a 440 Hz tone at amplitude
# $FAKE_MIC_LEVEL (0..1, default 0.3).
# FAKE_MIC_FAIL=1: "pw-record: no source" on stderr, exit 1.
if [[ -n ${FAKE_MIC_FAIL:-} ]]; then
    echo "pw-record: no source" >&2
    exit 1
fi
exec python3 -I -c '
import math, os, struct, sys, time
amp = float(os.environ.get("FAKE_MIC_LEVEL", "0.3")) * 32767
rate, step = 16000, 800  # 50 ms
t0, n = time.monotonic(), 0
out = sys.stdout.buffer
while True:
    out.write(struct.pack("<%dh" % step, *(int(amp * math.sin(2 * math.pi * 440 * (n + i) / rate)) for i in range(step))))
    out.flush()
    n += step
    wait = t0 + n / rate - time.monotonic()
    if wait > 0:
        time.sleep(wait)
'
