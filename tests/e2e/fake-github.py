#!/usr/bin/env python3
"""A fake GitHub for the M48 suite (R127): the releases API and the download host.

  fake-github.py <port-file> <log> <dir>

  GET /repos/richard-plg/rade/releases[?…]  <dir>/releases.json, read on each request
                                            (500 while <dir>/fail exists)
  GET /download/<file>                      302 to /assets/<file>, like github.com
  GET /assets/<file>                        <dir>/<file>; with <dir>/slow in 64 KB chunks,
                                            100 ms apart
Log: one line per request, `<n> <method> <path> ua=<User-Agent> auth=<yes|no>`.
The port is written to <port-file> once the socket listens.
"""
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT_FILE, LOG, DIR = sys.argv[1:4]
LOCK = threading.Lock()
COUNT = [0]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _log(self):
        with LOCK:
            COUNT[0] += 1
            line = "%d %s %s ua=%s auth=%s\n" % (
                COUNT[0],
                self.command,
                self.path,
                self.headers.get("User-Agent", "-"),
                "yes" if self.headers.get("Authorization") else "no",
            )
            with open(LOG, "a") as f:
                f.write(line)

    def _send(self, code, body=b"", ctype="application/octet-stream", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._log()
        path = self.path.split("?", 1)[0]
        if path == "/repos/richard-plg/rade/releases":
            if os.path.exists(os.path.join(DIR, "fail")):
                return self._send(500, b'{"message":"fake failure"}', "application/json")
            with open(os.path.join(DIR, "releases.json"), "rb") as f:
                return self._send(200, f.read(), "application/json")
        if path.startswith("/download/"):
            return self._send(302, extra=[("Location", "/assets/" + path[len("/download/"):])])
        if path.startswith("/assets/"):
            name = os.path.basename(path)
            try:
                with open(os.path.join(DIR, name), "rb") as f:
                    data = f.read()
            except OSError:
                return self._send(404, b"not found", "text/plain")
            if not os.path.exists(os.path.join(DIR, "slow")):
                return self._send(200, data)
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            try:
                for i in range(0, len(data), 65536):
                    self.wfile.write(data[i:i + 65536])
                    self.wfile.flush()
                    time.sleep(0.1)
            except OSError:
                pass
            return
        self._send(404, b"not found", "text/plain")


srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
tmp = PORT_FILE + ".tmp"
with open(tmp, "w") as f:
    f.write(str(srv.server_address[1]))
os.rename(tmp, PORT_FILE)
srv.serve_forever()
