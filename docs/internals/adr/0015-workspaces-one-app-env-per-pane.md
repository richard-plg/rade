# Workspaces: one app, environment per Pane, a flat model

Status: accepted 2026-10-02 (M15, PRD R23, R24). Owner answers: v0.7.1:docs/design/workspaces-tasks-reports.md §11.

The owner works under several identities (company `globex`, `personal`), each a folder `~/workspaces/<ws>` with its own GitHub CLI config (`GH_CONFIG_DIR`), Claude account (`CLAUDE_CONFIG_DIR`) and git identity (`includeIf`). Agents of both run at the same time and must never act as the other identity, and an Agent waiting in a hidden identity must not be missed.

## Decision

1. **One app, one window, one Workspace shown.** Switching changes what the sidebar, Inbox, Status Bar, `Alt+n` and palette show; nothing is torn down, so hidden Workspaces keep running and keep raising Notifications (a cross-Workspace `!` on the switcher, the Workspace named in Toasts and desktop notifications).
2. **Identity is environment per Pane, never global.** Each Terminal starts with its Workspace's `[env]` from `<root>/.rade/workspace.toml`; RADE's own `git` / `gh` / Draft commands in a Worktree get the same. RADE never runs `gh auth switch` (global: it would flip every other terminal) and never stores tokens; auth stays with the CLIs (ADR-0013).
3. **A flat model.** All Sources stay in one list, each tagged with its `workspace`; Pane, Tab and Source ids stay globally unique, so addressing, Notifications, Fan-outs and the socket need no Workspace in every call. The active Workspace is the active Source's. Each Workspace has its own General (`general@<ws>`).
4. **The word "Project" moves.** What RADE called a Project is a Source (one repository); "Project" is reserved for M16 (a set of Sources with base branches). `rade project …`, `project.*`, `--project`, the `"project"` key and `RADE_PROJECT` stay as deprecated aliases with a warning until M16, so installed skills and Agent scripts keep working for one release; `state.json` goes to version 2 with a one-time `state.json.v1.bak`.
5. **Cloning runs in the CLI.** `rade source add org/repo` asks the app for the Workspace's root and env, runs `gh repo clone` itself (prompts and progress in the caller's terminal, the GTK thread never blocks), then registers the folder.

## Considered Options

- **A RADE instance per Workspace** (own socket and state): total isolation and almost no code, but no cross-Workspace attention and one window per identity. Rejected; it stays the fallback if the one-app model proves too complex.
- **A nested model** (`State { workspaces: [{ sources: … }] }`): mirrors the UI, but needs a shared id counter and reworks every lookup, Notification and Fan-out reference. Rejected for the flat tag.
- **A hard rename** (drop `project` at once): breaks every installed skill and Agent script on upgrade. Rejected for one release of aliases (a 0.MINOR bump either way).
- **`gh auth switch` on Workspace switch**: global, races between terminals. Rejected.

## Consequences

- A Workspace's `[env]` applies to new Panes only; running Panes keep the env they started with.
- A recorded Agent invocation's `VAR=value` prefix (R11.2) wins over `[env]`, and so does a login shell rc file exporting the same variable; `rade workspace doctor` (M15b) will check that.
- `rade setup` installs hooks into `~/.claude-*` config dirs only; a Workspace's `CLAUDE_CONFIG_DIR` elsewhere is not covered until M15b.
- `pane.list` covers the caller's Workspace by default (`--all` for every one), so one identity's Agents don't see another's by accident; this is a convenience, not a security boundary (same UID, ADR-0006).
