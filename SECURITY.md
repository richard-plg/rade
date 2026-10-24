# Security

## Reporting a vulnerability

Please don't open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/richard-plg/rade/security/advisories/new)
and include the version (`rade --version`), what an attacker can do and how to
reproduce it. We aim to answer within a week. Fixes ship in a patch
release; the advisory is published with it.

Only the latest release gets security fixes.

## What RADE trusts

- **The control socket** (`$XDG_RUNTIME_DIR/rade/rade.sock`) accepts only
  connections from your own user (checked with `SO_PEERCRED`). Anything that
  runs as you, including every Agent in a RADE Pane, can drive RADE — that is
  the point of the `rade` CLI. Don't run untrusted code as your user.
- **Agents run unsandboxed**, with your permissions, in the folder RADE opens
  for them. RADE doesn't restrict what an agent CLI does; use each agent's own
  permission settings.
- **A repository's `.rade.toml`** is ignored until you trust its exact content
  (`rade config trust`). A change to the file needs trusting again, so a
  branch can't slip in a new setup command.
- **The Browser Pane** runs RADE's automation in an isolated script world: a
  page can't read or fake what an Agent sees through `rade browser snapshot`.
  `rade browser eval` runs in the page on purpose.
- **`rade setup`** edits agent config files (hooks) only after showing the
  change (`--dry-run`) and keeps a backup of each file it changes.
- **Usage limits** are read from files the agent CLIs already write. The
  optional Detailed limits call reads the agent's own token and passes it to
  `curl` on stdin; RADE never stores it.
