# 0054 — Branch sets: one branch across a Project's Sources, one Agent

Status: accepted (2026-11-05).

## Context
A Project (R97) links several Sources, each with its own base (`web ← develop`, `api ← main`). A task that changes two of them today means a `git worktree add` in each, by hand, and two Agents that can't see each other's code, then two PRs that don't mention each other. The owner chose (2026-11-04, `PLAN-m54.md`) "one branch across a Project's Sources".

## Decision
- **A Branch set** is the Worktrees one `worktree.add` call made in several links of one Project with one branch name; each records `set = <name>` in `state.json`. A Worktree made by hand with the same name is not in the set.
- **All or nothing.** Every link is checked first (name valid and free, checkout resolvable) with no git command; then each Worktree is cut **from its own link's base**; a failure halfway removes what this call created (never an existing branch) and says so. Removing a set checks every Worktree's refusals before removing any.
- **One Agent for the set**, in a new Tab of the lead Worktree (first link in Project order), cwd = its folder, with `--add-dir <path>` for each other Worktree (a Preset's `add_dir` flag; `--add-dir` for the claude and codex Presets) and a prompt that names every Worktree, its path and base. `deliver` (Review Comments, Annotations, CI failures) for a set Worktree without its own Agent goes to the set's Agent.
- **PRs for a set** on one click: push and `gh pr create` per Source with commits, then `gh pr edit --body` adds `Part of <set>: …` to each; a failure keeps what was made and offers Retry for the rest.

## Considered Options
- **One Agent per Worktree**: simplest, but each sees only its own repository. Rejected.
- **A set folder of symlinks as the Agent's cwd**: any agent works, but the folder is not a Worktree, so Review, Changes and `deliver` get a new case. Rejected for now.
- **A set across Projects**: no shared base rule. Not in M54.

## Consequences
- Agents without an `add_dir` flag get only the paths in the prompt.
- `worktree.move` of a set Worktree out of its Project drops its `set`.
