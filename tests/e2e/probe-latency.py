#!/usr/bin/env python3
"""Main-loop latency probe (R130): call `pane.list` on the app's socket every
`--every` ms for `--for` s (each call is answered on the main thread), then
read `debug.restore`. Prints one JSON object: the probes as [t, latency] in
seconds since the probe started, and when the restore queue began (`shown`,
the window's first frame) on the same clock.
Usage: probe-latency.py <socket> [--every 50] [--for 30]"""
import json, socket, sys, time

sock = sys.argv[1]
args = dict(zip(sys.argv[2::2], sys.argv[3::2]))
every, total = float(args.get("--every", 50)) / 1000, float(args.get("--for", 30))


def call(method, timeout=30):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(timeout)
    s.connect(sock)
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(1 << 20)
        if not chunk:
            break
        buf += chunk
    s.close()
    return json.loads(buf)


t0 = time.monotonic()
probes = []
while time.monotonic() - t0 < total:
    start = time.monotonic()
    try:
        call("pane.list")
        probes.append([round(start - t0, 3), round(time.monotonic() - start, 3)])
    except OSError:
        pass  # not listening yet
    time.sleep(every)
r = call("debug.restore").get("result", {})
now = time.monotonic() - t0
shown = round(now - r.get("since_ms", 0) / 1000, 3) if r else None
print(json.dumps({"probes": probes, "shown": shown, "restore": r}))
