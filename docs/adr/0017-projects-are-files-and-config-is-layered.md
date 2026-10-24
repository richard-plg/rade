# Projects are files under the Workspace; config is layered with an allow-list; repo config needs trust

Status: accepted 2026-10-02 (M16, PRD R28–R32). Builds on ADR-0015 (Workspaces) and ADR-0016 (setup through files and CLIs).

Amended by ADR-0034 (1.0.0): Named Bases superseded by Source links.

The owner works on products made of several repositories that share branches (BEACON: a backend and an app, each with an `beacon-dev` and an `beacon-prod`-like branch, design doc §11 answer 1). M17 adds Tasks and M18 Reports per such product. RADE needs a name for the set, a way to say "start from dev" across all of its Sources, and per-product settings (which Agent command, which Draft command) without one global `config.toml`.

## Decision

1. **A Project is a file: `<root>/.rade/projects/<id>/project.toml`.** A folder per Project, not `<id>.toml`, because M17 puts `tasks/` and M18 `reports/` next to it. The Workspace root holds it, so it travels with `workspace.toml` (`setup --from <git-url>` clones the whole `.rade`). `state.json` keeps only what is machine-local: which Projects are folded, and trusted `.rade.toml` contents.
2. **The model stays flat; no `state.json` v3.** A Worktree gains two optional fields, `project` and `base` (the branch it started from); the state gains `collapsed_projects` and `trusted_configs`. All are optional, so an older RADE reading the file only drops them. A version bump is kept for changes of meaning.
3. **Named Bases resolve only with a Project.** `bases = ["dev", "prod"]` (the first is the default) and each Source maps a name to its branch. `--base dev` means the Named Base only when `--project` is given (or implied), so a branch literally called `dev` stays reachable without one.
4. **Five layers with an allow-list.** built-in → user `config.toml` → `workspace.toml` → `project.toml` → `<source>/.rade.toml`; tables merge, scalars and arrays replace. Only `agents`, `scm`, `fanout`, `viewer` and `sources` (and `.rade.toml`'s `setup`) are layered: keybindings, limits, notifications, the Status Bar and idle time stay app-wide in the user file, because one window can't have two keymaps and a limit per Project would let a Project raise it. A lower layer setting them is reported as `ignored (app-wide)`, never an error. Each layer is validated alone so an error names its file; a broken layer is skipped. M17 extends the allow-list (`profiles`, `task`), M18/M19 (`report`, `batch`).
5. **A repository's `.rade.toml` is ignored until trusted** (`rade config trust <source>`), like direnv: it can set `setup` (a shell command RADE runs) and Agent commands, so cloning a repository must not run its code. Trust stores the file's exact content in `state.json`; any change untrusts it. Comparing content needs no hash dependency and can't collide.
6. **The word moves.** The M15 aliases (`rade project` = `rade source`, `--project`, `project.*`, `"project"` keys, `RADE_PROJECT`, `[projects.*]`, `project_<n>`) are removed in the same release that gives `rade project` its new meaning, with errors that name the new spelling (`'rade project add <path>' is now 'rade source add <path>'`, `[projects.*] is now [sources.*]`). `RADE_PROJECT` is not reused for the new meaning: a script reading it would silently get a different value.
7. **Files are polled, not watched**: every 2 s the shown Workspace's `projects/*/project.toml` `(mtime, size)` list is compared; socket calls always read the files fresh.

## Considered Options

- **Projects in `state.json`** (like Sources): one more registry to sync by hand across machines, and Tasks (files) would point into machine state. Rejected.
- **`workspace.toml` `[projects.<id>]` tables**: one file, but every Task folder would still need a home, and one broken Project would break the Workspace's env. Rejected.
- **Every key layerable**: a Project could rebind keys or raise Subagent limits; ambiguous when two Projects are on screen. Rejected for the allow-list.
- **Trusting `.rade.toml` by path** (once trusted, always): a `git pull` could change what runs. Rejected for content trust.
- **Keeping the aliases one more release**: `rade project list` can't mean both things. Rejected; 0.MINOR breaking change with a CHANGELOG "Removed" entry and hint errors.

## Consequences

- A Source can be in several Projects; it then shows under each, with only that Project's Worktrees (plus unassigned ones — changed in 0.3.1, below).
- Session Restore, the Draft command, the Judge and the Viewer's editor read the layers for their Source; the user `config.toml` stays the only file watched for live reload (the others are read when used).
- Scripts using the M15 aliases break with a hint; `rade setup` reinstalls the skills with the new names.

## Follow-up (0.3.1): unassigned Worktrees go by branch

With two Projects over the same Sources (the owner's `beacon` and `globex-web-main` over `globex-web` + `globex-api`), showing the main checkout and every Worktree without a recorded `project` under every Project made both Projects look identical. Now a Worktree with no recorded `project` (or one naming a Project that doesn't list its Source) shows under a Project only while its current branch is one of that Project's Named Base branches for the Source (no `base` = the Source's main branch; `origin/x` matches `x`); otherwise it shows under the plain "Sources" group, which then lists that Source too. The main checkout follows the same rule. A recorded `project` still wins, and the Worktree row's **Move to Project ▸** sets or clears it. Branches are read from each Worktree's `HEAD` on every sidebar refresh, so a checkout moves the row within one refresh (≤ 5 s); a Source's main branch is read once per app run.
