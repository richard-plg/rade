# Workspace setup may edit the user's git config and call gh / ssh-keygen; it never stores a secret

Status: accepted 2026-10-02 (M15b, PRD R25–R27). Extends ADR-0008 ("no auth integration in the app") and ADR-0015.

A Workspace (ADR-0015) is only useful when its identity is real on the machine: a git identity for every repository under the root, an SSH key on the right GitHub account, a `gh` login in the Workspace's own `GH_CONFIG_DIR`, hooks in its Agent config dirs. On the owner's machine this lives in hand-written `includeIf` blocks, a zsh `chpwd` hook and a README; another person, or the owner on a new machine, has to redo it by hand. The design (v0.7.1:docs/design/workspaces-tasks-reports.md §4.1) asked RADE to do it.

## Decision

1. **Local CLI commands, not app methods.** `rade workspace setup | import | clone | doctor` run in the caller's terminal (prompts, the `gh` device flow and progress show there; the GTK thread never blocks); only registering and `clone` talk to the app.
2. **Every file edit takes `rade setup`'s path**: a diff, `--dry-run`, a `<file>.rade-bak-<stamp>` backup, an atomic write that keeps the mode and edits a symlink's target. RADE writes `<root>/.rade/workspace.toml`, `<root>/.gitconfig` (through `git config --file`, no hand-written parser) and **one marked block** in `~/.gitconfig` (`# >>> rade workspace <ws> >>>`); nothing outside the block. It fills gaps and never overwrites: a different existing value is left and reported by `doctor`.
3. **Git identity through `includeIf "gitdir:<root>/"` + `core.sshCommand`** in the Workspace's `.gitconfig`, not a host alias in `~/.ssh/config`: one user file fewer, remote URLs unchanged, and it applies outside RADE too (editors, cron). An existing alias setup (the owner's `github-globex` + `url.insteadOf`) is left alone and read by `import`. `clone` passes `GIT_SSH_COMMAND` because no `includeIf` applies before the folder exists.
4. **Secrets stay with the CLIs.** RADE runs `ssh-keygen` (no passphrase, like the owner's keys; mode 600), `gh auth login --insecure-storage` (the token goes into the Workspace's own `hosts.yml`, mode 600: the keyring has one slot per host for every `GH_CONFIG_DIR`, so a second account silently replaces the first), and `gh ssh-key add`. It never reads a private key, never prints or stores a token, and `doctor` checks only that `oauth_token` is present. It never runs `gh auth switch` (ADR-0015).
5. **`[env]` is the one place for accounts.** The design's `[accounts]` table is dropped: `CLAUDE_CONFIG_DIR` / `CODEX_HOME` in `[env]` (portable with `~`) are what Panes get and where `rade setup` installs hooks. The design's `workspace.local.toml` is dropped too: the root is already machine-local in `state.json`.
6. **Agents set up Workspaces through these commands only** (skill `rade-workspace`): they write `workspace.toml` or pass flags and run `setup --dry-run`, `setup`, `doctor`; they never hand-edit `~/.gitconfig`, `~/.ssh` or `gh` directories.

## Considered Options

- **A host alias per Workspace in `~/.ssh/config`** (the owner's current way): needs rewritten remotes (`url.insteadOf`) and a second user file. Rejected for new setups; supported on import.
- **Keyring storage for `gh`**: safer at rest, but one slot per host breaks two accounts. Rejected (`--insecure-storage`, file mode 600, documented).
- **Overwrite drifting values**: would "fix" a machine in one run, but could flip the identity of a Workspace the user set up on purpose. Rejected: fill gaps, report the rest.
- **A setup dialog in the app**: the `gh` device flow and key prompts need a terminal anyway. Rejected for a palette form that runs the command in a Terminal Pane.

## Consequences

- RADE now edits a user file outside its own directories (`~/.gitconfig`), always inside its marked block and with a backup; removing the block undoes it.
- The plain-text `gh` token (mode 600) is a deliberate trade-off; `doctor` warns when a Workspace's token is in the keyring instead.
- `doctor` makes read-only network calls (`gh api`, `ssh -T`).
