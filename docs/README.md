# RADE documentation

## First steps

- [Getting started](getting-started.md) — install, hooks, first Project, first reviewed change.
- [Organizing Workspaces and Projects](guides/organizing-workspaces-and-projects.md) — how to set
  up identities, products and Worktrees so everything else just works.

## Features

| Page | What it covers |
|---|---|
| [Agents](features/agents.md) | Start agents, status, Notifications, Inbox, Session Restore, `rade setup`, Presets |
| [Agent Overview](features/agent-overview.md) | Every Agent of every Workspace on one screen (`Ctrl+Shift+Space`) |
| [Terminals and layout](features/terminals-and-layout.md) | Tabs, splits, zoom, Ctrl+click paths, image paste, pause, system monitor |
| [Projects and Worktrees](features/projects-and-worktrees.md) | Sources, Projects and their bases, checkouts, drift, the Navigator, Project home |
| [Workspaces and identity](features/workspaces-and-identity.md) | `workspace.toml`, git / SSH / `gh` per Workspace, setup, import, doctor |
| [Review](features/review.md) | The Review Pane: Viewed, inline comments, rounds, Approve |
| [Changes and PRs](features/changes-and-prs.md) | Stage, commit, push, Create PR, Generate drafts |
| [Code navigation](features/code-navigation.md) | Definition, references, callers, hover, Impact, `rade code` |
| [Viewer, files and search](features/viewer-files-search.md) | Code, Markdown + Mermaid, HTML, PDF; Files, Search, command palette |
| [Browser and Annotations](features/browser.md) | Browser Pane, annotate → Agent, `rade browser`, Chromium |
| [Fan-out](features/fan-out.md) | One prompt to several Agents, Judge, Winner |
| [Agent Accounts and usage](features/accounts-and-usage.md) | Logins per Workspace, the `+` menu, limits and forecasts |
| [Settings and config](features/settings-and-config.md) | Settings page, `config.toml`, layered config, trust |
| [The `rade` CLI](features/cli-for-agents.md) | How agents drive RADE and each other |

## Showcases

[All showcases](showcases/README.md): Claude asks Codex for a review · Review in rounds · Fan out
and pick a winner · Fix a UI bug with annotations · One feature across two repositories · Two
companies, one laptop.

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
