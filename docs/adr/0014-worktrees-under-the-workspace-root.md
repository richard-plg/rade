# New Worktrees go under the Workspace root

Status: accepted 2026-10-02 (M15). Supersedes ADR-0007 for Sources in a Workspace other than `default`.

ADR-0007 put every Worktree RADE creates in `$XDG_DATA_HOME/rade/worktrees/<project>/<name>`: outside the repository, so ripgrep, tsc and IDE indexers don't scan each copy. With Workspaces (R23) that place falls outside the Workspace, and the owner's setup keys identity by folder: a zsh `chpwd` hook (`~/.config/zsh/workspaces.zsh`) sets `GH_CONFIG_DIR` from `$PWD` being under `~/workspaces/<ws>/` — and **unsets** it outside, so a shell in an XDG Worktree would undo the Workspace env (R23.3) — and Claude Code, direnv and similar tools read `CLAUDE.md` / `.envrc` / settings from parent folders, so a Workspace-level file applies only below the root.

Git identity is *not* the reason: checked on 2026-10-02 (M15, the design doc's UNVERIFIED item), `includeIf "gitdir:<root>/"` matches a linked worktree anywhere, because its gitdir is `<main checkout>/.git/worktrees/<name>`, inside the root. R23-b still checks the identity in the new place end to end.

Decision: a new Worktree of a Source in Workspace `ws` (root `R`) is created at `R/.worktrees/<source id>/<name>` (`R/.worktrees-dev/…` for a dev build, ADR-0010). The `default` Workspace (rooted at `$HOME`) keeps `$XDG_DATA_HOME/rade[-dev]/worktrees/<source id>/<name>`: `~/.worktrees` would be a new dot-folder in every user's home for no identity gain. The folder is still outside every repository (a sibling of the clones), so ADR-0007's indexer concern still holds; tools started at the Workspace root do see it — `.worktrees` is a dot-folder, which most of them skip.

Existing Worktrees are never moved: their absolute paths are in `state.json`, and Claude Code keys its sessions by cwd, so `git worktree move` would orphan Session Restore's resume (R11.2). They stay listed, open and removable where they are.

## Considered Options

- **Keep XDG and set `user.email` per Worktree** (`git config --worktree`): needs `extensions.worktreeConfig` on the owner's repos and copies one key, not the whole `includeIf` block (signing key, `core.sshCommand`). Rejected.
- **Inside the clone** (`<source>/.worktrees/`): indexers and `git status` of the main checkout see every copy (ADR-0007). Rejected.
- **Move existing Worktrees on upgrade**: orphans Agent sessions. Rejected.

## Consequences

- R23-b checks the identity and the folder end to end; the `includeIf` question is settled above.
- Removing a Workspace's last Source leaves `R/.worktrees` (RADE never deletes folders it did not create in that call).
