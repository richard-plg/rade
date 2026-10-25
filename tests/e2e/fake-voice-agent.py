#!/usr/bin/env python3
"""Fake voice agent for the voice E2E (R124): speaks Claude's stream-json.

Reads one user message per stdin line and, per message, emits a `Read` tool
use, the scripted draft as text deltas, then `result`; one process answers
several turns. Appends JSON lines to FAKE_VOICE_LOG: the start (argv, env,
cwd, pid, the exit code of one `rade status --hook`) and each message.
FAKE_VOICE_DRAFTS: JSON array of drafts by turn (the last repeats).
FAKE_VOICE_HANG=1: after one delta, sleeps forever.
"""
import json
import os
import subprocess
import sys
import time

LOG = os.environ.get("FAKE_VOICE_LOG")
DRAFTS = json.loads(os.environ.get("FAKE_VOICE_DRAFTS", json.dumps(
    ["Fix the promo discount applied twice in `useTotals()`.\n- Apply it once, in `applyPromo()`\n- Add a test for percent codes"])))


def log(obj):
    if LOG:
        with open(LOG, "a") as f:
            f.write(json.dumps(obj) + "\n")


def emit(obj):
    print(json.dumps(obj), flush=True)


try:
    hook = subprocess.run(["rade", "status", "running", "--hook"], input='{"session_id":"voice"}',
                          text=True, capture_output=True, timeout=10).returncode
except Exception as e:  # noqa: BLE001
    hook = str(e)
log({"start": True, "argv": sys.argv, "env": dict(os.environ), "cwd": os.getcwd(),
     "pid": os.getpid(), "status_hook": hook})
emit({"type": "system", "subtype": "init", "session_id": "voice", "cwd": os.getcwd()})

turn = 0
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        content = json.loads(line)["message"]["content"]
    except Exception:  # noqa: BLE001
        content = line
    log({"turn": turn, "pid": os.getpid(), "message": content})
    draft = DRAFTS[min(turn, len(DRAFTS) - 1)]
    turn += 1
    emit({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "name": "Read",
         "input": {"file_path": os.path.join(os.getcwd(), "src/checkout/useTotals.ts")}}]}})
    time.sleep(0.2)
    chunks = [draft[i:i + 12] for i in range(0, len(draft), 12)]
    for c in chunks:
        emit({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0,
                                                "delta": {"type": "text_delta", "text": c}}})
        if os.environ.get("FAKE_VOICE_HANG"):
            time.sleep(3600)
        time.sleep(0.05)
    emit({"type": "result", "subtype": "success", "is_error": False, "result": draft,
          "num_turns": 2})
