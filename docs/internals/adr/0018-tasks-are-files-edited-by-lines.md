# Tasks are files RADE edits by lines; the Tasks page is a page, not a Pane

Status: accepted 2026-10-02 (M17, PRD R33–R40). Builds on ADR-0017 (Projects are files).

Superseded by ADR-0033 (1.0.0).

The owner made RADE the primary task manager (design doc §11 answer 2). Tasks must be editable by Agents with ordinary file tools, by hand, and by RADE, without one clobbering the others, and must travel with the Workspace's `.rade` folder like `project.toml`.

## Decision

1. **A Task is a Markdown file with YAML frontmatter**, `<root>/.rade/projects/<p>/tasks/T-<n>-<slug>.md`. RADE reads the frontmatter with `yaml-rust` (already built through syntect: no new crate compiled) and **never re-serializes it**: it replaces single `key: value` lines (keeping a trailing comment) and appends `- <time> <text>` lines under `## Log`. Writes go to a temp file that is renamed; if the file changed while RADE computed the edit, the edit is redone once on the new text. Comments, key order and the body an Agent wrote survive byte for byte.
2. **Ids per Project, allocated under `flock(tasks/.lock)` with `O_EXCL` create.** Works across threads, the CLI and both app flavors on one machine. A file made by hand that reuses an id is listed with an error rather than silently merged.
3. **No `runs:` in the frontmatter.** The live Run is `Tab.task` in `state.json` (machine-local, like everything about Tabs); its history is the Log. RADE writes no YAML lists.
4. **The Task Agent gets the file's path, not its content.** The file stays the single live spec, the argv stays small, and an edit after start is seen.
5. **A Task Run is ordinary Worktrees named `task/T-<n>-<slug>`**, one per Source, made with the M16 Named Base resolution; the Tab lives in the first Source's Worktree, the others are passed in the prompt and `RADE_TASK_WORKTREES` (no symlink farm). Creation is all-or-nothing, like a Fan-out.
6. **The status machine is small and only moves `running ↔ review` by itself**, driven by the Run root Pane's Agent Status. A person's `done` / `dropped` is never overwritten. The live overlay (`waiting`, `error`) is computed, not written, so the file is not rewritten on every turn of a chatty Agent.
7. **The Tasks page is a named child of the main content stack**, not a Pane kind: a Pane must live in a Worktree and a Project owns none. `State.tasks_page` (optional) remembers it; no `state.json` v3.
8. **Agent Profiles are config, not files of their own**: `[profiles.<n>]` (and `[task] profile`) join the layered allow-list of ADR-0017; Presets gain `model_flag`, so no model name is ever built into RADE.
9. **An id in two Projects without a prefix is `AMBIGUOUS` (exit 4)**, the existing contract for lookups (API.md "Exit codes"); the plan's `USAGE` was changed to match it.

## Considered Options

- **Tasks in `state.json`**: Agents could not edit them with file tools; nothing to grep or commit. Rejected (design doc §5).
- **Re-serializing YAML** (serde_yaml): reorders keys and drops comments an Agent or the owner wrote; `serde_yaml` is also deprecated. Rejected for line edits.
- **A lock-free id** (timestamp or random): readable `T-12` ids are what people type. Rejected.
- **The Tasks page as a Pane** (like Review): needs a Worktree to live in and would be closed with it. Rejected.
- **Writing `waiting` into the file**: a file write per Agent turn, and racy with the Agent's own Log edits. Rejected for a computed overlay.

## Consequences

- An Agent may edit any part of the body; it must change `status:` only through `rade task status` (the `rade-task` skill says so), though a hand edit is still read.
- Two RADE instances on two machines sharing `.rade` through git can still allocate the same id; the duplicate shows as an error to fix by renaming.
- M18 Reports read the Log; M19 Batches drive `rade task start` and add `task wait`.
