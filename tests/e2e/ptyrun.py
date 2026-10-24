#!/usr/bin/env python3
# ptyrun.py TRIGGER RESULT cmd...: run cmd with stdout/stderr on a pty (no
# controlling tty, like a disowned job), close the pty master once TRIGGER
# exists (the terminal window closed), record the exit status in RESULT.
import os, pty, subprocess, sys, time

trigger, result, cmd = sys.argv[1], sys.argv[2], sys.argv[3:]
master, slave = pty.openpty()
p = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=slave, stderr=slave)
os.close(slave)
log = open(result + ".pty", "wb")
os.set_blocking(master, False)
while not os.path.exists(trigger) and p.poll() is None:
    try:
        log.write(os.read(master, 65536)); log.flush()
    except (BlockingIOError, OSError):
        pass
    time.sleep(0.05)
os.close(master)
rc = p.wait()
open(result, "w").write(str(rc) + "\n")
