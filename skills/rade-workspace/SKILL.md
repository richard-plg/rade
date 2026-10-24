---
name: rade-workspace
description: Set up, import, check or repair a RADE Workspace's identity on this machine - git name/email per folder (includeIf), an SSH key per account, a separate gh login, keys on GitHub, signing, Agent config dirs - with `rade workspace setup|import|doctor|clone`. Use when the user wants a new Workspace (e.g. "set up ~/workspaces/acme for my work account"), moves Workspaces to a new machine, or commits/pushes go out with the wrong account.
---

# Workspace setup with `rade workspace`

A Workspace is a folder (its root) with `.rade/workspace.toml`. Its `[identity]` (git name, email, GitHub login, SSH key, signing) and `[env]` (`GH_CONFIG_DIR`, `CLAUDE_CONFIG_DIR`, …) are made real by `rade workspace setup`. You edit **only** `workspace.toml` (or pass flags); `rade` does every other file and command.

**Never** hand-edit `~/.gitconfig`, `~/.ssh/*`, `~/.config/gh*` or shell rc files, never run `ssh-keygen`, `gh auth login` or `gh ssh-key add` yourself, never read private keys or print tokens.

## 1. Gather the facts
Ask the user, or read the machine (read-only):
- `git config --global --get-regexp includeif` — folders that already have their own git identity
- `ls ~/.ssh ~/.config | grep -E '^(id_|gh)'` — existing keys and gh config dirs
- `rade workspace list` — Workspaces RADE knows

An existing, already-working folder: `rade workspace import <root> --dry-run`, show the result, then run it without `--dry-run`. Done; go to step 4.

## 2. Plan
```sh
rade workspace setup ~/workspaces/acme --git-name "Ann Example" --email ann@acme.com \
  --github ann-acme [--ssh-key id_ed25519_acme] [--sign ssh|none] [--claude shared|separate] \
  [--default-owner acme-org] --dry-run
```
Show the user the diffs and the commands it would run. A flag that conflicts with `workspace.toml` fails: edit the file or drop the flag.

## 3. Run it — the login step in its own Pane
`gh auth login` is a browser device flow, so run setup where the user can see and answer it:
```sh
p=$(rade spawn --cmd 'rade workspace setup ~/workspaces/acme --git-name "Ann Example" --email ann@acme.com --github ann-acme' --name setup --json | jq -r .pane)
rade wait "$p" --until exited --timeout 900; rade read "$p" --scrollback
```
Tell the user to finish the login in that Pane (one-time code + browser). A second run prints `nothing to do`.

## 4. Check until it passes
```sh
rade workspace doctor ~/workspaces/acme
```
Each `FAIL` / `warn` line has a `fix:` command. Run `rade` fixes yourself; anything that logs in or adds a key goes in its own Pane as in step 3. Repeat `doctor` until it exits 0. A shell-rc warning names the variable and the rc file line: tell the user, don't edit it.

## Clone the repositories
```sh
rade workspace clone --workspace acme api web acme-org/tools
```
Clones with the Workspace's gh account and SSH key, skips ones already there, exits 2 if any failed.
