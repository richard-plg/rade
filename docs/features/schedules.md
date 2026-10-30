# Schedules

Have an Agent do something **every weekday at 9**, **every two hours**, or **once tonight at 23:00**
— without you being there to start it. A **Schedule** belongs to a Workspace and says when, which
agent (and account), where, and what prompt. RADE starts the Agent in a Pane at that time, where you
can read it, answer it and resume it later.

RADE runs Schedules **while the app is open**. A time that passes while it is closed (or the laptop
sleeps) is recorded as *missed* and not run later — unless you turn on **Run once when the app opens
after a missed time** for that Schedule.

## Make one

From the command palette: **New Schedule…** (or **Schedules** → **New Schedule…**):

- **Runs in**: a **Project** (its link's checkout; pick the link when the Project has several), or a
  **Folder** — that one runs in Scratch, started in the folder.
- **When**: **Repeat** with a cron expression (`minute hour day-of-month month day-of-week`, local
  time; the form shows the next three times as you type, or what is wrong) — or the presets
  *Hourly*, *Daily 09:00*, *Weekdays 09:00* — or **Once** at a date and time.
- **Agent** and **Account**: *Default* uses the Workspace's default account for that agent **at the
  time it runs**, so changing the default changes later runs.
- **Prompt**, and how many finished run Panes to keep open (3 by default).

Or from a terminal — the same thing, so an Agent can set Schedules up for you:

```sh
rade schedule next "0 9 * * 1-5" --count 3       # check an expression first
rade schedule add deps --project shop --cron "0 9 * * 1-5" "Check outdated dependencies and write a short report"
rade schedule add tonight --dir ~/notes --at "2026-11-02 23:00" --agent codex --prompt-file prompt.md
```

## Where the runs go

Each run is a new Agent Pane `deps-1`, `deps-2`, … in the Schedule's Tab **⏱ deps**, under its
Project link in the Navigator (or under Scratch); its Agent rows carry a small clock. The Tab is
never brought to the front: you get the usual notification when the Agent finishes or needs you.

- If the previous run is **still running or waiting for you**, the new time is *skipped* — one run
  at a time per Schedule.
- A paused Source skips its runs; a Project, account or agent that is gone makes the run *failed*
  (a Toast says so; the Schedule stays on).
- After a run starts, finished run Panes past the number to keep are closed.

## The Schedules page

Open it from the clock button in the titlebar, the Statusline's **⏱ n** (Schedules switched on in this Workspace), the palette
(**Schedules**), or a Project's right-click menu (**Schedules…**, filtered to that Project). The list
is grouped by Project, then Scratch, with **All · On · Off** and Workspace / Project filters. Each row
shows when it runs, the agent and account, how long ago it last ran and when it runs next; its LED is
the accent when on, hollow when off, red when the last run failed. Up / Down move through the list,
a double-click edits.

The selected Schedule has an **on / off switch** in its header and **⋯** for **Edit…** and
**Delete…** (which can close its Panes too). Under it: what happens next (*Next run in 2h 10m*), **Run
now**, its settings, the prompt, the next five times and its **Runs** — time, and a chip with how it
went (*running*, *needs input*, *done*, *skipped*, *missed*, *failed*…). **Show** takes you to a run
whose Pane is still open; **Resume** opens a new Pane `deps-3-r` that resumes the same agent session.
Either way the page closes and that Pane has the focus. Esc or ✕ goes back.

## From the CLI

| Command | Does |
|---|---|
| `rade schedule list [--all] [--project p \| --scratch]` | on / off, when, agent (account), next time, last run |
| `rade schedule show <name>` | every field, the next 5 times, the newest runs |
| `rade schedule edit <name> [--cron … \| --at …] [--account a \| --default-account] [--rename n] ["prompt"]` | change only what you give |
| `rade schedule on <name>` · `off <name>` | switch it on or off (its Panes stay) |
| `rade schedule run <name>` | one run now |
| `rade schedule runs <name>` | its runs, newest first |
| `rade schedule resume <name> [<n>]` | focus run n's Pane, or resume its session |
| `rade schedule remove <name> [--close-panes]` | delete it |

Every command takes `--json` and `--workspace <ws>`; the full contract is in [docs/API.md](../API.md).
