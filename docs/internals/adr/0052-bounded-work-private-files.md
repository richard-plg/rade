# 0052 — Bounded work, private files, a free UI thread

Status: accepted (2026-11-04)

## Context
An audit on 2026-11-04 (PRD R140–R145, `PLAN-m53.md`) found the same three faults in many places: child processes and socket calls without a time limit (a `rade` call, a `git fetch`, a language server write or an E2E helper could wait forever and wedge an Agent or a CI shard), seven hand-written "atomic" file writers that each guaranteed something different (files holding tokens rewritten `0644`, symlinks replaced), and git, `/proc` and config reads on the GTK main loop on every hook, plus a full re-render per model change.

## Decision
- **Every child process goes through `rade_core::proc`** (null stdin, own session, process-group kill at a limit, done on exit even when a grandchild holds the pipes, output cap). git gets one env: `LC_ALL=C`, `GIT_TERMINAL_PROMPT=0`, ssh `BatchMode=yes`, `ConnectTimeout=10` unless the user set an ssh command. Limits: local git 30 s, network git 120 s, probes 5 s.
- **Every `rade` socket call has a limit**: 30 s by default; methods that wait by design get their own limit + 10 s; git-heavy methods 300 s. Over the limit → exit 6 `TIMEOUT` (until now only `wait`), `--call-timeout` / `RADE_TIMEOUT` to change it, `0` for none. A compatible addition: a call that used to hang now ends.
- **Every write of RADE's own files goes through `rade_core::fs_atomic`** (unique temp, fsync file + folder, rename, symlinks written through). Files that can hold a secret or private session data are `0600`; user-owned config keeps its mode. Read-change-write of shared TOML files holds an advisory `flock`.
- **The UI thread only builds widgets.** Model changes coalesce into one `sync` per frame, each part syncs only when its inputs changed, and every git / `/proc` / config read runs off the UI thread with at most one in flight per kind and a staleness check on the result. Pure rules of these move to `rade-core` with unit tests.
- **CI runs every suite**, and a pass only on the retry is a failure unless the suite is listed with a reason.

## Consequences
- An Agent's `rade` call can now fail with exit 6 where it used to hang; the skill says to retry or raise `--call-timeout`.
- Users who relied on reading `state.json` as another user lose that; nothing in RADE did.
- New code that spawns a process or writes a file without these helpers is a review finding.

## Not done
- Signed updates (the update check verifies SHA256SUMS of the same release only): its own ADR and milestone.
- A PTY daemon or anything that changes Session Restore.
