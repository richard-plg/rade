# RADE

[![CI](https://github.com/richard-plg/rade/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/richard-plg/rade/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/richard-plg/rade)](https://github.com/richard-plg/rade/releases/latest) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**An agentic development environment for Linux.** Run Claude Code, Codex and other coding agents
side by side in GPU terminals, see at a glance which one needs you, review what they changed, and
let them drive each other — in one native GNOME window.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/workspace-light.png">
  <img alt="RADE: the Navigator with Projects, Worktrees and Agents; two Agents at work, one needs input; the Inspector's Changes" src="docs/images/workspace-dark.png">
</picture>

## Why RADE

- **Many agents, no chaos.** Each agent runs in its own git Worktree, with its status — working,
  needs input, done, failed — in the Navigator, on its Tab and in [Agent Overview](docs/features/agent-overview.md).
- **Agents drive agents.** A `rade` CLI lets any agent spawn another, wait for it and read its
  answer. *"Have Codex review this"* just works. → [showcase](docs/showcases/claude-asks-codex-for-a-review.md)
- **Review like a PR, in seconds.** Comment on the diff, send it back as a round, see only what
  changed, approve. → [Review](docs/features/review.md)
- **Built for real work.** Projects that span repositories, each at its own base branch; the right
  git identity, GitHub account and agent login per company; usage limits before you hit them.
- **Fast and native.** Ghostty's GPU terminal, GTK4 / libadwaita, Wayland. No Electron, no web UI.

![Two agents starting in their Worktrees; one asks a question](docs/images/agents.gif)
<sub>[Watch as MP4](docs/images/agents.mp4)</sub>

## Features

| | |
|---|---|
| **[Agents](docs/features/agents.md)** — Claude Code, Codex, Antigravity, OpenCode or any CLI; status from their hooks; Notifications and an Inbox; Session Restore resumes every session | **[Agent Overview](docs/features/agent-overview.md)** — `Ctrl+Shift+Space`: every Agent of every Workspace, most urgent first, live previews |
| **[Review](docs/features/review.md)** — one scrolling diff against the base, Viewed files, inline comments, rounds, Approve hand-off | **[Fan-out](docs/features/fan-out.md)** — one prompt to several agents, a read-only Judge, keep the Winner |
| **[Browser and Annotations](docs/features/browser.md)** — your dev server beside the agent; click what's wrong, send it as a prompt | **[Code navigation](docs/features/code-navigation.md)** — definition, references, callers, hover and Impact through real language servers |
| **[Projects and Worktrees](docs/features/projects-and-worktrees.md)** — products spanning repositories, a base branch per link, a Worktree per task | **[Workspaces and identity](docs/features/workspaces-and-identity.md)** — git email, SSH key, `gh` login and agent accounts per company |
| **[Agent Accounts and usage](docs/features/accounts-and-usage.md)** — several logins per agent, limits, forecasts, switch before you run out | **[Changes and PRs](docs/features/changes-and-prs.md)** — stage hunks, commit, push and open PRs; agents draft, you confirm |
| **[Viewer, files and search](docs/features/viewer-files-search.md)** — code, Markdown with Mermaid, HTML, PDF; file tree; project search; command palette | **[Terminals and layout](docs/features/terminals-and-layout.md)** — Tabs, splits, Ctrl+click `file:line`, image paste as a path, IME |

See them in action: **[Showcases](docs/showcases/README.md)**.

## Install

Ubuntu 26.04, GNOME on Wayland, amd64. Download the `.deb` from the
[latest release](https://github.com/richard-plg/rade/releases/latest), then:

```sh
sudo apt install ./rade_*_amd64.deb
rade setup            # teach your agents to report their status (shows a diff, keeps backups)
```

Open **RADE** from the app launcher. Then follow **[Getting started](docs/getting-started.md)** —
ten minutes to your first reviewed change. Building from source: [CONTRIBUTING.md](CONTRIBUTING.md).

## A taste of the CLI

```sh
rade spawn --agent claude --worktree new "Add coupon codes to the cart"
rade spawn --agent codex --name rev "Review git diff HEAD~1; list bugs as path:line"
rade wait rev --until done && rade read rev --lines 100
rade fanout --agents claude,codex,claude "Speed up the product grid"
rade browser open 5173 && rade browser snapshot
rade code callers src/money.ts:1:17
```

Full reference: [The `rade` CLI](docs/features/cli-for-agents.md) · [API](docs/API.md).

## Documentation

**[docs/](docs/README.md)** — getting started, the [Workspaces and Projects guide](docs/guides/organizing-workspaces-and-projects.md),
every feature, showcases, [keybindings](docs/keybindings.md).

## Status

RADE 1.0 is stable: the `rade` CLI, the socket API and the config and state files only change
incompatibly in a new major version ([docs/RELEASING.md](docs/RELEASING.md)). Linux only — Wayland,
GNOME, Ubuntu 26.04 are what it's built and tested on. Not planned: Flatpak/Snap (a sandbox can't
run arbitrary agent CLIs), X11, macOS, Windows.

## Contributing

Bug reports and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). Security issues:
[SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). Embeds [Ghostty](https://ghostty.org) (MIT) and code from
[limux](https://github.com/am-will/limux) (MIT); bundled fonts and icons under their own licenses —
see [THIRD_PARTY.md](THIRD_PARTY.md).
