# M47 — Agent Accounts: env, user-level accounts, detection, Settings (plan)

Owner request 2026-10-25: several logins of one Agent CLI, same binary, different env (the
owner's `cl` = `CLAUDE_CONFIG_DIR=~/.claude`, `clw` = `~/.claude-work` + `~/.claude-5-token.env`
with `CLAUDE_CODE_OAUTH_TOKEN`). Owner chose: (1) env per account, (2) accounts declared once for
every Workspace, (4) detect the account of a hand-started Agent, plus a Settings UI where each
account gets variables one by one and a `.env` file to load.

"Profile" is the owner's word; the term stays **Agent Account** (CONTEXT.md lists Profile under
_Avoid_). No new concept.

## 1. Scope and IDs

- Milestone **M47**, ADR **0047** (amends ADR-0035 decision 1 and its rejected option
  "Accounts in the user config.toml"), PRD **R125** (accounts: env, user level, detection) and
  **R126** (Settings → Accounts). R108.1, R108.2, R108.6, R112.1, R112.6 get 1.0.x amendments.
- Release impact: **MINOR** (new optional keys, new file, new CLI flags, new `account.list`
  fields; nothing removed or changed in meaning).

## 2. Fixed decisions

### D1 — Env per account (R125.1)
```toml
[[account]]
agent = "claude"
name = "clw"
config_dir = "~/.claude-work"
env_file = "~/.claude-5-token.env"      # optional
[account.env]                            # optional, one key per variable
ANTHROPIC_MODEL = "opus"
```
- `env_file`: dotenv subset — `KEY=VALUE`, optional `export `, `'…'` / `"…"` quotes (`\n`, `\"`
  in double quotes), `#` comments, blank lines. No `$VAR` interpolation, no multi-line values.
  A line that does not parse → error naming file and line. `~` expanded.
- Order on the Pane env (later wins): Workspace `[env]` → `env_file` → `[account.env]` →
  `CLAUDE_CONFIG_DIR` / `CODEX_HOME` from `config_dir` + `RADE_ACCOUNT`.
- Reserved keys in `env` / `env_file` → validation error naming the file: the agent's own
  `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (use `config_dir`), any `RADE_*`.
- The file is read **at spawn and at restore**, never cached. Missing / unreadable / bad → spawn
  fails `USAGE` naming the file, no Pane; at restore → plain shell in its cwd with
  `rade: not resumed: …` (same path as R108.7).
- **Secrets never leave the Pane env**: no value from `env_file` is written to `state.json`,
  Activity, logs, `debug.*`, `account.list` or `rade config explain` (keys only). Inline `env`
  values are in the TOML file by the owner's choice; `account.list` still returns keys only.

### D2 — User-level accounts (R125.2)
- New file **`$XDG_CONFIG_HOME/rade/accounts.toml`** holding `[[account]]` tables (same shape as
  D1). Not `config.toml`: `Config` is `deny_unknown_fields`, so an older installed `rade-app`
  sharing `config.toml` would reject the whole file.
- Every Workspace sees its own accounts **plus** the user accounts, unless its `workspace.toml`
  sets top-level `accounts = "own"` (then a user account there → `FORBIDDEN`, exit 7, as R108.3).
  RADE does not edit `viact`'s file; the docs tell the owner to add `accounts = "own"` to company
  Workspaces.
- One (agent, name) in both the Workspace and the user file → validation error naming both files.
- Default per agent: the Workspace's `default = true`, else the user file's `default = true`,
  else the first listed (Workspace accounts listed first).
- `Account` gains `scope: User | Workspace` (UI, `account.list`). Usage store stays keyed by
  `config_dir` (ADR-0035 decision 7): a user account shows once in `rade account usage`.
- `rade setup` installs hooks into user accounts' `config_dir` too.

### D3 — Detect a hand-started Agent's account (R125.3)
- When the session-start hook reports an invocation (`handlers.rs` → `set_invocation`) for a
  Pane **without** an account: take the Preset's account kind, the recorded `CLAUDE_CONFIG_DIR` /
  `CODEX_HOME` (unset → `~/.claude` / `~/.codex`), compare by `realpath` with the accounts the
  Pane's Workspace may use. One match → set `pane.account` + `account_detected = true`. Several →
  the default among them, else none. None → nothing.
- A detected account behaves like a chosen one for the Statusline, Pane header, Overview, usage
  and Subagent inheritance; it is recorded in `state.json`. No env is injected (the Agent already
  runs). It is cleared when the Pane's Agent ends.
- A Pane that already has an account is never re-detected.

### D4 — Settings → Accounts (R126)
- New category **Accounts** on the Settings page (R112), not a row-per-key list: one card per
  account the shown Workspace may use.
  - User accounts: editable. Workspace accounts: read-only with `from <root>/.rade/workspace.toml`
    and an "Open file" button (R112.6: no per-Workspace editing in Settings).
  - Card: agent (Claude Code / Codex), name, config dir (entry + folder chooser), Default switch,
    **`.env` file** (entry + file chooser + `Load` status: `n variables` or the error with line;
    the loaded keys listed, values hidden), **Variables** table: one row per `KEY` | `VALUE` |
    delete, `Add variable` button. A value whose key matches `TOKEN|SECRET|KEY|PASSWORD` is a
    password entry with peek. Reserved keys are refused inline.
  - `Add account` and `Delete account…` (confirm).
- Every change = one `toml_edit` write of `accounts.toml`, parsed before writing, atomic, comments
  and other bytes kept (as R112.3). Bad value → error shown, nothing written.
- Applies to **new** Panes only; running Panes keep their env (shown as a hint line).
- `debug.ui app_settings_accounts` returns the cards (keys, never `env_file` values).

### D5 — CLI / socket (docs/API.md, additive)
- `rade account add … [--user] [--env-file PATH] [--env KEY=VALUE]…`
- `rade account set <name> [--agent A] [--user] [--env-file PATH|--no-env-file] [--env K=V]… [--unset-env K]…`
  (method `account.set`).
- `account.list` / `rade account list --json` gain `scope`, `env_file`, `env_keys`, `detected`
  is on Panes (`pane.list`), never values.

### Out of scope
Per-account `args` (option 3); editing Workspace accounts in Settings; `$VAR` interpolation in
`.env`; switching a running Pane's account. Detailed limits (ADR-0035 d.9) for an account whose
env sets `CLAUDE_CODE_OAUTH_TOKEN`: show `not available with a token login` (whether a
setup-token can call the usage endpoint is UNVERIFIED; no test calls it).

## 3. Acceptance criteria (draft, final wording goes into the PRD)

- R125-a (unit): dotenv parse — quotes, `export`, comments, a bad line → error with line number.
- R125-b (unit): env order Workspace `[env]` < `env_file` < `env` < `config_dir` var + `RADE_ACCOUNT`;
  reserved keys → validation error naming the file.
- R125-c (e2e): `rade spawn --agent claude --account clw --cmd env` → Pane has the token from the
  `.env`, `CLAUDE_CONFIG_DIR=$HOME/.claude-work`, `RADE_ACCOUNT=clw`; `state.json`, `rade account
  list --json` and `debug.state` do not contain the token string.
- R125-d (e2e): `.env` removed → spawn exits 2 naming the file, no Pane; SIGKILL + restart with the
  file removed → plain shell with the notice; file back → next spawn works.
- R125-e (unit+e2e): user accounts in `accounts.toml` usable in Workspaces `W` and `personal`;
  `accounts = "own"` in `viact` → `--account clw` exits 7; same name in both files → error naming
  both; default precedence.
- R125-f (e2e): `rade setup --dry-run` in a temp HOME installs hooks into a user account's dir.
- R125-g (unit+e2e): a shell Pane runs `CLAUDE_CONFIG_DIR=~/.claude-work fake-agent` → Pane gets
  `account: clw`, `detected: true`; a Subagent it spawns inherits `clw`; Agent exits → cleared;
  unset var → `~/.claude` account; two accounts on one dir → the default.
- R125-h (e2e): an old-shape `config.toml` + `accounts.toml` → a 1.0.x `Config` still parses
  (no new key in `config.toml`).
- R126-a (unit): `accounts.toml` edits — add/delete account, set/unset variable, env_file,
  default move — keep comments byte for byte; bad input writes nothing.
- R126-b (e2e): Settings → Accounts: cards for user + Workspace accounts (Workspace one read-only),
  add variable, load a `.env` (shows `1 variable`, value hidden), bad `.env` line shows the error,
  new Pane gets the vars, running Pane unchanged; screenshots dark + light, looked at.

## 4. Tasks (each: tests first, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, commit)

1. **ADR-0047 + PRD R125/R126 + amendments + CONTEXT.md (Agent Account: "per Workspace or for
   every Workspace") + API.md** — `docs(M47): accounts with env, user level, detection`.
2. **`rade-core/dotenv.rs`** (new) — parser + tests R125-a. `feat(R125.1): …`
3. **`account.rs`** — `AccountDecl.env`, `env_file`; `Account.scope`; reserved keys; `apply_env`
   becomes fallible (reads the file) with the D1 order; R125-b. Callers in `rade-app` (spawn,
   restore → `refuse_restores_in` path for an unreadable file).
4. **User accounts** — `paths.rs` (`accounts.toml`, Devel shares it like `config.toml`),
   `workspace.rs` (`accounts = "own"`), `account_lists` merges D2, `resolve_spawn` FORBIDDEN for
   "own", default precedence, `setup.rs` dirs; `account.rs` toml writers take a target file;
   R125-e/f/h.
5. **CLI/socket** — `rade-cli/account.rs` flags, `account.set`, `account.list` fields;
   `rade-app/accounts.rs` handlers; reload user accounts on `accounts.toml` change (file monitor,
   like `config.toml`).
6. **Detection** — `rade-core` `State::detect_account(pane, invocation, home)` + `Pane.account_detected`
   (serde default false), call from `handlers.rs:486`, clear on Agent end; R125-g.
7. **Settings → Accounts** — `rade-core/settings_accounts.rs` (card model + `toml_edit` ops, R126-a),
   `rade-app/settings_view.rs` (category + cards), CSS in `style.css` with tokens only,
   `debug.ui app_settings_accounts`.
8. **E2E `tests/e2e/m47.sh`** (temp HOME, fake agent, private D-Bus) + `docs/internals/checks/M47.md`
   + `scripts/trace.sh`; CLAUDE.md command list line.
9. **User docs** — `docs/features/accounts-and-usage.md` (`.env`, user accounts, `accounts = "own"`
   for company Workspaces, migrating `cl`/`clw`), `skills/rade-config/SKILL.md`; demo scene for
   the Settings → Accounts screenshot.

## 5. Owner's own migration (after release, by hand — not part of the code)
```toml
# ~/.config/rade/accounts.toml
[[account]]
agent = "claude"
name = "cl"
config_dir = "~/.claude"
default = true

[[account]]
agent = "claude"
name = "clw"
config_dir = "~/.claude-work"
env_file = "~/.claude-5-token.env"
```
and in `config.toml`: `[agents.claude] command = "claude --dangerously-skip-permissions"` (drop the
`CLAUDE_CONFIG_DIR=` prefix, otherwise a `cl` spawn is refused by R108.7); `viact` gets
`accounts = "own"` + its own `[[account]]` if its Agents must stay on the work login.
