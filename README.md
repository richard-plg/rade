# RADE

[![CI](https://github.com/richard-plg/rade/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/richard-plg/rade/actions/workflows/ci.yml) [![Release](https://github.com/richard-plg/rade/actions/workflows/release.yml/badge.svg)](https://github.com/richard-plg/rade/releases/latest)

**Agentic Development Environment for Linux** (Ubuntu, GNOME Wayland). Run coding agents side by side, see which ones need you, and review their changes in one window. A Workspace holds your repository Sources; a Project groups the Sources a product needs, each on its own base branch. Agents work in Worktrees with their own Tabs and Panes. RADE embeds Ghostty's GPU terminal and works with Claude Code, Codex, Antigravity and OpenCode through a `rade` CLI, so any agent can spawn and drive others. It includes an Agent Overview, a Review Pane with review rounds, file and code navigation, git changes, browser previews with annotations, usage limits per agent account, and Fan-out (one prompt, several agents, a judge).

Requirements: [docs/PRD.md](docs/PRD.md) · CLI/socket contract: [docs/API.md](docs/API.md) · Glossary: [CONTEXT.md](CONTEXT.md) · Decisions: [docs/adr/](docs/adr/)

## Install (.deb)

From a [Release](https://github.com/richard-plg/rade/releases) (amd64, Ubuntu 26.04):

```sh
gh release download -R richard-plg/rade -p 'rade_*_amd64.deb' -p SHA256SUMS -D /tmp/rade
(cd /tmp/rade && sha256sum -c SHA256SUMS) && sudo apt install /tmp/rade/rade_*_amd64.deb
```

Changes per version: [CHANGELOG.md](CHANGELOG.md). Versioning, releasing and CI: [docs/RELEASING.md](docs/RELEASING.md).

Or build the package from a checkout:

```sh
scripts/package-deb.sh                              # → target/debian/rade_<version>+git<date>.<commit>_amd64.deb
sudo apt install ./target/debian/rade_*_amd64.deb   # or: pkexec apt install ./target/debian/rade_*_amd64.deb
```

A tagged release packages as `<version>`, any other commit as `<version>+git<date>.<commit>` (higher than the release it follows), so re-running both lines upgrades in place (same commit again: add `--reinstall`).

Installs `/usr/bin/rade` (CLI), `/usr/bin/rade-app` → `/usr/lib/rade/rade-app` with its bundled `libghostty-internal.so`, Ghostty's terminfo / shell integration / themes under `/usr/share/rade`, and the **RADE** launcher (`dev.rade.App.desktop`). No Flatpak/Snap: a sandbox can't spawn arbitrary agent CLIs and PTYs (ADR-0008). Building the package needs the build-from-source toolchain below plus `dpkg-dev`, `binutils` and `python3-gi` with the GdkPixbuf SVG loader (`gir1.2-gdkpixbuf-2.0`, `librsvg2-common`) for the icon PNGs; `tests/e2e/m10.sh` also needs `desktop-file-utils`.

### Moving from a dev build to the package

If you ran RADE from `target/release` with your own launcher or a `~/.local/bin/rade` symlink: quit RADE, install the .deb, then remove them (`rm ~/.local/bin/rade ~/.local/share/applications/rade.desktop`), or the dev CLI shadows `/usr/bin/rade` and talks to the dev app. State, config and Worktrees stay where they were (`~/.local/share/rade`, `~/.config/rade`); `rade setup` needs no re-run (hooks call plain `rade`).

## Build from source

Needs Rust, Zig 0.16, gettext, and the GTK4 / libadwaita / WebKitGTK 6 / GtkSourceView 5 dev packages.

```sh
git submodule update --init
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast)   # → ghostty/zig-out
cargo build --release
target/release/rade-app
```

A build run from `target/` is the **dev flavor** (ADR-0010): window "RADE (dev)", its own socket (`rade-dev.sock`) and state (`~/.local/share/rade-dev`), so it never touches the installed app. Config `~/.config/rade/config.toml` is shared.

`cargo install` also works; the binaries keep using the clone's `ghostty/zig-out` (don't delete it):

```sh
cargo install --path crates/rade-cli && cargo install --path crates/rade-app
```

## First run

Open **RADE** from the app launcher first. These commands use the installed app. For a source build, start `target/release/rade-app` and use `target/release/rade` instead of `rade` so the CLI connects to that dev instance.

```sh
rade setup --dry-run   # preview
rade setup             # install Agent hooks (Claude Code, Codex, Antigravity, OpenCode on PATH) + the Claude Code skill
```

Hooks call `rade` from `PATH` and do nothing outside a RADE Pane. For Codex/OpenCode, add [docs/AGENTS-snippet.md](docs/AGENTS-snippet.md) (installed at `/usr/share/rade/AGENTS-snippet.md`) to your `AGENTS.md`. Agent Presets, limits and keybindings: `~/.config/rade/config.toml` (reloaded live). Terminal look comes from your Ghostty config.

### One repo → one feature

1. Add an existing Git repo and group it in a Project. Replace `~/code/app` with its actual path; `app` is the Source name and `my-app` is the Project name.

   ```sh
   rade source add ~/code/app --name app
   rade project add my-app --source app
   rade project show my-app
   ```

2. Select **my-app** in the sidebar to open its Project home; **Open terminal** starts a shell in the Project's checkout. Start an Agent from the **+** menu, or give it its own Worktree with `rade spawn --agent claude --worktree new "…"`. Sign in to that agent's CLI if necessary.

3. When the Agent's turn ends, open the **Review Pane** (`Ctrl+Shift+R`): read the diff file by file, comment on lines, then **Request changes** to send them back as the next review round, or **Approve**.

4. Commit and create a PR from **Changes** in the Inspector. For a web app, open its dev-server port in a Browser Pane (`Ctrl+Shift+O`) and annotate anything to change.

You do not need several repos or Fan-out for your first feature. Keep RADE open while Agents work: Session Restore restores the layout and resumes supported Agent sessions, but processes do not survive an app restart.

## Key bindings

| Keys | Action |
|---|---|
| `Ctrl+Shift+P` | Command palette (actions, Projects, Panes, files, Fan-out…) |
| `Ctrl+Shift+T` | New Tab |
| `Ctrl+Shift+Enter` / `Ctrl+Shift+D` | Split right / down |
| `Ctrl+Shift+H/J/K/L` | Focus left / down / up / right |
| `Ctrl+Shift+Z` | Zoom the Pane |
| `Ctrl+Shift+W` | Close the Pane |
| `Ctrl+Shift+PgUp` / `PgDn` | Previous / next Tab |
| `Ctrl+Shift+N` | Add Project |
| `Alt+1` … `Alt+9` | Go to Project 1–9 |
| `Ctrl+Shift+G` | Jump to the newest unread Notification |
| `Ctrl+Shift+E` | File tree |
| `Ctrl+Shift+B` | New Worktree |
| `Ctrl+Shift+R` | Review Pane |
| `Ctrl+Shift+O` | Browser Pane (on this Worktree's dev-server port; `Ctrl+L` URL bar) |
| `Ctrl+click` | Open a path (`file:line`) in the Viewer, or a URL |
| `Ctrl+Shift+V` | Terminal paste; an image pastes as a saved PNG path (Ghostty binding, configurable) |
| `Ctrl+V` | Passed to the running program; Agent CLIs may handle image paste themselves |

Rebind under `[keybindings]` in the config.

## CLI cheat sheet

```sh
rade spawn --agent claude "fix the flaky test"   # new Tab (or a Subagent next to the calling Agent)
rade spawn --cmd "npm test" --split down         # any command in a Pane
rade list --tree                                 # Panes, Agents, status
rade send <pane> "also update the docs" --enter
rade wait <pane> --until done --timeout 900      # done|waiting|exited|error|running|idle|/regex/
rade read <pane> --scrollback
rade focus <pane> · rade close <pane>
rade notify "build finished" --title CI
rade open src/main.rs:42                         # show a file in the Viewer
rade source add ~/code/app --name app
rade project add my-app --source app
rade worktree add app feat-x · rade spawn --agent codex --worktree new "try approach B"
rade review add src/lib.rs:10-14 "use a Result"  # Review Comments; `rade review send` → the Worktree's Agent
rade fanout --agents claude,codex "implement X"  # then: rade fanout status|judge|choose|discard
rade browser open 5173 · rade browser snapshot   # Browser Pane; then click|fill|press <ref>, eval, console, screenshot
```

Inside a Pane `RADE_SOCKET`, `RADE_PANE_ID`, `RADE_PROJECT`, `RADE_WORKTREE(_PATH)` are set; outside, `rade` talks to the installed app (`$XDG_RUNTIME_DIR/rade/rade.sock`). `--json` prints raw results. Full contract: [docs/API.md](docs/API.md).

## Development

See [CLAUDE.md](CLAUDE.md) for commands, the code map and the E2E harnesses (`tests/e2e/*.sh` headless, `tests/live/` on the real desktop). License: MIT; third-party code in [THIRD_PARTY.md](THIRD_PARTY.md).
