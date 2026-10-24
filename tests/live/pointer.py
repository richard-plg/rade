#!/usr/bin/env python3
"""Move the real pointer to window point (x, y) of the live test RADE window.

ydotool only moves relatively (GNOME applies pointer acceleration, and
Wayland hides window positions), so this is a closed loop: read where the
pointer is over the window (`rade debug window`), move by the remaining
distance scaled by the gain seen so far, repeat. Exit 0 when within 1 px,
1 when the pointer is not over the window, 2 when it doesn't converge.
"""
import json, subprocess, sys, time

def where():
    out = subprocess.run(["rade", "debug", "window", "--json"], capture_output=True, text=True, check=True).stdout
    p = json.loads(out).get("pointer")
    return None if p is None else (p["x"], p["y"])

def main(tx, ty):
    gain = [0.5, 0.5]  # device units per pixel, per axis
    pos = where()
    if pos is None:  # no motion since the pointer entered: nudge once
        subprocess.run(["ydotool", "mousemove", "-x", "1", "-y", "1"], check=True, capture_output=True)
        time.sleep(0.1)
        pos = where()
    for _ in range(60):
        if pos is None:
            return 1
        d = [tx - pos[0], ty - pos[1]]
        if abs(d[0]) <= 1 and abs(d[1]) <= 1:
            return 0
        step = []
        for i in (0, 1):
            s = round(d[i] * gain[i])
            s = max(-300, min(300, s))
            if s == 0 and abs(d[i]) > 1:
                s = 1 if d[i] > 0 else -1
            step.append(s)
        subprocess.run(["ydotool", "mousemove", "-x", str(step[0]), "-y", str(step[1])], check=True, capture_output=True)
        time.sleep(0.06)
        new = where()
        if new is None:
            return 1
        for i in (0, 1):
            moved = new[i] - pos[i]
            if step[i] and moved and (moved > 0) == (step[i] > 0):
                # damped update of units-per-pixel
                gain[i] = 0.5 * gain[i] + 0.5 * (step[i] / moved)
        pos = new
    return 2

if __name__ == "__main__":
    sys.exit(main(float(sys.argv[1]), float(sys.argv[2])))
