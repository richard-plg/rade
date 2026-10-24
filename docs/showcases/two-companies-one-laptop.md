# Two companies, one laptop

**The idea:** you work for Acme and keep personal projects (or two clients) on the same machine.
Commits must go out with the right email, pushes with the right GitHub account, and agents must
run — and spend — on the right subscription. RADE makes each of these a property of the folder.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/switcher-light.png">
  <img alt="The Workspace switcher with acme and personal" src="../images/switcher-dark.png">
</picture>

## 1. One Workspace per identity

```sh
rade workspace setup ~/workspaces/acme --name acme \
  --git-name "Sam Rivera" --email sam@acme.dev --github sam-acme --claude separate
rade workspace setup ~/workspaces/personal --name personal \
  --git-name "Sam Rivera" --email sam@hey.dev --github samrivera
```

Each gets its own git identity (through `includeIf`, so every Worktree under it too), SSH key
with signing, `gh` login in its own config dir and agent config dirs. `rade workspace doctor`
checks all of it end to end.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/doctor-light.png">
  <img alt="rade workspace doctor output" src="../images/doctor-dark.png">
</picture>

## 2. Agent Accounts per Workspace

```sh
rade account add --workspace acme     --agent claude --name acme-work --config-dir ~/.claude-acme --default
rade account add --workspace acme     --agent codex  --name acme      --config-dir ~/.codex-acme
rade account add --workspace personal --agent claude --name personal  --config-dir ~/.claude --default
```

An Agent started in `acme` runs on `acme-work` — never on your personal seat. RADE refuses
(exit 7) an account from another Workspace, even if a command line tries to point at it.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/new-menu-light.png">
  <img alt="The + menu: this Workspace's accounts with their limits, other Workspaces' accounts locked" src="../images/new-menu-dark.png">
</picture>

## 3. Work in both

Switch with `Ctrl+Alt+1` / `Ctrl+Alt+2`. The other Workspace keeps running; when one of its
Agents needs you, the switcher shows `!` and the notification says `[personal] blog needs input`.
`Ctrl+Shift+Space` shows the Agents of both:

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/overview-by-workspace-light.png">
  <img alt="Agent Overview grouped by Workspace" src="../images/overview-by-workspace-dark.png">
</picture>

## 4. Watch the limits

The Usage page shows every login's tightest limit and when it resets. When `acme-work` is about to
run out, a Toast offers **Next agents use acme-team** — for new Agents only.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/usage-light.png">
  <img alt="The Usage page with several accounts" src="../images/usage-dark.png">
</picture>

## Why it works

- Identity follows the folder, not a global switch you might forget (`gh auth switch` is never
  used).
- Every Pane gets its Workspace's environment, so even a plain `git push` in a terminal goes out
  as the right account.
- Logins are locked to their Workspace.

## What you used

[Workspaces and identity](../features/workspaces-and-identity.md) ·
[Agent Accounts and usage](../features/accounts-and-usage.md) ·
[Agent Overview](../features/agent-overview.md) ·
[Organizing Workspaces and Projects](../guides/organizing-workspaces-and-projects.md)
