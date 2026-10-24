# Agent Accounts per Workspace, locked in rade-core; usage read from statusline JSON and Agent logs

Status: accepted 2026-10-23 (M33, PRD R108–R110). Amends ADR-0015 (a Workspace's single `CLAUDE_CONFIG_DIR` becomes a list of accounts per Agent) and ADR-0016 (`rade setup` installs hooks into every declared `config_dir`). Builds on ADR-0021 (test-only switches). Owner request in docs/internals/design/redesign-2026-10-23.md §9–§10; Detailed limits option A chosen in docs/internals/design/redesign-build-plan.md §7.

The owner runs Claude Code and Codex under several logins (`~/.claude`, `~/.claude-work`, `~/.codex`) and needs to know, before giving an Agent work, how much quota each login has left, to pick the right one when opening an Agent, and never to run one Workspace's Agent under another Workspace's login. Each CLI already separates logins by its config directory (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`). Limits differ per plan and change between CLI versions (Claude Code 2.1.309: `five_hour`, `seven_day`, `spend_limit` in statusline JSON; codex-cli 0.163.0: `primary` / `secondary` windows per `limit_id`, e.g. `codex_bengalfox` = Spark, in rollout files).

## Decision

1. **An Agent Account = agent + `config_dir` + name**, declared per Workspace in `<root>/.rade/workspace.toml`:
   ```toml
   [[account]]
   agent = "claude"
   name = "acme-work"
   config_dir = "~/.claude-work"
   default = true
   ```
   At most one `default = true` per agent per Workspace (two is a validation error naming the file); with none, the first listed is the default. The same `config_dir` may be declared in two Workspaces to share a login on purpose. Names are unique per (Workspace, agent); a claude and a codex account may share a name, and commands taking a name accept `--agent` and answer `AMBIGUOUS` without it when two agents share it.
2. **The lock is decided in `rade-core`**, not only in the UI: a spawn (socket, CLI or UI) whose account is not declared in the Workspace of the Pane being created fails with the new error kind `FORBIDDEN`, exit code **7**. The UI shows other Workspaces' accounts dimmed with a lock.
3. **Pane env**: the spawned Pane gets the account's `CLAUDE_CONFIG_DIR` or `CODEX_HOME` and `RADE_ACCOUNT=<name>`, for that Pane only; the Pane records its account in `state.json` (restored with it). `rade spawn --account <name>`; without it a child spawned from a Pane inherits the parent Pane's account, else the Workspace default. `RADE_PROJECT` stays absent (R31-c). The account's variable wins over the Workspace's `[env]`, and a Preset command or a recorded invocation whose `VAR=value` prefix sets it to another directory is refused at spawn with `FORBIDDEN` naming the Preset, never silently overridden either way: otherwise `CLAUDE_CONFIG_DIR=~/.claude claude` as a Preset would run an `acme` Agent under the personal login and slip past decision 2. A restored Pane keeps its recorded account; a refused recorded invocation restores it as a plain shell (PRD R108.7).
4. **`rade setup` installs hooks into every declared `config_dir`** of every Workspace (backups and atomic apply as today), not only `~/.claude-*`.
5. **Usage sources, read-only:**
   - Claude limits: `rade setup` sets the statusline to `rade status --statusline [--then "<previous statusline command>"]`, which records `rate_limits` from the JSON on stdin to the Usage store, then passes stdin unchanged to the previous command and prints its output (nothing printed if there was none). Values update only while a session runs, so the UI always shows their age.
   - Codex limits and all token counts: RADE tails `$CODEX_HOME/sessions/**/rollout-*.jsonl` (`token_count`: `rate_limits`, `info.total_token_usage`) and Claude transcripts `$CLAUDE_CONFIG_DIR/projects/*/*.jsonl` (`message.usage`) **by offset** — never re-reading a file from the start — every 30 s per account and when one of its Agents ends a turn. The newest sample per `limit_id` wins; tokens are attributed to a Project by the session's `cwd`.
6. **Limits are a dynamic list** `{account, group, name, window, used %, resets_at, source}`, not a fixed "5h + weekly". An unknown `limit_id` shows under its raw name. The **Tightest limit** is the one with the highest used %.
7. **Usage store**: `$XDG_STATE_HOME/rade/usage/<agent>-<sha8 of the canonical config_dir>.jsonl` (`rade-dev` instead of `rade` for the Devel flavor, ADR-0010): limit samples over time and token totals per day × Project × model, pruned past 90 days. Keyed by the directory, because the quota belongs to the login in it: two Workspaces declaring one dir share one history, and equal account names in two Workspaces never collide.
8. **Tolerant parsers**: unknown fields are ignored, broken lines skipped; when nothing can be read the UI shows `no usage data (format changed?)`, never a wrong number. Parser tests use anonymised real fixtures per CLI version.
9. **Detailed limits (option A)**: Claude's per-model weekly limits exist only behind the undocumented `api/oauth/usage` endpoint. Off by default, opt-in per account; at most one request per 5 minutes per account; the login token is read from the account's `config_dir` at call time and never stored, logged or put in state; an unexpected response disables the option for that account and says so in Activity. Tests use a fake server through `RADE_CLAUDE_USAGE_URL`, honoured only in test mode (ADR-0021); no test ever calls the real endpoint.
10. **Out of scope**: switching an Agent's account mid-turn, automatic load balancing across accounts, billing or cost in money (except Claude's `spend_limit`, shown as the CLI reports it). A forecast toast only changes the default for Agents opened later.

## Considered Options

- **Accounts in the user `config.toml`**: global; nothing ties an account to a Workspace's identity. Rejected (ADR-0015: identity is per Workspace).
- **Lock only in the UI**: an Agent can call `pane.spawn` with any account. Rejected for the core check and its own exit code.
- **`USAGE` for a cross-Workspace account**: hides "not allowed" among bad arguments. Rejected for `FORBIDDEN` / 7.
- **Store keyed by account name**: two Workspaces' `acme` would collide; one login declared twice would split its history. Rejected.
- **Poll the endpoint for everyone (option B's opposite)** or **statusline only (option B)**: the first spends a login token silently; the second shows `—` for Opus / Sonnet weekly. Rejected for the opt-in (A).
- **Replace the user's statusline**: loses what they had. Rejected for the `--then` wrapper.

## Consequences

- A new exit code 7 in API.md; scripts treating any non-zero as "bad arguments" still fail safe.
- The statusline wrapper adds one process per statusline refresh; it must stay fast and never fail the user's statusline (errors go to the store's log, the previous command's output is still printed).
- CLI log formats are internal; a CLI upgrade can blank the numbers until a parser update, by design rather than showing wrong ones.
- OpenCode and Antigravity accounts can be declared but show `no usage data` until their formats are verified.
