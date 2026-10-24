# Changelog

All notable changes to RADE are recorded here. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the
rules in [docs/RELEASING.md](docs/RELEASING.md) (0.x before 1.0.0). From 1.0.0
each release's section is generated from its Conventional Commits (ADR-0043);
the 0.x sections were written by hand. Requirement IDs (R…) refer to [docs/internals/PRD.md](docs/internals/PRD.md); milestones (M…) to
[docs/internals/MILESTONES.md](docs/internals/MILESTONES.md).

## [Unreleased]

RADE 1.0.0 is a redesign around Projects: a new look and window chrome, the
Navigator and Project home, one checkout per Project link, Agent Accounts with
their usage and limits, and layouts for narrow windows and large text. The
Task, Batch, Initiative and Report engines are removed. From 1.0.0 the `rade`
CLI, the socket methods and RADE's config and state files are stable: a change
that breaks them will come only in a new major version. Changes marked
**Breaking** need you to do something or change how a command, socket method,
file or setting behaves.

### Added

- Reviews now go in rounds. **Request changes** sends your draft comments to the Agent and remembers the files as they were, so **Since round n** shows only what the Agent changed afterwards. Sent comments stay on the diff with their state (`code changed` or `untouched` once the Agent's turn ends); you can **Resolve** them or **Follow up**. **Approve ▾** asks the Agent to commit (or commit and open a PR), or opens Commit, Create PR or Choose Winner. A finished Agent's Toast and Inbox row have a **Review** button, its Worktree row shows `R2 · 3 open` or `approved`, Agent Overview opens a card's Review with `Alt+R`, and the palette has *Review: Since Last Round*. Agents can read the review with `rade review status` and say they are done with `rade review request --summary` (R119).

- The Review now says whose work it shows: the Agent, the task it was started with and the branch's commits. Files are listed as a folder tree with lock and generated files folded last. Each file can be marked **Viewed** (it folds, and becomes unread again when it changes), and comments are written and shown right under their lines. New keys: `n`/`p` hunk, `N`/`P` file, `]`/`[` comment, `v` viewed, `c` comment, `w` hide whitespace changes, `i` Impact. Impact is a rail beside the diff, and `Discard…` asks before dropping draft comments (R118).

- Each Agent shows its own logo instead of a status square (Claude, Codex,
  Antigravity, OpenCode; a robot for other agents), coloured by its status (blue pulsing while it
  works, amber glowing when it needs you, green when done, red on an error).
  It shows in the Navigator, on Tabs, in Pane headers, in the Inbox, in the
  `+` menu and in the system monitor (R115).
- An agent you start by hand in a shell (`codex`, `agy`, `opencode`, `claude`)
  gets its glyph within a few seconds, before it reports anything, and loses it
  when it exits (R115).
- `[agents.<name>] icon = "~/icons/….svg"` shows your own icon for an agent (in
  its own colours, with a small status light) instead of RADE's glyph (R115).
- A system monitor in the Statusline: CPU, memory, GPU (NVIDIA, AMD) and
  network, each figure behind its own icon, with only a high figure turning
  amber or red. A click shows load,
  swap, each network interface and the busiest Panes (a click focuses one).
  It samples on its own thread and stops while the window is minimized;
  `system_monitor = false` turns it off (R116).
- Right-click menus show the keyboard shortcut of each item (your own
  keybindings when you changed them), and the Terminal menu gets Zoom and
  Close Pane (R19.8).
- Several Viewers in one Tab: Open to the Side (`Ctrl+Shift+click` a path,
  middle-click or right-click in Files, `Ctrl+Enter` in the palette, `rade open
  --side`); a plain open goes to the Viewer you used last. The Viewer header
  has a Zoom button (Esc restores), and code, Markdown, HTML and images zoom
  with `Ctrl+scroll`, `Ctrl++` / `Ctrl+−` / `Ctrl+0` or the header's `− 100% +`
  (R113).
- The Viewer shows PDF files (pages, zoom, search, copy) and HTML pages
  rendered, with a Source button to see the markup; a local HTML page runs its
  scripts unless `[viewer] html_scripts = false` (R114).
- Remove from Scratch… on a Scratch row of a Source that a Project links: it
  closes the terminals left in that Source's main folder (after one
  confirmation naming them) and the row goes away; the Source and its Projects
  stay (R104.5).
- Agent Overview (`Ctrl+Shift+Space`, the titlebar's grid button, or `Agent
  Overview` in the palette): every Agent in every Workspace on one screen,
  grouped by what needs you first — needs input, failed, interrupted, then
  finished but not yet seen, running, and quiet. Each card shows where the
  Agent runs and its last screen lines; type to filter, use the arrows and
  `Enter` to jump straight to it (switching Workspace if needed), or hold
  `Ctrl+Shift` and tap `Space` to flip through the cards and let go to jump.
  `Alt+G` groups the cards by Workspace instead. Clicking the switcher's `!`
  or a Statusline count opens it filtered. With GNOME's Reduce Animation it
  appears without motion.
- Agent Overview previews: rest the pointer on a card, or let the keyboard
  selection rest, and a larger view of that Agent's screen (its last 24 lines,
  updated every second) opens beside the card without moving anything. Quiet
  rows get one too, so you can read a finished Agent's last answer without
  jumping. Click it to jump; `Alt+P` turns them off for the keyboard (hover
  still works) and the choice is remembered (R117.9).
- Settings (`Ctrl+,` or `Settings` in the palette), laid out like VS Code: search,
  categories, a description per setting, a marker and a reset on changed ones. The
  Terminal font (family, style, size, line height, ligatures) with a live preview,
  the code font of the Viewer, Review and Markdown code (follows the Terminal's
  unless you pick one), the color scheme (system / dark / light), notifications, Agent limits and the
  Statusline. Changes apply at once and are saved to `config.toml` with your
  comments kept (R112).
- `rade setup` sets each Claude Code config dir's statusline to `rade status
  --statusline` so RADE can record the session and weekly limits Claude Code
  reports (1.0.0). Your own statusline keeps working: its command is kept byte
  for byte after `--then` and still prints what it printed; running setup again
  changes nothing, and a statusline that is not a command is left alone with a
  note (R109.1).
- `rade status --statusline [--then '<command>']` (1.0.0): the statusline
  command `rade setup` installs. It records the limits on its input, runs your
  previous statusline command with the same input and prints only that
  command's output; it always exits 0 and never waits more than 2 s for RADE,
  so your statusline keeps working when RADE is not running (R109.1).
- Usage collection (1.0.0): RADE reads the limits and token counts of each
  Agent Account from the CLIs' own session logs (Codex rollouts, Claude Code
  transcripts) every 30 s and whenever one of its Agents finishes a turn, and
  keeps 90 days of samples per login under `~/.local/state/rade/usage/`. Tokens
  are counted per day, Project and model without cache reads. A limit passing
  80 %, blocking or resetting adds a line to Activity, e.g. `account acme: Spark
  5-hour over 80%` (R109.2–R109.4, R110.5).
- Detailed limits (1.0.0, off by default): set `detailed_limits = true` on a
  Claude account in `workspace.toml` to also see its per-model weekly limits
  (Opus, Sonnet). RADE asks Claude's usage endpoint at most once every 5
  minutes, reads the login token only at that moment and never stores or logs
  it; if the answer looks unexpected it turns the option off in
  `workspace.toml` and says so in Activity (R109.5).
- `rade account usage [<name>] [--json]` (1.0.0) now shows real numbers: one
  line per account of your Workspace with its tightest limit and next reset
  (`acme  codex  81% Spark 5-hour  resets in 2h 30m`), or for one account every
  limit grouped as Session / Weekly / Extra usage (Codex by limit) and its
  tokens per day, Project and model (R110.2).
- The Usage page (1.0.0): open it from the Statusline usage cell, the titlebar
  gauge button or `> usage` in the palette. It lists your Agent Accounts sorted
  by their tightest limit (an account near its limit is always on top; 80 % and
  above in amber with a warning mark), with plan, tokens and when the numbers
  last changed, filtered by Workspace (the shown one, or All workspaces) and by
  Today / 7 days / 30 days. Next resets shows when quota comes back and where a
  limit is heading; the selected account's card has every limit by group, a
  7-day tokens chart (cache reads excluded) and tokens by Project and model.
  Add account adds one to the Workspace's `workspace.toml` (R110.2).
- The Statusline shows the focused Agent's account usage (1.0.0): `claude 5h
  62% · wk 41%` with a small meter, its account in the tooltip; hidden for a
  Pane without an account or without numbers yet (R110.3).
- Limit forecast Toast (1.0.0): when an account will hit a limit before it
  resets at the current pace, a Toast says so (`acme-work will hit its 5-hour
  limit in ~40m at this pace`, with the figures and your least used other
  account), once per limit per window, and Activity records it. `Next agents
  use acme-team` makes Agents you open later default to that account for this
  session; running Agents keep theirs and `workspace.toml` is not changed
  (R110.4).
- Activity (1.0.0): the Inspector's Activity tab lists what happened since RADE
  started — an Agent waiting (with its question) or done (with its last
  message), a pull request opened, a commit — newest first. It is kept in memory
  only (R107.2).
- Agent Accounts (1.0.0): declare several Claude Code / Codex logins per
  Workspace as `[[account]]` tables in `<root>/.rade/workspace.toml` (`agent`,
  `name`, `config_dir`, `default`). `rade setup` now installs its hooks into
  every declared `config_dir`, even one without a `settings.json` yet (R108.1,
  R108.5).
- `rade spawn --account <name>` runs an Agent under one of its Workspace's
  accounts: the Pane gets that login's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (over
  the Workspace's `[env]`) and `RADE_ACCOUNT`, and keeps it across restarts.
  Without `--account` a Subagent uses its parent's account, else the
  Workspace default; so do Fan-out Agents and the Judge. An account of another Workspace, or a Preset command or
  recorded invocation that points the account at another config dir, is
  refused with exit code 7 (`FORBIDDEN`); a restored Pane refused this way
  comes back as a plain shell that says why. `--agent` and `--cmd` now work
  together: the Preset's account and settings, your command (R108.2, R108.3,
  R108.7).
- `rade account list|add|default|usage` (methods `account.*`, `usage.*`):
  list a Workspace's accounts (`claude  acme-work  ~/.claude-work  ✓`), add
  one or move the default with every other byte of `workspace.toml` kept, and
  name an account for usage (`--agent` when a Claude and a Codex account share
  a name) (R108.4).
- The titlebar `+` (and the tab strip's `+`) opens a menu (1.0.0): `New in
  Shop / shop-web / checkout-v2`, then Shell, your Workspace's Claude Code and
  Codex accounts (the default ticked; an Agent with no account shows `workspace
  default`), the accounts of your other Workspaces greyed out with a lock —
  RADE never runs an Agent under another Workspace's login — then Fan-out… and
  Browser. `↑` `↓` `Enter` `Esc` work in it; picking an account opens that Agent
  in a new Tab with `RADE_ACCOUNT` set. Each account shows its usage: `5h 62% ·
  wk 41%`, or its tightest limit (`Spark 5h 81%`, amber with a warning mark from
  80 %), or `no data yet` (R108.6, R110.2).
- Narrow windows and large text (1.0.0). Below 1440 px the right panel floats
  over the Terminals (hidden until `Ctrl+Shift+E` or the panel button; while it
  shows, your keystrokes and clicks still go to the Terminals), the
  command field shrinks before the breadcrumb shortens, the Statusline drops
  key hints, then ports, then your git email, and the Usage page puts the
  account detail under the list. With GNOME's text size at 150 % or more,
  RADE's own text follows it (it did not before), the left column becomes a
  48 px strip of Project icons (a click opens the full list over the window),
  list rows, palette rows and `+` menu rows grow with the text,
  the Tab shows the focused Pane with the others as title bars, and tables,
  the palette, the New Project dialog, the `+` menu and the limit Toast move
  details to a second line instead of cutting text off or scrolling sideways
  (R111.1, R111.2).

- Each Project link now has its own checkout: the repo's main checkout when it
  is on the link's base, else a Project checkout under
  `.worktrees/.projects/<project>/<repo>` made the first time you open a
  Terminal on the link (never imported as a Worktree, never switched). The
  choice is kept in `state.json`. `rade project show` lists every link with its
  checkout, HEAD, ahead/behind, uncommitted files, the other Projects using it
  and a `drift` mark when the checkout left its base (R98, R100).
- `rade project plan` previews the checkouts of a new Project without writing
  anything; `rade project link add|remove|set-base` edits one link of
  `project.toml`; `rade project link to-base` puts a drifted checkout back on
  its base, and refuses when it has uncommitted changes (R97, R98, R100).

### Changed

- In a Tab with 2 or more Panes every Pane now has a header with a ✕ to close it (a plain shell's header says `Terminal`). Closing a Tab or a Terminal from the window — ✕, middle-click, `Ctrl+Shift+W`, menus, palette — now always asks first, naming the Tab or the Pane and its title and any Agent still working; `rade close` and `rade tab close` still close without asking (R19.9).

- A new Tab (New Tab key or the `+` menu) opens right after the Tab you are on,
  like a browser, instead of at the end; `rade spawn --tab new` still adds it
  last (R19.7).
- A Pane whose Agent waits for you no longer gets the amber ring around it; it
  shows `needs input` in its header and the LED in the Navigator, so the only
  lit border is the focused Pane's (ADR-0036).
- Project home's title now reads `<Project> / Home` (was `<Project> / Overview`).
- The Viewer, Review and rendered Markdown now match the app's look: a Pane-style
  header, RADE's backgrounds, Inter for Markdown text, and code colored like VS
  Code's GitHub Dark / Light Default theme in both styles (ADR-0037).
- File and folder icons in the file tree and the palette are Catppuccin's outline
  icons drawn in RADE's colors, replacing the Material icons (R8.5).
- New window chrome (1.0.0): a 40 px titlebar with the Workspace button, the
  breadcrumb `Project / Source / Worktree ← base ↑n`, a command field that opens
  the palette, and a Review button showing how many files Review would open;
  flat tabs named after you, the Agent or the Worktree (no more `Tab 3` or
  terminal titles) with a status light; Pane headers `name · agent · model ·
  status` with a glow on the focused Pane; a Statusline with the Workspace,
  Project, Worktree ahead / behind, Agent counts, ports and your git email.
  `Ctrl+K` opens the palette and `Ctrl+B` toggles the right panel only when the
  focus is not in a Terminal or Browser, so programs keep those keys. Terminals
  without a theme of your own use RADE's background in both styles (R103, R102.6).
- Terminals now use RADE's own look even when your Ghostty config sets a theme: the
  design's colors, the Pane's padding and Maple Mono Medium at 14 px, which ships
  with RADE (nothing is installed on your system). Keybinds and scrollback still
  come from Ghostty. Choose `Look: ghostty` in Settings, or set
  `[terminal] theme = "ghostty"`, to keep your Ghostty theme and font
  (R102.6, ADR-0037).
- The right panel is now the Inspector (1.0.0): a fixed 340 px column with
  text tabs `Changes n`, `Files` and `Search` plus an Activity tab (empty for
  now); `Ctrl+Shift+E` toggles it from anywhere and `Ctrl+B` from outside a
  Terminal. Empty places show one quiet line with key hints instead of a big
  icon (a clean checkout, an empty folder, Search with no query, a Worktree
  with no tabs). Commit is the screen's only highlighted button and only while
  there is something to commit, and a Toast moves up so it never covers a
  button such as the Review Pane's Send to Agent (R103.4, R103.6).

- The left sidebar is now the Navigator (1.0.0): your Projects, each with its
  Sources (name and base), their Worktrees and Agents, then Scratch for the
  Workspace root and work outside Projects (a Worktree with no Project offers
  `→ Shop?`). A Source that left its base shows its branch with a warning mark,
  one that another Project also uses shows a link mark with `Also in …` as the
  tooltip, and a collapsed row carries the worst status light and how many
  Agents wait. The Inbox moved into a popover opened from `<agent> needs input`
  at the foot of the Navigator or the bell; `Ctrl+Shift+G` opens the newest
  unread from anywhere and `Ctrl+G` from outside a Terminal. `Alt+1…9` open
  your Projects in order (R104).
- A Project row opens Project home (1.0.0): Agents, Worktrees and uncommitted
  files at a glance, a table of the Project's Sources with base, checkout, HEAD,
  ahead / behind and a state chip (`clean`, `off base · 3 changes`, `also in ·
  Billing @ release/2.4`), Open terminal per Source (creating the Project's
  own checkout when it needs one), the drift actions Back to base, Keep and
  Move to a Worktree…, the Project's Worktrees and its recent activity. Ahead /
  behind count against the base's upstream when the base tracks one (R105,
  R100).
- New Project, Add Source, Change base… and Project settings are one dialog
  (1.0.0): a table of Sources, each with its base branch and what RADE will do
  for its checkout (`main checkout · on develop`, `new project checkout · main
  checkout is on feat/checkout-v2`), shown before anything is written. Create
  writes only the Project's `project.toml`; Change base never moves your
  existing Worktrees' base. Project settings is the Name and that table, with
  Open project.toml and Customize with Claude… in its menu. Open them from the
  Navigator's `+`, Project home, a Source row's menu or the palette (R106).
- The Changes tab shows the whole Project (1.0.0): `PROJECT · SHOP +212 −58`,
  then one card per Source with its files, its own message box, Draft and
  Commit (a click on a file stages or unstages it), the warning card of a
  Source that left its base with its actions, and clean Sources folded to one
  line. Details opens the single-checkout page with hunks, Push, Pull and
  Create PR (R107.1).
- The palette lists your Sources per Project (`shared-ui · Shop · develop`),
  Projects, Worktrees with their Project and base, Agents by name and actions
  such as `Add shared-ui to Project…`, `Change base of shared-ui in Shop…` and
  `Fan-out on shared-ui…`, in sections, without paths or Pane ids; Enter on a
  Source opens a terminal on its checkout, and the selected row says what Enter
  does (`Enter open terminal`). Scope chips: All, Actions, Agents, Sources,
  Files, Symbols (R107.3).
- Search has a scope inside a Project (1.0.0): `link` searches the focused
  Source's checkout, `Project` every Source of the Project, results grouped by
  Source (R107.4).
- When the focused checkout's git email differs from the Workspace's identity,
  the Statusline's email turns amber with a tooltip naming both; in Changes,
  every checkout that would commit under another email shows that warning
  above its own Commit (R107.5).
- Faster with big setups (1.0.0): the Navigator redraws only the rows that
  changed (a status change in a tree of 260 rows takes under 2 ms), the Usage
  page and the `+` menu open in well under 0.1 s with 20 accounts and 90 days
  of history, and the first read of a 1 GB folder of Codex logs takes about
  half a second without slowing the window (R111.3).

- New look (1.0.0, R102): RADE is dark by default (it follows the system when
  it asks for light) with a terminal-native palette, one mint accent and
  hairlines instead of shadows; it ships its own Inter and JetBrains Mono fonts
  and Phosphor icons, so it looks the same whatever fonts are installed.
- **Breaking (1.0.0):** a Project's `project.toml` now gives each repo exactly
  one base (`base = "develop"`) and an optional `checkout = "auto" | "main" |
  "<path>"`; Named Bases (`bases = [...]`, `base = { dev = …, prod = … }`) are
  gone. The first read of an old file rewrites it once, keeping your comments,
  with each repo's branch for the first Named Base, and saves the original as
  `project.toml.v1.bak`; a repo that had no branch for it uses its main branch
  and `rade project show` warns about it. Need prod too? Make a second Project.
  `rade project add --source <repo>[:<branch>]` replaces the
  `--source <repo>:<base>=<branch>` form, and Project settings and New
  Worktree… show the one base per repo (R97).
- **Breaking (1.0.0):** `rade worktree add <repo> <name>` now belongs to the
  only Project that links the repo: it starts from that link's base (e.g.
  `develop`, not `main`), records the Project and base, and shows under the
  Project. A repo linked by several Projects needs `--project`. Review,
  Changes, the `+/−` counts and Create PR compare a Worktree with its recorded
  base, else its link's base, else the main branch; changing a link's base never
  changes the base of its existing Worktrees. A base that exists only on the
  remote (e.g. `release/2.4`) works: the Worktree starts at `origin/release/2.4`
  without a local copy of that branch and without tracking it (R99).
- **Breaking (1.0.0):** `state.json` moves to version 3: the first start of 0.8 keeps a copy of the
  old file as `state.json.v2.bak` (written once, never overwritten), then drops
  the Task, Batch, Initiative, Report and Start here fields. Every Workspace,
  Source, Worktree, Tab and Pane is kept (R101).
- **Breaking (1.0.0):** `[task]`, `[profiles]`, `[batch]`, `[report]` and `[initiative]` in any RADE
  config file (`config.toml`, `workspace.toml`, `project.toml`, `.rade.toml`)
  are now ignored instead of applied: `rade config explain` lists them as
  `ignored (removed in 1.0.0)` and the file still loads.
- The Fan-out Judge's prompt has the diffs and each candidate's Impact; it no
  longer runs a verify command or adds a review rubric.
- The Right Panel's Code page is gone: references, callers and callees,
  implementations, a file's outline and the language server's state (indexing,
  the install hint, Restart) now open in a popover on the Viewer or Review Pane
  that asked, one popover at a time with hover and peek; Esc or × closes it.
- Browser identifies WebKit, offers Chromium and the system browser, and supports
  keyboard annotation selection with accessible control names.

### Removed

- **Breaking (1.0.0):** Tasks, Task Runs, the Tasks page and Board, Task Detail,
  Brainstorm, Batches, Initiatives, the Plan gate, Verified Runs, Reports, Digests,
  Flow metrics, Lessons, the Event Journal and Start here are gone from the app:
  no pages, sidebar rows, menu or palette items, and the Viewer's review bar is
  gone. Their socket methods now answer "removed in RADE 1.0.0". Task, Batch,
  Initiative and Report files on disk are left untouched (R101).
- Project settings no longer have the Tasks, Run defaults and Verify sections.
- **Breaking (1.0.0):** the `rade task`, `rade batch`, `rade initiative`,
  `rade digest`, `rade report`, `rade flow` and `rade lessons` commands are gone;
  running one exits 2 with "'<command>' was removed in 1.0.0". `rade task start
  --profile` (Agent Profiles) went with them.
- **Breaking (1.0.0):** the `rade-task`, `rade-batch`, `rade-initiative`, `rade-design` and
  `rade-report` skills are gone; `rade setup` now installs `rade`,
  `rade-workspace` and `rade-config` and removes the copies of the old five it
  installed (a file of yours in one of those folders is kept).
- **Breaking (1.0.0):** `rade project set` no longer takes `task.verify`, `task.profile`,
  `batch.parallel` or `batch.review`; it answers "unknown key".

### Fixed

- The port list (Navigator tooltip, palette `Open localhost:<port>`, `rade browser ports`) no longer shows the private local port that Codex opens for itself in every session; dev servers an Agent starts still show (R18).

- Navigator: an Agent row shows its session's title (what Claude Code, Codex
  or agy name the session) instead of just `claude` / `codex` (R104.7).
- Navigator: the focused Agent's row is highlighted, not only its Project or
  Worktree row (R104.6).
- Navigator: a click on a Project's Source whose Terminal opens in its Project
  checkout now keeps that Source selected instead of jumping to its row under
  Scratch (R104.5).
- A split, a new Tab or an Agent opened next to a Terminal in a Project
  checkout now starts in that checkout, not in the Source's own folder, so it
  stays under its Project instead of showing up in Scratch (R104.5).
- Remove Worktree… on a clean Worktree with submodules (such as a repository
  with a vendored submodule) now removes it instead of failing with "contains
  submodules; pass --force"; the dialog says its submodules go with it (R12.3).
- Clicking into another Pane of a Tab now moves the focus border and glow to it
  and lifts its dimming at once; before, the previous Pane stayed lit until
  something else redrew the window (R103).
- The breadcrumb and the Statusline name the Project of the link you are on
  (by the focused Terminal's folder), not the first Project that lists the
  Source (R104.5).
- Palette rows: the name, icon and selection bar are centered in the row again
  instead of sitting 7 px high.
- Resuming Agents with configured model/effort/permission flags no longer turns
  duplicate option values into an unintended prompt; quoted resume arguments stay intact.
- Session load failures preserve a recovery copy before saving a fresh session;
  failed saves remain visible with Retry and State location actions.
- Known unsafe YouTube navigation is blocked before WebKit loads it, with an
  external-browser alternative. Other website crashes remain possible.
- Quick-start commands and paste shortcuts match the current application.

## [0.7.1] - 2026-10-06

### Changed

- **Callers show every call, as a tree.** `Alt+Shift+H` (and `rade code
  callers`) now list each call site, not one row per calling function: a
  caller that calls it five times shows five lines. On the Code page each
  caller shows how many calls it makes and opens to them and to *its*
  callers, as deep as you click (a recursion is marked, not followed); a Go
  to button jumps to the caller. Uses that are not calls (the function
  passed or stored, e.g. `cond ? momentWithTZ : moment`) are listed under
  **Other references (not calls)** (TypeScript/JavaScript and Python;
  rust-analyzer and gopls already count such uses as calls). `rade code callers --depth N` prints the
  callers of callers indented; `--json` adds `calls` (with `from_ranges` and
  `children`) and `others`.

### Fixed

- **Holding Ctrl now shows what you can click.** With Ctrl held, the name
  under the pointer in a Viewer or on a Review diff line is underlined in
  the link color and the pointer becomes a hand, as in VS Code; letting go
  of Ctrl or moving away puts it back.
- **Ctrl+click no longer selects code.** Ctrl+click on a name goes to its
  definition without moving the caret, changing the selection or starting
  a drag-select; in the Review it no longer changes the selected rows.
- **No second language server for library files.** After jumping into the
  Rust standard library (or a Go module cache, typeshed) or into a
  package's `node_modules`, navigating from there uses the repository's
  server instead of starting another one (a second rust-analyzer used
  gigabytes of memory).

## [0.7.0] - 2026-10-05

### Added

- **Code navigation in the Viewer (R83–R85).** In a code file, **F12** or
  **Ctrl+click** on a name goes to its definition, **Shift+F12** lists its
  references, **Alt+Shift+H** shows who calls it (switch to what it calls),
  **Ctrl+F12** its implementations, and **Alt+Left** / **Alt+Right** go back
  and forward. Results show on the Right Panel's new **Code** page, grouped
  by file; a click opens the line. RADE starts rust-analyzer or
  typescript-language-server (from your PATH or the repo's `node_modules`)
  the first time you navigate in a repo, stops it after 15 idle minutes and
  runs at most 4 at once (`[code] idle_minutes`, `max_servers`). While a
  server is still indexing, or when it isn't installed (the page shows the
  install command), you get whole-word text matches, labelled as such. Turn
  it off for a repo with `[sources.<name>] code = false`.
- **Code navigation in the Review (R87).** Click a name on a diff line and
  press **F12** (or **Ctrl+click** it), **Shift+F12** or **Alt+Shift+H**, as
  in the Viewer: added and unchanged lines ask the language server; a name on
  a removed line is looked up in the current code, else found as text at the
  base commit (labelled). Definitions open in the Tab's Viewer, and
  **Alt+Left** walks back through your jumps.
- **Impact (R88).** The Review's new **Impact** button lists every function
  the diff changes, adds or removes, with its callers outside the diff (and
  matching lines in tests the server missed, and removed functions still
  used), e.g. `2 changed · 3 call sites in 3 files (1 in tests) · 1 removed,
  still used`; a click opens the caller. `rade code impact` prints the same
  for Agents, and the Batch Reviewer, Fan-out Judge and Initiative acceptance
  Agent now get it in their prompts automatically (within 20 s, text matches
  if the server is slow).
- **`rade code` for Agents (R86):** `rade code def|refs|callers|callees|impl
  <file:line:col>` and `rade code symbols <file>` answer from the same warm
  servers (text or `--json`), so every Agent shares one index per worktree;
  the `rade` skill tells Agents to check callers before changing a
  function. `rade code status` and `rade code restart <file>` manage them.

- **Hover, peek and outline (R90, R91).** In the Viewer, **Ctrl+I** (or
  holding **Ctrl** with the pointer on a name) shows a name's signature and
  docs, **Alt+F12** peeks at its definition with an Open button, and
  **Alt+Shift+O** (or the new Outline button) lists the file's functions and
  types on the Code page. Ctrl+I and Alt+F12 also work on a Review's diff
  lines. `rade code hover` gives Agents the same.
- **Find a symbol anywhere (R92).** In the command palette, type `$` and a
  name to jump to any function or type in the Worktree;
  `rade code symbols --query <name>` for Agents.
- **Python and Go (R93).** Code navigation now starts pyright (or
  basedpyright) for Python and gopls for Go when they are installed, and
  shows the install command when they are not.

### Fixed

- A language server that never finishes starting no longer makes every
  later code navigation wait 30 s: RADE answers with text matches at once
  and tries it again after 5 minutes.
- Programs started from RADE (Terminals, language servers, verify commands)
  no longer inherit the app's open graphics and other file descriptors.
- Several code navigation requests waiting on one language server that
  stopped answering now each give up after their own 10 s, not one after
  another.
- New Terminals no longer fail to open ("failed to create ghostty surface")
  once many Terminals are open across RADE windows (R89). Terminals used
  io_uring, whose rings count against a small per-user locked-memory limit
  shared by all your programs; RADE's Terminals now use epoll. Restart RADE
  after upgrading to free the old rings.

## [0.6.0] - 2026-10-05

### Changed

- **The sidebar no longer moves rows when you switch branches (R30).** A
  worktree shows under a Project only when it belongs to it (made from the
  Project or a Task, Move to Project ▸, or a click on its **→ BEACON?**
  suggestion). Each repo's own folder sits in a pinned **Main checkouts**
  section with its current branch, and worktrees that belong to no Project
  are listed under **No Project**. Projects show their worktrees as flat
  rows with a small repo chip instead of repeating every repo as a header;
  a Project with nothing in it folds to one line. On the first start, every
  worktree that 0.5 showed under exactly one Project is recorded there, so
  nothing you see under a Project disappears.
- **Tasks are grouped by what you need to do (R38):** Needs you, Running,
  To do (inbox and ready together) and Done. A To do Task shows **✓ ready**
  or **Needs info**; a row that waits for you has its one button on it
  (Review, Approve plan…, Confirm verify…, Retry merge). The Board's
  columns are To do, Running, Review and Done.
- **RADE never refuses you on readiness (R55).** A Task that is missing its
  goal, its "done when" or how to check it shows a yellow **Needs info**
  block saying what is missing, with **Fix with Claude** and **Start
  anyway**; setting it ready yourself works too (the Task's Log notes what
  was missing). `rade task start`, `rade task status … ready` and
  `rade batch run` still refuse a Task that isn't ready, so Agents keep
  writing complete Tasks.
- The Task Detail starts with Repos, Base, Priority and Agent; the other
  fields are under **More fields**.
- **New features have fewer approvals (R74).** The Agent writes the
  concept and the spec as one document, so a backend feature needs your OK
  twice (spec, plan) before it is built, a UI one three times (spec,
  design, plan). Features you already started keep their steps;
  `[initiative] gates = "full"` keeps the old seven for new ones too.

### Added

- **Run several Tasks without writing a file (R80):** tick To do Tasks and
  press **Run *n* tasks…**: choose how many run at once, who reviews (you,
  Claude then you, or nobody) and whether approved Tasks are combined on one
  branch. RADE writes the batch file for you and the Tasks show as a
  **Running together** group in the Tasks list, with Pause, Cancel and
  Push… there and Approve… / Request changes… on each Task.
- **Fix with Claude** for several Tasks at once: one Agent asks you what is
  missing and fills it in after you confirm (`rade task brainstorm T-22
  T-25`).
- **Start here** on each Project's Tasks page: repos, branches, verify
  command, first task, run tasks together, ticked as you go (dismissable).
- **Project settings** (the gear on the Tasks page, or the Project's menu):
  branches per repo and base, the default base, the Verify command, the
  Agent and the run defaults, each saved as one line of `project.toml`. A
  Verify command you type there runs without the extra confirmation.
- **Features in the Tasks list (R79):** an Initiative is a card among the
  Project's Tasks with its four steps (Brief · Spec · Plan · Build) and the
  one thing to do next — Review spec v2, Continue to M2…, Start Agent —
  and its milestone Tasks listed under it. **Plan a feature…** on the Tasks
  page starts one from a title, your notes and files.
- Documents to review open in the Markdown viewer with **Approve… /
  Request changes… / Line comments** right above them.
- **Run presets:** Careful, Parallel and Overnight at the top of the Run
  dialog fill in its choices; add your own with `[batch.presets.<name>]`.
- `rade project set <project> <key> <value>` edits one `project.toml` key and
  keeps your comments; `rade batch add … --run` adds and runs a batch in one
  go; `--review agent-then-human` lets Claude review first and you decide.

## [0.5.1] - 2026-10-05

### Fixed

- RADE no longer crashes when you Ctrl+click a link (or does anything else
  that logs) after the terminal you started it from was closed.

## [0.5.0] - 2026-10-04

### Added

- **Initiatives (R74–R79)** — take a product or a large feature from your
  notes to merged milestones: `rade initiative add "Coffee shop" --kind ui
  --input notes.pdf` makes a folder with your brief and `inputs/`, and
  `rade initiative start I-1` (or Start Agent on its page) starts an Agent
  that drafts a concept (users, non-goals, Mermaid user flows, 2–3
  approaches), a design with a clickable prototype for UI products, a spec
  with acceptance criteria and a plan of milestones. You approve or ask for
  changes at every step on the new **Initiative page** — comment on single
  lines, compare Versions, open the prototype in the Browser and annotate
  it. Then each milestone is built for you: the Agent writes Ready Tasks, a
  Batch runs them (verify, review) on the branch `initiative/I-<n>`, an
  independent Agent checks the result into `checks/M<k>.md`, and you
  Continue or ask for changes. Several Initiatives run at once under one
  Agent cap by priority (`[initiative] agents`), and RADE warns early when
  their branches conflict. Find them in the sidebar under the Project, in
  the Project menu's **Initiatives** and in the Inbox. Nothing is merged
  into your base branch or pushed.
- `rade task add --label` (repeatable).
- Skills `rade-initiative` and `rade-design` (anti-slop design rules),
  installed by `rade setup`.
- **Browser Annotations (R71)** — the Browser Pane's new pencil button
  (`Ctrl+Shift+A`) turns on annotate mode: click an element or drag a
  region, type what should change, and the annotations collect in a panel
  under the page (edit, delete, kept across reloads and restarts). **Send
  to Agent** pastes them to the Agent beside the Browser as one prompt
  with the page URL, a CSS selector, a ref, the element's text and box, a
  cropped screenshot and the page's console errors. Works on dev servers
  and local HTML prototypes. `rade browser annotations [--send]` and
  `rade browser annotate on|off` from the CLI.
- **`rade browser --engine chromium` (R72)** — the same `rade browser`
  commands on a real Chromium (Playwright's Chrome for Testing, `chromium`
  or `google-chrome`; or `[browser] chromium` in `config.toml`), headless
  with a throwaway profile, or `--headed` with an annotation toolbar. For
  pages WebKit can't run (video sites, WebRTC, File System Access) and
  trusted clicks and keys; `rade browser --engine chromium close` quits it.
- The skills recommend Playwright tests (Chromium) as the verify command
  of web Projects.

- **Ready Tasks (R58–R62)** — new Tasks get a fuller template: Goal,
  Context, Constraints (with "Not in scope"), Done when, Verify, Open
  questions and Plan, plus optional `kind` (feature, bug, chore, spike),
  `risk` and `depends` keys (`rade task add --kind/--risk/--depends`, the
  new `rade task set <task> <field> <value>`, and Kind / Risk / Depends on
  in the Task Detail). Existing Task files keep working unchanged.
- **Definition of Ready** — moving a Task to ready (CLI, Board, Task
  Detail) or starting it now checks that it has a Goal, at least one Done
  when item, a way to verify it and no open questions, and lists what is
  missing (in the Task Detail, `rade task show`, and a dialog with "Set
  ready anyway" / "Start anyway"; `--force` on the CLI). Vague or too many
  Done-when items are flagged as warnings.
- **Dependencies** — `rade task start` waits for a Task's `depends` to be
  done, and a Batch orders its Tasks by their `depends` automatically.
- The Task Agent is now told to read Context and Constraints first, stay in
  scope, run the Verify checks and log the evidence before stopping; a
  `bug` Task reproduces the bug as a failing test first. Add your own line
  with `[task] prompt_extra`. Brainstorm prompts and the `rade-task` skill
  write the new template.

- **Verified Runs (R63–R66)** — set `[task] verify = "cargo test"` (in
  `config.toml`, `workspace.toml`, `project.toml` or a trusted `.rade.toml`)
  and RADE runs it in the Task's Worktrees when the Task Agent says it is
  done. The Task goes to review only if it passes; a failure goes back to
  the Agent with the end of the output, and after `verify_rounds` (default
  2) failures the Task reaches review marked "verify failed". The output is
  kept in a log file linked from the Task Detail, which shows Verifying… /
  ✓ Verify passed / ✗ Verify failed. `verify_timeout` (default 600 s) kills
  a hung check and everything it started. `rade task verify T-12` runs it on
  demand. A command written in a Task file is never run.
- Batch items show `verifying` while the check runs, and an item that keeps
  failing it fails. The Batch Reviewer and the Fan-out Judge now get each
  Done-when item to mark met or unmet with evidence, the verify result, and
  your `[task] review_guide` (e.g. `REVIEW.md`); the Batch page shows the
  Reviewer's "Done when n/m" checklist. The Fan-out Judge sees each
  candidate's verify result.
- Task Agents of web Tasks are asked to attach Browser Pane screenshots to
  the Task Log.

- **Plan gate (R67)** — a `risk: high` Task (or every Task of a Batch with
  `plan: required`, `rade batch add --plan required`) first asks its Agent to
  write a `## Plan` — files, order, risks, how it will be verified — and
  stop. The Task waits in "plan review" until you click **Approve plan…** or
  **Request changes…** in the Task Detail or on the Batch page; only then
  does the Agent implement it. Agents can't approve their own plan: there is
  no command for it, and the Task can't be moved to review or done while
  the plan waits.
- **Flow metrics (R68)** — `rade flow [--project p] [--from … --to …]`
  shows how your Task Runs went: first-pass rate, review rounds, verify
  failure rate, and the median time from start to review and from review to
  done. The Digest gets a Flow group, Report drafts see the range's Flow,
  and the Reports page shows the last 7 days.
- **Lessons (R69)** — `rade lessons` (and **Lessons…** on the Reports page)
  collects the review changes, plans sent back and failed verifies of the
  last 30 days and has your draft command propose rules for `CLAUDE.md`, a
  skill or your review guide, as a diff to copy. RADE never edits those
  files itself.
- The Tasks list and the sidebar mark a Task whose last verify failed
  (✗ verify), and verify logs older than 30 days are cleaned up.

### Changed

- **A new or changed verify command waits for your OK (R70)** — before RADE
  runs a `[task] verify` command it hasn't seen, or one whose text changed
  (an Agent can edit `project.toml` too), the Task waits with "Verify waits
  for your OK"; **Review command…** in the Task Detail (or on the Batch
  row) shows the command and the file it comes from, and runs it once you
  confirm. `rade task verify` refuses an unconfirmed command, and
  `rade config trust` alone is not enough.
- `rade batch status` prints which items each item waits for (`after T-1`),
  and the Task Detail's readiness list wraps long lines under their text.
- `rade task add --status ready`, `rade task status … ready`, `rade task
  start` and `rade batch run` refuse Tasks that don't meet the Definition of
  Ready (exit 2 with the list; `--force` on the first three).

### Fixed

- A Task Agent can no longer skip the verify by writing `status: review`
  into its own Task file or running `rade task status … review` from its
  Run: RADE puts `running` back (with a Log line), verifies, then moves it
  to review (R73).

## [0.4.0] - 2026-10-03

### Added

- **Task Detail (R55)** — on the Tasks page a click now selects a Task and
  shows its details beside the List or Board: edit the title, status,
  priority, due date, base, Sources, profile, estimate, labels and links in
  place, read the description rendered or edit it as Markdown, and see its
  Run, Log and the Start / Brainstorm / Open file / Drop actions. Each edit
  changes only that line of the Task file (comments and the rest stay as
  they were); a description changed on disk while you type is never
  overwritten — you choose to keep yours or theirs. Edits made by an Agent or
  in an editor show up within a couple of seconds. Double-click or Enter
  opens the Task's Run Tab.
- **New task dialog (R56)** — "+ New task" (and the palette's New Task…)
  asks for a description, a Project and optionally a priority and base; the
  first line becomes the title and the rest the Goal.
- **Brainstorm with Claude (R57)** — from the same dialog (or the palette's
  Brainstorm Tasks…), an Agent opens in the Workspace's General, asks you
  questions, proposes a split into Tasks and creates them only after you
  confirm. They appear on the Tasks page as they are created.
- Task files may carry `labels: [..]` and `estimate:`; `rade task list/show
  --json` include them.

### Changed

- **Tasks page: a click no longer jumps away (R55.1)** — clicking a Task
  used to open its Run Tab or its file in a Viewer; now it selects it. Use
  double-click / Enter for the Run Tab and "Open file" for the file.

## [0.3.1] - 2026-10-03

### Added

- **Move to Project ▸ (R30.4)** — right-click a Worktree in the sidebar to
  put it under one of the Projects that list its Source, or back to
  "No Project (by branch)". The choice is saved; a Project that no longer
  lists the Source counts as none.

### Changed

- **Sidebar: Projects over the same Sources no longer look identical
  (R30.1)** — a Worktree you didn't make for a Project (the main checkout
  included) now shows under a Project only while it is on one of that
  Project's base branches (e.g. `feat/dev/beacon`, or `master`); everything
  else is listed under "Sources". Worktrees made for a Project stay under it.
  Worktrees from `rade worktree add` without `--project`, Fan-out members
  and imported ones now follow this rule too; use Move to Project ▸ to pin
  one under a Project.

### Security

- **Hidden `debug.*` socket methods are off unless RADE is started for
  tests (R2.9, ADR-0021)** — they press the app's own buttons (Approve…,
  Push…, Commit), so any Agent in a RADE Terminal could use them to approve
  a Batch item or confirm a push. They now answer "debug methods are
  disabled" unless `rade-app` was started with `RADE_DEBUG_SOCKET=1` (the
  test harnesses), and Terminals never inherit that variable.
- **Only one RADE per state file** — a second `rade-app` pointed at another
  socket used to load the same state next to the running one (and overwrite
  its saves); it now exits with "another RADE is using …".

## [0.3.0] - 2026-10-02

### Added

- **Batches (R48–R53)** — run several ready Tasks in one go:
  `rade batch add "OTP sprint" --task T-12 --task T-18 --parallel 2`, then
  `rade batch run B-1`. RADE starts each Task in its own Tab as slots free
  up (`after:` for order), reviews it (`review: none | agent-judge | human`;
  agent-judge asks a read-only Reviewer and sends its notes back), and with
  `merge: integration` squash-commits each approved Task onto a local
  branch `batch/B-1` — never your base branch. `push: ask` adds a Push…
  button with the usual confirmation. `stop_on` pauses the Batch after
  failures; Pause and Cancel never stop running Agents; a restart picks the
  Batch up where it was. Approving is a button on the Batch page only — no
  command or Agent can approve for you. The sidebar shows
  `B-1 OTP sprint  2/4 · 1 needs you` under the Project; `[batch]` in any config
  layer sets the policy defaults.
- **`rade task wait` (R52)** — `rade task wait T-12 T-18 --any --until review`
  returns when the first (or with no `--any`, every) Task gets there.
- **`rade-batch` skill (R54)** — installed by `rade setup`: how an Agent
  writes and runs a Batch as the Orchestrator.
- **Reports (R43, R44)** — `rade report new daily | weekly | custom`
  drafts your work report (English: Done, In progress, Blockers, Next) from
  your Digests and open Tasks with the same no-tools command as Generate
  (`[report] draft_command`, else `[scm] draft_command`). Every change is a
  new numbered, read-only version: `rade report revise <id> "shorter"`,
  `rade report save <id> --file -` for your own edits, `rade report diff`.
  A Project has its own Reports (`beacon/2026-W40`) and may have its own
  template: `rade report style` prints the one in effect.
- **Digest (R42)** — `rade digest [--date D]` shows what you did on a day,
  per Project, without asking a model: your commits in every Source (by your
  git email in that Source), PRs (from `gh` and from Changes), Task Log
  lines and status changes, and Agent runs with their time. Past days are
  saved under `.rade/digests/` once; edit the file and your Reports use it.
- **Event Journal (R41)** — RADE logs Agent starts and ends, Task status
  changes and PRs you create to `<root>/.rade/events/<day>.jsonl` (kept out
  of git).
- **Slack and DingTalk export (R45)** — `rade report export <id> --format
  slack | dingtalk` converts a Report for pasting; `--json` prints the
  webhook body. RADE never posts by itself.
- **Reports page (R46)** — open it with the palette's "Reports: open", a
  Project's menu or the Tasks page's **Reports** button: the Reports with
  their versions, a version picker, Diff, **Revise…**, **Copy ▾**
  (Markdown, Slack, DingTalk) and **New ▾** (Daily, Weekly, Custom).
- **`rade-report` skill (R47)** — installed by `rade setup`: Agents draft,
  revise and export your Reports, and post them to Slack or DingTalk only
  when you ask, after showing you the text.

- **Tasks (R33, R34)** — RADE is now a task manager: a Task is a Markdown
  file with YAML frontmatter in `<root>/.rade/projects/<p>/tasks/`
  (`T-12-otp-login.md`: title, status, Sources, Named Base, Agent Profile,
  priority, due, links; Goal, Done when and a Log in the body). `rade task
  add | list | show | status | done | drop`. RADE edits only single lines
  and appends to the Log, so whatever you or an Agent wrote stays as it was;
  ids never collide, even with many `task add`s at once. Refer to a Task as
  `T-12`, `12` or `beacon/T-12`.
- **Task Runs (R35, R37)** — `rade task start T-12` makes a `task/T-12-…`
  Worktree in each of the Task's Sources from its Named Base (all or
  nothing), opens a Tab `T-12 <title>` and starts the Task Agent with the
  Task file's path; it gets `RADE_TASK`, `RADE_TASK_FILE` and
  `RADE_TASK_WORKTREES`. When the Agent's turn ends the Task moves to
  `review`, and back to `running` when you prompt it again; `done` and
  `dropped` stay yours. `rade task add --from-tab` turns a Free Tab into a
  Task's Run; `rade task brainstorm` opens an Agent that writes the spec with
  you; `rade task done --remove-worktrees` cleans up.
- **Tasks page (R38)** — a Project's Tasks in the main area: a List by
  section (Needs you, Running, Review, Ready, Inbox, Done this week,
  Dropped), or a Board where dragging a card to a column sets its status.
  Search and Status / Source / Base filters, **New task**, and a right-click
  menu (Start, Brainstorm, Status, Open file, Drop). Open it from the
  sidebar's **All tasks**; a Tab's menu has **Make Task from this Tab…**.
- **Task-centric sidebar (R39)** — under each Project, its Live Tasks (with
  an open Run or in review), most urgent first: needs input, review,
  failed, running; up to 5, `⋯ N more` for the rest, but every Task that
  needs input shows. A click shows the Task's Tab (or the Tasks page). Their
  `task/…` Worktrees are folded into the Task row.
- **`rade-task` skill (R40)** — installed by `rade setup`: how Agents add,
  brainstorm and run Tasks through `rade task` without touching the
  frontmatter.
- **Agent Profiles (R36)** — `[profiles.deep] agent = "claude", model =
  "opus", args = [...]` in any config file (user, Workspace, Project,
  trusted Source), `[task] profile` as the default; a Task picks one with
  `profile:` or `rade task start --profile`. Presets gained `model_flag`
  (`--model` / `-m`).

- **Workspaces (R23.1, R23.2)** — `rade workspace add <root>` registers a
  folder such as `~/workspaces/acme` as a Workspace (Sources under it move
  in, it gets its own General); `rade workspace list | switch | remove`.
  `<root>/.rade/workspace.toml` sets its `name`, `color`, `[env]` and
  `[clone] default_owner`. The switcher's "Add Workspace…" does the same.
- **`rade source add owner/repo` (R24.2)** — clones with the GitHub CLI into
  the Workspace's root, using that Workspace's `gh` account (never the one
  of the terminal you run it from), then registers it (`repo` alone uses
  `default_owner`; a path, also a quoted `~/…`, is registered as is).
  Running it again is harmless.
  `rade list` shows your Workspace's Panes (`--all` for every one).
- **Workspace switcher (R23.4–R23.6)** — the sidebar shows one Workspace at a
  time; its heading is a switcher (color dot and name) listing every
  Workspace with its Unread count and worst Agent Status. Switch with it,
  `Ctrl+Alt+1…9` or the palette's "Switch Workspace"; you come back to the
  same Source, Tab and focus. Hidden Workspaces keep running: an amber `!` on
  the switcher means an Agent there needs input, and its Toast and desktop
  notification name the Workspace (clicking them switches to it).
- **Workspace environment (R23.3)** — a Workspace's
  `<root>/.rade/workspace.toml` `[env]` (e.g. `GH_CONFIG_DIR`,
  `CLAUDE_CONFIG_DIR`) goes into every new Terminal of its Sources, with
  `RADE_WORKSPACE`; Push, Pull, Create PR and Generate run with it too.
- **Worktrees under the Workspace (R23.8)** — new Worktrees of a Source in a
  Workspace go to `<root>/.worktrees/<source>/<name>`, so folder-based
  setups (a shell hook picking `GH_CONFIG_DIR` by directory, a Workspace
  `CLAUDE.md`) apply to them; existing Worktrees stay where they are.
- **Workspace setup (R25)** — `rade workspace setup <root> --git-name …
  --email … --github …` gives a Workspace its own git identity (an
  `includeIf` block in `~/.gitconfig`, settings in `<root>/.gitconfig`), SSH
  key, `gh` login (in its own `GH_CONFIG_DIR`), keys on GitHub (auth and
  commit signing) and Agent hooks. It only fills what is missing, shows a
  diff and backs up every file it edits, and `--dry-run` shows the plan
  without changing anything. `rade workspace import <root>` writes
  `workspace.toml` from a folder you already set up by hand;
  `rade workspace clone api web …` clones several repositories with the
  Workspace's account. Also in the palette: "Create Workspace…". It never
  adds the key to a `gh` account other than the Workspace's, and when `gh`,
  `ssh` or your shell rc file doesn't answer it says so and stops waiting.
- **`rade workspace doctor` (R26)** — checks each Workspace end to end (git
  identity, signing, SSH key, `gh` account and token storage, keys on GitHub,
  `ssh -T`, Claude login and hooks, shell rc files overriding `[env]`) and
  prints the command that fixes each problem.
- **`rade-workspace` skill (R27)** — installed by `rade setup`, so an Agent
  can set up or repair a Workspace for you through `rade workspace`.

- **Projects (R28, R31)** — a Project is a set of Sources that ship
  together, each with its branch for every Named Base (`dev`, `prod`), kept in
  `<workspace>/.rade/projects/<id>/project.toml`. `rade project add BEACON
  --source api:dev=beacon-dev,prod=beacon-prod --source app`, `rade project list`,
  `rade project show` (checks every branch). `rade worktree add api fix-1
  --project beacon --base prod` starts from `beacon-prod`; its Review, `+/−`
  counts and pull request compare with that branch instead of `main`.

- **Layered config (R29)** — `[agents.*]`, `[scm]`, `[fanout]`, `[viewer]`
  and `[sources.*]` can also be set in a Workspace's `workspace.toml`, a
  Project's `project.toml` and a repository's own `.rade.toml` (which counts
  only after `rade config trust <source>`, and again after every change), so
  one company's Agents can run with other flags than another's. `rade config
  explain [<key>]` shows which file sets each value; keybindings, limits and
  notification settings stay in `config.toml` (elsewhere they are reported as
  ignored).

- **Sidebar by Project (R30)** — when a Workspace has Projects, the sidebar
  shows a foldable header per Project with its Sources and the Worktrees made
  for it (a folded header shows the worst Agent status and Unread count), then
  the other Sources; `Alt+1…9` follow that order. Right-click a Project for
  New Worktree… (pick the Source and Named Base), Open project.toml and
  Customize Project… (an Agent that edits it with you). Edits to
  `project.toml` show within a few seconds.
- **`rade-config` skill (R32)** — installed by `rade setup`: an Agent can
  create Projects and change layered config for you, checking its work with
  `rade project show` and `rade config explain`.

### Changed

- **Projects are now Sources (R24.1)** — a registered repository or folder is
  a *Source* everywhere: the sidebar, the palette, menus, `rade source …`,
  `--source`, `RADE_SOURCE` and `[sources.<id>]` in `config.toml`. The word
  Project now means a set of Sources (see Projects above).

### Removed

- **The old Project names (R24.3, R31.2)** — `rade project …` no longer
  means `rade source …` (it now manages Projects, below); `--project`,
  `RADE_PROJECT` and the `"project"` copies in `--json` results are gone; in
  `config.toml`, `[projects.<id>]` and `project_<n>` keybindings must become
  `[sources.<id>]` and `source_<n>` (RADE names the new spelling when it
  loads the file). `rade project add <path>` tells you to use `rade source
  add <path>`. Your saved state is upgraded on first start, recently used
  palette items included (the original is kept as `state.json.v1.bak`).

### Fixed

- **Batch merges wait for a conflict (R51.1, R49.1)** — with
  `merge: integration`, a Task approved while another one's merge conflict
  is still open in `batch/B-n` waits (`approved`) and is merged after
  Retry merge, instead of failing on the other Task's files (or, retried
  first, committing that fix under its own name). A merging Task keeps its
  `parallel` slot, so a conflict never leaves more Tasks running than
  `parallel`.
- **A Batch Task that finished just before RADE quit goes on (R49.3)** —
  restored without a status, it waited forever; it now gets the
  "RADE restarted…" prompt like an interrupted one.
- **`rade batch run` lists bad policy values with every other problem
  (R48.2)**, and `batch status` flags a bad value edited in after run as
  `policy_edited`.
- **Digests list every commit of yours, and only yours (R42.1)** — one
  commit with an old date (a skewed clock) no longer hides the commits
  below it; an author whose email merely ends in yours (`some@x.io` vs
  `me@x.io`) is left out; your email is read with the Workspace's `[env]`
  (e.g. its `GIT_CONFIG_GLOBAL`), like the Terminals that commit; and a
  hung `gh` costs one 10 s timeout per Digest or Report, not one per
  Source and day.
- **Slack export of bold headings and odd links (R45.1)** — a heading with
  bold text is one bold line (Slack cannot nest `*`), and a link whose URL
  holds `|`, `<`, `>` or `&` stays one working link.
- **A failed `rade task start` keeps your work (R35.1)** — when starting a
  Task again failed in its second Source, the first Source's existing
  `task/T-…` branch was deleted with its commits; only branches the start
  itself made are removed now.
- **Task files (R33)** — a file copied to reuse an id shows an error on both
  files instead of two Tasks with one id; a `# comment` after a quoted value
  (`status: "ready"  # …`) survives a status change; titles like `0x1F` or
  `010` stay text; Vietnamese titles get readable slugs (`Sửa lỗi đăng nhập`
  → `T-3-sua-loi-dang-nhap.md`); an Agent writing the Task file while RADE
  updates its status no longer loses lines, and an automatic `review` never
  replaces a `done` written a moment before (R37.1).
- **Session Restore and Pause use the Worktree's Project (R29.2)** — an Agent
  in a Worktree started in a Project comes back with that Project's Agent
  command, also when its Source is in several Projects.
- **A `.rade.toml` can't hang RADE (R29.4)** — a repository's `.rade.toml`
  that is not a regular file (a link to `/dev/zero`, a pipe) or is over 1 MiB
  is reported as an error instead of being read.
- **Project branches (R28.1, R31.1)** — `base = "branch"` in a Project without
  `bases` starts new Worktrees from that branch (it was ignored), and `rade
  project show` says `no branch` for a Named Base a Source has no branch for
  (it showed the main branch).

## [0.2.0] - 2026-10-01

### Added

- **Changes in the Right Panel (R21)** — the Worktree's staged, changed and
  untracked files; stage, unstage or discard a whole file or a single hunk
  (discarding asks first); a click shows the file's diff. Write a message and
  press Commit (or Ctrl+Enter); Pull, Push (asks first, sets the upstream on
  the first push) and Create Pull Request (through the GitHub CLI). Open it
  with Ctrl+Shift+M or the palette's Show Changes.
- **Generate (R21.7)** — next to the commit message and in the pull request
  form: an Agent drafts the text from your staged diff (or the branch's diff)
  and the repository's rules (`CLAUDE.md`, `AGENTS.md`, `CONTRIBUTING.md`, PR
  template); you edit it and confirm. RADE never commits or pushes by itself.
  The command is `[scm] draft_command` in `config.toml` (default: Claude Code
  headless, without tools).
- **Search in the Right Panel (R22)** — search the Worktree's files with
  Match Case, Whole Word and Regular Expression, plus files to include /
  exclude; `.gitignore` is respected. Results are grouped by file; a click opens
  the file at that line. Open it with Ctrl+Shift+S or "Search in Worktree".

- **Tab menu (R19.1)** — right-click a Tab for Rename…, Duplicate (a shell in
  the same folder, next to it), Close, Close Other Tabs and Close Tabs to the
  Right; closing several Tabs asks once when Agents in them are still working.
  Fan-out Tabs are left open (discard them from their bar). Also in the command
  palette, and `rade tab list | rename | close [--others | --right]`.
- **Project menu (R19.2)** — right-click a Project for Rename…, Pause / Resume,
  Open in VS Code, Open in Files and Remove Project… (asks; never deletes
  files). `rade project rename | pause | resume`.
- **Pause a Project (R19.3)** — stops every process in it but keeps its Tabs
  and Panes (shown as "Paused — Resume"); it stays paused across restarts and
  isn't started at launch. Resume brings Agents back into their sessions.
- **Rename a Pane (R19.4)** — `rade rename <pane> <name>`, the Terminal menu or
  the palette; the name shows in the Pane's caption.
- **Find in the Terminal (R19.5)** — `Ctrl+Shift+F` opens a search bar over the
  Terminal (scrollback included, `3/17` count, Enter / Shift+Enter, Esc).
- **Terminal right-click menu (R19.6)** — Open the path or link under the
  pointer, Copy, Paste, Select All, Search…, Clear, Split Right / Down, Rename
  Pane…. Programs that use the mouse still get their right-clicks;
  Shift+right-click opens the menu over them.

- **A new look: "Adwaita Pro" (R20, M14)** — Agent Status is a glyph (shape and
  color: a spinner while running, "!" needs input, ✓ done, × failed, ‖
  interrupted) everywhere, with light and dark palettes; an Agent that needs
  you gets an amber ring around its Pane and marks its Tab, and nothing else
  shouts. Pane captions show name, Preset, title, a status chip and Zoom /
  menu buttons; the focused Pane has an accent edge and other splits are
  slightly dimmed.
- **Agents in the sidebar (R20.6)** — each Worktree lists its Agents (status,
  "needs input" or time running); click one to jump to it. Worktree rows show
  `+added −removed`; Projects fold with a chevron; "+" adds a Project.
- **Toast and Status Bar (R20.9)** — "codex-tests needs input · Show" when an
  Agent starts waiting off screen; a bottom bar with Agent counts, listening
  ports (click opens the Browser) and the Commands / Next unread keys. Hide it
  with `status_bar = false` in `config.toml` or palette "Toggle Status Bar".
- **Right Panel switcher (R20.10)** — Files / Changes / Search (Changes and
  Search arrive in the next update); header buttons for New Tab, Review,
  Browser, the Right Panel, and the Command Palette in the sidebar header.

### Changed

- A Pane with a name now shows its caption (name, then title), like Agent
  Panes do.
- Review opens in its own full-width Tab, "Review · N files" (R13.1).
- Tab labels are never paths (a path title shows its last folder); a Tab with
  several Agents shows how many (`Refunds · 4`); Unread counts moved from the
  Tab label to its tooltip (R1.8, R3.4).
- The Notification panel is now the Inbox: compact two-line rows with the
  event's status glyph and `project › worktree`, "Mark all read" (R3.2).
  Unread counts are neutral grey, never the accent color.
- Terminals follow the system light / dark style (Ghostty's Adwaita themes)
  when your Ghostty config sets no theme or colors of its own (R20.8).

### Fixed

- The Tab right-click menu works with the keyboard (Down / Tab no longer close
  it), and the Terminal right-click menu takes the keyboard (R19.1, R19.6).
- Creating a Worktree no longer fails with "already has a worktree" when
  `rade worktree list` ran at the same moment (R12).
- A desktop notification no longer shows twice when an Agent's status and its
  own notification arrive together (R3.5).
- After a split, typing reaches the new Terminal: the keyboard sometimes
  landed on the split divider instead, mostly on a busy machine (R1.3).

## [0.1.0] - 2026-10-01

Initial release: everything built in milestones M0–M12.

### Added

- **Terminal workspace (M0, M1; R1, R4, R9, R17)** — a GTK4/libadwaita app
  embedding Ghostty's GPU terminal (pinned fork, `libghostty` built with Zig).
  Projects → Worktrees → Tabs → Panes in a sidebar with a General project;
  splits, Zoom, focus by direction, the "Terminal starts in the Worktree root"
  cwd rule, IBus input with preedit (Vietnamese tested), the user's Ghostty
  config for the terminal look, keybindings in `~/.config/rade/config.toml`
  (reloaded live), Tab labels and Pane captions that follow the terminal title.
- **`rade` CLI and socket API (M2; R2, R16)** — `spawn` (Agent Presets for
  Claude Code, Codex, Antigravity, OpenCode, with a prompt), `send`, `read`
  with a scrollback budget, `wait`, `list --tree`, `close`, `focus`, `status`;
  a same-UID Unix socket with JSON-RPC ([docs/API.md](docs/API.md)). The Agent
  Grid tiles up to 8 Subagents with Title Bars; per-Project limits.
  `rade setup` installs Agent hooks and the `rade` Claude Code skill.
- **Notifications (M3; R3)** — OSC 9/777, bell and `rade notify`; a
  Notification panel with Unread counts, jump to the newest unread
  (`Ctrl+Shift+G`), click to focus, desktop notifications with a Focus action.
- **Viewer and file tree (M4; R7, R8)** — `Ctrl+click` paths (`file:line`)
  and URLs; a Viewer Pane with GtkSourceView highlighting, line jumps and
  reload; Markdown via WebKitGTK with Mermaid, relative images and links;
  `rade open`; a right-sidebar file tree with git status and Material icons;
  "Open in VS Code".
- **Paste and drops (M5; R5, R6)** — a pasted image becomes a saved PNG path,
  `text/uri-list` pastes and drops, paste-file cleanup, Shift+Enter per Agent.
- **Agent Status and Session Restore (M6; R10, R11)** — status dots in Panes,
  Tabs, Worktrees and Projects (hook-driven, with OSC/bell/idle fallbacks), a
  Codex hint, restore after restart or `kill -9` that resumes Agents with
  their real invocation and never replays prompts.
- **Worktrees and Review (M7; R12, R13)** — `rade worktree add|list|remove`, import and prune,
  a setup command, `spawn --worktree new`, Worktrees outside the repo
  (ADR-0007); a Review Pane of changes since the merge-base with Review
  Comments sent to an Agent.
- **Command palette (M8; R14)** — `Ctrl+Shift+P`: fuzzy search over actions,
  Projects, Worktrees, Tabs, Panes and files, inline forms, recently used,
  off-thread ranking (50k-file repos stay responsive).
- **Fan-out (M9; R15)** — one prompt to several Agents in their own
  Worktrees (`rade fanout`, palette form, Fan-out bar), a Judge verdict, and a
  Winner merged with `git merge --squash` (keep or discard the rest).
- **Packaging (M10)** — `scripts/package-deb.sh` builds an amd64 `.deb`
  (`/usr/bin/rade`, `/usr/lib/rade` with the bundled `libghostty`, terminfo,
  shell integration, desktop file, icons). Dev builds from `target/` are a
  separate flavor with their own socket, state and title (ADR-0010).
- **Known-limit fixes (M11)** — one virtualized all-files Review list;
  Review Comments re-anchored or marked outdated when files change; Agent
  Status `interrupted` with a Fan-out Continue button; `rade close` asks
  before closing Fan-out Agents.
- **Browser Pane (M12; R18)** — a WebKitGTK Pane (`Ctrl+Shift+O`) Agents
  drive with `rade browser open|snapshot|click|fill|press|eval|screenshot|console|wait|back|forward|reload|ports`
  using Playwright-style refs from an isolated script world (ADR-0011);
  listening ports per Pane; cookies kept per flavor.
- **Release engineering** — CI on this machine's self-hosted runner (fmt,
  clippy, unit tests, every headless E2E suite, the `.deb` and its E2E), tagged
  releases with the `.deb` and `SHA256SUMS`, `rade-app --version`, this
  changelog (ADR-0012).

[Unreleased]: https://github.com/richard-plg/rade/commits/main
