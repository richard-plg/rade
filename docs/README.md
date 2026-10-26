# RADE documentation

## First steps

- [Getting started](getting-started.md) — install, hooks, two Workspaces, a Project, a team of Agents.
- [Organizing Workspaces and Projects](guides/organizing-workspaces-and-projects.md) — how to set
  up identities, products and Worktrees so everything else just works.

## Features

| Page | What it covers |
|---|---|
| [Workspaces and identity](features/workspaces-and-identity.md) | A Workspace per company or client; switching; git / SSH / `gh` per Workspace; setup, import, doctor |
| [Projects and Worktrees](features/projects-and-worktrees.md) | Sources, Projects and their bases, checkouts, drift, the Navigator, Project home |
| [Agent Overview](features/agent-overview.md) | Every Agent of every Workspace on one screen (`Ctrl+Shift+Space`), live Previews |
| [Agents](features/agents.md) | Start agents, the Agent Grid of Subagents, status, Notifications, Inbox, Session Restore, Presets |
| [Voice prompts](features/voice.md) | Hold `F8`, speak, a read-only voice agent drafts the prompt; revise by voice; speech-to-text setup |
| [Viewer, files and search](features/viewer-files-search.md) | Files tree; code, Markdown + Mermaid, HTML, PDF; Search, command palette |
| [Terminals and layout](features/terminals-and-layout.md) | Tabs, splits, zoom, Ctrl+click paths, image paste, pause, system monitor |
| [The `rade` CLI](features/cli-for-agents.md) | How agents drive RADE and each other |
| [Agent Accounts and usage](features/accounts-and-usage.md) | Logins per Workspace, the `+` menu, limits and forecasts |
| [Browser and Annotations](features/browser.md) | Browser Pane, annotate → Agent, `rade browser`, Chromium |
| [Code navigation](features/code-navigation.md) | Definition, references, callers, hover, Impact, `rade code` |
| [Changes and PRs](features/changes-and-prs.md) | Stage, commit, push, Create PR, Generate drafts |
| [Fan-out](features/fan-out.md) | One prompt to several Agents, Judge, Winner |
| [Review](features/review.md) | The Review Pane: Viewed, inline comments, rounds, Approve |
| [Settings and config](features/settings-and-config.md) | Settings page, `config.toml`, layered config, trust |

## Showcases

[All showcases](showcases/README.md): One agent, a team of agents · Three Workspaces, one window ·
Two companies, one laptop · Claude asks Codex for a review · Fix a UI bug with annotations · One
feature across two repositories · Fan out and pick a winner · Review in rounds.

## Reference

- [Keybindings](keybindings.md)
- [CLI and socket API](API.md) — the stable contract (since 1.0.0)
- [AGENTS.md snippet](AGENTS-snippet.md) for Codex, OpenCode, Antigravity
- [Glossary](../CONTEXT.md)
- [Changelog](../CHANGELOG.md) · [Releasing](RELEASING.md)

## For contributors

[CONTRIBUTING.md](../CONTRIBUTING.md), and [internals/](internals/): requirements (`PRD.md`), build
order (`MILESTONES.md`), architecture decisions (`adr/`), verification records (`checks/`),
research notes and design specs.
