# Organizing Workspaces and Projects

RADE has four levels of organization. Get them right once and everything else follows: which git
identity a commit uses, which agent login runs, what a Review compares against and where new
Worktrees go.

| Level | What it is | Lives in |
|---|---|---|
| **Workspace** | A working identity: a company, a client, your personal work | a folder, e.g. `~/workspaces/acme`, with `.rade/workspace.toml` |
| **Source** | One git repository (or plain folder) in that Workspace | `~/workspaces/acme/shop-web` |
| **Project** | The Sources that ship together, each at one base branch | `~/workspaces/acme/.rade/projects/shop/project.toml` |
| **Worktree** | One checkout where work happens, with its own Tabs and Agents | `~/workspaces/acme/.worktrees/shop-web/checkout-v2` |

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/workspace-light.png">
  <img alt="The Navigator: Project Shop with three Source links, a Worktree with two Agents, then Project Billing and Scratch" src="../images/workspace-dark.png">
</picture>

The Navigator on the left shows exactly this tree for the Workspace on screen:
**Project → Source link → Worktree → Agent**, then **Scratch** for terminals that belong to no
Project.

## Workspaces: one per identity

Make one Workspace for each identity you work under. The usual test: *would a commit, a push or
an agent login go out under a different account?* If yes, it's a different Workspace.

- `~/workspaces/acme` — your employer: work email, work GitHub account, the company's Claude and
  Codex logins.
- `~/workspaces/personal` — side projects: personal email and accounts.
- `~/workspaces/client-x` — a client you contract for, with the login they gave you.

What a Workspace gives every Terminal, Agent, Push and PR opened inside it:

- **Its environment** from `[env]` in `workspace.toml`. Typically `GH_CONFIG_DIR`, so `gh`
  is logged in to the right account in that Workspace and no other. RADE never runs
  `gh auth switch`; each Pane simply gets its Workspace's variables.
- **Its git identity** through a `[includeIf "gitdir:~/workspaces/acme/"]` block in
  `~/.gitconfig`, which also covers every Worktree under the root.
- **Its Agent Accounts** (`[[account]]` entries): which Claude Code / Codex login runs there.
  An Agent in `acme` can never be started with a `personal` login by mistake — RADE refuses it.

Set all of that up with one command instead of by hand:

```sh
rade workspace setup ~/workspaces/acme --name acme \
  --git-name "Sam Rivera" --email sam@acme.dev --github sam-acme --claude separate
rade workspace doctor acme      # checks git identity, SSH key, gh login, keys on GitHub, agent config
```

`setup` shows every file change first (`--dry-run` to only look), never overwrites a value you
already set, and backs up each file it edits. Already set up by hand? `rade workspace import
~/workspaces/acme` writes `workspace.toml` from how your machine is configured today.
See [Workspaces and identity](../features/workspaces-and-identity.md) for the details.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/switcher-light.png">
  <img alt="The Workspace switcher listing acme and personal with their keys and waiting Agents" src="../images/switcher-dark.png">
</picture>

One window shows one Workspace. The others keep running: their Agents keep working, and when one
of them needs you, the switcher shows an amber `!` and the notification names the Workspace.
Switch with `Ctrl+Alt+1…9` or the switcher. To see the Agents of every Workspace at once, open
[Agent Overview](../features/agent-overview.md) (`Ctrl+Shift+Space`).

### Keep repositories under their Workspace root

Clone (or move) each repository under the root of the Workspace it belongs to:

```sh
rade source add owner/shop-web --workspace acme   # clones with acme's gh login, then registers it
rade source add ~/workspaces/acme/shop-api         # registers an existing clone
```

Folder location is what makes git's `includeIf`, a folder-based shell hook or a Workspace-level
`CLAUDE.md` apply. RADE puts new Worktrees under the same root (`<root>/.worktrees/…`), so they
inherit all of it too.

The `default` Workspace (rooted at `$HOME`) always exists. It's fine for trying RADE out; once
you work under more than one identity, give each its own root.

## Projects: what ships together

A **Project** is a named set of Sources that you work on together, each pinned to **one base
branch**. A Source used by a Project is a **Source link**.

```sh
rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop
rade project add Billing --source billing:main --source shared-ui:release/2.4
```

The base matters everywhere:

- a new Worktree on a link is cut from the link's base,
- its Review, its `+/−` counts and Changes compare against that base,
- Create PR targets it by default.

So a Worktree of Shop's `shop-web` shows only *its own* changes, never the commits that are on
`develop` but not on `main`.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/project-home-light.png">
  <img alt="Project home of Shop: Agents, Worktrees and uncommitted counts, the link table with base, checkout, HEAD and state" src="../images/project-home-dark.png">
</picture>

Click a Project in the Navigator to open its **Project home**: each link's base, which checkout
it uses, ahead/behind, and whether it is clean, off its base, or also used by another Project.
**Open terminal** starts a shell in the right checkout.

### One Project per base, not per branch

A link has exactly one base. If you need the same repositories at two bases — say `develop` for
feature work and `release/2.4` for fixes to the release — make two Projects:

```sh
rade project add "Shop"        --source shop-web:develop     --source shop-api:develop
rade project add "Shop · 2.4"  --source shop-web:release/2.4 --source shop-api:release/2.4
```

Feature branches are **not** Projects. A feature is a Worktree inside a Project.

### A Source can be in several Projects

`shared-ui` above is in both Shop (at `develop`) and Billing (at `release/2.4`). That's expected;
the Navigator marks such a link with a chain icon and its tooltip says where else it is used
(`Also in Billing @ release/2.4`).

Two links can't share one folder at two different branches, so RADE gives a link its own
**Project checkout** — a git worktree at `<root>/.worktrees/.projects/<project>/<source>` on the
link's base — whenever the repository's own folder (its *main checkout*) is on another branch or
another link already uses it. Links with the same Source **and** the same base share one
checkout. RADE shows this plan before it creates anything and makes the checkout only when you
first open a terminal or an Agent on the link.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/new-project-light.png">
  <img alt="The New Project dialog: one row per Source with its base and the checkout each link will use" src="../images/new-project-dark.png">
</picture>

### Base drift

Once a link has a checkout, RADE never switches, stashes or resets it for you. If you (or a tool)
check out another branch in it, the link is **off base**: a warning diamond on its Navigator row,
a chip on Project home and a card in Changes.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/drift-light.png">
  <img alt="A Changes card: shop-api is on feat/checkout-v2, Shop's base is develop, with Keep and Move to a Worktree buttons" src="../images/drift-dark.png">
</picture>

You choose what happens:

- **Back to base** — only when the checkout is clean (`rade project link to-base shop shop-api`).
- **Keep** — hide the warning until HEAD moves again.
- **Move to a Worktree…** — put the checkout back on its base and continue the branch in its own
  Worktree (when clean). This is usually what you want: the branch was a feature all along.

### Scratch

Not everything needs a Project. **Scratch** holds terminals at the Workspace root and Worktrees
that belong to no Project — a quick experiment, a one-off script, reading logs. Use it freely;
when a Scratch Worktree turns out to belong somewhere, RADE suggests the Project (`→ Shop?`).

## Worktrees: one per task

Give each piece of work — and each Agent working on its own — its own Worktree:

```sh
rade worktree add shop-web checkout-v2 --project shop       # cut from Shop's base (develop)
rade spawn --agent claude --worktree new "Add coupon codes to the cart"
```

- **Agents don't trip over each other.** Two Agents in one folder edit the same files and run the
  same dev server; two Worktrees don't.
- **Reviews stay small.** The Review Pane shows one Worktree's changes against its base.
- **Your main checkout stays clean**, on its base, ready for the next Worktree.

`--worktree new` names it `wt-<agent>-<n>`; pass `--name` for something you'll recognize in the
Navigator. When the work is merged, remove the Worktree from its Navigator menu (it refuses while
there are uncommitted changes).

A Source can run a **setup command** once in every new Worktree (install dependencies, copy
`.env`):

```toml
# in the repository's own .rade.toml (committed with the code)
setup = "npm ci"

# or, without touching the repository: ~/workspaces/acme/.rade/workspace.toml
[sources.shop-web]
setup = "npm ci"
```

## Config: put each setting at the right level

Settings that differ by company, product or repository go in layered files. Lowest precedence
first:

| Layer | File | Put here |
|---|---|---|
| Built-in | — | RADE's defaults |
| User | `~/.config/rade/config.toml` | Your taste: keybindings, fonts, notifications, limits, app-wide settings |
| Workspace | `<root>/.rade/workspace.toml` | Identity, `[env]`, Agent Accounts, the agent command or model this company uses |
| Project | `<root>/.rade/projects/<id>/project.toml` | The links and their bases; settings for one product |
| Source | `<repo>/.rade.toml` (committed) | What the repository itself needs: its `setup` command, code-navigation settings |

A repository's `.rade.toml` comes from git, so RADE ignores it until you trust its exact content
(`rade config trust shop-web`); any change to it needs trusting again. When you wonder why a
value is what it is:

```sh
rade config explain agents.claude.command --project shop
```

prints the value every layer sets and which one wins.

## Three layouts that work

**Solo, one identity.** One Workspace `~/workspaces/personal`. One Project per product
(`Blog`, `CLI`), usually a single link each. A Worktree per feature or per Agent. Scratch for
everything else.

**A job and side projects.** Two Workspaces, `acme` and `personal`, each with its own git email,
`gh` login and Agent Accounts. In `acme`, one Project per product the team ships (`Shop` with web,
api and shared-ui at `develop`; `Billing` at `main`), plus a release Project when you maintain a
release branch. Agents in `personal` can never use the company's login, and vice versa.

**Two companies or clients.** One Workspace each, set up with `rade workspace setup` so each has
its own SSH key and `gh` config. Keep their repositories under their own roots. Use Agent Overview
to watch both at once, and the [Usage page](../features/accounts-and-usage.md) to see which login
is close to its limit.

## Mistakes to avoid

- **One big Workspace for everything.** Commits and PRs go out under the wrong account, and
  agents use the wrong login. Split by identity.
- **A Project per feature branch.** Projects are long-lived; features are Worktrees.
- **Agents working in the main checkout.** They block the link's base and mix their changes with
  yours. Give each its own Worktree.
- **Switching a link's checkout to a feature branch.** That's Base drift; use *Move to a
  Worktree…* instead.
- **Repositories outside the Workspace root.** Identity rules keyed on the folder won't apply.
  Clone with `rade source add owner/repo --workspace <ws>`.

## Next

- [Projects and Worktrees](../features/projects-and-worktrees.md) — every command and screen.
- [Workspaces and identity](../features/workspaces-and-identity.md) — setup, import, doctor.
- [Agent Accounts and usage](../features/accounts-and-usage.md) — logins per Workspace, limits.
- [Showcase: two companies, one laptop](../showcases/two-companies-one-laptop.md).
