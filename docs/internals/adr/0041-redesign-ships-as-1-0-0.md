# The redesign ships as 1.0.0 and docs/API.md is stable

Status: accepted 2026-10-24 (owner decision). Amends ADR-0012 §2 (the pre-1.0 policy ends here); the target version of ADR-0033 … ADR-0040 and PRD R97+ is 1.0.0, not 0.8.0.

## Context

ADR-0012 §2 kept RADE on 0.x until "docs/API.md is declared stable". The redesign (docs/internals/design/redesign-2026-10-23.md, M33–M44) was planned as 0.8.0. It removes the Task, Batch, Initiative and Report engines (ADR-0033), settles the Project model on Source links with one base each (ADR-0034) and adds Agent Accounts and usage (ADR-0035). After it, the CLI, socket and file formats are the ones the owner means to keep. 0.8.0 was never tagged.

## Decision

1. The redesign is released as **1.0.0**. Every "0.8.0" in the living docs, code and tests now reads "1.0.0": the removed-command and removed-method answers (`'<cmd>' was removed in 1.0.0`, `data.removed = "1.0.0"`), `ignored (removed in 1.0.0)` in `rade config explain`, the PRD's Removed / Superseded / Amended markers. Plans, checks and the redesign design notes keep "0.8.0" as written history.
2. **docs/API.md is stable from 1.0.0.** The stable contract is: the `rade` CLI (commands, flags, exit codes, `--json` output), the socket methods and their params, results and error kinds, the keys of `config.toml`, `workspace.toml`, `project.toml` and `.rade.toml`, and `state.json` version 3 with its migrations. Not part of it: `debug.*` methods and `rade debug …` (test mode, ADR-0006), the UI's layout and texts, and the environment variables RADE's own tests use.
3. **SemVer from 1.0.0**: MAJOR for an incompatible change to the stable contract, MINOR for features and compatible additions (a new command, method, optional param, result field or config key), PATCH for fixes only. Adding a field to a JSON result is compatible; agents must ignore unknown fields.

## Consequences

- The next breaking change to the CLI, socket or file formats is 2.0.0. Work still open that would change them (the overview-mode proposal, redesign follow-ups) should land before the 1.0.0 tag or be shaped as compatible additions.
- `scripts/release.sh 1.0.0` cuts the release; nothing else changes in the release flow (ADR-0012 §3–§6).
- Downgrading to 0.7.1 still needs `state.json.v2.bak` (ADR-0033).
