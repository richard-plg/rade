# Agent Accounts carry their own env, can be declared once for every Workspace, and are detected on hand-started Agents

Status: accepted 2026-10-25 (M47, PRD R125–R126). Amends ADR-0035 decision 1 (accounts only in `workspace.toml`) and reverses its rejected option "Accounts in the user `config.toml`" for a separate user file with an opt-out per Workspace.

The owner runs one Claude Code binary under two logins through shell aliases: `cl` = `CLAUDE_CONFIG_DIR=~/.claude claude …` and `clw` = `~/.claude-work` plus a `.env` file holding `CLAUDE_CODE_OAUTH_TOKEN` (a `claude setup-token` login). An Agent Account (ADR-0035) sets only the config dir, so `clw` cannot be expressed; accounts must be declared again in every Workspace (the owner has six personal ones); and an Agent typed by hand in a Terminal (`clw`) has no account, so the Statusline, usage and Subagents don't know which login runs.

## Decision

1. **Env per account.** `[[account]]` gains `env_file` (a dotenv file: `KEY=VALUE`, optional `export `, single / double quotes, `#` comments; no interpolation, no multi-line values) and `[account.env]` (variables one by one). A Pane's env, later wins: Workspace `[env]` → `env_file` → `[account.env]` → the account's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` + `RADE_ACCOUNT`. The agent's config-dir variable and `RADE_*` are refused as keys (a validation error naming the file): the login stays chosen by `config_dir` alone (ADR-0035 decision 3).
2. **The env file is read at spawn and at restore, never stored.** Its values never reach `state.json`, logs, Activity, `debug.*`, `account.list` or Settings (keys only). A missing or broken file refuses the spawn (`USAGE` naming the file and line, no Pane) and restores the Pane as a plain shell with the reason, like ADR-0035's refused invocation.
3. **User accounts** live in `$XDG_CONFIG_HOME/rade/accounts.toml` (shared by both flavors like `config.toml`), not in `config.toml`: `Config` rejects unknown keys, so an older installed `rade-app` reading a shared `config.toml` would refuse the whole file. Every Workspace may use them besides its own, unless its `workspace.toml` sets `accounts = "own"`: then a user account there is `FORBIDDEN` (exit 7), so a company Workspace stays locked to its logins. A name declared for one agent in both files is a validation error naming both. Default per agent: the Workspace's `default = true`, else the user file's, else the first listed (Workspace accounts first). The usage store stays keyed by the config dir (ADR-0035 decision 7), so a user account has one history whatever Workspace uses it.
4. **Detection.** When an Agent's session-start hook reports its invocation in a Pane without an account, RADE matches the invocation's config-dir variable (unset = `~/.claude` / `~/.codex`) by `realpath` against the accounts the Pane's Workspace may use; exactly one match (several: the default among them) becomes the Pane's account, marked detected. It acts like a chosen one (Statusline, usage, Subagents inherit it) but injects nothing into the running process, and is cleared when that Agent ends.
5. **Settings → Accounts** edits the user file with `toml_edit` (one write per change, parsed first, atomic, other bytes kept). Workspace accounts are shown read-only (R112.6: no per-Workspace editing in Settings). Changes reach new Panes only.

## Considered Options

- **A new "Profile" concept next to Agent Accounts**: the same thing twice — lock, usage store, `+` menu and restore would all need a second path. Rejected; "Profile" stays an avoided word (CONTEXT.md).
- **More Presets (`[agents.clw]`)**: works today, but bypasses the Workspace lock, splits usage by Preset instead of login, and puts the token in a command string. Rejected.
- **User accounts in `config.toml`**: breaks an older `rade-app` sharing that file (`deny_unknown_fields`). Rejected for a separate file.
- **Store the env file's values in state for restore**: puts a login token on disk a second time. Rejected; it is read again.
- **Detection by `RADE_ACCOUNT` only**: a hand-typed alias never sets it. Rejected for the config dir match.

## Consequences

- `accounts = "own"` is opt-in: a company Workspace that declared nothing is open to user accounts until its owner adds it (the docs say so).
- A detected account changes Statusline and usage attribution of hand-started Agents; nothing about their process.
- Detailed limits (ADR-0035 decision 9) read the token from the config dir; an account whose env sets `CLAUDE_CODE_OAUTH_TOKEN` has no token there, so the option says it is not available for a token login (whether such a token may call the endpoint is unverified).
