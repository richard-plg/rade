#!/usr/bin/env python3
"""Fake OpenAI-compatible transcription endpoint for the voice E2E (R124).

POST /v1/audio/transcriptions (multipart: file, model, response_format,
language, prompt). Answers the scripted texts in order (FAKE_STT_SCRIPT, a
JSON array; the last text repeats) as {"text": " <text> \n"} and appends one
JSON line per request to FAKE_STT_LOG: the form fields, the WAV's header
facts, the Authorization header. Not started = connection refused.
Usage: fake-stt.py [port]   (default 8178)
"""
import email.parser
import email.policy
import http.server
import json
import os
import struct
import sys
import threading

SCRIPT = json.loads(os.environ.get("FAKE_STT_SCRIPT", '["fake words"]'))
LOG = os.environ.get("FAKE_STT_LOG")
LOCK = threading.Lock()
COUNT = [0]


def wav_facts(data):
    if len(data) < 44 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        return {"valid": False, "bytes": len(data)}
    channels, rate = struct.unpack("<HI", data[22:24] + data[24:28])
    return {"valid": True, "bytes": len(data), "rate": rate, "channels": channels,
            "samples": (len(data) - 44) // 2}


class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n)
        raw = b"Content-Type: " + self.headers["Content-Type"].encode() + b"\r\n\r\n" + body
        msg = email.parser.BytesParser(policy=email.policy.HTTP).parsebytes(raw)
        fields, wav = {}, {}
        for part in msg.iter_parts():
            name = part.get_param("name", header="content-disposition")
            payload = part.get_payload(decode=True) or b""
            if name == "file":
                wav = wav_facts(payload)
            else:
                fields[name] = payload.decode("utf-8", "replace")
        with LOCK:
            i = COUNT[0]
            COUNT[0] += 1
            text = SCRIPT[min(i, len(SCRIPT) - 1)]
            if LOG:
                with open(LOG, "a") as f:
                    f.write(json.dumps({"n": i, "path": self.path, "fields": fields, "wav": wav,
                                        "authorization": self.headers.get("Authorization")}) + "\n")
        out = json.dumps({"text": f" {text} \n"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8178
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", port), H)
    print("ready", port, flush=True)
    srv.serve_forever()
