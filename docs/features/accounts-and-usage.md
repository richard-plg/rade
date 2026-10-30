# Agent Accounts and usage

Most people who use agents a lot have more than one login: a company Claude seat, a team seat, a
personal subscription, a Codex plan. RADE lets each Workspace declare its **Agent Accounts**,
runs every Agent under exactly one, and shows how close each login is to its limits — before you
hit them.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/usage-light.png">
  <img alt="The Usage page: accounts sorted by their tightest limit, next resets with forecasts, and one account's limits and tokens per day" src="../images/usage-dark.png">
</picture>

## Declare accounts

An account is an agent (`claude` or `codex`), a name and the config dir that holds its login
(`CLAUDE_CONFIG_DIR` / `CODEX_HOME`). Declare them in the Workspace's `workspace.toml`:

```sh
rade account add --workspace acme --agent claude --name acme-work --config-dir ~/.claude-acme --default
rade account add --workspace acme --agent claude --name acme-team --config-dir ~/.claude-acme-team
rade account add --workspace acme --agent codex  --name acme      --config-dir ~/.codex-acme
rade account list
rade account default acme-team --agent claude
```

Then log in once per config dir the agent's own way (e.g. run `CLAUDE_CONFIG_DIR=~/.claude-acme
claude` and `/login`), and run `rade setup` so each dir gets RADE's hooks.

## Accounts for every Workspace

Logins you use everywhere — say `cl` on `~/.claude` and `clw` on `~/.claude-work` — go in
`~/.config/rade/accounts.toml` once instead of every Workspace's file. Each Workspace offers them
after its own accounts:

```sh
rade account add --user --agent claude --name cl  --config-dir ~/.claude --default
rade account add --user --agent claude --name clw --config-dir ~/.claude-work
rade account list            # … claude  clw  ~/.claude-work  (user)
```

A Workspace that must stay on its own logins (a company one) opts out in its `workspace.toml`:

```toml
accounts = "own"
```

Then only its own accounts can be used there (a user account → exit 7). A Workspace's own
`default = true` wins over the user file's.

## Extra environment: a .env file and variables

An account can carry more than its config dir: a `.env` file (`KEY=VALUE` lines, `export`,
quotes and `#` comments allowed) read each time one of its Agents starts, and variables of its own.
A `claude setup-token` login, for example:

```toml
[[account]]
agent = "claude"
name = "clw"
config_dir = "~/.claude-work"
env_file = "~/.claude-5-token.env"   # export CLAUDE_CODE_OAUTH_TOKEN=…

[account.env]
ANTHROPIC_MODEL = "opus"
```

```sh
rade account set clw --env-file ~/.claude-5-token.env --env ANTHROPIC_MODEL=opus
rade account set clw --unset-env ANTHROPIC_MODEL
```

The Workspace's `[env]` comes first, then the `.env` file, then the account's variables, then the
account's config dir and `RADE_ACCOUNT` — so the login itself is always the config dir (setting
`CLAUDE_CONFIG_DIR`, `CODEX_HOME` or `RADE_*` there is refused). Values from the `.env` file never
reach RADE's state, logs or answers; a missing or broken file stops the spawn (exit 2, with the
file and line) and a restored Agent comes back as a shell that says why. Detailed limits aren't
available for a token login.

**Settings → Accounts** (`Ctrl+,`) does the same on screen: your accounts listed on the left
(grouped by agent, `✓` on the default this Workspace uses, `.env · n vars`, a lock on a
Workspace's own), the selected one on the right with one **Environment** table in the order an
Agent gets it — the login row `CLAUDE_CONFIG_DIR` (empty = the default login `~/.claude`), the
`.env` file and the keys it holds (never their values), then your variables. *+ Add variable*
adds a row in place (Enter saves, Esc drops it), *Paste KEY=VALUE lines* adds a whole block, and
*+ New account* writes nothing until *Create*. A Workspace's own accounts show read-only, with
*Open file*. Changes reach Agents started afterwards.

## The default login

Leave the config dir out (`rade account add --user --agent claude --name clp`) and the account
uses the agent's default login: its Agents get no `CLAUDE_CONFIG_DIR` / `CODEX_HOME` at all, as a
plain `claude` would (Claude Code then keeps its `~/.claude.json`). Such an account lives only in
`accounts.toml`. If you also declare `~/.claude` written out (`cl`), the two are one login and
one quota: Settings says so, and the forecast never suggests one instead of the other.

## A Workspace that sets its login in `[env]`

A Workspace made with `rade workspace setup` may set `CLAUDE_CONFIG_DIR` in its `[env]` and
declare no account. It keeps that login: its default is your account on that dir (`clw` for
`~/.claude-work`), or no account at all when none is on it — never your personal default.
`--account` can still pick any of yours; add `accounts = "own"` to forbid that.

## Moving from aliases or a command prefix

If `config.toml` starts the agent with a login, such as

```toml
[agents.claude]
command = "CLAUDE_CONFIG_DIR=~/.claude-work claude --dangerously-skip-permissions"
```

every account on another login is refused (exit 7, `remove the CLAUDE_CONFIG_DIR=… prefix from
[agents.claude] command`). Drop the prefix and let accounts choose:

```toml
[agents.claude]
command = "claude --dangerously-skip-permissions"
```

then declare one account per alias (`cl`, `clw`, `clv` …) and make the one your Subagents should
use the default.

## Agents you start by hand

If you type `clw` (or `CLAUDE_CONFIG_DIR=~/.claude-work claude`) in a Terminal, RADE matches the
config dir the Agent reports against your accounts and gives the Pane that account while the
Agent runs (`rade list --json` shows `"account_detected": true`): the Statusline and usage follow
it, and Subagents it starts inherit it. Without `CLAUDE_CONFIG_DIR` it is the account on
`~/.claude`.

## Start an Agent under an account

The titlebar **`+`** menu lists the accounts of the Workspace on screen, grouped by agent, with
the default ticked and each one's current limit. Accounts of other Workspaces are shown dimmed
with a lock: they can't be used here.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/new-menu-light.png">
  <img alt="The + menu: Shell, Claude Code accounts with their limits, Codex accounts, Fan-out and Browser" src="../images/new-menu-dark.png">
</picture>

```sh
rade spawn --agent claude --account acme-team "Review the payment retries"
```

Without `--account`, a Subagent uses its parent's account, anything else the Workspace default.
The Pane gets that account's config dir and `RADE_ACCOUNT`. RADE refuses (exit 7) an account from
another Workspace, or an agent command whose `CLAUDE_CONFIG_DIR=` / `CODEX_HOME=` prefix points at
a different login — a company Agent can't quietly run on your personal seat, or the other way round.

## Usage and limits

RADE reads limits and tokens from what the agent CLIs already write — nothing is sent anywhere:

- **Claude Code**: `rade setup` wraps the statusline command (keeping yours, run after it) so
  RADE receives the 5-hour, weekly and spend limits Claude Code reports.
- **Codex**: RADE reads the rate limits and token counts in its session logs.
- **Tokens** for both come from the local session logs, by day, Project and model (cache reads
  excluded).

The **Usage page** — from the Statusline's usage cell, the titlebar gauge or `> usage` in the
palette — shows every account sorted by its **tightest limit** (the one closest to 100 %),
**Next resets** with forecasts (`hits 100% in ~40m`, `97% at reset`), and the selected account's
limits and tokens per day, per Project and per model. Filter by period and Workspace. Close it
with the `✕` in its header or `Esc`.

The **Statusline** shows the focused Agent's account: `claude 5h ▰▰▱ 62% · wk 41%`.

## Before you hit a limit

When a limit is forecast to reach 100 % before it resets, a Toast offers to switch:

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/forecast-light.png">
  <img alt="A Toast: acme-work will hit its 5-hour limit in about 40 minutes, with Next agents use acme-team" src="../images/forecast-dark.png">
</picture>

**Next agents use acme-team** changes only the default for Agents you start afterwards; running
ones keep their login. Activity logs each account passing 80 %, getting blocked and resetting.

```sh
rade account usage               # every account, one line each
rade account usage acme-work --json
```

## Detailed limits (opt-in)

Claude's per-model weekly limits (Opus / Sonnet) aren't in the statusline. With
`detailed_limits = true` on a Claude account, RADE asks Claude's usage endpoint for them at most
once every 5 minutes. The login token is read from the account's config dir only for that call,
passed to `curl` on stdin and never stored or logged. Any unexpected answer turns the option off
and says so in Activity.

## Related

- [Workspaces and identity](workspaces-and-identity.md)
- [Showcase: two companies, one laptop](../showcases/two-companies-one-laptop.md)
