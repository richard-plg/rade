# Claude asks Codex for a review

**The idea:** a second model catches what the first one missed. Instead of copying a diff between
two terminals, the first agent starts the second itself, waits for its answer and acts on it —
while you watch both side by side.

![Claude Code starts Codex beside it, waits, reads two findings and fixes them](../images/delegate.gif)
<sub>[Watch as MP4](../images/delegate.mp4)</sub>

## What happens

1. Claude Code finished coupon codes on Worktree `coupons`. You ask it:

   > Ask Codex for a second opinion on the coupon change

2. Claude Code runs, in its own terminal:

   ```sh
   rade spawn --agent codex --name rev "Review git diff HEAD~1; list bugs as path:line"
   rade wait rev --until done --timeout 900
   rade read rev --lines 40
   ```

   `rev` opens **beside** it in the same Tab — a Subagent — in the same Worktree, so it sees the
   same files. In the Navigator it shows under `coupons`.

3. Codex reviews and answers with two findings. `rade wait` returns, `rade read` hands Claude the
   answer, and Claude fixes both, runs the tests and tells you it's ready.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/delegate-light.png">
  <img alt="Claude Code on the left with the rade commands it ran and its fixes; Codex on the right with its two findings" src="../images/delegate-dark.png">
</picture>

## Why it works

- **No glue.** Any agent that can run a shell command can drive RADE. `rade setup` gives Claude
  Code the `rade` skill; for Codex and others append
  [AGENTS-snippet.md](../AGENTS-snippet.md) to `AGENTS.md`.
- **It knows when the other is done.** `rade wait` follows the reviewer's status from its hooks —
  `done`, or `waiting` if it asks something (then `rade read` it and answer, or ask you).
- **You stay in control.** Both agents are on screen. Either one needing you raises a
  Notification; you can type into either.
- **Guard rails.** At most 8 live Subagents per agent and 3 levels deep, so a loop of agents
  spawning agents stops.

## Try it

With the skill installed, just say it in plain words: *"have codex review this"*, *"ask claude to
write the tests while you do the migration"*, *"get a second opinion before you commit"*.

## What you used

[CLI for agents](../features/cli-for-agents.md) · [Agents](../features/agents.md) ·
[Projects and Worktrees](../features/projects-and-worktrees.md)
