# Live E2E harness (real desktop)

`tests/e2e/` proves RADE headless (ADR-0006). `tests/live/` covers what only
the real desktop can: real keys through GNOME/IBus, real mouse, the real
Wayland clipboard, the real notification server and real Agents. It runs the
owner-manual checks of M1–M5 (`docs/internals/checks/M*.md`, `docs/internals/checks/R6.md`).

```sh
tests/live/run.sh            # all: m1 … m9 (~25 min)
tests/live/run.sh m1 m3      # some milestones
tests/live/run.sh path/x.sh  # one ad-hoc script with the harness loaded
NO_BUILD=1 KEEP=1 tests/live/run.sh m5
```

Results: `target/live/results.txt` (PASS / FAIL / N/A with reasons),
screenshots `target/live/*.png`, `notify.log` (D-Bus monitor), `app.log`.
Exit code 0 only when nothing failed; 3 = aborted by the focus guard.

## Prerequisites

- GNOME Wayland session, nobody else typing: the run takes over the screen
  (the test window goes fullscreen), the pointer and the keyboard.
- `/dev/uinput` writable by you (ydotool). Temporarily:
  `pkexec setfacl -m u:$USER:rw /dev/uinput` (undo: `pkexec setfacl -x u:$USER /dev/uinput`).
- `ydotool`/`ydotoold` 1.x, `wl-clipboard`, `jq`, `dbus-monitor`, `gdbus`, `ibus`
  with the Bamboo engine, `python3`; for M2/M5 logged-in `claude` (and
  `~/.claude-work`, the `clw` alias in your shell), `codex`, `agy`, `opencode`;
  for M4-5 VS Code (`code`). `rade setup` hooks installed.
- Real Agent turns cost a few tokens (prompts are "reply with exactly: PONG").

## Safety design

- **Your RADE is never touched.** The harness starts a separate `rade-app`
  (binaries copied to a temp dir, so rebuilds can't swap them) with temp
  `XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`XDG_CACHE_HOME` and its own
  `RADE_SOCKET=$XDG_RUNTIME_DIR/rade-live-<pid>.sock`; `RADE_PANE_ID` is
  unset, and it refuses to run if the socket equals yours. `HOME` stays real
  (Agents need their logins); your `config.toml` is copied in, plus test
  settings (`[limits] max_children = 10`, a VS Code command with its own
  `--user-data-dir`).
- **Focus guard.** Before every key press, typed text or click, `guard` asks
  the test app (`rade debug window`) whether its window is the active
  toplevel. If not, it calls `rade debug action present` once and re-checks,
  else aborts the whole run without sending input. (GNOME ignores `present()`
  without an activation token, so in practice a lost focus aborts; a freshly
  launched window gets focus, which is how the run starts and recovers after
  the VS Code check.)
- **Aimed clicks.** Wayland hides window positions and GNOME accelerates the
  pointer, so `pointer.py` moves it in a closed loop: `rade debug window`
  reports where the real pointer is in window coordinates, ydotool moves it
  relatively, repeat until within 1 px. A click is sent only after the pointer
  is confirmed at the target. Targets come from `rade debug window --pane P
  --text T` (a Terminal cell) or `--widget <css class | tooltip>`.
- **IBus**: the current engine is saved and restored; commands are typed
  with `xkb:us::eng`, Bamboo only for the IME check.
- **Fan-out (M9)**: a temp repo; two Codex Agents and a Codex Judge run through a `m9codex` Preset whose `-c` override trusts only the temp repo and its two Worktrees; the run stages the Winner and removes every Worktree and branch.
- **Worktrees (M7)**: the test repo and every Worktree live in the run's temp
  dir (`$XDG_DATA_HOME/rade/worktrees`); the run removes its Worktree and
  branch. Codex runs as `codex --no-daemon` with a per-session `-c
  projects=…` override trusting only those temp folders — `~/.codex` is not
  written. Its prompt asks for exactly `GOT-IT` (a few tokens).
- **Projects (M16)**: the Workspace root, its repos, Projects and Worktrees
  live in the run's temp dir; fake Agents only. Run it with a temp HOME so
  nothing in yours is read or written by the app:
  `XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) tests/live/run.sh m16`
  (`XDG_CONFIG_HOME` stays real only so IBus finds its bus; the app gets the
  harness's temp config dir).
- **Tasks (M17)**: like M16 (temp HOME, temp Workspace); the Task Agent runs
  the fake Agent through a Workspace Profile (`[task] profile`), so your own
  `claude` is never started:
  `XDG_CONFIG_HOME=$HOME/.config HOME=$(mktemp -d) flock /tmp/claude-1000/rade-live.lock tests/live/run.sh m16`
- **Clipboard**: M5 replaces it (`wl-copy`); your text clipboard is put back
  at the end (an image clipboard is not).
- **Notifications**: `dbus-monitor` on the real session bus, read-only; the
  banners the test app sends do appear on your screen.
- **Cleanup** (also on Ctrl+C / abort): quit the test app, stop ydotoold and
  the monitor, kill only the VS Code processes with the test
  `--user-data-dir`, restore the IBus engine, remove the temp dir.

## What stays manual (reported as N/A)

Things outside the test window or needing another user: the Add Project
portal dialog, IBus/GNOME input-source hotkeys, cross-UID socket, clicks in
GNOME's notification list, drag and drop from Nautilus/Firefox, OpenCode
model turns (no funds), and judging visual quality (the screenshots are
there to look at).
