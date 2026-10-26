# 0045 — The Changes page follows VS Code's Source Control

Status: accepted (owner, 2026-10-25). Amends ADR-0013 (auto-fetch) and R21 / R107.1.

## Context
The Inspector's Changes page had two UIs for one job: Changes by Project (a row click staged the file, no discard, no diff, Push / Pull / PR hidden) and a single-checkout Details page with diffs squeezed into the side panel. Owners know VS Code's Source Control and expect it.

## Decision
- One page, one card per Source link, VS Code's groups and row actions; a row click opens the diff in the centre, never stages.
- Diffs live in a new Pane kind, the Diff Pane (working tree vs index, index vs HEAD), rendered with the Review Pane's highlighting; per-hunk and per-line staging live there.
- Commit gets VS Code's variants (Amend, Commit & Push, Commit All, Undo Last Commit). RADE never force-pushes, so Amend and Undo are disabled once HEAD is on a remote.
- Sync / Publish / Fetch as VS Code's action button; push still always asks (ADR-0013).
- Auto-fetch exists but is off unless the owner turns it on (`[scm] autofetch`), the only git network call RADE makes without a click; fetch never changes a branch.
- Quick Diff marks in the Viewer's gutter.
- Untracked files are discarded to the trash.

## Not done
Commit graph, stash, merge editor, branch switching (a Worktree's branch belongs to RADE's Worktree model), force push, blame.
```
