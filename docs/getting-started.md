# Getting started

From install to your first reviewed change in about ten minutes.

## 1. Install

RADE runs on **Ubuntu 26.04 with GNOME on Wayland** (amd64). Download the `.deb` from the
[latest release](https://github.com/richard-plg/rade/releases/latest):

```sh
gh release download -R richard-plg/rade -p 'rade_*_amd64.deb' -p SHA256SUMS -D /tmp/rade
(cd /tmp/rade && sha256sum -c SHA256SUMS) && sudo apt install /tmp/rade/rade_*_amd64.deb
```

(or download both files from the release page in a browser). This installs `/usr/bin/rade`,
`/usr/bin/rade-app` and the **RADE** launcher. To build it yourself, see
[CONTRIBUTING.md](../CONTRIBUTING.md).

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

## 3. A Workspace and a Project

Put your repositories under a folder per identity (see
[Organizing Workspaces and Projects](guides/organizing-workspaces-and-projects.md)):

```sh
rade workspace add ~/workspaces/acme
rade source add ~/workspaces/acme/shop-web
rade project add Shop --source shop-web:develop     # the branch your work starts from
```

**Shop** appears in the Navigator on the left. Click it for its Project home.

Using several GitHub accounts or git emails? Run
`rade workspace setup ~/workspaces/acme --dry-run` — see
[Workspaces and identity](features/workspaces-and-identity.md).

## 4. Start an Agent on a Worktree

Give the work its own Worktree so the Agent doesn't touch your checkout:

```sh
rade spawn --source shop-web --agent claude --worktree new --name coupons "Add coupon codes to the cart"
```

(or the palette's *Spawn Agent…* with *New Worktree*). A new Tab opens in Worktree `coupons`, cut
from `develop`. Watch it in the Navigator: blue while working, amber when it needs you, green
when done. When it needs you, `Ctrl+Shift+G` jumps there.

## 5. Review

When the turn ends, click **Review** on the Toast (or `Ctrl+Shift+R`):

1. Read the diff file by file; `v` marks a file viewed.
2. Select lines, `c`, write a comment, `Enter`.
3. **Request changes** — the Agent gets your comments as `Review round 1`.
4. When it's done, the Review shows only what changed since. Repeat, or **Approve ▾ → Ask Agent
   to commit**.

## 6. Ship

Open **Changes** (`Ctrl+Shift+M`), check the commit, **Push**, then **Create PR…** —
**Generate** drafts the title and description. The PR targets `develop`, the same base the Review
used.

## Keep going

- Agents can drive each other: ask Claude Code to *"have codex review this"*. → [CLI for agents](features/cli-for-agents.md)
- Try it three ways at once. → [Fan-out](features/fan-out.md)
- Point at what's wrong on a web page. → [Browser and Annotations](features/browser.md)
- Every Agent of every Workspace: `Ctrl+Shift+Space`. → [Agent Overview](features/agent-overview.md)

Keep RADE open while Agents work: quitting stops their processes. On the next start Session
Restore brings back the layout and resumes each Agent's session.
