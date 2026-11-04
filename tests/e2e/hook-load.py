#!/usr/bin/env python3
"""Agents reporting like installed hooks (R142-a): every `--every` ms each
Pane gets one `pane.set_status` call (the params `rade status --hook` sends,
with the `reporter` of its Agent process), `running` 49 times out of 50
and `done` once (a turn of 10 s), for `--for` s. One thread per slice of Panes, one call at a
time per Pane. Prints {"calls": n, "errors": n}.
Usage: hook-load.py <socket> <pane>... [--every 200] [--for 30] [--agent claude]"""
import json, socket, sys, threading, time

sock = sys.argv[1]
rest = sys.argv[2:]
opts = {}
panes = []
i = 0
while i < len(rest):
    if rest[i].startswith("--"):
        opts[rest[i]] = rest[i + 1]
        i += 2
    else:
        panes.append(rest[i])
        i += 1
every = float(opts.get("--every", 200)) / 1000
total = float(opts.get("--for", 30))
agent = opts.get("--agent", "claude")
stats = {"calls": 0, "errors": 0}
lock = threading.Lock()


def call(pane, status):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(10)
    s.connect(sock)
    params = {"pane": pane, "status": status, "agent": agent,
              "reporter": {"binary": agent, "headless": False}}
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "pane.set_status", "params": params}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    return json.loads(buf)


def run(slice_, offset):
    t0 = time.monotonic()
    n = 0
    while time.monotonic() - t0 < total:
        started = time.monotonic()
        for k, p in enumerate(slice_):
            status = "done" if (n + k * 5 + offset) % 50 == 0 else "running"
            try:
                r = call(p, status)
                ok = "error" not in r
            except OSError:
                ok = False
            with lock:
                stats["calls"] += 1
                stats["errors"] += 0 if ok else 1
        n += 1
        time.sleep(max(0, every - (time.monotonic() - started)))


threads = [threading.Thread(target=run, args=(panes[k::6], k)) for k in range(6)]
for t in threads:
    t.start()
for t in threads:
    t.join()
print(json.dumps(stats))
