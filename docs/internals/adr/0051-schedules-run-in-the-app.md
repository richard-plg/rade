# 0051 — Schedules run in the app

Status: accepted (2026-10-30)

## Context
The owner wants Agents started on a cron expression or at a given time, per Workspace, in a Project or a folder, and managed fully through the `rade` CLI (PRD R136–R139). Agents in RADE live in Terminal Panes; a run the owner cannot see, continue or resume is not what they asked for.

## Decision
- **The app runs Schedules.** A check every 15 s and on a wall-clock jump; a run is an Agent Pane spawned like `pane.spawn`, in the Schedule's Tab `⏱ <name>` of the target checkout (a Project link's, resolved at run time, or Scratch with the folder as cwd). No systemd timer, no headless `claude -p`, no second process model.
- **Missed runs are not caught up by default.** A closed or sleeping app records one `missed` Run (with the count) per Schedule; `catch_up = "once"` starts one run instead. A burst of runs after a long suspend would spend quota and pile up Panes.
- **Schedules are saved in the flavor's `state.json`** (an additive field, no version bump). The installed and the Devel app read different state, so a Schedule never fires twice. The CLI needs the app for everything but `rade schedule next`.
- **The account is resolved at run time** when the Schedule names none, so changing a Workspace's default changes later runs; a named account is checked at create, edit and run (R108 rules, `FORBIDDEN`).
- **One run at a time per Schedule**: a run whose previous run is still `running` is `skipped`; older finished run Panes past `keep_runs` are closed. Every started Run keeps its session and account so it can be resumed after its Pane is closed.
- **Cron in local time** (5 fields, Vixie day matching, DST: a skipped time fires at the next existing minute, a repeated one once).

## Not done
- Runs while the app is closed (a user daemon or systemd timers), a fresh Worktree per run, seconds-level cron, a time zone per Schedule, sending to an existing Pane, chaining Schedules.
