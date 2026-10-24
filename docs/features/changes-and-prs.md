# Changes and pull requests

The Inspector's **Changes** tab is a git panel grouped by Project: one group per Source link, each
with its own staged and unstaged files, commit box, push and PR. RADE runs git or `gh` only when
you click — agents can draft the message, but you confirm.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/changes-commit-light.png">
  <img alt="Changes: staged and unstaged files of a Worktree, a generated commit message and the Commit button" src="../images/changes-commit-dark.png">
</picture>

## Stage, discard, commit

`Ctrl+Shift+M` opens Changes (`Ctrl+Shift+E` toggles the Inspector).

- Files are listed as **Staged**, **Changes** and **Untracked**, with stage / unstage / discard
  per file. Click a file to see its diff; each hunk has its own Stage / Unstage / Discard.
  Discarding asks first.
- Type a message and **Commit** (`Ctrl+Enter`) — only what's staged is committed.
- **Generate** asks an agent for a draft message from the staged diff, the repository's
  `CLAUDE.md` / `AGENTS.md` / `CONTRIBUTING.md` and recent subjects. It only fills the box.
- When the checkout's `git config user.email` differs from the Workspace's identity, the
  Statusline cell turns amber and the commit area warns you.

## Push, pull, PR

The branch shows `↑ahead ↓behind`. **Pull** is `--ff-only`. **Push** asks first, naming the
remote, branch and commit count (and sets the upstream the first time).

**Create PR…** opens a dialog with title, body and base — by default the Worktree's base, the
same base its Review used. Generate fills the title and body from the branch's diff and commits
(and your PR template). Creating pushes the branch first if needed, then runs `gh pr create`
under the Workspace's `gh` login; the PR link shows in a Toast.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/pr-dialog-light.png">
  <img alt="The Create Pull Request dialog with a generated title and description" src="../images/pr-dialog-dark.png">
</picture>

## Base drift card

When a link's checkout is on another branch than its base, Changes shows a card with what to do:
**Back to base**, **Keep, show in Shop** or **Move to a Worktree…**. See
[Base drift](projects-and-worktrees.md#base-drift).

## Settings

```toml
[scm]
draft_command = 'claude -p --tools "" --no-session-persistence'   # the default; any command that reads a prompt on stdin
```

Set it per Workspace or Project to use another agent or model for drafts.

## Related

- [Review](review.md) — *Approve ▾ → Commit myself…* and *Create PR…* open these.
