---
name: rade-config
description: Create or change RADE Projects (a set of Source links, each with one base branch such as develop) and RADE's layered config (Agent commands, Draft command, Judge, editor, a Source's setup) per Workspace, Project or repository - by editing RADE's own TOML files and checking them with `rade project show` and `rade config explain`. Use when the user wants a Project ("group api and app as BEACON on develop"), to link or unlink a repo or change its base, a different Agent command or model for one company or Project, a setup command for new Worktrees, or asks why a setting has some value.
---

# Projects and layered config with `rade`

Everything is a file under a `.rade` folder or a `rade` command. You edit **only** these files:

| File | What |
|---|---|
| `<workspace root>/.rade/projects/<id>/project.toml` | a Project: its Source links (one base each), config for its Sources |
| `<workspace root>/.rade/workspace.toml` | a Workspace's config (next to its `[env]`, `[identity]`) |
| `<source>/.rade.toml` | a repository's own config (counts only once the user trusts it) |
| `~/.config/rade/config.toml` | the user's own config (ask before editing it) |
| `~/.config/rade/accounts.toml` | the user's Agent Accounts for every Workspace (`[[account]]`: `agent`, `name`, `config_dir`, `default`, `env_file`, `[account.env]`); change it with `rade account add --user` / `rade account set`, ask first, never write a token into it |

**Never** edit `state.json` or anything under `~/.local/share/rade*`, never run `rade config trust` without the user saying so (a trusted `.rade.toml` runs its `setup` command), never edit `~/.gitconfig` or `gh` files (that is the `rade-workspace` skill).

## 1. Look first
```sh
rade workspace list                 # Workspaces and their roots
rade source list                    # Sources of your Workspace (ids)
rade project list                   # Projects (file, links and their bases, warnings, errors)
rade config explain                 # every value set in a file, and ignored keys
```

## 2. A Project
A Project links Sources; each link has **one** base branch: new Worktrees start from it and their Review compares with it. Need `dev` and `prod`? Make two Projects (`BEACON` on `beacon-dev`, `BEACON prod` on `beacon-prod`).

Preview, then create it (a commented template; never overwrites; never creates a checkout):
```sh
rade project plan BEACON --source backend:beacon-dev --source app:beacon   # which checkout each link would use; writes nothing
rade project add BEACON --source backend:beacon-dev --source app:beacon     # --source <src>[:<branch>]; no branch = the Source's main branch
```
or edit `project.toml` directly:
```toml
name = "BEACON"
[[source]]
id   = "backend"                 # a Source id from `rade source list`
base = "beacon-dev"                # no `base`: the Source's main branch
[[source]]
id       = "app"
base     = "beacon"
checkout = "auto"                # auto (default) | main | "<path of an existing checkout>"
```
`checkout = "auto"` uses the repo's main checkout when it is on the base, else a Project checkout under `<root>/.worktrees/.projects/<project>/<source>` that RADE makes the first time a Terminal opens on the link. RADE never switches a checkout's branch for you.

One link or key at a time, keeping every comment and the rest of the file (what the app's **Project settings** form does):
```sh
rade project link add beacon web --base main          # link a Source
rade project link set-base beacon backend beacon-dev2   # its Worktrees keep the base they were cut from
rade project link remove beacon web                   # its checkout and Worktrees are kept
rade project set beacon source.app.checkout main      # keys: name, source.<id>.base, source.<id>.checkout; no value removes it
```
A link whose checkout left its base shows `drift` in `rade project show`; `rade project link to-base beacon backend` switches a clean one back (a dirty one is refused: tell the user, never stash for them).

A Source that is not added yet shows as `not added`: `rade source add <path or owner/repo>`. There is no remove command: delete the Project's folder. An old `project.toml` with `bases = [...]` is rewritten once on the first read (the original kept as `project.toml.v1.bak`); read the warnings `rade project show` prints after that.

New work in a Project: `rade worktree add <source> <name> [--project beacon] [--base <ref>]` (`--project` is needed only when several Projects link the Source; no `--base` = the link's base); its Review compares with the base it recorded.

## 3. Config layers
Lowest first: built-in → user `config.toml` → `workspace.toml` → `project.toml` → the Source's `.rade.toml`. Tables merge key by key; a scalar or array replaces. Only these keys are layered:
```toml
[agents.claude]                  # an Agent Preset, merged into the built-in one
command = "claude --model opus"
[scm]
draft_command = "codex exec -"   # Generate (commit message / PR text)
[fanout]
judge = "codex"
[viewer]
editor = "codium"
html_scripts = false             # a local HTML file in the Viewer runs no JavaScript
[agents.codex]
icon = "~/icons/openai.svg"      # your own icon (SVG/PNG) instead of RADE's glyph
[sources.backend]                # by Source id or name
setup = "npm ci"                 # runs once in a new Worktree's first Terminal
```
In a `.rade.toml`, `setup = "npm ci"` at the top means that Source's setup. `keybindings`, `limits`, `desktop_notify`, `status_bar`, `idle_seconds` work only in the user's `config.toml`; anywhere else they are `ignored (app-wide)`. Old `[task]`, `[profiles]`, `[batch]` and `[report]` tables are `ignored (removed in 1.0.0)`: delete them when you see that line.

## 4. Check until clean
After every edit:
```sh
rade project show beacon                          # each link: base, checkout, HEAD, drift; warnings; error with the file and line
rade config explain scm.draft_command --project beacon
rade config explain --source backend            # what a Pane in backend gets; says if .rade.toml is untrusted
```
Fix every `error`, warning and unexpected `ignored` line, then run them again. A `.rade.toml` that says `untrusted` is shown but not applied until the user runs `rade config trust <source>`; any later change to the file untrusts it again.

Code navigation (`[code]`, layered): `[sources.<id or name>] code = false` turns the language servers off for one repository; `[code.server.rust]` / `[code.server.typescript]` take `command` (an array), `settings`, `init` and, for a repository on TypeScript < 5, `tsserver_path` (a TypeScript 5 `lib` folder). `[code] idle_minutes` (15) and `max_servers` (4; about 2 GB each) count only in `config.toml`. A `.rade.toml` `[code]` (a server command from the repository) applies only after `rade config trust`.
