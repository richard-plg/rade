# Source control in the app: RADE runs git / gh only on the owner's click; Agents only draft

Status: accepted 2026-10-02 (M14 phase 2). Supersedes the "No commit/push/PR" line of ADR-0008 for this flow; the rest of ADR-0008 stands. Narrowly extended by ADR-0020 (a Batch squash-commits approved Tasks onto its own local Integration Branch without a click).

ADR-0008 kept commit / push / PR out of v1: Agents and the CLI did it, the app had no git-hosting integration. The owner asked (redesign-proposal §5) for a VS Code-like Source Control page in the Right Panel: stage / unstage / discard per file and per hunk, a commit box, Pull / Push, Create PR, and messages an Agent drafts. The risk ADR-0008 avoided is an app (or an Agent through it) changing history or publishing work on its own. The rule that keeps it out:

- **RADE runs `git` / `gh` only when the owner clicks.** Commit, Pull, Push and Create PR are buttons (or palette actions that open the same confirmations); nothing runs on a timer, on an Agent's request or on a socket call. Push and Create PR ask first (a dialog naming the remote, branch, commit count and whether the upstream is set); Discard asks first (it loses work). Commit needs a message and staged changes, and commits only what is staged.
- **Agents only draft text.** "Generate" runs a configurable one-shot command (`[scm] draft_command`, default `claude -p --tools "" --no-session-persistence`: Claude Code headless with no tools) in the Worktree, with a prompt on stdin (the staged diff or the branch's diff, the repository's rules files, recent commit subjects); its stdout fills the message box or the PR title / body. The owner edits and presses the button. The draft command gets no tools by default, so it can't run git itself.
- **No socket or CLI method commits, pushes, pulls or opens a PR.** Agents already run git in their Terminals; a RADE method would only add a second, unconfirmed way. The hidden `debug.scm` (E2E only, ADR-0006) drives the same UI, including its confirmation dialogs.
- **No auth integration.** git uses the user's credential helpers; PRs go through the GitHub CLI (`gh`) and its own login. `gh` missing or not logged in → its message in the page, nothing else breaks.

## Considered Options

- **Keep ADR-0008 as is** (Agents commit and push): the owner wanted to review and confirm in the app, the way VS Code works. Rejected by the owner.
- **Agent commits on a "Generate & Commit" click**: one click less, but the message would be committed unread. Rejected: draft, then confirm.
- **A RADE-owned LLM API client** for drafts: an API key and a provider to manage in RADE. Rejected: a shell command reuses the Agent CLIs already logged in and lets the owner pick any (codex, a local model).
- **libgit2 (git2-rs)**: no subprocess, but a second git implementation next to the CLI the rest of RADE uses (R12, R13), different config / hook / credential behaviour. Rejected: the `git` CLI, as everywhere else.

## Consequences

- Pull is `git pull --ff-only`: a diverged branch fails with git's message instead of making a merge commit the owner didn't ask for; the owner resolves it in a Terminal.
- Hunk staging is `git apply --cached` (unstage `--cached -R`, discard `-R`) of one hunk cut from `git diff` output; it covers modified files. Added, deleted, renamed, binary and untracked files are staged or discarded whole.
- git hooks (pre-commit, commit-msg) run as in a Terminal; their output is shown when they refuse.
- The draft prompt sends the diff to whatever `draft_command` runs (by default Anthropic's API through Claude Code, as the owner's Agents already do). Large diffs are cut (stated in the prompt).
