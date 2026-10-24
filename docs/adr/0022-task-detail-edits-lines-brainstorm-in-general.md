# The Task Detail edits single lines; Brainstorm with Claude runs in the Workspace's General

Status: accepted 2026-10-03 (M20, PRD R55–R57). Builds on ADR-0018 (Tasks are files edited by lines).

Superseded by ADR-0033 (1.0.0).

The owner found the Tasks page hard to use: a click jumped to a Terminal or opened the `.md` file, and a new Task was a bare title. M20 adds a Task Detail beside the List / Board and a New Task dialog with an Agent brainstorm. The file must stay the source of truth that Agents and people edit with ordinary tools.

## Decision

1. **A click selects; it never navigates.** Single click = select + Task Detail; double click / Enter = the Run Tab if one is open. Opening the file is an explicit action (Open file).
2. **Every inline field edit writes only its own frontmatter line** through the existing `set_field_raw` (comment kept, missing key inserted before `---`, a block list collapsed to one flow line). It does **not** touch `updated:` or the Log: a field edit is the owner's tidy-up, and `updated:` keeps meaning "the status or the Run moved" (what the List's age and M18 Reports read). Status keeps the M17 path (`status:` + `updated:` + Log line + Event Journal line).
3. **The description is swapped whole, with an optimistic check.** The editor holds the body above `## Log`; a save rebuilds the file from the current frontmatter bytes + the new description + the current Log tail, inside `task::edit` (temp file + rename, redone if the file moved meanwhile). If the description on disk is no longer the one editing started from, nothing is written and the owner chooses Use theirs / Keep mine. Log lines an Agent appends while the owner types survive.
4. **A title change keeps the file name.** Agents, Logs and Batch files hold the path; the slug is only a hint.
5. **Brainstorm with Claude opens a new Tab in the Workspace's General**, cwd = the Workspace root, like `rade task brainstorm` (R35.4), and shows it. The Tasks page is a page of the main area, not a Pane, so it cannot share the screen with a Terminal without a new layout concept; the owner returns to the page (sidebar **All tasks**) and the new Tasks are there (index poll). The Agent is the Project's `[task] profile` Preset with its model (default `claude`).
6. **The Agent creates the Tasks, not RADE.** RADE only writes the prompt; the Agent proposes a split and runs `rade task add` per Task after the owner confirms in the conversation. The rule is in the prompt and the `rade-task` skill; RADE adds no "proposed Tasks" state or approval API.

## Considered Options

- **Bump `updated:` on every field edit**: two lines change per edit and the List's age stops meaning progress. Rejected.
- **Re-serialize the frontmatter from a form**: drops comments and key order (ADR-0018). Rejected.
- **Last writer wins for the description**: silently loses an Agent's edit of the Goal. Rejected for the optimistic check.
- **Brainstorm Pane split beside the Tasks page**: needs the page to host Panes (a Worktree to live in, ADR-0018 §7). Rejected for now.
- **RADE parses the Agent's proposal and creates the Tasks on a button**: a new protocol and state for something `rade task add` already does. Rejected.

## Consequences

- A field edit made in RADE is visible in `git diff` as exactly one line.
- Two people editing the same field at once: the last write wins for that line (as with any editor); the description is protected by the check.
- The brainstorm Agent can still create Tasks without asking if it ignores its prompt; same-UID Agents are not a security boundary (ADR-0021). The owner sees every new Task in the page and can drop it.
