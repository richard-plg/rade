# PLAN M50 — Staged Session Restore (R130)

Status: **implemented** (approved 2026-10-30, "ok làm hết đi"; Q1 = 300 ms, Q2 = background). Target release **1.4.1** (PATCH: no CLI / socket / config / state change). Checks: docs/internals/checks/M50.md.

**Built differently from this plan:**
- **No "Starting…" placeholder and no Start now button (Q3 moot).** A waiting Pane is never on screen: GTK realizes a Terminal when it is shown, and that starts it. The queue only holds hidden Terminals, so there is nothing to draw.
- **Promotion on show needs no code.** Tab, Worktree and Workspace switches, Overview, and the palette start a Pane through GTK's realize. Only the socket paths call `start_now`: `terminal()` and `pane.wait`.
- **Measurement.** `tests/e2e/probe-latency.py` calls `pane.list` in-process every 50 ms, so the latency of starting the `rade` CLI does not count. R130-c runs hands off; R130-d and R130-f run in a second relaunch.

## Problem (measured 2026-10-30)

The owner opened 1.4.0 three times. "Not Responding" appeared every time. The third launch recovered after a few seconds.

- **The owner's state.** 62 Agent Panes in 7 Workspaces. Session Restore starts all of them at launch: `App::sync` step 1 makes a Terminal for every Terminal Pane, "started even if never shown". Each Ghostty surface is created on its `GtkGLArea` realize, synchronously on the main thread.
- **Main thread, headless repro.** A copy of the owner's `state.json`, fake `claude` / `codex`, the socket probed every 0.1 s with `rade list`. The main loop is blocked about 5.5 s in total after launch, and the worst single wait for a reply is 3.8 s.

  | Version | Terminals started | Blocked total |
  |---|---|---|
  | 1.2.0 | 50 | 5.3 s |
  | 1.3.0 | 62 | 5.5 s |
  | 1.4.0 | 62 | 5.6 s |

  GNOME shows "Not Responding" after 5 s without a ping reply.
- **The machine (journal).** 62 real `claude --resume` / `codex resume` processes start at the same instant: 4–5 min of CPU in 16–21 s of wall time, a 12–15 GB memory peak. The main thread competes with all of them, so the real block is longer than in the repro.
- **Why it showed up after the upgrade.** 1.2.0 → 1.4.0 restores 12 more Panes: the user account `asd` from `accounts.toml` (R125, 1.3.0). 1.2.0 brought those back as shells.
- **Not the cause.** M49's `foreign_report` costs about 4.5 ms per report.

## Goal

At launch the window answers within 250 ms at every moment, whatever the number of Panes. Every Pane still comes back as R11.2 says: same session, same invocation, same account. The other Workspaces still run (R23: "one window shows one Workspace; the others keep running").

## Design: R130 Staged Session Restore

1. **Pending Panes.** After `prepare_restore`, every restored Terminal Pane is *pending*. This is a run-time set in the app, never saved, so `state.json` and API.md are unchanged.
   - A pending Pane shows a light placeholder like the paused one (R19.3): "Starting…", its Agent glyph and name, and a **Start now** button.
   - It creates no `GtkGLArea`, no Ghostty surface and no PTY.
   - Its model entry (Agent, session, invocation, account, status `interrupted` or none) is exactly what R11.2 left.
2. **Order.** The restore queue starts Panes in this order:
   1. The focused Pane.
   2. The other Panes of the visible Tab.
   3. The other Tabs of the shown Worktree.
   4. The rest of the shown Workspace.
   5. The other Workspaces, in Navigator order.

   Shells and Viewer / Review / Diff / Browser Panes are cheap. They are built at once and never queued.
3. **Pace.**
   - At most one Terminal per main-loop turn, through `glib::idle_add` at low priority, so input, pings and drawing go first.
   - After each Agent start, the queue waits **`300 ms`** before the next one, to spread the CPU and memory spike of the agents themselves.
   - Shell-only Panes have no gap.
   - With 62 Agents everything runs within about 20 s. The shown Workspace (about 10 Agents in the owner's state) is up within about 3 s.
4. **Promotion.** A pending Pane jumps to the front of the queue and starts at once when:
   - It becomes visible: a switch of Tab, Worktree or Workspace, Overview Enter, or a palette jump.
   - Its **Start now** button is clicked.
   - The socket targets it: `send`, `read`, `focus`, `wait`, `review request`, Voice, Fan-out Continue. Socket calls wait for the surface, up to the same 5 s budget that a new spawn has today. `read` of a pending Pane returns its placeholder text instead of waiting.
5. **Not changed.**
   - Paused Sources stay paused: they are not pending and not queued.
   - Close or Kill on a pending Pane removes it without starting it.
   - Quit with pending Panes saves them as they are, so the next launch restores the same sessions.
   - A Fan-out keeps its Agents' order. Its Continue promotes them.
6. **Same-session duplicates (seen in the owner's state).** Out of scope: the owner's state has 4 sessions resumed in 2 Panes each. Mentioned in the checks file, not fixed here.

No config key for the pace: the PRD prefers fixed behaviour, and `300 ms` is a constant in `rade-core`. If the owner wants one later, it is a MINOR change.

## Code

| Where | What |
|---|---|
| `rade-core/src/restore_queue.rs` (new, unit tests) | `RestoreQueue`: `plan(&State, focused)` gives the order, `promote(pane)`, `next(now)` gives the Pane due or the wait. Pure, GTK-free |
| `rade-app/src/app.rs` | `pending: RefCell<RestoreQueue>`. `sync` step 1 gives a pending Terminal Pane the "Starting…" placeholder (`placeholders` map, like paused), not `panes::create`. A low-priority idle drains the queue |
| `rade-app/src/app.rs` / `tabs.rs` / `sidebar.rs` / `overview_view.rs` | Tab / Worktree / Workspace switch and focus call `promote` |
| `rade-app/src/handlers.rs` | Socket methods that need a surface call `ensure_started(pane)`, then wait as spawn does. `read` on a pending Pane returns the placeholder text |
| `rade-app/src/main.rs` | The queue starts after the first frame (`connect_map` + idle), so the window paints before any Terminal is built |
| `rade-app/src/debug.rs` | `debug.restore` (test mode): pending list, order, started-at timestamps |

## Proof (ACs, `scripts/trace.sh`)

- **R130-a (unit).**
  - *Given:* 3 Workspaces with 2 Tabs each, the focused Pane in Tab 2 of Workspace B, and shell and Viewer Panes.
  - *When:* `plan` runs.
  - *Then:* the order is: the focused Pane, its Tab, its Worktree, Workspace B, then A and C in Navigator order. Shells and Viewers are not in the queue.
- **R130-b (unit).**
  - *Given:* a queue of 5 Agents.
  - *When:* `next` is called at t = 0, 100 and 300 ms, and Pane 4 is promoted at 150 ms.
  - *Then:* Panes start at 0 and 300 ms, Pane 4 starts second, and the gap only follows Agents.
- **R130-c (e2e).**
  - *Given:* a generated state of 62 fake-Agent Panes in 7 Workspaces, the owner's shape without the owner's data.
  - *When:* the app starts while `rade list` is probed every 100 ms.
  - *Then:*
    - No probe waits over 250 ms.
    - The shown Workspace's Agents report `resumed <id>` before any other Workspace's Agent starts.
    - All 62 report `resumed` within 30 s.
- **R130-d (e2e).**
  - *Given:* R130-c while Workspace C is still pending.
  - *When:* the switcher shows C, and `rade send` targets a pending Pane of A.
  - *Then:* C's visible Panes start at once, and the `send` text reaches A's Agent.
- **R130-e (e2e).**
  - *Given:* R130-c with 20 Panes still pending.
  - *When:* the app quits and starts again.
  - *Then:* `state.json` still holds those 20 Panes' sessions and invocations, and they resume.
- **R130-f (e2e).**
  - *Given:* a paused Source and a pending Pane closed with Close.
  - *When:* the restore runs.
  - *Then:* the paused Source starts nothing, and the closed Pane never started a process (`FAKE_ARGV` has no line for it).

**Regression suites:**
- m2 (Session Restore with kill -9), m6, m9 (Fan-out restore), m11, m13 (pause / resume), m14, m15 (Workspaces keep running), m38, m43 (Overview), m44, m47, m49.
- `tests/live/run.sh m40` on the real desktop: one launch with the owner's real state, read only. The owner watches for "Not Responding"; that check runs only on the owner's machine.

## Docs

- PRD: new **R130**, plus a pointer under R11.2 ("restored Terminals start in stages, R130").
- ADR-0050 "Staged Session Restore": why not all at once, why background and not on demand (R23), why a fixed pace.
- MILESTONES: M50.
- `docs/features/` restore page: what the "Starting…" placeholder means.
- `docs/internals/checks/M50.md`.
- CLAUDE.md: one m50 line.
- Release with `scripts/release.sh 1.4.1`, once you ask.

## Questions (defaults are used if you don't answer)

- **Q1. Pace between Agents.** Default `300 ms`: about 20 s for 62 Agents. Alternative: 0 ms, which keeps the window responsive but does not reduce the CPU and RAM spike.
- **Q2. Other Workspaces.** Default: started in the background, throttled (R23 "keep running"). Alternative: only when a Workspace is first shown. That is lighter, but their Agents do not run and send no Notifications until then.
- **Q3. Placeholder.** Default: "Starting…" plus a **Start now** button. Alternative: no button, only the text.
