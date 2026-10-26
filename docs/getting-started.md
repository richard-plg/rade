# Getting started

From install to two Workspaces, their Projects and a team of Agents in about ten minutes.

## 1. Install

RADE runs on **Ubuntu 26.04 with GNOME on Wayland** (amd64). Download the `.deb` from the
[latest release](https://github.com/richard-plg/rade/releases/latest):

```sh
gh release download -R richard-plg/rade -p 'rade_*_amd64.deb' -p SHA256SUMS -D /tmp/rade
(cd /tmp/rade && sha256sum -c SHA256SUMS) && sudo apt install /tmp/rade/rade_*_amd64.deb
```

(or download both files from the release page in a browser). This installs `/usr/bin/rade`,
`/usr/bin/rade-app` and the **RADE** launcher. To build it yourself, see
[CONTRIBUTING.md](../CONTRIBUTING.md). Later releases install from inside RADE: see
[Updates](features/updates.md).

You also need at least one coding-agent CLI on your `PATH` —
[Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex),
Antigravity or [OpenCode](https://opencode.ai) — logged in.

## 2. Install the hooks

Open **RADE** from the app launcher, then in its terminal:

```sh
rade setup --dry-run   # what it will change
rade setup
```

This teaches each agent to report its status to RADE (and installs the `rade` skills for Claude
Code). It backs up every file it changes and is safe to re-run. For Codex: run `/hooks` in Codex
once and trust RADE's entries. For Codex / OpenCode / Antigravity, also append
`/usr/share/rade/AGENTS-snippet.md` to your `AGENTS.md`.

## 3. Workspaces: one per identity

Put your repositories under a folder per company, client or personal work (see
[Organizing Workspaces and Projects](guides/organizing-workspaces-and-projects.md)):

```sh
rade workspace add ~/workspaces/acme
rade workspace add ~/workspaces/personal
rade source add ~/workspaces/acme/shop-web --workspace acme
rade source add ~/workspaces/acme/shop-api --workspace acme
rade source add ~/workspaces/personal/blog --workspace personal
```

Switch with `Ctrl+Alt+1…9` or the Workspace button at the top left. A Workspace you're not looking
at keeps running. Using several GitHub accounts or git emails? Run
`rade workspace setup ~/workspaces/acme --dry-run` — see
[Workspaces and identity](features/workspaces-and-identity.md).

## 4. Projects: what ships together

```sh
rade workspace switch acme
rade project add Shop --source shop-web:develop --source shop-api:develop   # the branch work starts from
```

**Shop** appears in the Navigator on the left with its two repositories. Click it for its Project
home. Add one Project per product; a Workspace can hold as many as you need.

## 5. An Agent, and its team

Give the work its own Worktree so the Agent doesn't touch your checkout:

```sh
rade spawn --source shop-web --agent claude --worktree new --name guest "Add guest checkout"
```

(or `+` in the titlebar, or the palette's *Spawn Agent…*). A new Tab opens in Worktree `guest`, cut
from `develop`. Watch it in the Navigator: blue while working, amber when it needs you, green when
done.

Now ask it to split the work: *"Use rade to give the form, the validation and the tests to three
Subagents, two Claude Code and one Codex, then wait for them."* Each Subagent opens beside it in
the Tab's **Agent Grid**, and the parent reads their answers when they finish. See
[One agent, a team of agents](showcases/one-agent-a-team-of-agents.md).

## 6. Who needs me? Agent Overview

Press `Ctrl+Shift+Space` from anywhere, even inside a terminal: every Agent of every Workspace,
most urgent first. Rest on a card to read its **Preview**, `Enter` to jump to it — in whichever
Workspace it is. `Ctrl+Shift+G` jumps straight to the newest Agent that needs you.

## 7. Files

`Ctrl+click` any `path:line` an Agent prints to open it in a Viewer beside it. `Ctrl+Shift+E`
shows the Inspector; its **Files** tab is the focused Worktree's tree with git status colours.
Markdown opens rendered, with Mermaid diagrams.

## Keep going

- Read what an Agent changed and send it comments. → [Review](features/review.md)
- Commit, push and open a PR against the Project's base. → [Changes and PRs](features/changes-and-prs.md)
- Try it three ways at once. → [Fan-out](features/fan-out.md)
- Point at what's wrong on a web page. → [Browser and Annotations](features/browser.md)

Keep RADE open while Agents work: quitting stops their processes. On the next start Session
Restore brings back the layout and resumes each Agent's session.
