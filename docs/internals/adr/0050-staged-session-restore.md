# 0050 — Staged Session Restore

Status: accepted (2026-10-30)

## Context
Session Restore (R11.2) recreates every Pane at launch, and since M0 every Terminal starts even if never shown (R2.8): `App::sync` queued one idle callback that realized every hidden `GtkGLArea`, and each realize creates the Ghostty surface (GL renderer, PTY, fork) synchronously. With the owner's state — 62 Agent Panes in 7 Workspaces, 12 more than 1.2.0 resumed since R125's user accounts — that callback blocked the main loop for ~5.5 s on a headless run of the state (one socket call waited 3.5 s), and on the desktop 62 agents started at the same instant (4–5 min of CPU in 16–21 s, a 12–15 GB peak). GNOME shows "Not Responding" after 5 s without a ping reply. M49's report check was not the cause (~4.5 ms per report).

## Decision
- Restored Terminals wait in a run-time queue (`rade-core::restore_queue`), never saved: nothing in state.json, the socket or the CLI changes, so this is a PATCH (1.4.1).
- Order: the focused Pane, its Tab, its Worktree, its Workspace (the active Source first), then the other Workspaces in order. The Panes on screen are realized by GTK when the window maps; the queue only drops them.
- Pace: one Terminal per main-loop turn at `G_PRIORITY_LOW`, so input, frames and the compositor's ping go first; 300 ms after each Agent to spread the agents' own start-up, none after a shell. 62 Agents are all running within ~20 s; the shown Workspace within a few seconds.
- Background, not on demand: R23 says the other Workspaces keep running (their Agents notify, Fan-outs progress), so the queue runs to the end on its own.
- Out of turn: a Pane shown by any switch starts as GTK realizes it (no extra code), and the socket's `terminal()` / `pane.wait` start a waiting Pane before using it. A waiting Pane closed never runs. Panes made after launch start at once, as before.
- No placeholder: a waiting Pane is never on screen (being shown starts it), so the "Starting…" page planned in PLAN-m50.md was not built.
- No config key for the pace: a fixed constant until someone needs another.

## Not done
- Making one surface cheaper to create (GL context, shader compile per surface): the first frame still builds the shown Tab's Terminals in one go (~1.4 s before the window shows in the headless run of the owner's state).
- Two Panes resuming the same Claude session (4 such pairs in the owner's state) — a separate fix.
