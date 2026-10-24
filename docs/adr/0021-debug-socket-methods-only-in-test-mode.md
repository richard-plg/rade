# The hidden `debug.*` socket methods are served only in test mode

Status: accepted 2026-10-03 (0.3.1, PRD R2.9). Amends ADR-0006 (one socket for Agents, users and tests); restores what ADR-0013 and ADR-0020 promise ("only the owner commits, pushes, approves"; "no socket path approves").

ADR-0006 put the test harness on the same socket as Agents. The hidden `debug.*` methods drive the UI's own buttons — `debug.batch approve`, `debug.batch push` + `debug.dialog respond push`, `debug.scm commit|push|pr`, `debug.fanout` choose, `debug.menu … activate` — so any process of the owner's UID on the socket, which is every Agent in a RADE Terminal, could press Approve or confirm a push. The skills tell Agents not to, but a prompt-injected or careless Agent only needs one `rpc` call.

## Decision

1. **`debug.*` needs test mode.** `rade-app` reads `RADE_DEBUG_SOCKET` once at start, before importing the login shell's environment; only the value `1` in the environment that launched it turns test mode on. Otherwise every `debug.*` method returns `USAGE` (exit 2): `debug methods are disabled; they are for tests: start rade-app with RADE_DEBUG_SOCKET=1 (ADR-0021)`. All of them, read-only ones (`debug.state`, `debug.window`) included: one rule is easier to reason about than an allow-list, and nothing outside the tests needs them.
2. **Not inheritable.** The app removes the variable from its own environment right after reading it, so no Pane, Agent, git or other child process gets it, and it is dropped from a Workspace's `[env]` before a Pane starts. A Pane setting it for itself changes nothing: the flag belongs to the running app, not to the caller.
3. **The window hand-off is public.** A second launch shows the running window through the new `app.present` method instead of `debug.action present`.
4. **One app per `state.json`.** The app holds an exclusive `flock` on `<state>.json.lock` (in its flavor's data dir) for its whole life; a second app that can't take it exits 1 (`another RADE is using …`). A second app started on another socket (`RADE_SOCKET=…`) used to load the same state and could run in test mode next to the owner's app (found in acceptance); it also overwrote the other's saves. The kernel drops the lock when the process dies, `kill -9` included.
5. **Every harness sets it**: `tests/e2e/lib.sh` and `tests/live/lib.sh` export `RADE_DEBUG_SOCKET=1` for the apps they start (CI runs the same scripts). The installed `.deb` and desktop launches never set it.

## Considered Options

- **Gate per caller** (refuse `debug.*` from a Pane, by `caller`/`RADE_PANE_ID` or peer PID ancestry): `caller` is self-reported, and PID ancestry breaks for daemons and double forks. Rejected.
- **A second, test-only socket**: a second control path, against ADR-0006, and every harness call would need it. Rejected.
- **Gate only the mutating methods**: the list would have to be kept in step with every new `debug.*` method. Rejected.
- **Build-time feature (`cfg(debug_assertions)` / a cargo feature)**: the E2E and live suites run the release binary and m10 runs the packaged one. Rejected.

## Consequences

- Same-UID processes are still not a security boundary: an Agent can run `git push` itself, edit files, kill RADE and start its own with the flag, or start one in test mode on a copy of the state in another data dir. The gate closes the socket path that turned the app's own confirmations into an API; the confirmations stay the owner's.
- `rade debug …` on an installed RADE prints the error with the hint. Tests that start `rade-app` by hand must set the variable (the harnesses do).
- Reading `/proc/<app>/environ` shows the launch environment but does not change the decision taken at start.
