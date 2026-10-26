#!/usr/bin/env python3
"""Stand-in for Claude's usage endpoint (M39, R109-h): never the real one.

Usage: fake-usage-server.py <port-file> <log-file> [good-count]
Listens on 127.0.0.1 (a free port, written to <port-file>), answers the first
<good-count> (default 1) GETs with the known response shape (the GOOD
constant of crates/rade-core/src/usage/detailed.rs) and every later one with
an unexpected shape. Each request appends one line to <log-file>:
`<n> <path> auth=<yes|no>` (the token itself is never written).
"""
import datetime
import http.server
import sys

# The weekly limits reset 3 days from now (a fixed date expired on
# 2026-10-26 09:00 UTC and the stored limits were dropped as past).
WEEK = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=3)).replace(
    hour=9, minute=0, second=0, microsecond=0).isoformat()

GOOD = (
    '{"five_hour":{"utilization":62.0,"resets_at":"2026-10-23T15:12:00+00:00"},'
    '"seven_day":{"utilization":41.0,"resets_at":"' + WEEK + '"},'
    '"seven_day_oauth_apps":{"utilization":3.0,"resets_at":"' + WEEK + '"},'
    '"seven_day_opus":{"utilization":58.0,"resets_at":"' + WEEK + '"},'
    '"seven_day_sonnet":{"utilization":22.0,"resets_at":"' + WEEK + '"},'
    '"seven_day_cowork":null,"extra_usage":{"is_enabled":false},"new_field":1}'
)
BAD = '{"limits":[{"kind":"weekly","pct":"58"}],"version":2}'

port_file, log_file = sys.argv[1], sys.argv[2]
good = int(sys.argv[3]) if len(sys.argv) > 3 else 1
count = 0


class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        global count
        count += 1
        auth = self.headers.get("Authorization", "").startswith("Bearer ")
        with open(log_file, "a") as f:
            f.write(f"{count} {self.path} auth={'yes' if auth else 'no'}\n")
        body = (GOOD if count <= good else BAD).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


srv = http.server.HTTPServer(("127.0.0.1", 0), H)
with open(port_file, "w") as f:
    f.write(str(srv.server_address[1]))
srv.serve_forever()
