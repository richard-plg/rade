# 0049 — One Agent per Pane: who may relabel it

Status: accepted (2026-10-29)

## Context
A Pane's Agent identity — its Preset (`agent`), resumable session id and recorded invocation (R11.2) — comes from hook reports, and every process started in the Pane inherits `RADE_PANE_ID`. An Agent started by the Pane's Agent (`codex exec` from Claude's Bash tool) reports too. Since c5b7166 the CLI drops such reports when its process-tree walk finds two Agents above the hook (`nested_agent`), but the walk stops at the first ancestor without the Pane's environment: an Agent whose parent has gone (`setsid -f`, `nohup … &`, reparented to `systemd --user`) looks like the Pane's own. The owner's state (1.2.0) held four Claude Panes relabelled `codex` with a `codex exec … $imagegen` invocation; one had already been restored as that Codex job.

## Decision
- The CLI keeps its walk and also sends `reporter`: the nearest ancestor inside the Pane running a Preset's program (shells skipped) and whether that run is one-shot (`codex exec`, `claude -p`). Nearest first, so a process whose parent has gone is still named; the walk stops at the Pane's edge, so an Agent that started RADE itself is never one.
- The app decides, not the process tree alone: while a Pane has a live Agent (a Preset, not `exited`), a report from another kind of agent (the glyph kinds of R115: by the Preset it names, else the reporter) is dropped whole unless that kind runs in the Pane's foreground when the report arrives (read then; the 3 s detection poll skips Agent Panes) — an Agent started by hand after one ended without `exited`. Kinds rather than program names, so wrapper Presets (`zsh -ic 'clv'`, `npx …`) and aliases keep working. A one-shot reporter never speaks for a Pane that isn't one-shot itself. Same kind, no reporter (by hand, older CLIs) or an unknown kind → taken as before.
- A dropped report answers `{"ignored": …}` (additive) and writes one Activity row per (Pane, session), so a skill that runs other Agents is visible, not silent.
- Session Restore never re-runs a recorded one-shot invocation; such a Pane comes back as a shell with a notice. This repairs state from 1.0–1.3 without a migration.

## Not done
Comparing session ids for reports without a Preset (Codex `notify` after `/new` carries a new thread id with no Preset, and must keep working); matching the reporter's pid against the Pane's process group (a Pane command `zsh -ic 'alias'` keeps the Agent in the shell's group); Agents started from a Pane that are neither one-shot nor another program (a second interactive Claude in a Claude Pane whose parent has gone) — still the CLI walk's job.
