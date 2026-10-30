# RADE — Product Requirements

Status: accepted 2026-09-30. Terms follow [CONTEXT.md](../../CONTEXT.md). Decisions behind these requirements live in [adr/](adr/). CLI/socket contract: [API.md](../API.md). Build order: [MILESTONES.md](MILESTONES.md).

Every requirement has an ID. Commits, tests, and reviews reference IDs (e.g. `R2.4`). An acceptance criterion (AC) is done only when an automated test or a recorded manual check proves it.

**1.0.0 (M33):** R97–R111 are the 1.0.0 requirements. Their ACs are one line each, `- R97-a [M36] (unit): Given <state>. When <action>. Then <result>.` (the milestone whose tests prove it, the kind of test), and `scripts/trace.sh` maps each to the tests named after it (`r97_a_…`, `check "R97-a …"`) in [checks/trace.md](checks/trace.md). Older sections are marked *Removed (1.0.0)* (ADR-0033), *Superseded (1.0.0)* or carry a **1.0.0 amendment** line; their numbers are never reused.

Target: Ubuntu 26.04, Wayland (GNOME), hybrid Intel + NVIDIA GPU, IBus input method.

---

## R1 — Sources, Worktrees, Tabs, Panes

**1.0.0 amendment (R104):** The left sidebar is the Navigator (Project → Source link → Worktree → Agent, R104) and General is replaced by Scratch (one per Workspace, rooted at its root); read R1.1, R1.2, R1.4, R1.6 and R1-b with those words. The rest stands.

(M15, R24.1: "Project" was renamed "Source"; the word Project is reserved for M16.)

1. Left sidebar lists the shown Workspace's Sources (R23.4), each expandable to its Worktrees; its General is always present, pinned last, and cannot be removed (or paused, R19.3).
2. A Source with only its main Worktree hides the Worktree level (Source row acts as the Worktree).
3. Each Worktree has its own Tabs; each Tab has its own Layout of Panes (split right / split down, resize, close, directional focus).
4. A new Terminal opens with cwd = root of its Worktree (General: `$HOME`), regardless of which Pane was focused.
5. Add Source: pick a folder (dialog or `rade source add <path>`); remove Source never deletes files.
6. Sidebar rows (M14, R20.6): a Source row shows its name, a chevron that collapses it (kept in `state.json`), the worst Agent Status glyph of its Panes while collapsed, and its Unread count; a Worktree row shows a branch glyph, its name (the branch in the tooltip when it differs), `+added −removed` against its base and the worst Agent Status glyph; under them one row per Agent (R20.6).
7. Zoom toggles one Pane to fill its Tab and back (`Ctrl+Shift+Z`); Layout is unchanged after un-Zoom.
8. Tab labels follow the terminal title (OSC 0/2) the program in the Tab sets — Agent Grid Parent first, else the focused Pane, else any titled Pane — with Agent spinner glyphs stripped, ≤ 60 characters shown (full title in the tooltip), at most 4 updates/s per Pane; a title that is a path (`/…`, `~/…`, `user@host:path`) shows only its last folder (M14: never a raw path; the path is in the tooltip); with no title, the Agent Grid Parent's (else the first Agent's) name; otherwise `Tab <n>`. A Tab with two or more Agents adds their count (`Refunds · 4`); a Tab holding only a Review is `Review · <n> files`. A Tab the user renamed (double-click its label, palette "Rename Tab…"; empty = follow the title again) keeps its name, across restarts. Pane captions and Title Bars show the title after the Preset. Decisions: [checks/M12-titles.md](checks/M12-titles.md).

AC:
- R1-a: `rade source add /tmp/x`, open a Terminal in it, `pwd` prints `/tmp/x`.
- R1-b: A Terminal in General prints `$HOME` for `pwd`.
- R1-c: Switching Sources and back restores the exact Tabs, Layout, focus.

## R2 — Control CLI and agent skill

**1.0.0 amendment (R101, R108):** R2.1: Panes also get `RADE_ACCOUNT` (R108) and no longer `RADE_TASK*` / `RADE_INITIATIVE*` (R101); still no `RADE_PROJECT`. R2.2: `rade task`, `batch`, `initiative`, `digest`, `report`, `flow`, `lessons` are removed (R101); `account` is added and `spawn` takes `--account` (R108). R2-d's `debug.batch` example is a removed method now (R101.2).

1. `rade` CLI talks to the running app over a Unix socket; contract in [API.md](../API.md). Every Pane gets `RADE_SOCKET`, `RADE_PANE_ID`, `RADE_WORKSPACE`, `RADE_SOURCE` (and the deprecated `RADE_PROJECT` until M16, R24.3), `RADE_WORKTREE` in its environment, after its Workspace's `[env]` (R23.3).
2. Commands: `spawn`, `send`, `read`, `wait`, `list`, `close`, `focus`, `rename`, `notify`, `status`, `workspace`, `source` (alias `project` until M16), `tab`, `worktree`, `fanout`, `setup`.
3. Placement: a root spawn (caller is not an Agent, or `--tab new`) opens a new Tab. A spawn from an Agent Pane places the Subagent in the Parent's Tab using the Agent Grid. `--split right|down` and `--worktree new` override.
4. Agent Grid: Parent on the left, Subagents tiled on the right (1 full, 2–3 stacked, 4–8 in two columns) — all visible even if small (owner decision 2026-09-30). Only when `limits.max_children` is raised above 8 do extra Subagents render as Title Bars (the Pane caption of R20.4 without its buttons); clicking a Title Bar Zooms it. Manual resize/split in a Tab stops auto-arranging that Tab.
5. Limits: socket accepts only the same UID (peer credentials); max spawn depth 3; max 8 live Subagents per Parent (both configurable). Exceeding returns a non-zero exit and a clear error message.
6. Ship an agent skill (`skills/rade/SKILL.md`) plus an `AGENTS.md` snippet that teach any Agent to spawn, prompt, wait for, and read other Agents.

7. Agent Panes use a scrollback budget large enough for `rade read --scrollback` to return at least the last 10,000 lines, overriding a smaller user `scrollback-limit` (Ghostty counts it in page-memory bytes, not lines — see docs/internals/checks/M0.md row 6).
8. Spawning into a Tab that has never been shown still starts the process immediately.
9. **Hidden test methods are off by default** (0.3.1, ADR-0021). The `debug.*` socket methods (which press the UI's own buttons: Approve…, Push…, Commit) are served only when `rade-app` was started with `RADE_DEBUG_SOCKET=1` (the E2E and live harnesses); otherwise they return `USAGE` with that hint. The app removes the variable from its environment at start, so Panes never inherit it. Only one app runs per `state.json` (a lock in the data dir): a second one, even on another socket, exits 1.

AC:
- R2-a: From a Claude Code Pane, `rade spawn --agent codex --name rev "say hi"` creates a Pane in the same Tab; `rade wait rev --until done` returns; `rade read rev` contains the reply.
- R2-b: Spawning a 9th Subagent fails with exit code ≠ 0 and message naming the limit.
- R2-c: A process under another UID cannot connect to the socket.
- R2-d: In an app started without `RADE_DEBUG_SOCKET=1`, `rade debug state` exits 2 with the hint, and an Agent in a Pane calling `debug.batch approve` (even with the variable in its own env) gets `USAGE` and the item stays `awaiting-human`; a Pane of a test-mode app has no `RADE_DEBUG_SOCKET`; a test-mode app accepts the same call; a second `rade-app` started from a Pane with another `RADE_SOCKET` exits 1 without serving.

## R3 — Notifications

**1.0.0 amendment (R104):** R3.2: the Inbox is a popover opened from the pill at the Navigator's foot or the bell (`Ctrl+Shift+G`; `Ctrl+G` outside a Terminal, R104.3), not a fixed panel at the bottom of the sidebar; its rows are unchanged.

1. Sources: OSC 9 / 777 / 99, bell, Agent hooks (an Agent entering `waiting`, `done` or `error` — on the transition only; owner decision A, 2026-09-30), `rade notify`. Each Notification targets its exact Pane. The same text on the same Pane within 2 s is one entry. A bell from the focused Pane on screen raises none. (OSC 99 is not delivered by the pinned Ghostty fork — see docs/internals/checks/M3.md.) One Agent event is one Notification: a status Notification and an OSC / bell / `rade notify` message from the same Pane within 5 s of each other (either order) merge into one entry, status text first (`rev finished — PONG`; a bell adds no text; further messages are absorbed), and its desktop notification is replaced in place, never sent twice (decision in docs/internals/checks/M6.md).
2. The Inbox (the Notification panel at the bottom of the left sidebar, collapsible, collapsed state kept in `state.json`; M14: compact, at most ~220 px tall) lists Notifications newest first, two lines each: the Agent Status glyph of the event (`waiting` for "… needs input", `done` for "… finished", `error` for "… failed", none for messages), the text (bold while Unread), the relative time and an Unread dot; then `source › worktree` (never cut from the left; the full Source / Worktree / Tab / Pane path in the tooltip). Header: "Inbox · <unread>", a "Mark all read" link and Clear.
3. Clicking a Notification focuses its Source, Worktree, Tab, and Pane (un-collapsing a Title Bar via Zoom if needed) and marks it read. A Notification whose Pane is gone is shown greyed; clicking it only marks it read. `Ctrl+Shift+G` opens the newest unread one.
4. Focusing a Pane (including by showing its Tab or Source) marks its Notifications read. Unread counts show on Pane (caption or corner badge), Worktree and Source rows (a neutral count, never the accent color) and in the Tab's tooltip (M14: no `(n)` in the Tab label; the Tab's attention marker is for `waiting`, R20.3).
5. Desktop notifications (GIO `GNotification` via the freedesktop notification server, with a "Focus" button and default action `app.focus-pane` doing R3.3): `desktop_notify = "smart" | "always" | "off"`, default `smart` = only when the RADE window is not active or the Pane is not on screen (other Source/Worktree/Tab, Title Bar, or hidden by Zoom). Withdrawn once read in RADE. No sound.

AC:
- R3-a: `printf '\e]9;hello\a'` in a hidden Tab → panel entry + desktop notification (smart); clicking either lands in that Pane.

## R4 — Rendering performance

1. Terminals render through Ghostty's OpenGL renderer on the GPU.
2. Hidden Tabs/Panes do not render.
3. Targets (measured on the reference machine, both GPUs):
   - Scrolling and output tracking at the display refresh rate with no visible stutter.
   - `cat` of a 100 MB text file finishes within 1.2× standalone Ghostty 1.3.
   - Idle CPU < 1% with 20 open Terminals.
   - Keypress-to-glyph latency within 1 frame of standalone Ghostty.

AC: a benchmark script in `bench/` records the numbers above; M0 and every milestone after M4 re-run it. The Intel-iGPU target is N/A on machines whose display is attached to the discrete GPU (cross-GPU dmabuf import fails in Mutter, see docs/internals/checks/M0.md row 4).

## R5 — Image paste

1. `Ctrl+Shift+V` when the clipboard holds an image: save it as PNG to `~/.cache/rade/paste/<timestamp>.png` and paste the absolute path as a bracketed paste. Text clipboard: normal paste.
2. Drag-and-drop of image files onto a Terminal pastes their paths.
3. `Ctrl+V` is passed through to the program untouched.
4. Paste files older than 7 days are deleted on startup.

AC: screenshot to clipboard → `Ctrl+Shift+V` in Claude Code and in Codex → both Agents receive the image.

## R6 — Shift+Enter newline

1. Shift+Enter inserts a newline (not submit) in Claude Code, Codex, Antigravity CLI, OpenCode.
2. Works via the kitty keyboard protocol; an Agent Preset may declare a fallback byte sequence.

AC: manual check per Agent Preset, recorded in `docs/internals/checks/R6.md`.

## R7 — Open files from Terminals; Viewer

1. `Ctrl+click` on a detected file path (`path`, `path:line`, `path:line:col`, `~/…`, relative to the Terminal cwd, else its Worktree root; surrounding quotes, brackets and trailing punctuation ignored) or an OSC 8 `file://` link opens it in the Tab's Viewer (a new Viewer splits right of the Terminal if the Tab has none; the previous file goes on its Back list). Only existing files open. `http(s)` links open the system browser. `rade open <path>[:line]` does the same for Agents (API.md).
2. Viewer is read-only, syntax-highlighted, scrolls to `line`, reloads on file change, closes with `Esc`, and has "Open in VS Code" (`code <Worktree root> --goto <file>:<line>`: focuses the VS Code window that already has the Worktree folder open, else opens it and the file; a file outside any Worktree opens alone; the command is `[viewer] editor` in `config.toml`, default `code`; when it is not on `PATH`, "Open in `$EDITOR`" — a Terminal below — or the default app when `$EDITOR` is unset) and "Copy path" actions. Files over 5 MB or binary show a message. Viewer Panes are part of Session Restore.
3. Markdown is rendered (WebKitGTK): headings, tables, task lists, code highlighting, Mermaid, relative images; links to other `.md` files open in the same Viewer with back navigation (Back returns to the previous file at the line it was opened at); web links open the system browser. Light/dark follows the system theme. Works offline (renderers bundled); the page runs no script of its own and loads nothing remote.

AC:
- R7-a: Claude Code prints `src/main.rs:42` → Ctrl+click opens Viewer at line 42.
- R7-b: A README with a Mermaid block and a relative image renders both.

## R8 — File tree

1. The Right Panel (R20.10) shows the active Worktree's file tree on its Files page; toggle `Ctrl+Shift+E`.
2. Follows the active Worktree when focus changes; updates live on file changes.
3. Respects `.gitignore` (toggle to show ignored); shows git status colors (modified / added / untracked).
4. Click opens the file in a Viewer (R7.2).
5. Rows show Catppuccin's VS Code icons (bundled, MIT; 1.0.0 replaces PKief's Material Icon Theme, ADR-0037): by file name, then extension, folders open/closed by name, drawn in one ink per theme from the tokens (files `fg2`, folders `fg3`, an open folder `acc`); the palette's file rows use the same icons.

AC: agent creates a file → it appears in the tree within 1 s, colored as untracked.

Actions on the tree (copy path, reveal, terminal here, refresh, drag): R129 (M49).

## R9 — Input method (Vietnamese)

1. IBus (and Fcitx5) input works in Terminals: preedit text is shown at the cursor, committed text is sent to the program.

AC: typing "Tiếng Việt có dấu" via IBus-Bamboo/Unikey into Claude Code produces exactly that string. **M0 fails if this fails.**

## R10 — Agent Status

1. Each Agent Pane has an Agent Status: `running`, `waiting`, `done`, `error`, `exited`, `interrupted` (set only by Session Restore, R11.2: its turn was cut off; a prompt makes it `running` again).
2. Primary source: Agent hooks calling `rade` (installed by `rade setup`). Fallbacks (M6; `status_source: heuristic` in `rade list --json`), for Agent Panes only (a Preset; `--cmd` programs and shells get no status), conservative — they act only from `running`, never within 10 s of a hook report, and only for a status the Pane's hooks can't report (the Preset's hook kind; a Preset whose hooks never reported counts as having none; a status a hook did report counts as reportable):
   - OSC 9 / 777 notification or bell while `running` → `waiting` (no `waiting` hook: agy, hookless Presets); for a Codex Preset → `done` (its only OSC 9 is the turn-end reply; untrusted hooks).
   - Screen unchanged for `idle_seconds` (default 5) while `running` → `done` if there is no `done` hook, else `waiting` if there is no `waiting` hook (agy: its Stop hook would have said `done`, so it is asking something).
   - OSC 133 command finished (shell integration) in a shell Pane whose Agent was started by hand (`clw`) → `exited`; such a Pane is restored as a plain shell (R11.2), and the Agent's next session-start report makes it live again.
   - Limits: a hookless Agent's status follows only turns RADE sees start (spawn with a prompt, `rade send --enter`); a prompt typed by hand is not seen (screen changes can't tell typing from working). Suspending a hand-started Agent (Ctrl+Z) also ends its shell command, so it reads as `exited`.
   - Not used: terminal titles and OSC 9;4 progress (Agent-specific, only Claude Code emits progress, and it already has every hook). Measured per Agent in docs/internals/research/agent-integration.md §6.
3. Status shows as a glyph — shape and color, never color alone (M14, R20.2) — on the Pane caption, Title Bar, Tab (worst in the Tab), sidebar Source / Worktree / Agent rows (worst of its Panes; worst first: error, waiting, interrupted, running, done, exited), Inbox and Status Bar, in a palette tuned for light and dark (running a fixed blue spinner — never the accent — waiting amber "!", interrupted violet "‖", done green "✓", error red "×", exited grey "–"), with a tooltip naming the status, since when, and whether it was guessed; entering `waiting`, `done` ("… finished") or `error` ("… failed") also raises a Notification.
4. `rade wait <pane> --until done|waiting|exited|interrupted|idle|<regex>` resolves on these statuses (`interrupted` does not end a wait for `done`: the turn has not finished).

AC: Claude Code asks for permission → status `waiting` within 1 s and a Notification appears.

## R11 — Session Restore

**1.4.1 amendment (R130):** R11.2: restored Terminals start in stages — the Pane on screen first, the others one at a time — so the window never stops answering at launch.

**1.0.0 amendment (R108):** R11.2: a restored Pane keeps its recorded Agent Account; for a Pane with one, a recorded invocation whose `VAR=value` prefix sets that account's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` to another directory is not run: the Pane comes back as a plain shell with the account's env and the `FORBIDDEN` message (R108.7). Panes without an account are restored as before.

1. On quit/crash, state (Workspaces, Sources, Worktrees, Tabs, Layouts, per-Pane cwd, Agent Preset, agent session id) is saved to `~/.local/share/rade/state.json` (written atomically, at least every 5 s when changed).
2. On start, everything is recreated; Terminals start a shell in the saved cwd; Agent Panes resume with the saved session id (Claude Code: the RADE-chosen `--session-id`; Codex/Antigravity/OpenCode: the id their hooks reported). If the Agent's session-start hook recorded its real invocation (argv + the Preset's `config_env` variables, e.g. an alias `clw` = `CLAUDE_CONFIG_DIR=~/.claude-work claude --dangerously-skip-permissions`), the resume re-runs it: `VAR=value … <argv0> <its flags> <Preset resume args with the id>`, dropping the prompt and session/resume tokens; otherwise it runs the Preset's resume command. A launch prompt is never replayed: a session without a turn (or without a known id) starts fresh — the recorded invocation without prompt, else the Preset without the prompt. An Agent that was mid-turn (`running` or `waiting`) comes back with status `interrupted`: idle, not finished, never continued automatically (M11 decision, docs/internals/checks/M11.md); its tooltip says to send it a prompt. A `--cmd` Pane (no Preset) or a Pane whose Preset no longer exists is restored as a plain shell in its cwd; arbitrary commands are not re-run; so is a shell Pane whose hand-started Agent had exited (R10.2). Subagents keep their Parent.
3. Processes do not survive restart (see ADR-0005). The PTY source sits behind an interface so a daemon can be added later.

AC: kill -9 RADE with 3 Sources and a Claude Code Pane → restart → identical layout, Claude Code resumed into the same conversation.

## R12 — Worktrees

**1.0.0 amendment (R99):** `rade worktree add` takes `--project` and cuts from the link's base (R99.1); a Worktree in a Project lives under its link in the Navigator.

1. Create Worktree from UI (sidebar Source row menu "New Worktree…" or `Ctrl+Shift+B`: name + base branch) or `rade worktree add <source> <name> [--base <ref>]`: runs `git worktree add` under `<Workspace root>/.worktrees/<source>/<name>` (`.worktrees-dev` for a dev build; the `default` Workspace keeps `$XDG_DATA_HOME/rade/worktrees/<source>/<name>`, default `~/.local/share/rade/…`; ADR-0014, R23.8) on a new branch `<name>` from the base (default: the Source's main branch — `origin/HEAD`, else `main`, else `master` — else `HEAD`). The name must be a valid branch name; if branch `<name>` already exists it is checked out instead (the dialog says so). `rade spawn --worktree new` creates one (`--name`, else `wt-<preset>-<n>`) and spawns in a new Tab there.
2. Optional per-Source `setup` command (`[sources.<id or name>] setup = "…"` in `config.toml`) runs once, in the new Worktree's first Terminal, before its Agent (typed into it when it is a shell) — output visible in that Terminal; a failure is printed and the Agent still starts; never re-run on Session Restore.
3. Remove Worktree: confirms, refuses if uncommitted or untracked changes unless forced, refuses a locked one, closes its Panes, then `git worktree remove` and branch delete (asks; `git branch -d`, and `-D` only after a second confirmation or with `--force`). In the dialog, a clean Worktree with submodules says its submodules are removed with it and its Remove passes git's `--force` (nothing uncommitted is lost; the branch is still deleted with `-d`); `rade worktree remove` still needs `--force` for it (API.md).
4. Worktrees created outside RADE (`git worktree list`) are added on start, on Source add and on `rade worktree list`. A Worktree whose folder vanished is shown greyed with a Prune action (Remove prunes it). Worktrees, their Tabs and Review Comments are part of Session Restore.

AC: create Worktree `feat-x` → sidebar shows it under its Source with branch `feat-x`; new Terminal `pwd` is the worktree path.

## R13 — Diff review

**1.0.0 amendment (R118, R119, ADR-0040):** R13.2: comments are written and shown inline under their lines; sent comments stay with their Round and state (R119.3). R13.3: the Review Pane still runs no git, but it ends in a hand-off — *Approve ▾* opens the Changes page, Create PR or Choose Winner, or asks the Agent (R119.4).

**1.0.0 amendment (R99):** R13.1: the base is the Worktree's recorded base (`merge-base(<recorded base>, HEAD)`, R99.2), which `rade worktree add` now sets to the link's base unless `--base` names another ref; with nothing recorded, the link's base, else the Source's main branch (a Worktree in no Project).

1. Per Worktree: a Review Pane (sidebar menu "Review Changes", `Ctrl+Shift+R`, the header bar's Review button, `rade review open`; M14: in its own full-width Tab "Review · <n> files" in that Worktree, focused; opening it again shows that Tab) with the diff against its base (merge-base of `HEAD` with the Source's main branch) including commits on the branch, staged, unstaged and untracked (non-ignored) files: every changed file in one scrolling unified diff (M11): per file a header (status M/A/D/R/U, path, +/− counts; a click folds the file), its hunks (a click on the `@@` header folds one) and lines (syntax-highlighted, old/new line numbers), with a jump list of the files on the left; virtualized (only rows on screen are built), so a diff of thousands of lines scrolls smoothly and nothing is truncated. Refreshes every 3 s while shown.
2. Add Review Comments on lines (select lines, `c`, type, Enter; or `rade review add path:line[-end] "text"`), anchored to new-file line numbers plus the commented lines, 2 lines of context around them and the hunk header; when the file changes a comment moves to where its lines (with that context, else nearby within 100 lines ignoring indentation) are now, else it is marked **outdated** (kept, and sent with its stored quote and an "outdated" note); listed, editable and deletable below the diff; kept per Worktree in `state.json` until sent or cleared. "Send to Agent" composes one prompt (`path:line` + quoted code + comment for each) and sends it like `rade send --paste --enter` to the Worktree's Agent Pane (choose if several; if none, offer to spawn one from a Preset).
3. No commit / push / PR actions in the Review Pane; those live on the Right Panel's Changes page (R21, ADR-0013).

AC: two comments on two files → Agent Pane receives one prompt containing both `path:line` references.

## R14 — Command palette

**1.0.0 amendment (R107):** R14.1, R14.3: items are Source links, Projects, Worktrees, Panes, files and actions (R107.3) — no General, Task or Batch items, no Pane ids or absolute paths in titles; `Ctrl+K` opens it too while the focus is outside a Terminal (R103.1, R17 amendment).

1. `Ctrl+Shift+P` (RADE's own palette replaces Ghostty's on that key) opens a search overlay; `Esc` or `Ctrl+Shift+P` closes it and the keyboard returns to the Pane that had it. Fuzzy search over Panes (name, Agent Preset, Agent Status), Tabs, Worktrees (of Sources with more than one), Sources (with their `Alt+<n>` key), files in the active Worktree (`.gitignore` applied as in the file tree, at most 100,000, gathered in the background; none in General, which is `$HOME`), and app actions: every keybinding action with its key shown, plus Spawn Agent…, "Spawn <preset>" per Agent Preset, Mark All Notifications Read, Clear Notifications. Accents are ignored when matching (`viet` finds `Việt`).
2. Includes a "Fan-out…" form (R15.1).
3. Prefixes pick a category — `>` actions, `/` Sources and Worktrees, `@` Panes and Tabs, `#` files — shown as chips; `Tab` / `Shift+Tab` cycle them keeping the text. An empty query lists recently used items first (kept in `state.json`), then Panes, Tabs, Worktrees, Sources, actions; matches in a title rank above matches in its detail line.
4. `Enter` focuses a Source / Worktree / Tab / Pane, opens a file in the Viewer, or runs an action. An action that needs input shows an inline form in the palette (Spawn Agent…: Preset, optional prompt, New Tab / Split Right / New Worktree; New Worktree…: name, base): `Tab` / `Shift+Tab` between fields (wrapping, never out of the palette), `Enter` submits (from any field), `Esc` back to the list. Keyboard only: `Up`/`Down` (wrapping), `Page Up`/`Page Down`, `Enter`, `Esc`; other RADE keys don't fire while it is open (nor does it open over a dialog); input methods (IBus) work in its entry and keep their keys while composing. A Pane or Tab closed after the palette opened stays listed; `Enter` on it says it is gone.
5. The result list updates within 16 ms of a keystroke for ~1,000 items and stays responsive for 50,000 files: ranking runs off the GTK thread (a newer keystroke cancels it; `Enter` pressed meanwhile runs the new query's row), at most 60 rows are shown (the status line says `60 of N`).

AC:
- R14-a: `Ctrl+Shift+P`, type `>split r`, `Enter` → the Tab splits right; `Esc` on an open palette → typing reaches the previously focused Pane.
- R14-b: In a Worktree with 50,000 files, `#` queries update the list without blocking the window (measured in docs/internals/checks/M8.md).

## R15 — Fan-out, Judge, Winner

1. Start via the palette's "Fan-out…" form (prompt, a count per Agent Preset, base, name) or `rade fanout --agents claude,codex,codex [--source <p>] [--base <ref>] [--name <n>] "<prompt>"` (mixed Presets, repeats allowed; 2 ≤ Agents ≤ `limits.max_children`, a calling Agent's limits count them all; every Preset's program must be on `PATH`; checked before any git).
2. Creates one Worktree per Agent (`fanout-<n>-<i>`, or `<name>-<i>`) from the same base commit, each with the Source's `setup` (R12.2), and one Fan-out Tab in the Source's main Worktree: the Agents side by side (siblings, not an Agent Grid: ≤ 3 in a row, then two rows), each running in its own Worktree and given the prompt via its Preset. A Fan-out bar above them shows the prompt, progress (`2/3 done`, `· 1 interrupted` after a restart, with a Continue button that sends those Agents "Continue from where you left off" plus the prompt), the Judge's pick, one chip per Agent (status dot, files / +/− against the base, Diff) and Judge / Choose Winner… / Discard… buttons. If a Worktree can't be created, the ones already made are removed.
3. Diff opens that Worktree's Review Pane (R13). "Judge" (once every Agent finished) spawns a Judge Agent (Preset `[fanout] judge`, default `claude`) in the main Worktree, read-only whatever the Preset's permissions (Claude Code: Read/Grep/Glob tools only; Codex: read-only sandbox; others: told only), with the task and all diffs under neutral Worktree names in random order, asking for a ranked recommendation ending in `WINNER: <name>`; RADE reads that line and pre-selects the Winner (the user still decides).
4. "Choose Winner" (default: the Judge's pick): **squash** — its whole result (commits, edits, new files) is staged on the Source's current branch with `git merge --squash`, never committed (ADR-0008; the user or an Agent commits); refused on a detached HEAD, an operation in progress or uncommitted changes; conflicts are left to resolve — or **keep** its Worktree and branch as they are. Then offer to remove the other Worktrees and branches and close the Fan-out ("Discard" removes all of them).
5. A Fan-out is part of Session Restore: its Worktrees, Tab, each Agent's progress (resumed per R11.2; an Agent that was mid-turn is `interrupted` and not counted as done), the Judge's verdict and the Winner.
6. Closing one of its Agents asks, like closing the Fan-out Tab (M11): the close key opens a dialog (Close Agent / Discard Fan-out… / Cancel); `rade close` refuses it (`USAGE`, naming `rade fanout discard` and `--force`) until a Winner is chosen; `--force` closes it (it counts as finished).

AC: `rade fanout --agents claude,codex "add a README line"` → 2 Worktrees, 1 Tab with 2 Panes, both Agents received the prompt.

## R16 — Agent Presets and setup

**1.0.0 amendment (R108, R109):** R16.3: `rade setup` installs hooks into every declared Agent Account `config_dir` (R108.5), sets the Claude statusline wrapper (R109.1) and removes the skills of R101.1. R16.6: a Preset `command` prefix that sets the spawned account's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` to another directory refuses the spawn with `FORBIDDEN` naming the Preset (R108.7); without an account the prefix works as before.

1. Built-in Presets: `claude`, `codex`, `agy` (Google Antigravity CLI, which replaced Gemini CLI on 2026-05-19), `opencode` — each defines launch command, resume command, how to get a session id, hook type, image-path support, Shift+Enter fallback. Facts per Agent: [research/agent-integration.md](research/agent-integration.md).
2. Users override or add Presets in `config.toml`. `rade spawn --cmd "<anything>"` works without a Preset.
3. `rade setup` installs hooks into each Agent's config: shows a diff, backs up the original file, merges without removing user entries, is idempotent. Claude Code: into every config dir — `$CLAUDE_CONFIG_DIR`, `~/.claude`, and each `~/.claude-*` holding a `settings.json` — so every way the user starts Claude reports status.
4. Per-Agent specifics (from research): Claude Code gets a RADE-chosen session id at launch (`--session-id`); Codex, Antigravity and OpenCode report it from a hook event (Antigravity: `conversationId` on every hook). OpenCode needs a plugin file (`~/.config/opencode/plugins/rade.js`) instead of hooks. Antigravity hooks (`~/.gemini/config/hooks.json`) report only `running`/`done`: it has no "needs input" event, so its `waiting` comes from the R10.2 fallbacks. Codex hooks only run after the user trusts them once in `/hooks`: `rade setup` must say so, and RADE shows a one-time hint (a dismissable banner, once per app run: "Codex hooks not trusted — run /hooks in codex and trust the RADE entries") when a Codex Pane's turn has been `running` for 30 s with no hook report since it began (turns RADE saw start: spawn with a prompt, `rade send --enter`).
5. RADE never intercepts Shift+Enter in GTK (required for R6).
6. Each Preset also lists `config_env` (variables that select the Agent's config: Claude `CLAUDE_CONFIG_DIR`, Codex `CODEX_HOME`, OpenCode `OPENCODE_CONFIG`/`OPENCODE_CONFIG_DIR`, agy none), `value_flags` (flags taking a value, from each CLI's `--help`) and `session_flags` (session/prompt flags dropped on resume), all overridable in `config.toml`. A Preset `command` may carry a `VAR=value` prefix (`[agents.claude] command = "CLAUDE_CONFIG_DIR=/home/u/.claude-work claude --dangerously-skip-permissions"`); `--session-id` and the prompt are appended after it.

## R17 — Configuration

**1.0.0 amendment (R103, R104):** R17.3 stands for every key a Terminal or a Browser page can receive. The new plain-`Ctrl` defaults `Ctrl+K` (palette), `Ctrl+B` (Inspector) and `Ctrl+G` (newest unread) act only while the keyboard focus is in RADE's own chrome (Navigator, Inspector, Viewer, Review Pane, Project home, Usage page, titlebar); in a Terminal or a Browser page they reach the program (readline kill-line and back-char, the tmux prefix, abort). Their `Ctrl+Shift` forms `Ctrl+Shift+P`, `Ctrl+Shift+E`, `Ctrl+Shift+G` work everywhere, and key hints show those. The R17.3 unit test keeps covering every default a Terminal can receive.

1. App config: `~/.config/rade/config.toml` (`[sources.<id>]`, `[projects.<id>]` still read until M16; keybindings, Presets, limits, `desktop_notify`, `[viewer] editor`). Live-reload on save.
2. Terminal appearance (font, theme, colors, cursor): read from `~/.config/ghostty/config`.
3. Keybindings default to `Ctrl+Shift+<key>` for in-app actions, `Alt+<n>` for Source switching within the shown Workspace, `Ctrl+Alt+<n>` (1–9) for Workspace switching (R23.5); all rebindable. No default collides with GNOME's, Ghostty's or IBus's defaults (unit test). Programs keep all plain `Ctrl+<key>` combos. Defaults mirror the owner's Ghostty habits: split right `Ctrl+Shift+Enter`, split down `Ctrl+Shift+D`, focus `Ctrl+Shift+H/J/K/L`, Zoom `Ctrl+Shift+Z`, newest unread Notification `Ctrl+Shift+G`, New Worktree `Ctrl+Shift+B`, Review `Ctrl+Shift+R` (inside RADE this wins over IBus's Unicode-entry shortcut), command palette `Ctrl+Shift+P` (replaces Ghostty's own palette), Browser Pane `Ctrl+Shift+O` (Ghostty's `new_split:right`, which RADE's own split keys replace). `Ctrl+Alt+<arrow>` is never used (GNOME workspace switching).

## R18 — Browser Pane (M12, post-v1)

1. A **Browser Pane** shows a web page (WebKitGTK 6, ADR-0011) under a header with Back, Forward, Reload, a URL bar, the Web Inspector button and Close. `Ctrl+Shift+O` (palette "Open Browser") focuses the Tab's Browser, else opens one split right of the focused Pane — on the first port listening in this Worktree (R18.4), else blank with the URL bar focused. The URL bar is localhost first: `3000` or `:3000/x` → `http://localhost:3000/…`, `localhost:5173` → `http://`; a host with a dot gets `https://`; no web search. In the page: `Ctrl+L` URL bar, `F5` reload, `Alt+Left/Right` history. Links that open a new window open in the same Pane. Light/dark follows the system (`prefers-color-scheme`). A Browser Pane is part of Session Restore (its URL, reloaded; not its history).
2. Agents drive it through the CLI (API.md `rade browser`): `open <url>` (into the caller's Tab's Browser, else a new one split right of the caller; focus stays; waits for the load), `snapshot` (the page as indented text — ARIA role, name, value / checked / disabled / level / href — where every element an Agent can act on carries a ref `[ref=e3]` that stays the same until the page navigates), `click <ref>`, `fill <ref> <text>`, `press <key>`, `eval <js>` (JSON result), `screenshot <file.png>`, `console` (console messages and JS errors since the last call), `wait --load|--text|--ref`, `back`, `forward`, `reload`. A click or key that navigates waits for the new page. A JS dialog raised by an Agent's action is accepted and logged, so it never blocks the page.
3. Website data: one persistent store per RADE flavor (cookies, local storage in `$XDG_DATA_HOME/rade[-dev]/browser`, cache in `$XDG_CACHE_HOME/rade[-dev]/browser`), so logins to local dev apps survive a restart (persistent cookies; session cookies end with the app) and a dev build never shares them with the daily app (ADR-0010). No password storage in the desktop keyring.
4. Ports: TCP ports that processes in a Pane listen on (by `RADE_PANE_ID` and this app's `RADE_SOCKET` in their environment and `/proc/net/tcp{,6}`) show in the sidebar Source / Worktree row tooltip ("Listening: localhost:3000 (dev)"), as palette rows "Open localhost:<port>", and in `rade browser ports`.
5. Isolation: the automation API is reachable only through the RADE socket (same UID, R2.5). Its page-side code runs in an isolated script world: page scripts can't see, call or patch it, so a page can't fake a snapshot or hijack a click. `eval` deliberately runs as the page. The console hook lives in the page's world (only there can it see console calls), so a page can hide or fake its own messages.

AC:
- R18-a: An Agent runs `rade browser open 8000` for a dev server it started → a Browser opens beside it; `snapshot` lists the form fields with refs; `fill` + `click` the Save button → the next `snapshot` shows the saved text; `console` shows the page's `console.log` and an uncaught error; `screenshot` writes a PNG.
- R18-b: Typing `localhost:<port>` in the URL bar and Enter loads the page; clicking a button in it with the mouse works (live).
- R18-c: Quit and relaunch → the Browser comes back on the same URL with its persistent cookies.

## R19 — Basic actions (M13, post-v1)

Everyday terminal-app actions; decisions with trade-offs in [checks/M13.md](checks/M13.md).

1. **Tabs.** Right-click on a Tab (keyboard: arrows move through the items, Enter runs one, Esc closes): Rename…, Duplicate (a new Tab right after it, in its Worktree, with a shell in the source's focused Terminal's cwd — the Worktree root for any other Pane; an Agent is not copied), Close, Close Other Tabs, Close Tabs to the Right (items that would do nothing are left out). Also palette actions (Duplicate Tab, Close Tab, Close Other Tabs, Close Tabs to the Right) and `rade tab list | rename <tab> <name> | close <tab> [--others | --right] [--force]`. Closing Tabs from the UI always asks (R19.9), listing the Agents in them still `running` or `waiting`. A Fan-out Tab is never closed on the side: Others / Right leave it open (and say so), closing it alone opens its Discard dialog (CLI: `USAGE` naming `rade fanout discard`). No new default keys.
2. **Sources.** The sidebar Source menu adds Rename…, Pause / Resume, Open in VS Code (`[viewer] editor`, shown when it is on `PATH`), Open in Files (the folder's default app) and Remove Source… (asks; closes its Panes like `rade close`; never deletes files — the dialog says the folder and the Worktrees RADE created stay on disk). General: Rename… and Open in Files only. CLI: `rade source rename | pause | resume`.
3. **Pause / Resume.** Pausing a Source stops every process in all Panes of all its Worktrees (SIGHUP, SIGKILL after 12 s, like `rade close`) and keeps its Tabs, Layouts and Panes: each Pane shows a "Paused — Resume" placeholder, the sidebar row a ⏸ and dimmed. Each Pane gets what Session Restore would give it (R11.2): Agents resume their session with the recorded invocation, never replaying a prompt; one cut mid-turn (`running` / `waiting`) comes back `interrupted`. A paused Source is not started at launch and stays paused across restarts; Resume starts its Panes. Nothing new opens in it — spawn, split, new Tab, `--worktree new`, Fan-out — and `send` / `read` to its Panes are refused (`USAGE` suggesting `rade source resume <id>`). Pausing while one of its Fan-outs is at work asks (UI) / needs `--force` (CLI). General can't be paused. The UI asks first when Agents are still working.
4. **Rename.** Tab (R1.8), Pane (`rade rename <pane> <name>`, the Terminal menu, palette "Rename Pane…": shown in its caption, addresses it in the CLI; unique among the Panes running in its Worktree, not shaped like a Pane id; empty clears it), Source (display name only: its id — CLI, `RADE_SOURCE`, Worktree folders, `[sources.<id>]` — never changes; non-empty, not another Source's name or id).
5. **Terminal search.** `Ctrl+Shift+F` (Ghostty's own binding) opens a search bar over the Terminal: typing searches the screen and scrollback (Ghostty's search, matches highlighted), `Enter` / `Shift+Enter` and ↓ / ↑ go to the next / previous match, a `3/17` count, `Esc` closes and returns the keyboard to the Terminal. Input methods work in its entry. Also the Terminal menu's Search… and palette "Find in Terminal…".
6. **Terminal menu.** Right-click a Terminal (the menu takes the keyboard: arrows, Enter, Esc): Open "<path or URL>" (when one is under the pointer, as Ctrl+click finds it, R7.1), Copy (with a selection), Paste, Select All, Search…, Clear (Ghostty `clear_screen`), Split Right, Split Down, Rename Pane…. Like Ghostty, the right button goes to the program first: a program with mouse reporting on gets the click (no menu); `Shift`+right-click opens the menu over it; Ghostty's `right-click-action` applies.
7. **New Tab placement.** A Tab opened from the UI (New Tab key, the `+` menu's Shell or Agent rows) goes right after the active Tab of its Worktree, like a browser; `rade spawn --tab new` and other CLI / socket calls still append it last.
8. **Keys in menus.** Context menus (Terminal, Pane caption, Tab, Navigator rows) show at each item's right the key that runs it: RADE's binding as configured (none when unbound), or the Terminal's own key for Copy, Paste, Select All and Search. The Terminal menu also has Zoom and Close Pane.
9. **Close Pane button, confirm close.** In a Tab with 2 or more Panes every Pane has a header — a plain shell's named `Terminal` — and each header has a ✕ (Close Pane, `Ctrl+Shift+W`) after Zoom and ⋯; a Tab with one Pane shows no ✕. Closing a Tab or a Terminal from the UI — Tab ✕, middle-click, the Pane ✕, the close keys, menus, palette — asks first (Cancel is the default): the dialog names what closes — the Tab's label (several Tabs: each listed), the Pane's name and its title (`Close Pane “Terminal — RADE”?`) — and an Agent that is still `running` or `waiting`. A Viewer, Review or Browser (nothing runs in it) closes at once; a Fan-out Tab / Agent keeps its own dialog (R15). `rade close`, `rade tab close` and the socket never ask.

AC:
- R19-a: `rade tab close <t> --others` closes the other Tabs of its Worktree but not a Fan-out Tab; the Tab menu's Close Other Tabs asks first when an Agent is running.
- R19-b: Pause a Source with an Agent mid-turn → no process with its Panes' `RADE_PANE_ID` remains, placeholders show, `rade spawn --source <it>` exits 2; quit and relaunch → still paused, nothing started; Resume → the Agent resumed its session, `interrupted`.
- R19-c: `Ctrl+Shift+F`, type a word printed earlier (scrolled out of view) → the count shows every match; `Enter` moves through them; `Esc` closes.
- R19-d: right-click a Terminal → the menu; in a program with mouse reporting → the program gets it; Shift+right-click → the menu.
- R19-e: 3 Tabs, the 1st active, New Tab → the new Tab is 2nd and the other three keep their order.
- R19-g: split a shell Tab → both Panes show a header with ✕; ✕ → a dialog naming the Pane and its title; Cancel keeps the Pane, Close closes it and the ✕ goes away with the last split; the Tab's ✕ asks too; `rade close <pane>` closes without a dialog.
- R19-f: right-click a Terminal → Paste `Ctrl+Shift+V`, Split Right `Ctrl+Shift+Enter`, Zoom `Ctrl+Shift+Z`, Close Pane `Ctrl+Shift+W` shown at the right of their items; a rebound key shows the new key, an unbound one none.

## R20 — Visual design system (M14, post-v1) — Superseded (1.0.0) by R102, R103

Superseded in 1.0.0 by R102, R103 (ADR-0036: the terminal-native visual system and window chrome). Text: `git show v0.7.1:docs/PRD.md`.

## R21 — Source Control (M14 phase 2, post-v1)

**1.0.0 amendment (R107):** The Changes page shows the focused Project grouped by Source link, each group with its own message box and Commit (R107.1); Create PR's default base is the Worktree's recorded base, the link's base for a link's own checkout (R99.2). Per-file, per-hunk, Generate, Push and Pull stand.

**M45 amendment (R120–R122, ADR-0045):** items 1–5, 8 and 9 are replaced by R120 and R121 (one card per link, row click → Diff Pane, Commit ▾, Sync / Publish / Fetch); per-hunk staging (item 3) moved to the Diff Pane (R122.3). Create PR (6) and Generate (7) stand.

The Right Panel's Changes page for the active Worktree, VS Code-like. RADE runs `git` / `gh` only on the owner's click; Agents only draft text (ADR-0013, superseding ADR-0008's exclusion).

1. **Lists.** Staged, Changes (unstaged, conflicts marked `!`) and Untracked, from `git status --porcelain=v2`, each with a count; a row shows the status letter (M A D R U !), the file name and its folder (dim). The Changes toggle of the switcher shows the number of changed files. Outside a git repository (General, a plain folder) the page says so.
2. **Per file.** Buttons on each row: Stage (Changes, Untracked), Unstage (Staged), Discard (Changes: restore from the index; Untracked: delete the file). Discard asks first. A click on a row shows its diff under it (staged rows: index vs `HEAD`; Changes: working tree vs index) with Open File (the Viewer).
3. **Per hunk.** In a shown diff of a modified file each hunk has Stage / Unstage / Discard (asks), applied with `git apply --cached` / `--cached -R` / `-R` of that hunk alone. Added, deleted, renamed, binary and untracked files are handled whole.
4. **Commit.** A message box with Commit (and `Ctrl+Enter` in the box) commits the staged changes only (`git commit -F -`); disabled with an empty message or nothing staged. git's own refusal (a hook, no identity) is shown in the page.
5. **Branch.** The branch with `↑ahead ↓behind` against its upstream; Pull (`git pull --ff-only`) and Push. Push asks first, naming the remote, branch and commit count; without an upstream it runs `git push -u <remote> <branch>` (remote `origin`, else the only one).
6. **Create PR.** A dialog with title, body and base (default: the Source's main branch); creating it pushes the branch first when it has no upstream or unpushed commits (the dialog says so), then runs `gh pr create --base --title --body`; the PR URL shows in a Toast. `gh` missing or not logged in → its message in the page.
7. **Generate.** Next to the commit message and in the PR dialog: runs `[scm] draft_command` (config; default `claude -p --tools "" --no-session-persistence`) in the Worktree with a prompt on stdin — the staged diff (commit) or the branch's diff against its base and commit list (PR), cut at 60 KB; the repository's `CLAUDE.md`, `AGENTS.md`, `CONTRIBUTING.md` and PR template (each cut at 8 KB); the last 10 commit subjects — and fills the box with its output (PR: first line = title, the rest = body). Off the UI thread, at most 120 s, cancellable. Never commits or creates anything itself.
8. **Freshness.** git runs off the UI thread. The page refreshes when shown, on a Worktree switch, when the window gets focus, after its own actions, when `.git` changes, and every 3 s while the page is on screen and the window active; an unchanged status rebuilds nothing (open diffs stay open). A list shows at most 500 files ("… and N more"; Stage All still takes them all).
9. **Keys.** `Ctrl+Shift+M` shows the Changes page (opening the Right Panel); palette Show Changes, Commit Staged Changes (focuses the message), Pull, Push, Create Pull Request….

AC:
- R21-a: In a repository with a bare remote: stage one file, unstage it, stage one hunk of a two-hunk edit (the index then holds only that hunk), discard the other hunk after confirming, discard an untracked file after confirming → `git status` matches each step.
- R21-b: Generate with a fake `draft_command` fills the message from its output; the prompt it received holds the staged diff and `CLAUDE.md`; Commit (Ctrl+Enter) commits exactly the staged files with that message.
- R21-c: Push asks; confirming pushes and sets the upstream (first push); ahead/behind follow; Pull fast-forwards a commit pushed by another clone.
- R21-d: Create PR with a fake `gh` on `PATH` runs `gh pr create` with the base, title and body from the dialog; with no `gh` the page shows that it is missing.

## R22 — Search (M14 phase 2, post-v1)

**1.0.0 amendment (R107):** Search has a scope: `link` (the focused link's checkout or Worktree) or `Project` (every link of the Project, grouped by link), R107.4.

The Right Panel's Search page: search the active Worktree's files, VS Code-like. No replace (the Viewer is read-only, ADR-0008).

1. **Query.** A search entry plus Match Case, Whole Word and Regular Expression toggles; "files to include" and "files to exclude" (comma-separated globs, `.gitignore` syntax: `*.rs`, `src/**`). An invalid regular expression is reported under the entry.
2. **What is searched.** The Worktree's files with `.gitignore` / `.ignore` / git's exclude applied (as the file tree, R8), hidden files included, `.git` never; binary files (a NUL byte in the first 8 KB) and files over 4 MB are skipped.
3. **Results.** Grouped by file (path relative to the Worktree, match count; a click on the file folds it), one row per matching line with its number and a preview with the matches highlighted (long lines cut around the first match). A click on a line opens the file in the Viewer at that line (R7.2). A summary line: "N results in M files" (+ "— showing the first 2,000").
4. **Responsiveness.** Searches run off the UI thread on all cores, start 200 ms after typing stops, and a new query or Worktree cancels the running one; results stop at 2,000 matches.
5. **Keys.** `Ctrl+Shift+S` shows the Search page with the keyboard in its entry (opening the Right Panel); palette "Search in Worktree".

AC:
- R22-a: Regex, case, whole-word, include and exclude globs and `.gitignore` each change the results as expected on a test tree; a click on a result opens the Viewer at that line.
- R22-b: On a 50,000-file tree a search finishes within 2 s (headless, release build) and the UI thread keeps answering while it runs (a probe every 50 ms, as in M8).

## R23 — Workspaces (M15)

**1.0.0 amendment (R108):** R23.2: Agent accounts are declared as `[[account]]` in `workspace.toml` (R108.1); `[env]` `CLAUDE_CONFIG_DIR` / `CODEX_HOME` still work for Panes without an account. R23.3: for a Pane with an account, the account's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` wins over `[env]`, and a recorded-invocation or Preset prefix that sets it to another directory is refused, not preferred (R108.7). R23.4, R23.7: the Navigator and Scratch replace the sidebar and General (R104).

A **Workspace** is a working identity (a company, personal work) rooted at a folder, e.g. `~/workspaces/globex`. One window shows one Workspace; the others keep running. Decisions: ADR-0015 (one app, env per Pane, never `gh auth switch`), ADR-0014 (Worktree location).

1. **Registry.** `rade workspace add <root> [--name <n>]`, `list`, `switch <ws>`, `remove <ws>`. A Workspace `default`, rooted at `$HOME`, always exists: fresh state and migrated state (R24.4) start in it. Adding a Workspace moves every Source whose folder is under its root into it. `remove` refuses while it still has Sources other than its General; `default` can't be removed. Ids are slugs of the name (`--name`, else `name` in its `workspace.toml`, else the folder name).
2. **`<root>/.rade/workspace.toml`** (optional): `name`, `color` (`blue`, `green`, `yellow`, `orange`, `red`, `purple`, `brown`, `gray`, or `#rrggbb`), `[env]` (string values; a leading `~` is expanded) and `[clone] default_owner`. Unknown keys are ignored. M15b adds `[identity]` (R25.1); the design's `[accounts]` was dropped: `[env]` `CLAUDE_CONFIG_DIR` / `CODEX_HOME` say which Agent accounts a Workspace uses. It is read each time it is needed (no watcher).
3. **Env per Pane.** Every Terminal gets its Workspace's `[env]`, then `RADE_WORKSPACE` and the other `RADE_*` variables, which `[env]` can't override (nor RADE's `PATH` prefix). The app's own network commands in a Worktree get the same `[env]`: Push, Pull, Create PR (`gh`) and the Draft command (R21). Running Panes keep the env they started with; new Panes and Session Restore get the current file. An Agent whose recorded invocation carries a `VAR=value` prefix (R11.2, e.g. `CLAUDE_CONFIG_DIR`) keeps it: the prefix wins over `[env]`. A login shell that exports the same variable itself (an rc file) also wins; RADE can't see that.
4. **One window, one Workspace.** The sidebar, the Inbox, the Status Bar counts, `Alt+1..9` and the palette's Source and Worktree rows show only the shown Workspace. Hidden Workspaces keep their processes, Agent Statuses and Notifications.
5. **Switcher.** The sidebar header button shows the Workspace's color dot and name; its popover lists every Workspace with its `Ctrl+Alt+<n>` key, Unread count and worst Agent Status glyph; a click switches. While an Agent in a hidden Workspace is `waiting`, the button shows an amber `!`. `Ctrl+Alt+1..9` and the palette's Workspace rows ("Switch Workspace") switch too. Switching restores that Workspace's last Source, its Worktree, Tab and focus.
6. **Attention across Workspaces.** A Toast or desktop notification from a hidden Workspace names it (`[<Workspace>] <name> needs input`). Its Show / Focus, a Notification click and `rade focus` switch Workspace and focus the Pane.
7. **General per Workspace**, rooted at the Workspace root: id `general@<ws>` (the `default` Workspace's keeps `general`).
8. **New Worktrees** of a Source in a Workspace other than `default` go to `<root>/.worktrees/<source>/<name>` (`.worktrees-dev` for a dev build, ADR-0010), so folder-keyed setups (a shell hook setting `GH_CONFIG_DIR` by `$PWD`, Workspace-level `CLAUDE.md` / `.envrc`) apply to them as to the Source (git `includeIf "gitdir:"` already matches a linked worktree anywhere); `default` keeps `$XDG_DATA_HOME/rade/worktrees`. Existing Worktrees are never moved (ADR-0014).

AC:
- R23-a: Workspaces A and B with different `GH_CONFIG_DIR` in `[env]`: a Pane in each prints its own value, and a fake `gh api user` prints a different login in each.
- R23-b: `rade worktree add` on a Source in A creates `A/.worktrees/<src>/<name>`; `git config user.email` there is A's email through `includeIf`.
- R23-c: `Ctrl+Alt+2` → the sidebar shows only B's Sources; `Ctrl+Alt+1` → A's exact Tab and focus; A's Agent process stayed alive while hidden and `rade read` shows what it printed meanwhile.
- R23-d: An Agent in hidden A starts `waiting` → within 1 s the switcher shows `!` and A's Unread 1, and the desktop notification's title names A; activating it switches to A.
- R23-e: No default key collides with GNOME, Ghostty or IBus defaults (unit test); `Ctrl+Alt+<n>` works on real GNOME (live).

## R24 — Sources (rename of Project, M15)

1. **Rename.** What was a Project is a **Source** in the UI, CLI, socket, config and env: `rade source add|list|remove|rename|pause|resume`, `--source`, methods `source.*`, `RADE_SOURCE`, `[sources.<id>]` (`[projects.<id>]` still read).
2. **`rade source add <path | owner/repo | repo | URL> [--name <n>] [--workspace <ws>]`.** A path (an existing folder, or one starting with `/`, `.` or `~`) is registered. Anything else is cloned with `gh repo clone <spec> <root>/<repo>` (a bare `repo` gets `[clone] default_owner/` in front) under the target Workspace's `[env]`, run by the CLI in the caller's terminal (prompts and progress shown there), then registered. Idempotent: an existing clone folder is not cloned again, and a folder that already is a Source prints it and exits 0. The Workspace: `--workspace`, else the one whose root holds the folder (the most specific, `default` last), else the caller's, else the one shown.
3. **Deprecated aliases (removed in M16, R31.2, which reuses "Project" for a set of Sources).** `rade project …` (prints `warning: 'rade project' is now 'rade source'; the old name goes away in M16` on stderr), `--project`, methods `project.*`, the `"project"` parameter key, `RADE_PROJECT` and `[projects.*]` keep working. Results carry `"project"` / `"projects"` next to every `"source"` / `"sources"` key.
4. **`state.json` version 2.** A version-1 file is migrated on load: the original is copied once to `state.json.v1.bak` (never overwritten), keys are renamed, and every Source joins `default`. An older RADE refuses version 2 ("newer"); the backup is the way back.

AC:
- R24-a: A v1 fixture loads as v2; `state.json.v1.bak` is byte-identical to it and is not rewritten on the next start; Tabs, Panes and Agent session ids are restored.
- R24-b: With a fake `gh`, `rade source add org/x` in B clones under B's root (the fake `gh` logged B's `GH_CONFIG_DIR`) and registers it; running it again exits 0 and changes nothing.
- R24-c: `rade project list` prints the warning on stderr; its stdout and `--json` equal `rade source list`'s.
- R24-d: A v1 Worktree under `$XDG_DATA_HOME` is still listed, a Terminal opens in it, and it can be removed.

## R25 — Workspace setup (M15b)

A Workspace's identity is declared in its `workspace.toml` and made real on any machine by one idempotent command. RADE only calls the standard CLIs (`git`, `gh`, `ssh-keygen`, `ssh`) and never stores a secret (ADR-0016, extending ADR-0008).

1. **`[identity]`** in `workspace.toml`: `git_name`, `git_email`, `github` (the expected login), `ssh_key` (a file name in `~/.ssh`), `sign` (`ssh` or `none`). Paths in the file use `~`; the root itself is machine-local (`state.json`).
2. **`rade workspace setup <root> [--name] [--git-name] [--email] [--github] [--ssh-key] [--sign ssh|none] [--claude shared|separate] [--default-owner] [--env K=V]… [--from <git-url>] [--dry-run]`** runs, in order: check `git`, `gh`, `ssh-keygen` are installed; `--from`: clone the definition into `<root>/.rade` when it is missing; create `<root>/.rade` and `<root>/.worktrees`; merge `workspace.toml` (missing keys only; defaults `ssh_key = id_ed25519_<ws>`, `sign = ssh`, `[env] GH_CONFIG_DIR = ~/.config/gh-<ws>`, `CLAUDE_CONFIG_DIR = ~/.claude-<ws>` with `--claude separate`); set the missing keys of `<root>/.gitconfig` (`user.name`, `user.email`, `core.sshCommand = ssh -i <key> -o IdentitiesOnly=yes`, and with `sign = ssh`: `gpg.format = ssh`, `user.signingkey = <key>.pub`, `commit.gpgsign`, `tag.gpgsign`); add a marked block (`# >>> rade workspace <ws> >>>` … `# <<< rade workspace <ws> <<<`) with `[includeIf "gitdir:<root>/"]` to `~/.gitconfig` unless an `includeIf "gitdir:"` (any of `~/`, absolute, `gitdir/i:`) already points at the root; generate `~/.ssh/<key>` (`ssh-keygen -t ed25519 -N ""`) when missing; when `gh api user` fails under the Workspace's `GH_CONFIG_DIR`, run `gh auth login --hostname github.com --git-protocol ssh --insecure-storage --scopes admin:public_key[,admin:ssh_signing_key]` (plus `--skip-ssh-key` when the installed `gh` has it) in the caller's terminal; `gh ssh-key add` the public key as an authentication key (and a signing key with `sign = ssh`) unless the account has it; install Agent hooks into the Workspace's own Agent config dirs (R25.5); register the Workspace when the app runs (else print the `rade workspace add` hint). Every file edit shows a diff and goes through `rade setup`'s path (backup `<file>.rade-bak-<stamp>`, atomic write, a symlink's target is edited and stays a symlink, the mode is kept). It fills gaps and never overwrites: a key already set to another value is left (doctor reports it); a flag that conflicts with a value already in `workspace.toml` → `USAGE`. `--dry-run` prints the diffs and the commands it would run, runs only read-only probes, writes nothing. A run with nothing to do prints `nothing to do`. A root that is a git checkout → `USAGE` ("that's a Source").
3. **`rade workspace import <root> [--env K=V]… [--dry-run]`** writes only `<root>/.rade/workspace.toml` (missing keys) and registers the Workspace. It infers: the identity from `git config` inside the first Source under the root (which applies `includeIf`), else `<root>/.gitconfig`; `ssh_key` from `user.signingkey`, else the `-i` of `core.sshCommand`, else `ssh -G <alias>` for a `url.<alias>.insteadOf` GitHub alias; `sign` from `gpg.format` + `commit.gpgsign`; `github` from `gh api user`; `GH_CONFIG_DIR`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME` from a login-shell probe in the root (R26 check 11); `default_owner` from the most common owner among the Sources' `origin` remotes. A root that is a git checkout → `USAGE`.
4. **`rade workspace clone [--workspace <ws>] <spec>…`** clones each spec like `rade source add <spec>` (R24.2) with `GIT_SSH_COMMAND` from `[identity] ssh_key` as well as the Workspace's `[env]`; an existing clone is skipped (and registered); it goes on after a failure, prints `cloned n, skipped n, failed n`, and exits 2 when any failed.
5. **`rade setup`** also installs hooks into the `CLAUDE_CONFIG_DIR` and `CODEX_HOME` of each registered Workspace's `[env]` (it reads `state.json` read-only).
6. **Palette "Create Workspace…"**: an inline form (root, name, git name, email, GitHub login, SSH key, Claude shared / separate) that opens a Terminal Tab in the shown Workspace's General with `rade workspace setup …` typed and run, so the `gh` device-flow login shows in a visible Pane.

AC:
- R25-a: Setup on an empty temp HOME (fake `gh`, `ssh-keygen`, `ssh`) produces `workspace.toml`, `<root>/.gitconfig` (name, email, sshCommand, signing), one marked block in `~/.gitconfig`, a key pair, a `gh auth login` logged with `--insecure-storage` and the Workspace's `GH_CONFIG_DIR`, and `ssh-key add` for both authentication and signing; `git config user.email` in a Source cloned under the root equals the Workspace's email.
- R25-b: A second run prints `nothing to do`: no new backup, no second login, keygen or `ssh-key add`.
- R25-c: `--dry-run` prints the diffs and planned commands; a sha256 snapshot of HOME is unchanged and the fakes logged no mutating call.
- R25-d: Every file edit leaves `<file>.rade-bak-<stamp>`; a symlinked `~/.gitconfig` stays a symlink and its target is edited; modes are kept.
- R25-e: A hand-written `includeIf` for the root → no marked block; moving the root and re-running replaces the block in place.
- R25-f: Import on a fixture of the owner's layout (host alias + `insteadOf`, `signingkey`, a `cd` hook in the rc file, an org remote) writes the expected TOML; only that file changes in HOME; a second import changes nothing.
- R25-g: Clone of 3 specs (1 already there, 1 failing) clones 1, skips 1, fails 1 and exits 2; the fake `gh` logged the Workspace's `GH_CONFIG_DIR` and `GIT_SSH_COMMAND`.
- R25-h: A Workspace with `CLAUDE_CONFIG_DIR` outside `~/.claude-*` gets hooks from both `rade setup` and `rade workspace setup`.
- R25-i: Submitting the palette form opens a Pane whose command line equals the expected `rade workspace setup …`.

## R26 — Workspace doctor (M15b)

`rade workspace doctor [<ws|root>] [--json]` checks a Workspace (no argument: every registered one but `default`, read from `state.json`) and prints `ok` / `warn` / `FAIL` per check with the command that fixes it; exit 1 when any check fails (warnings don't change the exit code). Checks: (1) the root exists, is not a git checkout, `workspace.toml` parses; (2) `git` ≥ 2.36, `gh` ≥ 2.40, `ssh-keygen` installed; (3) `user.name` / `user.email` inside a Source (else for a path under the root) equal `[identity]`; (4) with `sign = ssh`: `gpg.format = ssh`, the signing key file exists, `commit.gpgsign = true`; (5) `~/.ssh/<key>` and `.pub` exist, the private key is mode 600; (6) `GH_CONFIG_DIR` is in `[env]` and `gh api user` there returns `github` (not `gh auth status`); (7) its `hosts.yml` holds an `oauth_token` (token in the file, not the shared keyring slot), else warn; (8) the key is on the account (authentication, and signing with `sign = ssh`); (9) `ssh -T` with only that key (`-F /dev/null`, `IdentitiesOnly`, `BatchMode`, no known_hosts update) greets `github`; (10) Claude: the config dir exists, its `.claude.json` (`~/.claude.json` for `~/.claude`) has `oauthAccount.emailAddress`, RADE hooks are installed (fail when `[env]` declares `CLAUDE_CONFIG_DIR`, else warn); (11) a login shell started in the root with `[env]` exported doesn't change an `[env]` variable (else warn naming the variable and the rc files that set it; a probe timeout warns); (12) the running app has the root registered (warn). A Workspace without `[identity]` gets one warning instead of checks 3–9. Private keys are never read and tokens never printed.

AC:
- R26-a: Each check fails (or warns, for 7, 11, 12) on a broken fixture with its fix hint; the healthy fixture exits 0.
- R26-b: An rc file exporting a different `GH_CONFIG_DIR` gives a warning naming the variable and the rc file.

## R27 — Skill `rade-workspace` (M15b)

`skills/rade-workspace/SKILL.md`, installed by `rade setup` next to the `rade` skill. The Agent asks the user (or reads the machine: `git config --get-regexp includeif`, `ls ~/.config/gh*`, `ls ~/.ssh`), edits only `workspace.toml` (or passes flags), runs `rade workspace setup --dry-run`, shows the plan, runs it for real with the login step in its own Pane (`rade spawn --cmd … && rade wait`), and repeats `rade workspace doctor` until it passes. It never hand-edits `~/.gitconfig`, `~/.ssh` or the `gh` directories.

AC:
- R27-a: The skill file is installed by `rade setup` and every edit it describes is a `rade` command.

## R28 — Projects (M16) — Superseded (1.0.0) by R97, R99

Superseded in 1.0.0 by R97, R99 (ADR-0034: Source links with one base each; Worktrees belong to a link). Text: `git show v0.7.1:docs/PRD.md`.

## R29 — Layered config (M16)

**1.0.0 amendment (R101):** R29.1: `task`, `profiles`, `batch` and `report` leave the layered allow-list; a layer setting them is reported `ignored (removed in 1.0.0)`, never an error (R101.4).

1. **Layers**, lowest precedence first: built-in → user `$XDG_CONFIG_HOME/rade/config.toml` → the Workspace's `workspace.toml` → the Project's `project.toml` → the Source's `<source>/.rade.toml`. Tables merge key by key; a scalar or an array replaces. Only `agents`, `scm`, `fanout`, `viewer` and `sources` are layered (in `.rade.toml` also a top-level `setup` = that Source's `[sources.<id>] setup`); `keybindings`, `limits`, `desktop_notify`, `status_bar` and `idle_seconds` are app-wide (user file only) and are ignored elsewhere. Each layer is validated alone, so an error names its file; a broken layer is skipped and the rest still applies.
2. **Context.** A Pane's Project is its Worktree's `project`, else the one Project that lists its Source (none if zero or several). The app reads the layers for that context when it spawns an Agent (its Preset), restores or resumes Panes, runs a new Worktree's `setup`, the Draft command, the Judge and the Viewer's editor.
3. **`rade config explain [<key> | <prefix>] [--pane <p> | --source <s> [--worktree <w>] | --project <p> | --workspace <ws>] [--json]`** (method `config.explain`): the effective value and every layer's value (✓ on the winner), then the context. No key: every value set in a file, and ignored keys. Default context: the caller's Pane, else the Source on screen.
4. **Trust.** A Source's `.rade.toml` comes from the repository, so it is ignored until `rade config trust <source>` (method `config.trust`), which stores the file's exact content in `state.json`; any change makes it untrusted again (the first Terminal of a new Worktree prints one `rade: .rade.toml untrusted …` line).

AC:
- R29-a: With the same key in the user file, `workspace.toml` and `project.toml`, `explain` shows the Project's value with ✓, in the documented text and JSON.
- R29-b: `[agents.<preset>] command` in Workspace A's `workspace.toml` applies to spawns in A only.
- R29-c: An untrusted `.rade.toml` `setup` does not run and `explain` says `untrusted`; after `trust` it runs; editing the file makes it untrusted again.
- R29-d: `[keybindings]` in `workspace.toml` is shown as `ignored (app-wide)`.

## R30 — Sidebar by Project (M16; rewritten in M26, ADR-0028) — Superseded (1.0.0) by R104

Superseded in 1.0.0 by R104 (ADR-0034: the Navigator). Text: `git show v0.7.1:docs/PRD.md`.

## R31 — Project CLI and socket; the M15 aliases are gone (M16, breaking)

**1.0.0 amendment (R97):** R31.1: `rade project add <name> [--workspace <ws>] [--source <src>[:<branch>]]…` (one base per link; a `=` → `USAGE`), `rade project show` lists links (R97.3), and `rade project link add|remove|set-base|to-base` and `rade project plan` are added; `rade project set` keeps the keys `name`, `source.<id>.base`, `source.<id>.checkout`. R31.2 and R31-a…d stand.

1. **`rade project list [--workspace <ws> | --all]` · `rade project show <p>` · `rade project add <name> [--workspace <ws>] [--source <src>[:<base>=<branch>,…]]…`** (methods `project.list|show|add`). `show` checks every Source's branch for every Named Base (`git rev-parse --verify`, off the UI thread). There is no remove: delete the folder.
2. **Removed** (R24.3): `rade project` as `rade source`, `--project` as `--source`, methods `project.*` as `source.*`, the `"project"` parameter, the `"project"` / `"projects"` result keys, `RADE_PROJECT`, `[projects.*]` and `project_<n>` / `*_project` keybinding names in `config.toml`.

AC:
- R31-a: `rade project add ./x` (or any argument that is a path) exits `USAGE` with `'rade project add <path>' is now 'rade source add <path>'`.
- R31-b: `--project` on `spawn`, `list`, `tab list`, `review` and `fanout` is rejected (exit 2).
- R31-c: No result carries a `project` / `projects` alias key; Panes have no `RADE_PROJECT`.
- R31-d: `[projects.x]` or `project_1` in `config.toml` gives a reload error naming `[sources.x]` / `source_1`.

## R32 — Skill `rade-config` (M16)

`skills/rade-config/SKILL.md`, installed by `rade setup` next to `rade` and `rade-workspace`: where each file lives and its keys, the layering and app-wide keys, the trust rule, and the loop *edit a file → `rade project show` → `rade config explain` until clean*. It never edits `state.json`.

AC:
- R32-a: `rade setup` in a temp HOME installs it; every edit it describes is a `.rade` file or a `rade` command.

## R33 — Task files (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R34 — Task CLI and socket (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R35 — Task Run (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R36 — Agent Profiles (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R37 — Automatic Task status (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R38 — Tasks page (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R39 — Task-centric sidebar (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R40 — Skill `rade-task` (M17) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R41 — Event Journal (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R42 — Digest (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R43 — Report new (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R44 — Revise, save, diff (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R45 — Export (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R46 — Reports page (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R47 — Skill `rade-report` (M18) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R48 — Batch files (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R49 — Scheduler (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R50 — Review and stop policies (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R51 — Integration Branch and Push (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R52 — `rade batch` and `rade task wait` (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R53 — Batch page and sidebar row (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R54 — Skill `rade-batch` (M19) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R55 — Task Detail (M20) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R56 — New Task dialog (M20) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R57 — Brainstorm with Claude (M20) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R58 — Task template v2 (M21) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R59 — Definition of Ready (M21) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R60 — Depends (M21) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R61 — Task Agent prompt (M21) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R62 — Writers of v2 Tasks (M21) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R63 — Verify command (M22) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R64 — Verified Run (M22) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R65 — Reviewer rubric (M22) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R66 — Evidence (M22) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R67 — Plan gate (M23) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R68 — Flow metrics (M23) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R69 — Lessons (M23) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R70 — Verify command confirmation (M23) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R71 — Browser Annotations (M24)

The owner points at what is wrong on a web page instead of describing it (v0.7.1:docs/design/agentic-sdlc-roadmap.md § M24; ADR-0026).

1. **Annotate mode** in a Browser Pane: the header's annotate toggle or `Ctrl+Shift+A` in the Browser (Esc in the page ends it). On: a transparent layer over the page (in a closed shadow root, from RADE's isolated script world) highlights the element under the mouse with its role and name; page clicks don't reach the page. Off: nothing of it is in the page. Works on dev servers, `file://` prototypes and any page the Pane shows. `rade browser annotate on|off` (method `browser.annotate`) turns it on for the owner.
2. **Pick and comment.** A click picks the element (also inside open shadow roots; its selector is then relative to its shadow root); a drag (> 6 px) picks a region, anchored on the smallest element holding all of it. A box under it takes the comment (Enter adds, Shift+Enter new line, Esc cancels). RADE keeps per Annotation: the page URL, a stable CSS selector (the closest unique test id — `data-testid`, `data-test`, `data-cy`, `data-qa` —, stable `id` or form field `name`, then `tag:nth-of-type(n)` steps; generated ids like `:r1:` or `ember123` are skipped), the element's `rade browser` ref, role, name and text (≤ 300 characters), its box in page coordinates, a crop of the screenshot (the box + 8 px, PNG in `$XDG_CACHE_HOME/rade/annotations`, deleted after 7 days like pasted images) and the page's last 5 console errors.
3. **List.** A panel under the page lists the pending Annotations like Review Comments: number, crop thumbnail, what it points at, the comment (editable; emptying is not deleting), delete; Clear; Send to Agent (n). The page shows numbered marks on its Annotations while annotate mode is on. They are kept on the Browser Pane in `state.json` until sent or deleted: a reload or a relaunch keeps them.
4. **Send to Agent** composes one prompt — per Annotation: what it is, `page:`, `selector:`, `ref:` (with "works until the page navigates"), `text:`, `box:`, `screenshot: <path>`, `console errors on the page:`, `comment:` — and pastes it with Enter into the Agent in the Browser's Tab (the one that opened it), else the Worktree's only Agent; several → choose; none → offer to spawn one from a Preset (like R13.2). Then they are cleared.
5. **`rade browser annotations [--send [--to <pane>] [--agent <preset>]]`** (method `browser.annotations`): the pending Annotations and their prompt; `--send` sends them like the button.

AC:
- R71-a: Annotate on → a click on the Save button and a typed comment → its selector `[data-testid="save"]`, its snapshot ref, URL, text, box, a crop about the button's size and the page's `console.error` are kept; a drag gives a `region` anchored on `#hero`; edit and delete in the panel; a reload and a relaunch keep them; off → no overlay, page clicks work.
- R71-b: Send to Agent → the Agent beside the Browser receives one prompt with both Annotations' URL, selector, ref, screenshot path, console errors and comments; they are cleared. A `file://` prototype can be annotated the same way.
- R71-c (live): a real click on an element, a real drag, typing and the Send button.

## R72 — Chromium engine (M24)

1. **`rade browser --engine chromium <command>`** (`--engine` and `--headed` go anywhere on the line): `open`, `snapshot`, `click`, `fill`, `press`, `eval`, `screenshot`, `console`, `wait`, `back`, `forward`, `reload`, `annotate`, `annotations`, and `close`, with the same arguments, results (`pane: "chromium"`) and errors as a Browser Pane; `ports` stays RADE's. The first `open` starts a small daemon (`rade __chromium`, its own process group) and Chromium; later commands talk to it on its own same-UID socket beside RADE's (`rade[-dev]-chromium.sock`, 0600). No Chromium running → `NOT_FOUND` with how to start one; `close` with none is fine.
2. **Which Chromium**: `[browser] chromium = "<path>"` in the user's `config.toml` (only there: an app-wide key, ignored in Workspace / Project files), else Playwright's (`$PLAYWRIGHT_BROWSERS_PATH`, else `~/.cache/ms-playwright/chromium-<n>/chrome-linux64/chrome`, newest first — Chrome for Testing), else `chromium`, `chromium-browser`, `google-chrome-stable`, `google-chrome`, `chrome` on `PATH`. None → `NOT_FOUND` naming `npx playwright install chromium` and the config key. `open`'s result names it: `chromium: {path, pid, profile, headed}`.
3. **How it runs**: CDP over `--remote-debugging-pipe` (no TCP port), a fresh profile in the temp dir (`<flavor>-chromium-profile-<pid>`, 0700) deleted on exit, background networking, sync and first-run UI off, `--no-sandbox` (as Playwright), headless unless `--headed` (then a real window with the annotation toolbar: Annotate, n pending, Send to Agent, Clear). A headless Chromium idle for 30 min quits; a headed one quits with its window; SIGTERM / SIGHUP / SIGINT to the daemon clean up too, and a profile left by a killed daemon is removed by the next one. `open --headed` while it runs headless (or the other way round) → `USAGE` ("close first").
4. **Same page library**: `browser.js` + `annotate.js` run in an isolated world `rade` (`Page.createIsolatedWorld`, kept for the session, so refs stay valid between commands until the page navigates); the snapshot text equals the Browser Pane's for the same page. `click` sends trusted mouse events at the element's center (so overlays and `isTrusted` behave as for a person), `press` trusted key events (`Input.dispatchKeyEvent`: Enter types `\r`, shortcuts type nothing), `fill` the same setter as WebKit; `eval` runs in the page's world; console messages and uncaught errors come from `Runtime.consoleAPICalled` / `exceptionThrown` (objects from their preview); a JS dialog is accepted while an action runs (always when headless). Annotations come from a CDP binding exposed to the `rade` world only (a page can't add any); their crop is cut by Chromium; `annotations --send` (or the toolbar) goes to the Agent whose Pane opened Chromium, else `--to`.
5. **Verify web Tasks with Playwright**: the `rade`, `rade-task` and `rade-config` skills recommend Playwright tests (Chromium by default, `npx playwright test`) as a Project's `[task] verify` for web Sources, and `rade browser --engine chromium` for checks WebKit can't run (docs/internals/research/webkit-compat.md).

AC:
- R72-a: Against the local test site: `open` (pane `chromium`, CDP over a pipe, a temp profile, headless), `snapshot` identical to the Browser Pane's, stable refs, `fill` + trusted `click` submit a form, `press` types and submits, `eval`, `console` (console.error + an uncaught error with source:line), `screenshot` 1280 px wide, `wait`, history, isolation from a hostile page (trusted click, no fake snapshot), `close` removes the process, profile and socket.
- R72-b: Annotate in Chromium with a real (trusted) click on the overlay and typed keys → an Annotation with the same selector; the crop is the box + 8 px; `--send` reaches the Agent that opened it, saying `--engine chromium`.
- R72-c: With no Chromium on the machine the E2E records the skip with its reason; CI installs Chrome for Testing so it runs there.

## R73 — A running Task's status is RADE's (M24) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R74 — Initiatives (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R75 — Document gates (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R76 — Design phase (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R77 — Build (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R78 — Parallel Initiatives (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R79 — Initiative page and skills (M25) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R80 — Run dialog and Running together (M26) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R81 — Start here (M26; activation amended in M32) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R82 — Project settings (M26) — Superseded (1.0.0) by R106

Superseded in 1.0.0 by R106 (ADR-0034: Project settings as the link table). Text: `git show v0.7.1:docs/PRD.md`.

## R83 — Language servers (M28)

Code navigation uses real language servers, one per Worktree root and language, owned by the app and shared by the owner's Viewer and every Agent (ADR-0029; owner decisions in research/code-navigation.md). TS/JS and Rust in M28; Python and Go in M30 (R93).

1. **Which server.** `.rs` → rust-analyzer; `.ts .tsx .js .jsx .mjs .cjs .mts .cts` → typescript-language-server. The root is the outermost folder with a `Cargo.toml` (Rust) or the nearest with a `tsconfig.json` / `jsconfig.json` / `package.json` (TS/JS) inside the Worktree, else the Worktree. The command is looked up in the repository's `node_modules/.bin` (TS/JS only), then on the Workspace `PATH`; RADE never installs one: a missing server is reported with its install command (`rustup component add rust-analyzer`, `npm i -g typescript-language-server typescript@5`).
2. **Built-in settings** (code-nav-spike.md): rust-analyzer `checkOnSave = false`, `files.watcher = "server"`, `cargo.targetDir = true`; typescript-language-server `disableAutomaticTypingAcquisition = true`, and when the repository's TypeScript is older than 5.0 the owner's `[code.server.typescript] tsserver_path` (else a one-line hint). Documents are opened only around a request (`didOpen` → request → `didClose`).
3. **Lifecycle.** A server starts on the first navigation request for its root (never at app start), in its own session with the Workspace `[env]`. It stops after `[code] idle_minutes` (default 15) without a request, when more than `[code] max_servers` (default 4) run (least recently used first), when its Worktree is removed, and on quit — `shutdown` (no params), `exit`, then SIGTERM / SIGKILL to its process group. A server that exits is restarted on the next request; after 3 crashes for one root it stays down until Restart (`rade code restart`, the Code page's button).
4. **Config and trust.** `[code]` is layered (`enabled`, `server.<rust|typescript>` with `command`, `settings`, `init`, `tsserver_path`); `idle_minutes` and `max_servers` are read from `config.toml` only. A Source's `.rade.toml` `[code]` applies only while its exact content is trusted (R29.4), read from the main checkout. Per repository off: `[sources.<id or name>] code = false`.
4a. **Files outside the tree** (0.7.1, ADR-0029 amendment): a file in a dependency folder (`node_modules`, `site-packages`, `.venv`, `venv`) belongs to the project holding the folder; a file outside every Worktree reached by navigation (the std library, a module cache) is asked through the server of the Worktree it was reached from. Neither starts a server of its own.
5. **Readiness and fallback.** While a server is loading (`$/progress` work, rust-analyzer `serverStatus.quiescent = false`), missing, off, crashed, slow (10 s for the UI) or without the capability, the answer is **text matches**: whole-word matches from the Search engine (R22; declaring lines for a definition), labelled with why.

AC:
- R83-a: Unit tests: framing, positions in UTF-8 / UTF-16, server requests answered, readiness, root detection, command lookup and install hints, old-TypeScript handling, the pool (LRU, idle, crashes, removal), `[code]` config.
- R83-b: E2E with a fake server: one server per root, the spike's settings reach it, idle stop with `shutdown` then `exit`, `max_servers` LRU, Worktree removal and quit stop servers, a crash restarts on the next request and 3 crashes need Restart, an untrusted `.rade.toml` server command never runs (it runs once trusted), `code = false` answers text matches.

## R84 — Navigation in the Viewer (M28)

**1.0.0 amendment (R101):** R84.2: several results go to a results popover in the Viewer (one popover at a time, as R90), not the Code page, which is removed (R85).

1. **Keys** (the Viewer's own; only for a code or text file, not Markdown): **F12** or **Ctrl+click** on a name → its definition; **Shift+F12** → references; **Alt+Shift+H** → callers (call hierarchy); **Ctrl+F12** → implementations; **Alt+Left** / **Alt+Right** → Back / Forward. The Viewer shows a caret; the name at (or just before) the caret is asked about.
2. A definition or implementation with exactly one result **jumps** there with the caret on the name, while the Viewer still shows the file it was asked from; otherwise the results go to the Code page (R85).
3. **Back and Forward.** A jump puts the caret's place on Back (a jump within one file too) and clears Forward; Back moves the current place to Forward; opening a file the old way (R7) clears Forward. The header shows Back and, when there is one, Forward. Forward is kept in `state.json` like Back.
4. **Ctrl shows links** (0.7.1, as VS Code). While Ctrl is held with the pointer over a name (not a keyword), the name is underlined in the link color and the pointer is a hand; moving shows the name under it, letting Ctrl go or leaving restores both. Found from the text, no server request. Ctrl+click navigates without moving the caret or the selection and never starts a drag-select; Ctrl+click off a name is an ordinary click. The same on the Review Pane's diff lines (R87), where Ctrl+click also leaves the selected rows alone.

AC:
- R84-a: E2E: F12 and Ctrl+click jump to the definition (caret column right after a multi-byte character too), Alt+Left / Alt+Right return and repeat, a same-file jump is a Back step, Shift+F12 / Alt+Shift+H / Ctrl+F12 fill the Code page or jump; F12 on Markdown does nothing.
- R84-b: Live: real F12, Ctrl+click and Alt+Left keys on the desktop.

## R85 — Code page (M28) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show v0.7.1:docs/PRD.md`.

## R86 — `rade code` for Agents (M28)

1. `rade code def|refs|callers|callees|impl <path:line:col>` and `rade code symbols <file>` (`code.query`): a relative path is resolved against the caller's directory; the file must be in a RADE Worktree. Text output: a `# <query> <name> · <summary>` header, the hint as `# …`, then `path:line:col<TAB>name<TAB>line text` relative to the Worktree; `--json` gives the answer (`state`, `source`, `hint`, `items`). `--wait <s>` (default 20) waits for an indexing server before answering with text matches.
1a. `rade code callers` lists every call site (0.7.1), named by the calling function, then `# other references (not calls): n` and those lines; `--depth N` (1–10, default 1) adds the callers of each caller, indented two spaces per level (a recursion is named `(recursive)` and not followed). `--json` adds `calls` (each with `from_ranges` and, past depth 1, `children`) and `others`.
2. `rade code status` lists running servers (root, language, pid, idle); `rade code restart <file>` stops that file's server and forgets its crashes.
3. The `rade` skill tells Agents to use `rade code callers` before changing a function's signature.

AC:
- R86-a: E2E: each query's JSON on the fake server, the exact text output, `NOT_FOUND` for a missing file; one real-server smoke check (rust-analyzer def + callers) when a rust-analyzer binary exists, else skipped with the reason.


## R87 — Navigation in the Review Pane (M29)

**1.0.0 amendment (R101):** R87.3: results fill the results popover of the Review Pane (R84 amendment), not the Code page.

The Viewer's code navigation (R84) on the Review Pane's diff lines (owner decision 4 in research/code-navigation.md; ADR-0030).

1. **Keys and clicks.** A click on a diff line puts the **point** on the character under the pointer; the footer names the word there (`greet · F12 definition · Shift+F12 references · Alt+Shift+H callers`). **F12** or **Ctrl+click** → definition, **Shift+F12** → references, **Alt+Shift+H** → callers, **Ctrl+F12** → implementations, about the word at the point (else the first name of the first selected line). The keys are the Review Pane's own, like the Viewer's.
2. **Sides.** An added or context line asks the Worktree's server at its new line and column (the working tree is what the server indexed). A **removed** line's name is looked up in the current tree: when a line there declares it (text search), the query is asked from that declaration and the hint says `Removed line: <name> looked up in the current tree (<path>:<line>)`; else the answer is `git grep -w` at the base commit, state `base`, labelled `text matches at the base`, the hint naming the base commit.
3. Results fill the Code page (R85) as from a Viewer; a definition with one result, a Code page row and an Impact row open the **Tab's Viewer** at the place (a new one right of the Review when the Tab has none), with the caret on the name; that Viewer's place goes on its Back list (Forward is cleared), so Alt+Left returns through every jump made from the Review.

AC:
- R87-a: E2E on the fake server: F12 on a context line and Ctrl+click on an added line open the Tab's Viewer at the definition (caret column), Alt+Left returns to the first jump; Shift+F12 and Alt+Shift+H fill the Code page; a removed line's name gone from the tree gives text matches at the base (labelled, base commit in the hint), one still in the tree gives the server's answer with the hint.
- R87-b: Live: a real click and F12, a real Ctrl+click on a diff line.

## R88 — Impact (M29)

**1.0.0 amendment (R101):** R88.4: the Batch Reviewer and the Initiative acceptance Agent are removed, so only the Fan-out Judge's prompt gets the `## Impact` section; the Review Pane section and `rade code impact` stand.

A diff's **Impact**: each symbol it changes, adds or removes, with its callers outside the diff — for the owner in the Review Pane, for Agents in `rade code impact` and in Reviewer, Judge and acceptance prompts (owner decision 5; ADR-0030).

1. **What it lists.** The diff is the Worktree's changes against its base (R13.1; Fan-out candidates: their base commit). For each changed file the server's `documentSymbol` ranges that contain an added line, or both lines around a removal (the innermost one that is not a function's local; `added` when the line with its name is new and no removed line declared that name, else `changed`); for each, its callers: incoming calls (each call site, with the calling function) for a function or method, else references — minus places on the diff's added lines and inside the symbol itself — plus whole-word **text matches in test files** the server did not return (import lines, docs and data files are never call sites) (labelled `text match`, `test`). A name declared on a removed line and declared nowhere in the tree now is `removed`, with the text matches still using it. Before asking, RADE sends `workspace/didChangeWatchedFiles` for the diff's files. Without a server (missing, off, unsupported language, still indexing, failed, hung) the symbols come from declarations at or above the changed lines (indented less) and every caller is a text match, all labelled. At most 60 symbols in all, 50 callers each; past the time budget no more callers are looked up (`partial`). An explicit `--base` that does not resolve is an error (`USAGE`).
2. **Review Pane.** An **Impact** toggle in the header (it starts the Worktree's server like any navigation) shows a section under it: `<n> changed · <k> call sites in <f> files (<t> in tests) · <r> removed, still used`, where it came from, then a row per symbol (`~` changed, `+` added, `−` removed; kind, place, caller count) and its callers (place, calling function, line, `test` / `text match` tags). A row opens the Tab's Viewer there (R87.3). While a server indexes it shows text matches, says so and asks again by itself; it is computed again when the diff changes while shown.
3. **`rade code impact [--worktree <name>] [--base <ref>] [--wait <s>] [--json]`** (`code.impact`): the Worktree holding the current directory (or the named one, in the current Source first); default base the Worktree's; `--wait` (default 60) is the whole time budget, after which the rest are text matches. Text output: the summary line, `# <hint>`, then `~ name (kind) path:line — n call sites` and indented `path:line [caller] [tags]: line` lines.
4. **Prompts.** The Batch Reviewer (one section per Source of the Task), the Fan-out Judge (one per candidate, in the candidates' random order) and the Initiative acceptance Agent (the integration branch) get an `## Impact` section (at most 60 lines) appended to their rubric, computed when the prompt is built within **20 s**; a slow, indexing or hung server never holds the review longer (text matches, saying why). No section when nothing changed. Review Comments sent to an Agent get none (the owner chose what to say).
5. **Budget for one hung server.** Queries waiting behind a request that a hung server never answers each give up after their own timeout (M28 acceptance, Low); a server start that failed while another request waited for it is not retried at once by that request.

AC:
- R88-a: Unit tests: touched lines, innermost symbols and `added` vs `changed`, the text fallback's declarations, test files, the summary and the bounded render, Impact against the fake server (callers outside the diff, test text matches, a removed symbol still used, `didChangeWatchedFiles`), an indexing and a hung server within the budget.
- R88-b: E2E: `rade code impact` JSON and exact text, `--worktree`, `--base`; the Review's Impact rows and counts, a row → the Viewer, recomputed on a diff change, indexing → the server's answer by itself; the Reviewer's, Judge's and acceptance Agent's argv contain the section; a hung server: the Reviewer starts within the budget with text matches; screenshots light and dark.

## R89 — Terminals start under load (M30)

1. A new Terminal must not fail because other Terminals (in this or another RADE instance) are open. RADE's Ghostty event loops use epoll, not io_uring: io_uring rings count against the user's shared locked-memory limit (ADR-0031). The app holds no io_uring ring.

AC:
- R89-a: Unit: `without_locked_memory` sets the soft `RLIMIT_MEMLOCK` to 0 only inside (io_uring refused there) and restores it.
- R89-b: E2E: 16 Terminals in two Tabs all start and the app holds no `anon_inode:[io_uring]` fd; m19 passes locally (it lost 4 checks to failed surfaces).

## R90 — Hover and peek (M30)

1. **Hover.** In a code Viewer, **Ctrl+I** at the caret, or **Ctrl** held with the pointer resting 500 ms on a name, shows a popover with the name's signature, type and docs (`textDocument/hover`, Markdown). Without a server (missing, indexing, off, failed) it shows the declaring line as a **text match**, saying why; a pointer hover with nothing to say shows nothing. In the Review Pane, Ctrl+I asks about the point (removed lines as in R87.2).
2. **Peek.** **Alt+F12** shows the definition's lines (2 before, 10 after) in a popover with its place and an **Open** button (a jump, R84.3); several definitions say `1 of n`.
3. One popover at a time; Esc, a click, another navigation key, the Viewer showing another place, the Review scrolling or the pointer moving away (when the pointer opened it) closes it; pressing Ctrl with the pointer already resting on a name also hovers. `rade code hover <path:line:col>` prints the hover (`doc` in JSON).

AC:
- R90-a: Unit: hover contents (MarkupContent, MarkedString, arrays, empty), server and text-match hovers. E2E: Ctrl+I, Ctrl+hover, Esc, a name without information, peek + Open + Back, a text-match hover with the missing-server hint, Ctrl+I / Alt+F12 in the Review; live: a real Ctrl+I and Ctrl+hover with gopls; screenshots light and dark.

## R91 — Outline (M30)

**1.0.0 amendment (R101):** The outline shows in a popover from the Viewer header's Outline button (`Alt+Shift+O`), not on the removed Code page.

1. **Alt+Shift+O** or the Viewer header's **Outline** button lists the file's symbols on the Code page (`Outline of <file>`): the server's `documentSymbol` tree, children indented, each with its kind; a row jumps there. Without a server: the lines that declare something (labelled text matches).

AC:
- R91-a: Unit: the server outline with kinds, the text outline. E2E: key and button, nesting and kinds, a row jumps; not on Markdown.

## R92 — Workspace symbols (M30)

1. In the command palette, **`$`** searches the symbols of the Worktree of the focused Viewer's file, else the Worktree on screen (`workspace/symbol` of the Worktree's running servers, then of each other language found in it at its usual root — a monorepo's package too; rust-analyzer asked for all symbols, gopls for the workspace only), ranked on their names; a row shows its kind and place, Enter opens the Viewer there with the caret on the name. While no server can answer, declarations whose name contains the text, labelled `text matches (indexing)` etc., asked again by itself while indexing.
2. `rade code symbols --query <text> [<path>]` gives the same list (`code.query` `workspace` with `text`).

AC:
- R92-a: Unit: `SymbolInformation` / `WorkspaceSymbol` parsing, text symbols, the `$` filter and symbol items. E2E: `$gree` from the server, Enter opens it, an indexing server's text matches then its answer, `rade code symbols --query`; live: `$Tot` from gopls with real keys.

## R93 — Python and Go servers (M30)

1. `.py .pyi` → `pyright-langserver --stdio`, else `basedpyright-langserver --stdio` (the first one on the Workspace PATH; never the repository's `node_modules`), the root the nearest folder with `pyproject.toml` / `setup.py` / `setup.cfg` / `pyrightconfig.json` / `requirements.txt`, `python.pythonPath` = the root's `.venv/bin/python` when there is one. `.go` → `gopls`, the root the outermost `go.work`, else the nearest `go.mod`. Install hints: `npm i -g pyright (or: pipx install basedpyright)`, `go install golang.org/x/tools/gopls@latest`. `[code.server.python|go]` override them like the others.
2. The text fallback knows `def` and `func` (methods with receivers too).
3. A server that gives no answer to `initialize` in time (30 s) is not started again for 5 minutes (or until Restart): later requests answer with text matches at once, saying why (M29 open item).

AC:
- R93-a: Unit: languages, roots, alternatives, hints, venv, settings; the hung-`initialize` pause. E2E: gopls from PATH answers def and hover, Python's hint then basedpyright; a hung `initialize` costs one wait, then answers at once. Manual: real typescript-language-server (TS 5), pyright and gopls answer def, hover and workspace symbols.

---

## R94 — Session error recovery (M32, P1-01)

**1.0.0 amendment (R101):** R94.2: "Task files" no longer exist for RADE (R101.4: left on disk, never read); the recovery rules stand for `state.json`.

1. An unsuccessful state save displays a persistent, wrapping notice with Retry saving and State location. A successful retry clears the save warning; errors never appear only on stderr.
2. An unreadable/newer-version state is preserved in a uniquely named recovery file before a fresh session can save. Earlier recovery copies survive. Failed backup blocks saves until Retry preserves the original. Repositories and Task files remain separate.
3. Startup explains the fresh session, the recovery path and that RADE must be closed before manual replacement. Recovery information remains after a fresh save. Atomic save and recovery sync files/directories.

AC: corrupt/future JSON, backup collision, permission-denied backup/save and ENOSPC preserve original bytes; UI feedback/Retry are proven in `tests/e2e/product-recovery.sh` and store tests.

## R95 — Guided results and accurate Run choices (M32) — Removed (1.0.0)

Removed in 1.0.0 (ADR-0033, R101). Text: `git show 011febf:docs/PRD.md`.

## R96 — Browser fallback and keyboard annotations (M32)

1. Embedded preview identifies WebKit and offers separate Chromium/default-browser actions for its current/requested URL. Launch progress/errors are visible, packaged CLI lookup works and repeated clicks cannot overlap launches. Existing WebKit annotations stay in their pane.
2. Known unsafe YouTube navigation is refused before loading, including host variants and redirects/links, with an actionable engine fallback. This mitigation does not claim arbitrary WebKit crash isolation.
3. Targets can be selected/commented using the keyboard. Native WebKit picker shortcuts run in the isolated world; page scripts cannot forge annotation commands. Comment controls have accessible names/visible focus; cancel returns to picking.

AC: unsafe hosts versus lookalikes, initial/link/redirect refusal with app survival, keyboard pick/comment/cancel and hostile synthetic events are tested. Compatibility/desktop accessibility evidence remains separate from fake-Agent E2E.

---

## R97 — Source links and `project.toml` v2 (M36)

A **Source link** is Project × Source × exactly one base; Named Bases are gone (ADR-0034). A Project that needs prod is a second Project (`Shop · prod`). Terms: [CONTEXT.md](../../CONTEXT.md).

1. **`<root>/.rade/projects/<id>/project.toml` v2**: `name`, one or more `[[source]]` with `id` (a Source of the Workspace), `base = "<branch>"` (absent = the Source's main branch) and `checkout = "auto" | "main" | "<path>"` (absent = `auto`, R98), plus the layerable keys of R29. A top-level `bases` key or a table-valued `base` marks a v1 file. Files are read on every socket call and polled every 2 s, as before.
2. **Migration from Named Bases.** The first time 1.0 reads a v1 file it rewrites it once: each Source's `base` = its branch for the first Named Base (the default), `bases` is dropped, `project.toml.v1.bak` holds the original bytes (written first, never overwritten), the new file is written atomically (temp + rename) with `toml_edit` keeping comments and other keys. Running it on its own output changes nothing. A Source with no branch for the first Named Base gets no `base` (its main branch) and a warning in `rade project show`. A failed write leaves the original file untouched and names it.
3. **CLI.** `rade project add <name> [--workspace <ws>] [--source <src>[:<branch>]]…` (a `=` in a spec → `USAGE` naming the new form); `rade project show <p>` lists per link its base, checkout kind and path, the other Projects whose links share its checkout (`shared_with`, R98.1) and those that link the same Source at any base (`also_in`), HEAD, ahead/behind against the base, dirty count and drift (R100); `rade project link add <p> <source> [--base <branch>] [--checkout auto|main|<path>]`, `rade project link remove <p> <source>`, `rade project link set-base <p> <source> <branch>` (methods `project.link_add`, `project.link_remove`, `project.link_set_base`) each change only their own lines of the file (`toml_edit`), refuse an unknown Source or Project with `USAGE` leaving the file unchanged, and never create, move or delete a checkout. `rade project set <p> <key> [<value>]` keeps the keys `name`, `source.<id>.base` and `source.<id>.checkout`; its v1 keys (`bases`, `source.<id>.base.<named base>`, `task.*`, `batch.*`) → `USAGE`.

AC:
- R97-a [M36] (unit): Given a v2 `project.toml` with `name = "Shop"` and `[[source]]` entries `shop-web`, `shop-api`, `shared-ui`, each `base = "develop"` and no `checkout`. When `rade-core` parses it. Then it yields 3 links with base `develop` and checkout `auto`, and 0 warnings.
- R97-b [M36] (unit): Given a v1 file with `bases = ["dev", "prod"]`, `shared-ui` at `base = { dev = "develop", prod = "release/2.4" }` and a comment line. When the migration runs. Then the file holds `base = "develop"` for `shared-ui`, has no `bases` key, keeps the comment, and `project.toml.v1.bak` holds the original bytes.
- R97-c [M36] (unit): Given the file migrated in R97-b. When the migration runs a second time. Then the file is byte-identical and `project.toml.v1.bak` keeps its bytes and mtime.
- R97-d [M36] (unit): Given a v1 file `bases = ["dev", "prod"]` where `billing` has a branch only for `prod`. When the migration runs. Then `billing` has no `base` key and `project.show` carries exactly 1 warning naming `billing` and the Named Base `dev`.
- R97-e [M36] (unit): Given a v1 file in a read-only folder. When the migration runs. Then the original `project.toml` bytes are unchanged, no `.v1.bak` or temp file is left, and the error names the file.
- R97-f [M36] (e2e): Given Workspace `acme` with Sources `shop-web`, `shop-api`, `shared-ui`. When `rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop` runs twice. Then the first run writes `.rade/projects/shop/project.toml` with 3 `[[source]]` entries and the second exits 0 with `existing: true` and the file byte-identical.
- R97-g [M36] (e2e): Given the same Workspace. When `rade project add Shop --source shop-web:dev=develop` runs. Then it exits 2, stderr names the form `--source <src>[:<branch>]`, and no file is written.
- R97-h [M36] (e2e): Given Project Shop with 3 links. When `rade project link add shop billing --base main`, then `rade project link set-base shop shared-ui release/2.5`, then `rade project link remove shop billing` run. Then each run changes only the lines of that link (byte diff), the final file equals the first except `shared-ui`'s `base` line, `link add shop nope` exits 2 with the file unchanged, and no folder under the Workspace's worktrees folder (`.worktrees-dev/`: E2E runs the Devel build, R98.1) is created or removed.

## R98 — Project checkouts (M36)

Which checkout a link uses, decided by one pure function and never by moving a checkout under the user (ADR-0034 decisions 3–4).

1. **Rule** (`rade-core/src/checkout.rs`, `resolve_checkout`). With `checkout = "auto"` a link uses the Source's **main checkout** when it is on the link's base and no other link of the Workspace needs the main checkout at a different base; otherwise a **Project checkout**: a durable git worktree `<root>/.worktrees/.projects/<project>/<source>` on the base (`.worktrees-dev/.projects/…` for a dev build, ADR-0010; `<data dir>/worktrees/.projects/<project>/<source>` in the `default` Workspace). Source ids are slugs (`[a-z0-9-]`), so the `.projects` folder never meets an ordinary Worktree's `.worktrees/<source>/<name>` (R12.1, R23.8), even when a Project and a Source have the same id (`billing`); a folder already at a new Project checkout's path that is not that Source's worktree on the base refuses the plan with `USAGE` naming the folder, nothing created. Project checkouts are never listed or imported as Worktrees (`worktree.list` skips the `.projects` folder and every recorded checkout). Links with the same Source and the same base share one checkout, labelled **shared** with the other Projects (`shared_with`). **Also in** (`also_in`) is a different list: every other Project that links the same Source, at any base, sharing the checkout or not, as `<Project> @ <base>`; it drives the Navigator's link marker (R104.2) and Project home's chip (R105.3). `checkout = "main"` forces the main checkout (refused with `USAGE` naming the other link when one holds it at another base); when the main checkout is on another branch (or detached) and the link has no record, it is never switched, clean or dirty: the plan is refused with `USAGE` naming the branch and the base and holding 0 commands (with uncommitted changes it names them too, R98-f), the link stays unrecorded with that warning in `rade project show`, and it resolves once the user puts the checkout on the base (a recorded main checkout that later leaves the base is Base drift, as below). `checkout = "<path>"` uses that existing checkout of the Source when it is on the base, else `USAGE` naming the path and the base. A Source folder that is not a git checkout refuses only that link's plan, with `USAGE` `<folder>: not a git repository` and 0 commands; the Workspace's other links still resolve. Checkouts are compared by `realpath`: a `checkout = "<path>"`, a recorded path or a Source folder that is a symlink to another checkout is that same checkout (kind and sharing as if the real path had been given), never a second checkout of the base and never created twice.
   **Chosen once.** The checkout a link resolves to is recorded in `state.json` (`link_checkouts` of its Workspace: project, source, base, the `checkout` key, path) the first time RADE resolves the link (it is added, migrated or first read) and kept: `resolve_checkout` takes the records as input (it stays pure) and returns a link's recorded checkout whatever that checkout's HEAD is — a HEAD off the base is Base drift (R100), never a new checkout. A link is resolved afresh only when its `base` or `checkout` key changes, when it is removed (its record is dropped), or when its recorded checkout no longer exists (folder gone or not in `git worktree list`). A recorded checkout that is off its base is never shared with another link: that link gets a Project checkout (spec C.19).
2. **Never touch a dirty checkout.** No plan contains `git switch`, `stash`, `reset` or `checkout` on a checkout with uncommitted changes; a plan that would need one is refused, saying what to do.
3. **Created on use, shown first.** A Project checkout is created only when the user opens a Terminal, an Agent or a Worktree on the link (with a notice naming the folder), never on start or on a file change. `rade project plan <name> --source <src>[:<branch>]…` (method `project.plan`) prints the plan without writing anything; the New Project dialog shows the same plan (R106).
4. **Invariants**, tested on generated Workspaces (with and without records): two links with different bases never share a checkout; links with the same Source and base never get two different checkouts that are both on the base (they share one); no command touches a dirty checkout; resolving twice gives the same plan; a recorded checkout that still exists is kept while its link's `base` and `checkout` keys are unchanged.

AC:
- R98-a [M36] (unit): Given `shared-ui` linked by Shop (base `develop`) and Billing (base `release/2.4`) with its main checkout on `main`. When `resolve_checkout` runs. Then Shop's link gets the Project checkout `<root>/.worktrees/.projects/shop/shared-ui` on `develop`, Billing's gets `<root>/.worktrees/.projects/billing/shared-ui` on `release/2.4`, and the plan has 0 commands on the main checkout.
- R98-b [M36] (unit): Given `shop-web` linked only by Shop (base `develop`) with its main checkout on `develop`. When `resolve_checkout` runs. Then the link uses the main checkout and the plan creates nothing.
- R98-c [M36] (unit): Given Shop and `Checkout revamp` both linking `shared-ui` at base `develop` with its main checkout on `main`. When `resolve_checkout` runs. Then both links get the same single checkout path and each has `shared_with` = the other Project.
- R98-d [M36] (unit): Given 500 generated Workspaces (random links, bases, HEADs and dirty flags, fixed seed). When `resolve_checkout` runs twice on each. Then there are 0 violations of the 5 invariants of R98.4.
- R98-e [M36] (unit): Given Shop's `shop-api` with `checkout = "main"` while Billing's link holds that main checkout at `release/2.4`. When `resolve_checkout` runs. Then the plan is refused with `USAGE` naming Billing and `release/2.4`.
- R98-f [M36] (unit): Given `shop-api`'s main checkout on `feat/checkout-v2` with 3 uncommitted changes and Shop's link at `checkout = "main"`, base `develop`, with no recorded checkout. When `resolve_checkout` runs. Then the plan is refused naming the 3 uncommitted changes, and it contains 0 git commands.
- R98-g [M36] (unit): Given a plan with one new Project checkout for Shop's `shared-ui` in a temp repository whose `develop` exists only on the remote. When the plan is applied. Then `<root>/.worktrees/.projects/shop/shared-ui` exists on `develop` tracking the remote branch, and the main checkout's HEAD and status are unchanged.
- R98-h [M36] (e2e): Given real repositories for `shop-web` (main checkout on `develop`) and `shared-ui` (main checkout on `main`, a bare remote with `develop`). When `rade project plan Shop --source shop-web:develop --source shared-ui:develop` runs. Then it prints `shop-web  main checkout` and `shared-ui  new project checkout .worktrees-dev/.projects/shop/shared-ui` (the Devel build's folder, R98.1), exits 0, and `git worktree list` and the `.rade/projects` folder are unchanged.
- R98-i [M36] (unit): Given Shop's `shop-api` link (base `develop`, `checkout = "auto"`) recorded on its main checkout while that was on `develop`, and the main checkout now on `feat/checkout-v2` with 3 uncommitted changes. When `resolve_checkout` runs with the records. Then the link still resolves to the main checkout, the plan has 0 commands, and `drift()` reports head `feat/checkout-v2`, base `develop`, dirty 3.
- R98-j [M36] (unit): Given the records of R98-i. When `resolve_checkout` runs for a new link of `Checkout revamp` holding `shop-api` at base `develop`, then for Shop's link after its base is set to `release/2.5`. Then the new link gets `<root>/.worktrees/.projects/checkout-revamp/shop-api` on `develop` (not the drifted main checkout), and Shop's link drops its record and gets `<root>/.worktrees/.projects/shop/shop-api` on `release/2.5`.
- R98-k [M36] (e2e): Given `rade project add Shop --source shop-api:develop` run while `shop-api`'s main checkout is on `develop`. When `git switch -c feat/checkout-v2` runs in that checkout, the app quits and starts again, and `rade project show shop --json` runs. Then the link has `"checkout_kind": "main"`, `path` = the main checkout, `drift.head` = `feat/checkout-v2`, and no folder exists under `.worktrees-dev/.projects/shop/` (the Devel build's folder).
- R98-l [M36] (unit): Given `shared-ui` linked by Shop (base `develop`), Billing (base `release/2.4`) and `Checkout revamp` (base `develop`) with its main checkout on `main`. When `resolve_checkout` runs and the links' labels are computed. Then Shop's link has `shared_with` = [`checkout-revamp` @ `develop`] and `also_in` = [`billing` @ `release/2.4`, `checkout-revamp` @ `develop`], Billing's has an empty `shared_with`, `also_in` = [`checkout-revamp` @ `develop`, `shop` @ `develop`] and a checkout path different from Shop's.
- R98-m [M36] (unit): Given Workspace `acme` at `<root>` with Project `billing` linking Source `shared-ui` (base `release/2.4`, main checkout on `main`) and a Source `billing`. When the Project checkout path of that link and the Worktree path of `rade worktree add billing shared-ui` are computed in the Installed and the Devel flavor, then a plan is computed with a plain folder already at the Project checkout's path. Then they are `<root>/.worktrees/.projects/billing/shared-ui` and `<root>/.worktrees/billing/shared-ui` (Installed), `<root>/.worktrees-dev/.projects/billing/shared-ui` and `<root>/.worktrees-dev/billing/shared-ui` (Devel), and the plan is refused with `USAGE` naming that folder and 0 git commands.
- R98-n [M36] (e2e): Given real repositories with Project `billing` linking `shared-ui` at `release/2.4` (main checkout on `main`) and a Source `billing`. When a Terminal is opened on Billing's `shared-ui` link, then `rade worktree add billing shared-ui` runs, then `worktree.list` runs for `shared-ui` and for `billing`. Then `.worktrees-dev/.projects/billing/shared-ui` (on `release/2.4`) and `.worktrees-dev/billing/shared-ui` both exist, both steps succeed, and neither list has a Worktree whose path is the Project checkout.
- R98-o [M36] (unit): Given `shop-api`'s clean main checkout on `feat/checkout-v2`, no other link holding it, and Shop's link `checkout = "main"`, base `develop`, with no record. When `resolve_checkout` runs, then runs again with the checkout on `develop`. Then the first plan is refused with `USAGE` naming `feat/checkout-v2` and `develop`, holds 0 commands and records nothing; the second resolves to the main checkout with 0 commands and records it.
- R98-p [M36] (unit): Given Shop linking `shop-web` (main checkout on `develop`) and `notes`, whose folder is not a git repository. When `resolve_checkout` runs. Then `notes`' plan is refused with `USAGE` naming its folder and `not a git repository` with 0 commands and no record, and `shop-web` still resolves to its main checkout.
- R98-q [M36] (unit): Given `shop-web`'s main checkout `<root>/shop-web` on `develop`, a symlink `~/src/shop-web` to it, Shop's link `checkout = "~/src/shop-web"` and `Checkout revamp`'s link `checkout = "auto"`, both at base `develop`. When `resolve_checkout` runs twice. Then both links resolve to the one checkout `<root>/shop-web` (the canonical path), each has `shared_with` = the other Project, both plans have 0 commands, and the second run gives the same plan.

## R99 — Worktrees belong to a link (M36)

1. **`rade worktree add <source> <name> [--project <p>] [--base <ref>]`**: without `--project` RADE takes the only link holding the Source, else answers `USAGE` listing the Projects; without `--base` it cuts from the link's base. The Worktree records `project` and `base`. A Source in no Project keeps the old rule (its main branch) and shows under Scratch (R104).
2. **Compared against the Worktree's recorded base.** Review (R13), Changes (R21), the Worktree's `+/−` counts and Create PR's default base use `merge-base(<recorded base>, HEAD)`. The recorded base is the link's base unless `--base` named another ref, and changing the link's base (`project link set-base`, Change base…, R106.2) never rewrites it. A Worktree with no recorded base uses its link's base, else the Source's main branch; a link's own checkout compares against the link's base. Cutting from the link's base by default is what fixes N2, N3 of the design.

AC:
- R99-a [M36] (e2e): Given `shop-web` linked only by Shop (base `develop`). When `rade worktree add shop-web checkout-v2` runs. Then the new branch `checkout-v2` points at `develop`'s commit and `worktree.list` shows `"project": "shop", "base": "develop"` for it.
- R99-b [M36] (e2e): Given `shared-ui` linked by Shop (base `develop`) and Billing (base `release/2.4`). When `rade worktree add shared-ui tokens` runs without `--project`, then with `--project billing`. Then the first exits 2 listing `shop` and `billing` and creates nothing; the second starts `tokens` at `release/2.4`.
- R99-c [M36] (e2e): Given Worktree `checkout-v2` of Shop/shop-web with 4 changed files of its own and `develop` 2 commits ahead of `main`. When `rade review open` and `debug.review info` run. Then the Review lists exactly the 4 files of the Worktree and none of the files of `develop`'s 2 commits.
- R99-d [M36] (unit): Given the Worktree of R99-c with `+162 −58` against `develop`. When its row counts and the Create PR default base are computed. Then the counts are `+162 −58` and the base is `develop`.
- R99-e [M36] (e2e): Given Worktree `checkout-v2` added on `shop-web` with `--project shop`. When `debug.ui sidebar` runs. Then its row is under Shop and no row of it is in a `No Project` or Scratch group.
- R99-f [M36] (e2e): Given Shop's link `shop-web` at base `develop`. When `rade worktree add shop-web hotfix --base main` runs. Then `hotfix` starts at `main` and records base `main` with project `shop`.
- R99-g [M36] (e2e): Given Worktree `hotfix` of R99-f (recorded base `main`) with 1 commit of its own changing 2 files, and `develop` 2 commits ahead of `main`. When `rade review open` and `debug.review info` run in `hotfix`. Then the Review lists exactly those 2 files and none of the files of `develop`'s 2 commits.
- R99-h [M36] (unit): Given Worktrees `hotfix` (link base `develop`, recorded base `main`), `tokens` (link base now `release/2.5`, recorded base `develop`) and `legacy` (link base `develop`, no recorded base). When the comparison base is computed for the Review, the `+/−` counts and the Create PR default base. Then it is `main`, `develop` and `develop` respectively, the same for all three uses.
- R99-i [M36] (e2e): Given Worktree `tokens` of Shop's `shared-ui` cut at `develop` with 3 changed files of its own. When `rade project link set-base shop shared-ui release/2.5` runs, then `worktree.list` and `debug.review info` for `tokens`. Then `tokens` still records base `develop` and its Review lists exactly the 3 files and none of `release/2.5`'s.

## R100 — Base drift (M36, M37)

1. **Detection** (`rade-core/src/drift.rs`). A link's recorded checkout (R98.1) whose HEAD is not the link's base (another branch, or a detached HEAD) is drifted; refreshed off the UI thread when the checkout's `HEAD` changes and at most every 3 s while shown.
2. **Actions.** **Back to base** (only when the checkout is clean: `git switch <base>`; `rade project link to-base <p> <source>`, method `project.link_to_base`, refuses with `USAGE` when dirty, and when the base is checked out in another checkout, naming that folder), **Keep** (hides the warning until the checkout's HEAD changes again), **Move to Worktree…** (when clean: switch the checkout back to base, then `git worktree add` of the drifted branch on the link, one confirmation naming both steps; when dirty: the dialog says the changes must be committed first and runs nothing). Button labels follow spec A.68 and B (`Keep, show in Shop`, `Move to a Worktree…`).
3. **Where it shows** (M37): the `warning-diamond` marker on the Navigator link row (R104), a chip on Project home (R105) and a card in Changes (R107); `rade project show` prints it (M36).

AC:
- R100-a [M36] (unit): Given Shop's link `shop-api` (base `develop`) whose clean checkout is on `feat/checkout-v2`. When `drift()` runs. Then it reports head `feat/checkout-v2`, base `develop` and exactly the 3 actions Back to base, Keep, Move to Worktree.
- R100-b [M36] (unit): Given the same checkout with 3 uncommitted changes. When `drift()` runs. Then it reports dirty 3 and exactly 2 actions, Keep and Move to Worktree, without Back to base.
- R100-c [M36] (unit): Given a checkout on `develop` and another detached at `develop`'s commit. When `drift()` runs for a link with base `develop`. Then the first is not drifted (0 actions) and the detached one is drifted with its short commit as head.
- R100-d [M36] (e2e): Given Shop's `shop-api` link recorded on its main checkout (R98.1, resolved while it was on `develop`), now on `feat/checkout-v2` with 3 uncommitted changes. When `rade project link to-base shop shop-api` runs. Then it exits 2 naming the 3 changes, HEAD is still `feat/checkout-v2`, and `git stash list` is empty.
- R100-e [M36] (e2e): Given the same checkout clean. When `rade project link to-base shop shop-api` runs. Then it exits 0, HEAD is `develop`, and branch `feat/checkout-v2` still points at its old commit.
- R100-f [M36] (e2e): Given the drifted dirty `shop-api` of R100-d. When `rade project show shop --json` runs. Then the `shop-api` link has `"drift": {"head": "feat/checkout-v2", "base": "develop", "dirty": 3}` and the text output marks the line `drift`.
- R100-g [M37] (e2e): Given the drifted dirty `shop-api` in scene A. When the window shows Shop. Then the Navigator row has the `warning-diamond` marker, Project home the chip `off base · 3 changes`, and Changes the card text `shop-api is on feat/checkout-v2, Shop's base is develop. 3 uncommitted changes, so RADE won't switch it.` with buttons `Keep, show in Shop` and `Move to a Worktree…` only.
- R100-h [M37] (e2e): Given the drift card of R100-g. When Keep is clicked, then `git switch -c other` runs in the checkout. Then the three markers hide after the click and all three show again within 3 s of the switch.
- R100-i [M37] (e2e): Given `shop-api` clean on `feat/checkout-v2`. When Move to a Worktree… is confirmed. Then the checkout is on `develop`, a Worktree of branch `feat/checkout-v2` exists under Shop's `shop-api` link, and the drift markers are gone.

## R101 — Removed in 1.0.0 (M34)

Tasks, Batches, Initiatives, Reports and their engines are removed, not hidden (ADR-0033). The R-IDs of R33–R70, R73–R81, R85 and R95 keep their headings, marked removed.

1. **What goes.** Task files, Task Runs, Task Detail, Board, Brainstorm, Definition of Ready, `depends`, Agent Profiles, Batches, Initiatives, the Plan gate, Verified Runs and the verify-command confirmation, the Event Journal, Digest, Reports, Report Styles, Slack / DingTalk export, Flow metrics, Lessons, Start here, the Code page and the Named Bases UI; skills `rade-task`, `rade-batch`, `rade-initiative`, `rade-design`, `rade-report` (`rade setup` removes the copies it installed).
2. **Removed CLI commands** `rade task|batch|initiative|digest|report|flow|lessons` exit 2 with stderr `rade: '<cmd>' was removed in 1.0.0 (ADR-0033)`. **Removed socket methods** (`task.*`, `batch.*`, `initiative.*`, `digest.*`, `report.*`, `flow.get`, `lessons.draft`, `plan.*`, the verify confirmation methods, `debug.batch`, `debug.initiative`, `debug.reports`, the Task / Batch / Report ops of `debug.ui`) answer `{"code": 2, "message": "'<method>' was removed in RADE 1.0.0 (ADR-0033)", "data": {"kind": "USAGE", "removed": "1.0.0"}}`.
3. **`state.json` 2 → 3.** The first 1.0 start writes `state.json.v2.bak` once (never overwritten), drops `plan_gates`, `tasks_page`, `Tab.task`, Batch / Initiative / Report fields and `start_here_dismissed`, and keeps every Workspace, Source, Worktree, Tab and Pane. Pane env loses `RADE_TASK`, `RADE_TASK_FILE`, `RADE_TASK_WORKTREES`, `RADE_INITIATIVE`, `RADE_INITIATIVE_DIR`.
4. **Files on disk are left alone**: `tasks/`, `batches/`, `initiatives/`, `reports/`, `digests/`, `.rade/events/` and verify logs are never deleted, moved, rewritten or read. Layered-config keys `task`, `profiles`, `batch` and `report` are reported `ignored (removed in 1.0.0)`, never an error.
5. **`scripts/check-removed.sh`** greps code, CLI help, skills and CSS for the removed words with a written list of exceptions (CHANGELOG, old ADRs, old checks) and runs in CI.

AC:
- R101-a [M34] (e2e): Given a running 1.0 app. When each of `rade task list`, `rade batch status`, `rade initiative list`, `rade digest`, `rade report list`, `rade flow`, `rade lessons` runs. Then all 7 exit 2 with stderr `rade: '<cmd>' was removed in 1.0.0 (ADR-0033)` naming their own command.
- R101-b [M34] (unit+e2e): Given the socket of a test-mode app. When `task.list`, `batch.status`, `initiative.list`, `digest.get`, `report.list`, `flow.get`, `lessons.draft` and `debug.batch` are called, and then the unknown `foo.bar`. Then the 8 removed methods answer code 2 with message `'<method>' was removed in RADE 1.0.0 (ADR-0033)` and `data` `{"kind": "USAGE", "removed": "1.0.0"}`, and `foo.bar` answers code 2 without `removed`.
- R101-c [M34] (unit): Given a 0.7.1 `state.json` fixture (version 2) with 2 Workspaces, 4 Sources, 3 Worktrees, 5 Tabs (one a Task Run), 9 Panes, `plan_gates`, `tasks_page`, Batches, Initiatives and `start_here_dismissed`. When 1.0 loads and saves it. Then the saved file has version 3, the same 2 / 4 / 3 / 5 / 9 ids, none of the dropped keys, and `state.json.v2.bak` equals the original bytes.
- R101-d [M34] (unit): Given an existing `state.json.v2.bak` and another version 2 `state.json`. When 1.0 loads it, and later loads a version 3 file. Then the backup keeps its first bytes and the version 3 load writes no backup.
- R101-e [M34] (e2e): Given a Workspace with `.rade/projects/shop/tasks/T-1-otp.md`, `batches/B-1-x.md` and `.rade/events/2026-10-20.jsonl`. When 1.0 starts, spawns an Agent, adds a Worktree and quits. Then each of the 3 files has its original bytes and mtime.
- R101-f [M34] (e2e): Given the 1.0 tree. When `scripts/check-removed.sh` runs, then runs again on a copy where `skills/rade/SKILL.md` mentions `rade task add`. Then the first exits 0 and the second exits 1 naming `skills/rade/SKILL.md` and the line.
- R101-g [M34] (e2e): Given a 1.0 app restored from the R101-c fixture. When `env` runs in the restored Task Run Tab's Pane. Then none of `RADE_TASK`, `RADE_TASK_FILE`, `RADE_TASK_WORKTREES`, `RADE_INITIATIVE`, `RADE_INITIATIVE_DIR` is set and the Pane's Tab still exists.
- R101-h [M34] (unit): Given `[task] verify = "make test"` and `[batch] parallel = 2` in Shop's `project.toml`. When `config.explain` runs for Shop. Then both keys are listed `ignored (removed in 1.0.0)` and the call succeeds with 0 errors.
- R101-i [M34] (e2e): Given a temp HOME where 0.7.1's `rade setup` installed the `rade`, `rade-config`, `rade-task`, `rade-batch`, `rade-initiative`, `rade-design` and `rade-report` skills. When 1.0's `rade setup --agent claude` runs. Then the 5 removed skill folders are gone and `rade`, `rade-workspace`, `rade-config` are installed.
- R101-j [M34] (e2e): Given the app restored from the R101-c fixture. When `debug.palette` runs for `>task`, `>batch`, `>report` and `debug.menu` lists the Tab and Project menus. Then the palette gives 0 results for each and no menu item contains Task, Batch, Initiative, Report or Start here.

## R102 — Visual system (M35)

Terminal-native, dark-first (ADR-0036; spec [README](design/spec/README.md)).

1. **One source of tokens.** `crates/rade-app/src/tokens.toml` (dark and light) → `build.rs` generates the GTK `tokens.css` (`@define-color`) and the mockup's `docs/internals/design/redesign-2026-10-23/tokens.css`. No raw hex color outside `tokens.toml` (a CI check). GTK has no `color-mix()`: mixed colors are precomputed tokens.
2. **Contrast from tokens.** `docs/internals/design/contrast.py` reads `tokens.toml`: every text pair ≥ 4.5:1 and every status glyph / LED pair ≥ 3:1 in both themes.
3. **Fonts.** Inter (UI, 12.5–13 px) and JetBrains Mono (metadata, `tnum`) are bundled in the app's GResource and listed in `THIRD_PARTY.md` (OFL-1.1).
4. **Icons.** Phosphor Regular / Fill (MIT) as `ph-<name>-symbolic.svg` in the GResource; every icon name used in code exists there. The status LED is CSS, never an icon.
5. **`debug.ui {"op": "geometry", "widget": <name>, "nth"?}`** (hidden, test mode, ADR-0021) returns a widget's allocation, margins, padding, font (family, size, weight, features), computed color and background, CSS classes, label and opacity, for spec checks.
6. **Themes.** Dark is the default; light follows the system style live. Terminals take RADE's look over the user's Ghostty config (ADR-0037): the `term-0`…`term-15` palette, `term-fg` on `bg0`, an `acc` cursor, a `term-sel` selection, opaque, no Ghostty padding (the Pane's margin, A.51), the `[terminal]` font of Settings (R112; default Maple Mono Medium 14 px, bundled, line height 1.15). Keybinds, scrollback and other Ghostty keys stay the user's. `[terminal] theme = "ghostty"` (config.toml only) keeps the Ghostty config's own look; R20.8 then applies.

AC:
- R102-a [M35] (unit): Given `tokens.toml`. When `build.rs` runs. Then the generated `tokens.css` has one `@define-color` per token per theme, its token names equal the Tokens table of `docs/internals/design/spec/README.md` (same count, 0 missing), and the mockup `tokens.css` has the same values.
- R102-b [M35] (unit): Given the source tree. When the raw-hex check scans `crates/rade-app/src/**/*.css` and `*.rs`. Then it finds 0 `#rgb` / `#rrggbb` colors outside `tokens.toml` and the generated file, and a fixture with `color: #4fe3c8;` in `style.css` fails naming the line.
- R102-c [M35] (unit): Given `tokens.toml`. When `python3 docs/internals/design/contrast.py` runs. Then every text pair is ≥ 4.5:1 and every glyph / LED pair ≥ 3:1 in both themes (exit 0), and a fixture with dark `fg3 = #5e6773` on `bg3` exits 1 naming that pair.
- R102-d [M35] (e2e): Given a test-mode app with a fontconfig that has neither Inter nor JetBrains Mono installed. When `debug.ui geometry` reads a Navigator row title and the breadcrumb branch. Then their font families are `Inter` and `JetBrains Mono`, the latter with the `tnum` feature.
- R102-e [M35] (unit): Given every `ph-…` icon name used under `crates/rade-app/src`. When the GResource is listed. Then 0 names are missing and every bundled SVG uses `fill="currentColor"` with no stroke.
- R102-f [M35] (e2e): Given a test-mode app showing scene A. When `debug.ui {"op": "geometry", "widget": "statusline"}` runs, and the same call goes to an app started without `RADE_DEBUG_SOCKET=1`. Then the first returns allocation, margins, font, color, background and classes with height 26 ± 1 px, and the second exits 2 with the ADR-0021 hint.
- R102-g [M35] (e2e): Given the app in dark. When `debug.action color_scheme_light` runs and then `color_scheme_dark`. Then the window background reads the light `bg1` token then the dark `bg1` token, and a Terminal's background equals `term-bg` of each theme, without a restart.
- R102-h [M35] (unit): Given `THIRD_PARTY.md` and the GResource manifest. When the license check runs. Then Inter (OFL-1.1), JetBrains Mono (OFL-1.1) and Phosphor Icons (MIT) are listed and both font files are in the manifest.
- R102-i [M35] (unit): Given `tokens.toml`. When RADE writes its Terminal theme files and parses `[terminal]` / `[appearance]`. Then each theme file has 16 `palette` lines equal to `term-0`…`term-15` and the `bg0` / `term-fg` / `acc` colors; the defaults are `theme = "rade"`, Maple Mono Medium 14 px, line height 1.15, ligatures on, `color_scheme = "system"`; `theme = "ghostty"` parses and an unknown theme is an error; `fonts.conf` includes the system config (or the user's `FONTCONFIG_FILE`) and adds RADE's fonts folder.

## R103 — Window chrome (M35)

Layout of design §4 and spec [a-workspace.md](design/spec/a-workspace.md).

1. **Titlebar** (40 px): Workspace button (mark, name, identity in the tooltip), a left-aligned breadcrumb `Project / Source / Worktree ← base ↑n` (no centred title), the command field in the middle (a click opens the palette; `Ctrl+Shift+P` opens it from anywhere, `Ctrl+K` only from RADE's own chrome, never from a Terminal: R17 amendment), then `+` (new, R108), split, usage (R110), Review with the count of files Review would open (the focused Worktree's or link checkout's changed files against its comparison base, R99.2; not the Project total that the Inspector's Changes tab counts), and the Inspector toggle.
2. **Flat tabs**: a 2 px accent bar on top of the open Tab, the Tab's status LED, its name = the name the user gave, else the Agent's name, else the Worktree's; never `Tab <n>`.
3. **Pane header** (26 px, mono): `name · agent · model ··· status`; the focused Pane has a 1 px accent border and the `glow`; no colored top edge; no Attention Ring (ADR-0036: a waiting Pane shows `needs input` in its header and the LED in the Navigator, so the focus ring is the only lit border); 6 px between Panes, 10–12 px terminal padding.
4. **Inspector**: text tabs Changes / Files / Search and an Activity icon tab; `Ctrl+Shift+E` hides and shows it from anywhere, `Ctrl+B` only from RADE's own chrome (R17 amendment).
5. **Statusline** (26 px, tmux-like): Workspace (accent cell), Project, Worktree with `↑ahead ↓behind`, Agent counts, ports, usage (R110), identity (`user.email` of the Workspace), key hints; `status_bar = false` hides it.
6. **Empty states**: one mono line plus key hints, never a warning icon. **One primary button** (`.suggested-action`) per screen, counted on the top surface. **A Toast never covers an action**: it moves up above any visible button it would overlap.

AC:
- R103-a [M35] (e2e): Given scene A at 1440 × 900 in dark and light. When `debug.ui geometry` measures the titlebar, tabs, Panes, Inspector and Statusline. Then rows A.2–A.16, A.39–A.57, A.70–A.73 and A.75–A.79 of the Measurements table of `docs/internals/design/spec/a-workspace.md` are within their tolerance.
- R103-b [M35] (e2e): Given focus in Worktree `checkout-v2` of Shop's `shop-web` (base `develop`, 3 commits ahead). When `debug.ui chrome` runs. Then the breadcrumb is `Shop / shop-web / checkout-v2 ← develop ↑3` and the Review button shows `4` (`checkout-v2`'s 4 changed files, not the Project's 7).
- R103-c [M35] (e2e): Given the keyboard in a Terminal running `cat -v`. When the command field is clicked, then `Esc`, then `Ctrl+Shift+P`, then `Esc`, then `Ctrl+K`, then focus moves to the Navigator and `Ctrl+K` is pressed. Then the palette opens 3 times (click, `Ctrl+Shift+P`, `Ctrl+K` from the Navigator), after the first 2 `Esc` typing reaches the same Terminal, and the Terminal's `Ctrl+K` prints `^K` without opening the palette.
- R103-d [M35] (e2e): Given a new shell Tab with no title in `checkout-v2`, and a Tab renamed `agents` holding Agents `planner` (done) and `tests` (waiting). When `debug.tabs` runs. Then the labels are `checkout-v2` and `agents` with count `2` and the waiting LED (since R115: the lead `planner`'s glyph in the waiting colour), and no label matches `Tab [0-9]+`.
- R103-e [M35] (e2e): Given Panes `planner` and `tests` with `tests` focused. When `debug.ui geometry` reads both Panes. Then only `tests` has class `focused`, a 1 px `acc` border and the `glow` shadow, and its header text is `tests · codex · gpt-5 · needs input`.
- R103-f [M35] (e2e): Given scene A, then a clean Worktree. When the visible `.suggested-action` widgets of the top surface are counted. Then each count is exactly 1 (`Commit` on A) or 0 when nothing can be committed.
- R103-g [M35] (e2e): Given a Review Pane whose Comment / Send to Agent buttons are on screen and an Agent in a hidden Tab. When the Agent starts `waiting` and its Toast shows. Then the Toast's bounds intersect 0 visible buttons.
- R103-h [M35] (e2e): Given a clean Worktree. When the Inspector shows Changes, then Files of an empty folder, then Search with no query. Then each shows one mono line with key hints and none shows a `dialog-warning` or StatusPage icon.
- R103-i [M35] (e2e): Given the Inspector shown and focus on the Navigator. When `Ctrl+B` is pressed twice, then focus moves to a Terminal running `cat -v` and `Ctrl+Shift+E` is pressed twice, then `Ctrl+B`. Then the Inspector hides and shows on each pair, its tabs read `Changes 7`, `Files`, `Search` plus the Activity icon tab, the center widens by 340 ± 1 px while hidden, and the Terminal's `Ctrl+B` prints `^B` with the Inspector still shown.
- R103-j [M35] (e2e): Given scene A. When `debug.ui chrome` reads the Statusline. Then its cells are, in order, `ACME`, `Shop`, `checkout-v2 ↑3 ↓0`, `1 waiting · 1 running · 2 done`, `:5173`, `richard@acme.io` and the key hints, and with `status_bar = false` it is hidden.
- R103-k [M35] (e2e): Given Panes `planner` and `tests` in one Tab with `tests` focused. When the keyboard focus moves into `planner` the way a click does, then back into `tests`. Then each time only the Pane with the keyboard has class `focused` (border and glow) and is the Tab's focused Pane, and the other is dimmed.

**M51 amendment (R103.2):** an Agent Tab's name is its lead Agent Pane's label (the Fan-out grid's owner, else the first Agent Pane, as R115.3): the name the user gave the Pane, else its session title (R104.7) cut to 24 characters with `…`, else its Preset — so Tabs of several Claude sessions no longer all read `claude`. A Tab rename still wins; the tooltip keeps the whole title (R1); the Agent count and glyphs are unchanged.

- R103-l [M51] (unit): Given an Agent Pane of Preset `claude`. When its title is `Tính năng projects trên SCRATCH`, `Fix login`, `Claude Code`, none, or 40 characters, and when the Pane is named `planner` or the Tab renamed `api`. Then the Tab label is `Tính năng projects trên…`, `Fix login`, `claude`, `claude`, 23 characters and `…`, `planner`, `api`.
- R103-m [M51] (e2e): Given two Agent Tabs whose Agents set OSC 2 titles `✳ Fix login` and `Tính năng projects trên SCRATCH`. When `debug.tabs` reads the strip. Then their labels are `Fix login` and `Tính năng projects trên…`, the second's tooltip has the whole title, and before any title the label is the Preset.

## R104 — Navigator (M37)

The left column replaces the sidebar (ADR-0034 decision 8; spec [a-workspace.md](design/spec/a-workspace.md) rows A.17–A.38).

1. **Tree**: Projects → Source links (name + base in mono) → Worktrees → Agents; main checkouts appear under each link that uses them, not as a section of their own. Then **Scratch** (the Workspace root's terminals and linked Worktrees with no `project`, each with its `→ <Project>?` suggestion, never moved by branch). Rows 26 px, 14 px indent per level.
2. **Markers**: `warning-diamond` on a drifted link (R100), `link-simple` on a link whose `also_in` is not empty (R98.1: other Projects link the same Source, at any base; tooltip `Also in Billing @ release/2.4`, one `<Project> @ <base>` per entry), the LED of each Agent. A parent row shows aggregates (worst LED, waiting count) only while collapsed. Fold state is kept in `state.json`.
3. **Inbox popover**: the pill at the Navigator's foot (`<agent> needs input`, key cap `Ctrl ⇧ G`) and the bell open a popover with the Notifications newest first; no fixed panel. `Ctrl+Shift+G` opens the newest unread from anywhere, `Ctrl+G` only from RADE's own chrome (R17 amendment).
4. `Alt+1..9` follow the Projects' order; the Navigator follows `project.toml` and `HEAD` changes within 3 s.
5. **Selection and Remove from Scratch**: the selected row follows the focused Pane: a Terminal whose cwd is inside a link's Project checkout (or path checkout) selects that link, even though it runs in the Source's main Worktree; a split of it, a New Tab while it is focused and an Agent it spawns (or the `+` menu's, with nothing named) start in that checkout too, not in the Source folder (R1.4 otherwise stands: the Worktree root, never the focused cwd). A Scratch row of a linked Source's main checkout (Panes no link claims) offers **Remove from Scratch…**: one confirmation naming the Pane count and the Agents it stops, then those Panes close and the row goes; the Source and its Projects stay. The Workspace root's row has no such item; a Source no Project links keeps Remove Source…, a Worktree Remove Worktree….
6. **The focused Agent's row is the selected one**: when the focused Pane has an Agent row shown, that row is selected (fill and bar), not its Worktree, link or Scratch row; while it is hidden (a parent folded) or the Pane is a shell, the checkout's row is selected as in 5; a Project home keeps its Project row.
7. **An Agent row's title**: the name the user gave, else the session title the Agent sets as its terminal title (Claude Code and Codex name each session; Codex's ` | <folder>` suffix dropped), else the Preset; a product name (`Claude Code`, `Codex`, …), a shell's `user@host:path` or a path is not a session title. The status, time or Preset stays on the right.

AC:
- R104-a [M37] (e2e): Given scene A. When `debug.ui sidebar` runs. Then Shop's rows are `Shop` (project, depth 0), `shop-web develop` (link, 1), `dev-server` (agent, 2), `checkout-v2` (worktree, 2), `planner` and `tests` (agent, 3), `shop-api` (link, 1, drift), `shared-ui develop` (link, 1, also_in), and the last section is `Scratch` with `~/workspaces/acme`.
- R104-b [M37] (e2e): Given scene A in dark and light. When `debug.ui geometry` measures the Navigator. Then rows A.17–A.38 of the Measurements table of `docs/internals/design/spec/a-workspace.md` are within their tolerance.
- R104-c [M37] (e2e): Given scene A. When the tooltips of both `shared-ui` link rows are read. Then Shop's says `Also in Billing @ release/2.4` and Billing's says `Also in Shop @ develop`, and `shop-web`'s row (linked only by Shop) has no `link-simple` marker.
- R104-d [M37] (unit): Given Shop with Agents `tests` (waiting) and `planner` (done). When the tree is built with Shop collapsed, then expanded. Then collapsed Shop carries the waiting LED and badge `1`, and expanded Shop carries no aggregate.
- R104-e [M37] (unit): Given the scene A model. When the Navigator tree is rendered as text. Then it equals the golden text fixture line for line (Project → link → Worktree → Agent order, drift and also-in flags).
- R104-f [M37] (e2e): Given a Terminal opened at the Workspace root and a linked Worktree `spike` of `shop-web` with no recorded `project`. When `debug.ui sidebar` runs. Then both rows are under Scratch, `spike` carries the suggestion `→ Shop?`, and no row is titled General or No Project.
- R104-g [M37] (e2e): Given 3 Notifications with `tests` waiting newest. When the Inbox pill is clicked, then `Esc`, then `Ctrl+Shift+G` with the keyboard in a Terminal. Then the pill reads `tests needs input`, the popover lists 3 entries newest first, and `Ctrl+Shift+G` focuses the `tests` Pane.
- R104-h [M37] (e2e): Given `shop-api`'s link on base `develop`. When `git switch feat/checkout-v2` runs in its checkout, then `git switch develop`. Then the drift marker appears within 3 s and disappears within 3 s.
- R104-i [M37] (e2e): Given Projects Shop, Billing, Docs in that order. When `Alt+2` is pressed. Then Billing's Project home is shown and the Billing row is selected.
- R104-j [M37] (unit): Given a Terminal in shared-ui's main Worktree with its cwd in Shop's Project checkout of shared-ui, and a shell in the main checkout. When each is focused and the Navigator is built. Then the Terminal selects Shop's `shared-ui` link row and the shell selects shared-ui's Scratch row. In the app the breadcrumb and the Statusline's Project cell then read `Shop`, not Billing (which also links shared-ui).
- R104-k [M37] (unit): Given a shell in shared-ui's main checkout that no link uses and a Terminal in its Project checkout. When the Panes for Remove from Scratch are listed. Then they are the shell only; the Workspace root, a link on the main checkout and a Source no Project links list none; and once they close shared-ui has no Scratch row.
- R104-m [M37] (unit): Given Source x at `/tmp/x` with a recorded link checkout `/tmp/w/.worktrees/.projects/shop/x` and a Terminal (and an Agent `lead`) whose cwd is inside it. When it is split, a New Tab is opened beside it, `lead` spawns a child and the `+` menu spawns an Agent. Then each new Pane starts in that checkout; beside a Terminal in `/tmp/x/deep`, and `rade spawn --source x` from outside, start in `/tmp/x`.
- R104-n [M37] (e2e): Given the Terminal of R105-d in Shop's shared-ui Project checkout, focused. When `split_right` and `new_tab` run. Then `pwd` in both new Terminals prints the Project checkout, no shared-ui Scratch row appears and Shop's `shared-ui` link row stays selected.
- R104-p [M37] (unit+e2e): Given an unnamed codex Pane under `checkout-v2`. When its title becomes `⠋ Kiểm tra tiến trình | checkout-v2`, then `Codex`. Then its row reads `Kiểm tra tiến trình`, then `codex`; `user@host:~/w`, `~/w` and `Claude Code` give no session title; in the app a shell that sets `Refactor auth` and then runs `agy` shows that title on the agy row.
- R104-o [M37] (unit+e2e): Given scene A. When `planner` is focused, then its Worktree `checkout-v2` is folded. Then only `planner`'s Agent row is selected, then only `checkout-v2`; in the app scene A selects `tests` (A.28).
- R104-l [M37] (e2e): Given scene A after R105-d and a shell spawned in shared-ui's main checkout. When its Scratch row's menu runs Remove from Scratch… and the dialog is confirmed. Then the dialog reads `Remove “shared-ui” from Scratch?` with `Closes its 1 Pane in <checkout>`, the shell is closed, the link's Terminal stays, the Scratch row is gone and Shop still links shared-ui.

## R105 — Project home (M37)

The page a Project row opens (design §3.3 P0; spec [b-project-home.md](design/spec/b-project-home.md)).

1. Header (Project icon and name, `project.toml` path, Sources count) with **Open terminal** (the one primary button), Add source…, settings.
2. KPIs: Agents (with waiting / running), Worktrees (names), Uncommitted (files in n sources).
3. **Link table**: Source, base, checkout (main checkout or the Project checkout's path), HEAD, ahead/behind against the base, state chip (`clean`, `off base · n changes`, `also in · <Project> @ <base>` from `also_in` (R98.1), first entry plus `+n` when there are more; off base wins over also in, also in over clean); a row's menu has the drift actions (R100) and Open terminal; opening a terminal on a link with no checkout yet creates its Project checkout (R98.3).
4. Worktrees of the Project (link, ahead, changed files, Agents) and Recent activity (the last Activity entries of the Project, R107).

AC:
- R105-a [M37] (e2e): Given scene B. When the Shop row is clicked. Then the breadcrumb is `Shop / Home` (M43: was `Shop / Overview`, renamed for Agent Overview, R117) and the KPIs read `3` `1 waiting · 1 running`, `2` `checkout-v2, perf-img` and `7` `files in 2 sources`.
- R105-b [M37] (e2e): Given scene B. When `debug.ui home` runs. Then the link table rows are `shop-web` main checkout `develop` `↓2` `clean`; `shop-api` main checkout `feat/checkout-v2` `↑4 ↓0` `off base · 3 changes`; `shared-ui` `.worktrees-dev/.projects/shop/shared-ui` (the Devel build's folder) `develop` `=` `also in · Billing @ release/2.4`.
- R105-c [M37] (e2e): Given scene B in dark and light. When `debug.ui geometry` measures the page and screenshots are taken. Then every row of the Measurements table of `docs/internals/design/spec/b-project-home.md` is within its tolerance.
- R105-d [M37] (e2e): Given Shop's link `shared-ui` (base `develop`) with no Project checkout yet and its main checkout on `main`. When Open terminal is chosen on that row and `pwd` runs in the new Terminal. Then it prints `<root>/.worktrees-dev/.projects/shop/shared-ui` (the Devel build's folder), `git -C` there shows HEAD `develop`, a notice names the new folder, and the main checkout's HEAD is still `main`.
- R105-e [M37] (e2e): Given scene B's Activity. When the page shows. Then Recent activity lists `tests asked which fixtures`, `planner finished · 24 tests passed`, `#482 opened from perf-img` newest first, and a Project with no Activity shows one empty-state line.
- R105-f [M37] (e2e): Given scene B. When the Worktree row `checkout-v2` is clicked. Then it reads `shop-web ← develop` `↑3 · 4 changed · 2 agents` before the click and the window then shows Worktree `checkout-v2` with its last Tab.

## R106 — New Project, Add Source, Change base, Project settings (M37)

The model's UI (design §3.3 P0; spec [c-new-project.md](design/spec/c-new-project.md)); replaces R82's form.

1. **New Project** dialog: Name, one row per Source link (include check, Source, base select, checkout plan text from `project.plan`), Add source… (from the Workspace, a folder, or `owner/repo`), the note `RADE never switches a dirty checkout. It makes a Worktree when a base is taken.`; **Create** writes `project.toml` and nothing else.
2. **Add Source** and **Change base…** on a Project / link show the plan first and write through `project.link_add` / `project.link_set_base`; existing Worktrees keep their recorded base.
3. **Project settings** = the link table (Source, base, checkout, Remove) and the Name; no Named Base grid, Verify, Agent or Run defaults. Its menu keeps Open `project.toml` and Customize with Claude….

AC:
- R106-a [M37] (e2e): Given scene C with Name `Checkout revamp` and rows `shop-web` `develop`, `shop-api` `develop`, `shared-ui` `release/2.5`. When the dialog's plan texts are read before Create. Then they equal spec C.19 and `project.plan` for the same input, and no file or folder has been written.
- R106-b [M37] (e2e): Given the dialog of R106-a. When Create is clicked. Then `.rade/projects/checkout-revamp/project.toml` has 3 `[[source]]` entries with those bases and `checkout = "auto"`, and `git worktree list` of each Source is unchanged.
- R106-c [M37] (e2e): Given scene C in dark and light. When `debug.ui geometry` measures the dialog. Then every row of the Measurements table of `docs/internals/design/spec/c-new-project.md` is within its tolerance and exactly 1 `.suggested-action` (Create) is visible.
- R106-d [M37] (e2e): Given Shop's `shared-ui` at `develop` and Worktree `tokens` of it recorded at `develop`. When Change base… to `release/2.5` is confirmed. Then the dialog showed the plan first, `project.toml` differs only in that link's `base` line, and `tokens` still records base `develop`.
- R106-e [M37] (e2e): Given Shop with 3 links. When Add source… picks `billing` with base `main`. Then a 4th `[[source]]` is appended and every other byte of the file is unchanged.
- R106-f [M37] (e2e): Given Shop. When Project settings opens. Then `debug.ui settings` lists the fields `name` and one row per link (`source`, `base`, `checkout`, Remove) and no field named `bases`, `task.verify`, `task.profile`, `batch.parallel` or `batch.review`.

## R107 — Changes by Project, Activity, palette, Search scope, identity (M37)

1. **Changes by Project** (R21 amended): the Inspector's Changes page shows the focused Project (`PROJECT · SHOP +a −r`), one group per link (its Worktree or checkout, files, its own message box and Commit), the drift card (R100) and clean links folded to `clean`. (M45: the card's layout is R120; Commit and sync are R121.)
2. **Activity**: an Inspector tab listing Agent waiting / done, PR opened, commit and account limit events (R110), newest first, at most 500, in memory only (nothing written; empty after a restart).
3. **Palette** (R14 amended): items are Source links (`shared-ui · Shop · develop`), Projects, Worktrees (`tokens · Billing · shared-ui ← release/2.4`), Panes by name, files, and actions such as `Add <source> to Project…`, `Change base of <source> in <Project>…`, `Fan-out on <source>…`; no absolute paths or Pane ids in titles; scope chips All / Sources / Worktrees / Actions / Files.
4. **Search scope** (R22 amended): `link` (the focused link's checkout or Worktree) or `Project` (every link of the Project, results grouped by link).
5. **Identity**: the Statusline identity cell; when the focused checkout's `git config user.email` differs from the Workspace's, the cell turns `wait` with a tooltip naming both, and the Changes commit area shows a one-line warning.

AC:
- R107-a [M37] (e2e): Given scene A. When the Inspector shows Changes. Then the header reads `PROJECT · SHOP`, there are 3 link groups (`shop-web` 4 files, `shop-api` 3 files with the drift card, `shared-ui` `develop clean`), and Commit in `shop-web` commits only `shop-web`'s staged files.
- R107-b [M37] (e2e): Given scene A in dark and light. When `debug.ui geometry` measures the Changes page. Then rows A.58–A.69 of the Measurements table of `docs/internals/design/spec/a-workspace.md` are within their tolerance.
- R107-c [M37] (unit): Given an empty Activity list. When 501 events of the 5 kinds are appended. Then it holds the newest 500 newest first, and a new list after a simulated restart is empty with no file written under `.rade/`.
- R107-d [M37] (e2e): Given a fake Agent `tests` that goes `waiting` and `planner` that goes `done`. When the Activity tab is shown. Then its first two rows are the `tests` waiting and `planner` done events with their times, newest first.
- R107-e [M37] (e2e): Given scene D. When the palette searches `shared`. Then the first two results are `kind: "link"` with details `shared-ui · Shop · develop` and `shared-ui · Billing · release/2.4 · 1 agent`, the actions `Add shared-ui to Project…`, `Change base of shared-ui in Shop…`, `Fan-out on shared-ui…` are listed, and no title contains `/home`, `p_` or `General`.
- R107-f [M37] (e2e): Given scene D in dark and light. When `debug.ui geometry` measures the palette. Then every row of the Measurements table of `docs/internals/design/spec/d-palette.md` is within its tolerance.
- R107-g [M37] (e2e): Given `theme` defined in `shop-web` and in `shared-ui` of Shop. When Search runs `theme` with scope `link` on `shop-web`, then with scope `Project`. Then the first shows only `shop-web` files and the second shows both, grouped under `shop-web` and `shared-ui`.
- R107-h [M37] (e2e): Given Workspace identity `richard@acme.io` and `shop-api`'s checkout with `user.email = other@example.com`. When a Pane in `shop-api` is focused, then one in `shop-web`. Then the identity cell first has the warning style and a tooltip naming both emails plus a warning line in Changes, then neither.

## R108 — Agent Accounts (M38)

**1.0.x amendment (R125):** R108.1: accounts may also be declared for every Workspace in `$XDG_CONFIG_HOME/rade/accounts.toml` (a Workspace opts out with `accounts = "own"`), and carry `env_file` / `[account.env]`. R108.2: a Pane with an account also gets that env, and a hand-started Agent can be given its account by detection. R108.6: the `+` menu lists user accounts with the Workspace's.

An **Agent Account** = agent + `config_dir` + name, declared per Workspace (ADR-0035; design §10.1; spec [f-new-agent.md](design/spec/f-new-agent.md)).

1. **`[[account]]` in `<root>/.rade/workspace.toml`**: `agent` (`claude` | `codex`), `name`, `config_dir` (`~` expanded), `default`. At most one `default = true` per agent per Workspace (two → a validation error naming the file); none → the first listed. The same `config_dir` may be declared in two Workspaces. A `name` is unique per (Workspace, agent): a claude and a codex account may share one (`personal`), two accounts of one agent may not (a validation error naming the file and the name; `account add` → `USAGE`). Commands that take an account name (`rade account default`, `rade account usage <name>`; `account.set_default`, `usage.get`) accept `--agent claude|codex` (`"agent"`); a name two agents share, given without it, → `AMBIGUOUS` (exit 4) naming both agents, nothing changed. `rade spawn --account` looks the name up for the Preset's agent, so it is never ambiguous.
2. **Spawn**: `rade spawn … --account <name>` (`pane.spawn` `account`); without it a child spawned from a Pane inherits its parent's account, else the Workspace default. The Pane gets the account's `CLAUDE_CONFIG_DIR` or `CODEX_HOME` and `RADE_ACCOUNT=<name>`, for that Pane only, and records its account in `state.json`.
3. **Workspace lock in `rade-core`**: an account not declared in the Workspace of the Pane being created → error kind `FORBIDDEN`, exit code 7, whatever the caller (CLI, socket, UI).
4. **`rade account list|add|default|usage`** (methods `account.list`, `account.add`, `account.set_default`; `usage` per R110). `add` appends an `[[account]]` with `toml_edit`; `default` moves `default = true`.
5. **`rade setup`** installs its hooks into every declared `config_dir` of every Workspace (backups and atomic apply as before).
6. **`+` menu** (titlebar): Shell first, then accounts grouped by agent with the default ticked, accounts of other Workspaces dimmed with a lock and not activatable, then Fan-out… and Browser.
7. **The account's login is never overridden.** A Pane with an account always runs with that account's `CLAUDE_CONFIG_DIR` / `CODEX_HOME`, over the Workspace's `[env]` (R23.3) and any `VAR=value` prefix. A Preset `command` (R16.6) or a recorded invocation (R11.2) whose prefix sets that variable to another directory (after `~` expansion, compared by `realpath`; the same directory is allowed) is refused at spawn in `rade-core`, never silently overridden: error kind `FORBIDDEN`, exit 7, message `preset '<preset>' sets <VAR>=<dir> but account '<name>' uses <dir> (ADR-0035)`, no Pane created. A restored Pane keeps its recorded account (none stays none, R11.2 unchanged for it); when its recorded invocation is refused this way it is restored as a plain shell in its cwd with that account's env, printing the message, and is not resumed.

AC:
- R108-a [M38] (unit): Given `workspace.toml` with `acme-work` (claude, `~/.claude-work`, default), `acme-team` (claude, `~/.claude-team`) and `acme` (codex, `~/.codex`, default). When it is parsed. Then the claude default is `acme-work`, the codex default is `acme`, and `acme-team`'s `config_dir` expands to `$HOME/.claude-team`.
- R108-b [M38] (unit): Given two claude accounts with `default = true`, then two with none. When each file is parsed. Then the first gives a validation error naming `workspace.toml` and both accounts, and the second makes the first listed the default.
- R108-c [M38] (unit+e2e): Given Workspaces `acme` (accounts `acme-work`, `acme-team`, `acme`) and `personal` (account `personal`). When a Pane in `acme` runs `rade spawn --agent claude --account personal`, and `pane.spawn` is called with the same account. Then both fail with kind `FORBIDDEN`, the CLI exits 7 naming `personal` and `acme`, and no Pane is created.
- R108-d [M38] (e2e): Given Workspace `acme`. When `rade spawn --agent claude --account acme-team --cmd env` and `rade spawn --agent codex --cmd env` run. Then the first Pane has `CLAUDE_CONFIG_DIR=$HOME/.claude-team` and `RADE_ACCOUNT=acme-team`, the second `CODEX_HOME=$HOME/.codex` and `RADE_ACCOUNT=acme`, and neither has the other's variable.
- R108-e [M38] (e2e): Given an Agent Pane started with `--account acme-team`. When it runs `rade spawn --agent claude` without `--account`, and a root spawn runs from outside any Pane. Then the child has `RADE_ACCOUNT=acme-team` and the root spawn has `RADE_ACCOUNT=acme-work`.
- R108-f [M38] (e2e): Given a Pane with account `acme-team`. When the app is killed with SIGKILL and started again. Then the restored Pane has `RADE_ACCOUNT=acme-team` and `state.json` records `account: "acme-team"` for it.
- R108-g [M38] (e2e): Given Workspace `acme` with 2 accounts. When `rade account add --agent claude --name acme-team --config-dir ~/.claude-team`, `rade account default acme-team` and `rade account list --json` run. Then `workspace.toml` gains one `[[account]]` with every other byte kept, `default = true` moves from `acme-work` to `acme-team`, and the list shows 3 accounts with `acme-team` default.
- R108-h [M38] (e2e): Given a temp HOME with accounts at `~/.claude-work`, `~/.claude-team` and `~/.codex` declared in `acme`. When `rade setup --dry-run` runs. Then its diff installs the hooks into all 3 directories and writes nothing.
- R108-i [M38] (e2e): Given scene F. When the `+` menu opens and `acme-team` is activated. Then rows F.1–F.7, F.11–F.14 and F.21 of the Measurements table of `docs/internals/design/spec/f-new-agent.md` are within tolerance, `personal` is insensitive with `other workspace`, and the new Claude Pane has `RADE_ACCOUNT=acme-team`.
- R108-j [M38] (unit+e2e): Given Workspace `acme` with account `acme-team` (claude, `~/.claude-team`) and `[agents.claude] command = "CLAUDE_CONFIG_DIR=~/.claude claude"`. When `rade spawn --agent claude --account acme-team` runs, `pane.spawn` is called with the same, and both run again after the command becomes `CLAUDE_CONFIG_DIR=~/.claude-team claude`. Then the first two fail with kind `FORBIDDEN` (the CLI exits 7) with a message naming the Preset `claude`, `~/.claude` and `acme-team`, and create no Pane; the last two create Panes with `CLAUDE_CONFIG_DIR=$HOME/.claude-team` and `RADE_ACCOUNT=acme-team`.
- R108-k [M38] (e2e): Given a Pane on account `acme-team` whose session-start hook recorded the invocation `CLAUDE_CONFIG_DIR=$HOME/.claude claude` (fake Agent). When the app is killed with SIGKILL and started again, and `env` runs in the restored Pane. Then `state.json` still records `account: "acme-team"` for it, the Pane is a plain shell that printed the message naming the Preset and `acme-team`, the recorded invocation was not run (fake Agent argv log unchanged), and `env` shows `CLAUDE_CONFIG_DIR=$HOME/.claude-team` and `RADE_ACCOUNT=acme-team`.
- R108-l [M38] (unit+e2e): Given Workspace `personal` with accounts `personal` (claude, `~/.claude`) and `personal` (codex, `~/.codex`), and separately a `workspace.toml` declaring two claude accounts named `personal`. When `rade account usage personal` and `rade account default personal` run, then both with `--agent codex`, then `rade spawn --agent claude --account personal --cmd env`, and the second file is parsed. Then the first two exit 4 with `AMBIGUOUS` naming `claude` and `codex` and `workspace.toml` unchanged, the `--agent codex` runs act on the codex account, the spawned Pane has `CLAUDE_CONFIG_DIR=$HOME/.claude`, and the second file gives a validation error naming `workspace.toml` and `personal`.

## R109 — Usage collection (M39)

Read-only collection of limits and tokens per account (ADR-0035 decisions 5–9; design §10.2).

1. **Claude limits**: `rade setup` sets each Claude `config_dir`'s `settings.json` `statusLine` to `{"type": "command", "command": "rade status --statusline [--then '<previous command>']"}`. Rewrite rules: the previous `statusLine.command` is kept byte for byte as the `--then` argument, single-quoted for `sh` (`'` → `'\''`); a command that already starts with `rade status --statusline` is kept as it is, so a second run never nests `--then` and leaves the file byte-identical; no `statusLine` → no `--then`; other `statusLine` keys (`padding`) are kept; a `statusLine` whose `type` is not `command` is left untouched and the run prints a note; `--dry-run` writes nothing. The wrapper records `rate_limits` (`five_hour`, `seven_day`, `spend_limit`) from stdin into the Usage store, passes stdin unchanged to the previous command (run with `sh -c`) and prints its output (nothing without one); it always exits 0 and never waits more than 2 s for RADE.
2. **Codex limits and all tokens**: RADE tails `$CODEX_HOME/sessions/**/rollout-*.jsonl` (`token_count`: `rate_limits` per `limit_id`, `info.total_token_usage`) and `$CLAUDE_CONFIG_DIR/projects/*/*.jsonl` (`message.usage`) by offset, every 30 s per account and when one of its Agents ends a turn; the newest sample per `limit_id` wins; tokens go to a Project by the session's `cwd` (else Scratch); cache reads are not counted in totals.
3. **Usage store**: `$XDG_STATE_HOME/rade/usage/<agent>-<sha8 of the canonical config_dir>.jsonl` (`rade-dev` for the Devel flavor): limit samples over time and token totals per day × Project × model, pruned past 90 days. Method `usage.record` (what `--statusline` calls) and `usage.refresh` (tail now).
4. **Tolerant parsers**: unknown fields ignored, broken lines skipped, an unknown `limit_id` shown under its raw name; nothing readable → `no usage data (format changed?)`, never a number.
5. **Detailed limits** (opt-in per account): at most 1 request per 5 minutes to Claude's usage endpoint, the token read from the `config_dir` at call time and never stored or logged; an unexpected response turns it off and says so in Activity; tests use `RADE_CLAUDE_USAGE_URL`, honoured only in test mode (ADR-0021).

AC:
- R109-a [M39] (unit): Given an anonymised Claude Code 2.1.309 statusline JSON with `rate_limits.five_hour.used_percentage = 62`, `seven_day = 41`, a `spend_limit` of 12 of 50 and an unknown field. When it is parsed. Then the samples are `five_hour` 62 % with its `resets_at`, `seven_day` 41 % and `spend_limit` $12 of $50, and the unknown field is ignored.
- R109-b [M39] (e2e): Given a running app and the statusline JSON of R109-a. When `rade status --statusline --then "printf prev"` reads it, then the same runs with no app. Then stdout is exactly `prev` both times, both exit 0, the second within 2 s, and the first appended 1 sample to the `claude-<sha8>.jsonl` store file.
- R109-c [M39] (unit): Given an anonymised codex-cli 0.163.0 rollout with `limit_id` `codex` (primary weekly, `secondary: null`), `codex_bengalfox` (`GPT-5.3-Codex-Spark`, 5-hour 81 %, weekly), `premium` and `credits`. When it is parsed. Then the limits are Codex weekly, Spark 5-hour 81 %, Spark weekly, Premium and Credits, and an older line for `codex_bengalfox` loses to the newest.
- R109-d [M39] (unit): Given a 1,000-line rollout already read to its end. When 3 lines are appended, then the file is truncated to 10 lines, then a half-written line is appended. Then the next tail reads exactly the 3 lines' bytes, the truncated file is read again from offset 0, and the half line is read only once it ends with a newline.
- R109-e [M39] (unit): Given a rollout with 2 broken JSON lines, an unknown `limit_id` `codex_newmodel` and, separately, a file with no readable sample. When both are parsed. Then the broken lines are skipped, `codex_newmodel` is listed under that raw name, and the second gives `no usage data (format changed?)` with no percentage.
- R109-f [M39] (unit): Given Claude transcript lines with `message.usage` for `opus` and `sonnet`, `cwd` in Shop's checkout and in `/tmp`, and cache-read tokens. When tokens are totalled. Then they are split by model, Shop gets its lines and Scratch the `/tmp` ones, and cache reads add 0 to every total.
- R109-g [M39] (unit): Given `config_dir` `~/.claude-work` and a symlink to it, and samples dated 91 and 89 days ago. When the store path is computed and the store is pruned. Then both dirs map to one `$XDG_STATE_HOME/rade/usage/claude-<sha8>.jsonl` (`rade-dev` in the Devel flavor) and only the 89-day sample remains.
- R109-h [M39] (unit+e2e): Given account `acme-work` with Detailed limits on and a fake server at `RADE_CLAUDE_USAGE_URL` in a test-mode app. When 3 refreshes run within 5 minutes, then the server answers an unexpected format. Then the server got 1 request, Opus 58 % and Sonnet 22 % are stored, the fake token appears 0 times under the XDG state, data and config dirs, and the option turns off with one Activity event.
- R109-j [M39] (unit+e2e): Given a temp HOME with Claude config dirs `~/.claude-work`, whose `settings.json` has `"statusLine": {"type": "command", "command": "bash -c \"echo 'hi'\"", "padding": 0}`, and `~/.claude-team` with no `statusLine`. When `rade setup --agent claude` runs, then runs again. Then after the first run `~/.claude-work`'s command is `rade status --statusline --then 'bash -c "echo '\''hi'\''"'`, whose `--then` argument as `sh` parses it equals the old command byte for byte, `padding` is still 0, and `~/.claude-team`'s is `rade status --statusline` with no `--then`; the second run leaves both files byte-identical and writes no new backup.
- R109-i [M39] (e2e): Given a fake Codex Agent on account `acme` that writes a rollout with Spark 5-hour 81 % and ends its turn. When `rade account usage acme --json` runs within 5 s. Then it lists `codex_bengalfox` `GPT-5.3-Codex-Spark` 5-hour at 81 %.

## R110 — Usage UI (M39)

Design §10.3; spec [e-usage.md](design/spec/e-usage.md) and the Toast of [f-new-agent.md](design/spec/f-new-agent.md).

1. **Tightest limit** = the account's limit with the highest used % (ties: the earlier reset); **Next resets** = upcoming resets in time order with a forecast (`hits 100% in ~40m`, `97% at reset`) from a linear fit of the samples in the current window. Samples are fit in timestamp order whatever order they arrived in; a sample stamped more than 60 s after now (clock skew) is left out of the fit; a reset time already past by the local clock gives no forecast (never a negative time) until a sample of the next window arrives.
2. **Usage page** (list–detail): opened from the Statusline usage cell, the titlebar `gauge` button or palette `> usage`; filters Today / 7 days / 30 days and Workspace (default the shown one, or All workspaces); the account table sorted by Tightest limit (no data last), ≥ 80 % in `wait` with icon and number; Next resets; the selected account's limits grouped (Session / Weekly / Extra usage; Codex by `limit_id`), tokens per day (cache reads excluded), by Project and by model. No money except Claude's `spend_limit` as reported. `rade account usage [<name>] [--json]` (methods `usage.list`, `usage.get`) gives the same data.
3. **Statusline cell**: the Tightest limit of the focused Agent's account (`claude 5h ▰▰▱ 62% · wk 41%`); hidden without an account or data.
4. **Forecast Toast**: when a limit is forecast to reach 100 % before its reset, a Toast with `Next agents use <other account>`; the action changes only the default for Agents opened later; a per-model limit suggests another model first.
5. **Activity** (R107) logs `account <name>: <limit> over 80%`, `blocked` and `reset`.

AC:
- R110-a [M39] (unit): Given `acme-work` limits `five_hour` 62 %, `seven_day` 41 %, Opus 58 %, and an account with no samples. When `tightest` runs. Then `acme-work`'s is `five_hour` 62 %, two limits at 62 % pick the earlier reset, and the empty account has none.
- R110-b [M39] (unit): Given `five_hour` samples rising 1 % per minute from 22 % to 62 % with the reset in 72 minutes, a flat series, and a series that resets midway. When `forecast` runs. Then the first gives `hits 100% in ~40m`, the flat one no forecast, and the third uses only the samples after the reset.
- R110-k [M39] (unit): Given the first series of R110-b with 2 samples delivered out of order, the same series plus a sample at 99 % stamped 10 minutes after now, and a window whose `resets_at` is 30 s before now. When `forecast` runs at the same now. Then the first two give `hits 100% in ~40m` as R110-b does, and the third gives no forecast and no negative time.
- R110-c [M39] (unit): Given scene E's samples. When `next_resets` runs at the scene's time. Then it returns the 4 rows of spec E.24 in that order with those texts.
- R110-d [M39] (e2e): Given scene E. When the Usage page opens from the Statusline cell, the titlebar `gauge` button and palette `> usage`. Then all three show it, the rows follow spec E.18, and every row of the Measurements table of `docs/internals/design/spec/e-usage.md` is within its tolerance in dark and light.
- R110-e [M39] (e2e): Given scene E with 7 accounts in 2 Workspaces. When the Workspace filter is `acme`, then All workspaces, and the period is Today, then 30 days. Then the table shows 4 then 7 accounts and the token cells change with the period.
- R110-f [M39] (e2e): Given scene A with `tests` on `acme` and `planner` on `acme-work`. When `planner`'s Pane is focused. Then the usage cell reads `claude 5h 62% · wk 41%` with a meter fill of 22 ± 1 px and tooltip `Claude · acme-work`, and with no account focused the cell is hidden.
- R110-g [M39] (e2e): Given `acme-work` samples forecast to hit 100 % in ~40 m before a reset in 1h 12m and `acme-team` at 8 %. When the forecast runs. Then the Toast reads as spec F.17 with action `Next agents use acme-team`, and after the click the next `+` → Claude opens with `acme-team` while running Panes keep `RADE_ACCOUNT=acme-work`.
- R110-h [M39] (e2e): Given account `acme` whose Spark 5-hour goes 79 % → 81 %, then `rate_limit_reached_type` names it, then it resets. When Activity is shown. Then it has the 3 lines `account acme: Spark 5-hour over 80%`, `… blocked` and `… reset`, once each.
- R110-i [M39] (e2e): Given scene F. When the `+` menu opens. Then rows F.8–F.10 of the Measurements table of `docs/internals/design/spec/f-new-agent.md` show `5h 62% · wk 41%`, `5h 8% · wk 12%` and Spark 5-hour 81 % in the `wait` color with its icon.
- R110-j [M39] (e2e): Given scene E's store. When `rade account usage` and `rade account usage acme-work --json` run. Then the first prints one line per account of the caller's Workspace with its Tightest limit and next reset, and the second lists every limit of `acme-work` grouped as spec E.30.

## R111 — Hardening (M40)

1. **1280 px**: every screen at 1280 × 800 follows the `1280 px` section of its spec sheet; no horizontal scroll anywhere.
2. **Text scale 200 %**: every screen follows the `Text scale 200 %` section of its sheet (Navigator as a 48 px icon rail, one main Pane); no clipped text.
3. **Performance**: 200 Navigator rows, 20 accounts and a 1 GB log folder stay within the budgets below; live checks on the real desktop.

AC:
- R111-a [M40] (e2e): Given scenes A–F at 1280 × 800. When each is shown. Then every scrolled view's horizontal range fits its page (0 horizontal scroll) and each follows the `1280 px` section of its sheet in `docs/internals/design/spec/`.
- R111-b [M40] (e2e): Given scenes A–F at text scale 200 %. When each is shown. Then the Navigator is a 48 ± 1 px rail, every label's allocation is at least its natural size (0 clipped), and each follows the `Text scale 200 %` section of its sheet.
- R111-c [M40] (e2e): Given 10 Projects × 5 links × 2 Worktrees × 1 Agent (260 Navigator rows). When one Agent's status changes 100 times and the Navigator is scrolled end to end. Then the row update is ≤ 16 ms at p95 and each scroll frame's own work (frame clock layout → paint end) is ≤ 16.7 ms at p95 (headless; the frame interval itself is checked live, R111-f).
- R111-d [M40] (e2e): Given 20 accounts with 90 days of samples. When the Usage page opens and the `+` menu opens. Then the page is drawn within 100 ms and the menu within 50 ms.
- R111-e [M40] (unit+e2e): Given a 1 GB folder of rollout files. When the first scan runs, then 10 lines are appended. Then the scan finishes within 10 s with the UI thread answering a 50 ms probe throughout, and the next tail reads only the appended bytes.
- R111-f [M40] (live): Given the real GNOME desktop at scale 1× and 2×. When `tests/live/run.sh m40` drives scenes A, D and F. Then the screenshots show Phosphor icons crisp at 16 px, the focused Pane's glow and the waiting LED, an end-to-end Navigator scroll keeps the frame interval ≤ 16.7 ms at p95, recorded in `docs/internals/checks/M40.md`.

## R112 — Settings (1.0.0, ADR-0037)

1. **Settings page.** `Ctrl+,` (the one plain-Ctrl default, R17.3 amendment: a legacy terminal has no code for it, so no program receives it) and the palette's `Settings` open a page over a scrim, laid out like VS Code: a search field, categories (Terminal, Code, Theme, Notifications, Agents, Statusline), one row per app-wide `config.toml` setting with its label, key and description, an `acc` bar and a ↺ reset on a modified row, `n modified` in the header, Esc or a click outside closes it.
2. **Terminal font.** Font family (the monospace families GTK sees, installed and bundled), font style (the family's upright faces), size in px, line height (× the font's own), ligatures, look (`rade` / `ghostty`), with a live preview Terminal. Maple Mono (OFL-1.1) ships with RADE and is the default, Medium at 14 px. Ghostty sees bundled fonts through a `fonts.conf` RADE writes in `$XDG_RUNTIME_DIR/rade/fonts/` and `FONTCONFIG_FILE`; nothing is installed.
3. **Live and safe.** A change is one edit of `config.toml` with `toml_edit` (comments and other keys kept, an emptied table removed on reset), checked by parsing before it is written; a bad value or a broken file shows its error and writes nothing. It applies at once: every Terminal reloads its Ghostty config, the color scheme follows `[appearance] color_scheme` (`system` = dark unless the system asks for light, `dark`, `light`).
4. **Code font.** `[code_font]` (family, style, size in px, ligatures) styles code outside Terminals: the Viewer, Review diffs, Impact and comment excerpts (`monospace` text) and Markdown `code` / `pre`. An empty family or style follows the Terminal's (Settings shows `Same as Terminal`); another family without a style is Regular. Default 13 px. The chrome's small mono metadata stays JetBrains Mono (ADR-0036).
5. **Code colors.** The Viewer, Review and Markdown code use the `syn-*` tokens (GitHub Dark / Light Default, MIT) on `bg0`: GtkSourceView schemes `rade-dark` / `rade-light` and the Markdown code CSS are generated from `tokens.toml`; the Viewer header is a Pane header and Markdown follows the visual system.
6. Not in 1.0.0: editing keybindings, per-Workspace / Project settings (the layered TOML files stay), syncing between machines.

AC:
- R112-a [M41] (unit): Given the Settings list. When each row's default is read from `Config::default()`. Then every row has a known category, its default fits its kind, none is modified, and `terminal.font` is `Maple Mono`.
- R112-b [M41] (unit): Given `config.toml` with comments. When `font_size`, `idle_seconds` and `appearance.color_scheme` are set and then reset, and bad values or a broken file are set. Then the comments and other keys stay byte for byte, new tables are appended, a reset removes the key and an emptied table, and each bad case is an error that writes nothing.
- R112-c [M41] (unit): Given `[terminal]` defaults, then Geist Mono 16 px, line height 1.0, ligatures off. When RADE builds its Ghostty look. Then every font key is reset first, `font-size` is px × 0.75 (10.5 / 12), `adjust-cell-height` is (line height − 1) × 100 %, the family and style follow, and ligatures off add `-calt`, `-liga`, `-dlig`.
- R112-d [M41] (unit): Given the Settings list. When it is searched with `font SIZE`, `font`, `light`, `` and `zzz`. Then the rows match every word in label, description, category or key, and only categories with a match are listed.
- R112-e [M41] (e2e): Given the app with a commented `config.toml`. When `Ctrl+,` opens Settings, it is searched, the font size is set to 24 and reset, a size of 99 is set, and Esc is pressed. Then the page lists the 5 categories with Maple Mono / Medium / 14 and the bundled font and style in its lists, the search narrows rows and categories, the file gains `[terminal] font_size = 24` with its lines kept and a Terminal has fewer columns without a restart, the reset restores the file and the columns, 99 shows an error and changes nothing, and Esc closes the page.
- R112-f [M41] (e2e): Given Settings open in dark. When `appearance.color_scheme` is set to `light`, then reset. Then the app turns light, then dark, live.
- R112-h [M41] (unit+e2e): Given `[code_font]` empty, then the Terminal font changed, then `family = "JetBrains Mono"` and `size = 15`. When the code font is resolved and applied. Then it is the Terminal's Maple Mono Medium at 13 px, then follows the new Terminal font, then JetBrains Mono Regular 15 px, written under `[code_font]`; the stylesheet sets family, px size, weight and ligatures with quotes and braces stripped from the family; the family row shows `Same as Terminal` while empty.
- R112-i [M41] (unit): Given `tokens.toml`. When the code schemes and the Markdown code CSS are generated. Then `rade-dark` / `rade-light` take `term-fg` on `bg0` for text and `syn-keyword` for `def:keyword`, every color in them is a token's, and the Markdown CSS colors function names with `syn-function`.
- R112-g [M41] (e2e): Given a temp HOME. When the app starts. Then `$XDG_RUNTIME_DIR/rade/fonts/fonts.conf` includes the system config and adds that folder, `fc-match 'Maple Mono:style=Medium'` under it is the bundled `MapleMono-Medium.ttf`, and no font file exists under HOME or `XDG_DATA_HOME`.

## R113 — Viewers as Panes: several per Tab, zoom (M42)

The owner's request of 2026-10-24 (plan `docs/plans/viewers-html-pdf-agent-marks.md`; no Pin button, owner's call). Amends R7.1: a Tab may hold several Viewers.

1. **Zoom from the header.** Every Viewer's header has the Zoom button (`Ctrl+Shift+Z`; the restore icon while zoomed). `Esc` in a zoomed Viewer restores the Layout; the next `Esc` closes it.
2. **Open to the Side** opens a file in a **new** Viewer split right of the Pane it came from: `Ctrl+Shift+click` on a path in a Terminal, the Terminal menu's "Open to the Side" (on a file), the Files tree's middle-click or right-click → "Open to the Side", `Ctrl+Enter` on a palette file row, `rade open <file> --side` (`viewer.open` `side`).
3. **Which Viewer a plain open uses:** the Pane itself when it is a Viewer, else the Tab's Viewer focused last (`last_viewer`, kept in `state.json`), else a new one right of the Pane. Back / Forward are per Viewer.
4. Viewers are ordinary Panes: split, resize, close, Session Restore, `rade list`.
5. **Content zoom.** Code, Markdown, rendered HTML and images zoom in steps (25 %–400 %): `Ctrl+scroll`, `Ctrl++` / `Ctrl+−`, `Ctrl+0` (100 %; an image: fit), or the header's `−  100%  +` (the label resets). An image opens fitted (`Fit`); a PDF keeps its own viewer's zoom (the controls hide). The zoom stays with the Viewer across files.

AC:
- R113-a [M42] (unit): Given a Terminal and a Viewer on `a.md`. When `b.md` opens to the side, then plain opens follow focus between the two Viewers, then the focused one closes. Then `b.md` is a second Viewer right of the first; a plain open goes to the Viewer focused last; from a Viewer it goes to that Viewer; Back is per Viewer; after the close it goes to the other one; to the side from the Terminal adds a third, focused.
- R113-b [M42] (e2e): Given a Viewer on `main.rs`. When `rade open docs/internals/PRD.md --side` runs, then the second Viewer's header Zoom, then `Esc` twice. Then there are two Viewers and the first keeps `main.rs`; the second is zoomed; the first `Esc` restores the Layout with both Viewers; the second closes it.
- R113-c [M42] (e2e): Given the Files tree and a Terminal showing `src/b.rs`. When a file row is middle-clicked and the Terminal menu is opened on the path. Then a new Viewer shows the file and the first keeps its own; the menu lists "Open to the Side" after "Open …".
- R113-e [M42] (unit+e2e): Given the zoom steps. When stepping in, out and back from 100 %, 130 % and an image's fit scale 0.37. Then they move to the neighbouring steps, stop at 25 % / 400 %, reset to 100 %; in the app a code view shows `125%` after two steps in and `100%` after `Ctrl+0`; a 48 px image opens `Fit`, is 52 px at `110%` and 192 px at `400%`; a Markdown file shows the Viewer's zoom.

## R114 — HTML and PDF rendered in the Viewer (M42)

The owner's request of 2026-10-24 (plan `docs/plans/viewers-html-pdf-agent-marks.md`). Before it, HTML showed as text and PDF as "Binary file".

1. **PDF** (`.pdf`, up to 50 MB) shows in WebKitGTK's built-in PDF viewer (pdf.js inside WebKitGTK: pages, zoom, search, text selection), offline, no new dependency; a bigger one says so, with Open.
2. **HTML** (`.html`, `.htm`, `.xhtml`) shows rendered by default; the header's **Source** toggle (`ph-file-code`) shows the markup in the code view (syntax-highlighted, Outline, code navigation), and toggles back. Relative CSS, images and scripts load from the file's folder.
3. **Scripts.** A local HTML page runs its JavaScript (the owner's call, 2026-10-24); `[viewer] html_scripts = false` turns it off. Rendered documents use an ephemeral NetworkSession of their own (nothing kept, nothing shared with Browser Panes); windows can't open by themselves.
4. **Links** as in R7.3: a click on a link to another existing file opens it in the same Viewer (Back); a web link opens the system browser; `#anchors` scroll; the page can't navigate itself away from local files.
5. Live reload on file change, keeping an HTML page's scroll position.

AC:
- R114-a [M42] (unit): Given `a.pdf`, `B.PDF`, `a.html`, `a.htm`, a 20 MB PDF and a PDF over 50 MB. When they are classified. Then the first two and the 20 MB one are `Pdf`, the HTML files `Html`, and the biggest `TooLarge`.
- R114-b [M42] (e2e): Given `report.html` with an `<h1>`, a relative image and an inline script. When `rade open report.html` runs, then the header's Source toggle twice. Then the page shows the heading, the image loaded and the script's text; Source shows the markup as `html` in the code view; the toggle returns to the rendered page.
- R114-c [M42] (e2e): Given a one-page PDF with the text `RADE PDF MARKER`. When `rade open spec.pdf` runs. Then the Viewer shows it rendered (`doc` view, the file loaded) and the screenshots in light and dark show the page and the PDF toolbar.

## R115 — Agent glyphs coloured by status (M42)

The owner's request of 2026-10-24 (plan `docs/plans/viewers-html-pdf-agent-marks.md`, Q3 = C: RADE's own glyphs now, official marks only with the vendors' written permission; ADR-0038). Amends R102 / ADR-0036 §5: an Agent's status is drawn on its glyph, not on a 7 px LED.

1. **Glyphs** (14 px): each agent's own logo, single-colour (Lobe Icons, MIT; owner's decision, ADR-0038 amendment 2): Claude Code the Claude spark, Codex the Codex logo, Antigravity and OpenCode theirs; any other Preset Phosphor Bold `robot`. A Pane's glyph comes from its Preset's name, else the program its command or recorded invocation runs; a shell has none.
2. **Colours** are the LED's (A.30): running `run` pulsing, waiting `wait` with a glow, done `ok`, error `err`, interrupted `wt`, idle `fg3`; `needs input` stays written where it was.
3. **Where:** Navigator Agent rows, Tab labels (the lead Agent; amended by M49, owner review 2026-10-29: a Tab holding Agents of more than one kind shows the lead's glyph then each other kind's in Pane order, at most 3, the extra ones 12 px and 2 px apart, each in the worst status of that kind in the Tab, and its tooltip lists `name · agent · status` per Agent Pane), Pane headers before the name, the Inbox pill and rows, the system monitor's BY PANE rows, and the `+` menu's account rows (no status, in place of the `C` / `X` badge). The Statusline counts, a collapsed parent's aggregate, Activity and Project home rows keep the LED (a status without an agent).
4. **Hand-started agents.** A Pane whose shell runs an agent in its foreground is an Agent Pane for the glyph before any hook reports (Codex reports at its first prompt, agy never): every 3 s RADE reads each Terminal's foreground process (the shell's `tpgid`) and matches its argv against the Presets' programs as hooks do (argv[0], or an interpreter's first non-flag argument; a Preset whose command starts with an interpreter is never matched). It shows the glyph, idle, in the Navigator, the Tab and the header until the program ends; nothing is saved, so Session Restore is unchanged; hooks still set the status.
5. **Your own icon.** `[agents.<name>] icon = "<path>"` (SVG or PNG, `~/` expanded) draws that file in its own colours instead of RADE's glyph, everywhere the glyph shows, with the status as a 5 px LED at its lower right. RADE ships no vendor logo; an unreadable file falls back to the glyph.

AC:
- R115-a [M42] (unit): Given Presets `claude-sonnet`, `codex`, `agy`, `opencode` and `reviewer`, and a `reviewer` Pane whose command is `FOO=1 /usr/bin/codex --model o3`. When their glyphs are chosen. Then they are Claude, Codex, Antigravity, OpenCode and other, the Pane's is Codex, a shell has none, and scene A's Agent rows carry `planner` claude, `tests` codex, while parents carry none.
- R115-b [M42] (e2e): Given scene A in dark and light. When `debug.ui sidebar`, `debug.ui pane`, `debug.ui inbox` and `debug.ui new_menu` read the glyphs. Then `planner` shows `claude` in `done`, `tests` `codex` in `waiting` (the Inbox pill's glyph too, and `tests`'s Pane header), the `+` menu's account rows show their agent's glyph, and the glyph colours meet 3 : 1 on `bg1` (`contrast.py`).
- R115-d [M42] (unit): Given a shell whose stat has `tpgid` 20 (argv `codex --yolo`), one in its own foreground, and Presets incl. a second one on `claude` and one run as `bash ~/bin/agent.sh`. When the foreground argv and its Preset are found. Then they are `codex --yolo` → `codex`, none for the shell; `/usr/local/bin/agy` → `agy`, `node …/codex.js` → `codex`, `claude` → `claude` (not the second Preset), and `bash build.sh` / `bash -c codex` / `vim codex.md` → none.
- R115-e [M42] (unit+e2e): Given a shell Pane in scene A. When `agy` runs in it and then ends (Ctrl+C). Then within 4 s it has an Agent row with the antigravity glyph and its header shows it, and after the end the row is gone; unchanged detection reports no change.
- R115-h [M49] (unit): Given Tab Panes [codex lead, claude, claude], a Fan-out grid owner after another kind, four kinds, one kind and none. When `tab_marks` reads them. Then they are [codex, claude] with each kind's worst status, the grid owner first, the first three kinds, one mark, none.
- R115-i [M49] (e2e): Given a fake Codex Agent Pane. When it spawns `--agent claude` into its own Tab. Then `debug.ui tabs` shows marks `["codex"]` before and `["codex","claude"]` after, and the tooltip lists both Agents; screenshots dark + light.
- R115-f [M42] (unit+e2e): Given `[agents.opencode] icon = "<tmp>/opencode.svg"` and `icon = "~/icons/openai.svg"` on codex. When the registry loads and `opencode` runs in a shell. Then the path is `$HOME/icons/openai.svg`, other Presets have none, and the Pane's glyph is the custom icon with status `idle`.
- R115-c [M42] (e2e): Given scenes A–F. When the goldens are compared. Then they match the goldens re-approved by the owner with the glyphs.

## R116 — System monitor (M42)

The owner's request of 2026-10-24; placement from a design review (option A of three: Statusline cell + popover; a titlebar sparkline and an Inspector tab were rejected — the titlebar is full, the Inspector is hidden at 1280 px / 200 %).

1. **Who and why.** With many Agents building and testing at once: is the machine saturated (stop starting Agents), is memory near the OOM killer, which Pane is the runaway, is the network busy (downloading, not stuck).
2. **Statusline cell** after the identity cell, before the key hints: an icon per figure instead of a word (owner, 2026-10-24) — `cpu` 34%, `memory` 61%, `graphics-card` 12%, `arrow-down` 1.2M, `arrow-up` 312K (Phosphor, 13 px `fg3`, a tooltip each) — figures in JetBrains Mono with tabular digits `fg2`, padded so the width stays put, 10 px between figures; its text form `cpu  34% · mem  61% · gpu  12% · ↓1.2M ↑312K` is what `debug.ui monitor` reports. GPU only when readable: AMD `gpu_busy_percent` in sysfs, NVIDIA through `nvidia-smi` at most every 10 s and only while the card is awake (never wakes it); Intel has no unprivileged metric. Network = bytes/s on physical interfaces (`/sys/class/net/<if>/device`, not `lo`): no bridges, veths or tunnels.
3. **High states** colour only that figure: CPU / GPU `wait` after 3 samples ≥ 90 %; memory `wait` ≥ 85 %, `err` ≥ 95 %; back after 2 samples at least 5 points under. The number is the text that goes with the colour (ADR-0036 §5).
4. **Narrow:** it drops after the hints and before ports and identity.
5. **Popover** (a click): `SYSTEM · <n> cores · load a b c`, cpu, mem with swap, gpu with its name, net per physical interface, BY PANE = the 5 busiest Panes (LED, name, % of a core, RSS) — a click focuses the Pane —, and `outside RADE <cpu> · <mem>`.
6. **Cost.** One worker thread: every 2 s while the window is active, 5 s otherwise, nothing while minimized, the Statusline hidden or `system_monitor = false` (app-wide, a Settings row under Statusline). The per-Pane scan of every process runs only while the popover is open. The UI thread sets the label only when it changed; the cell is a persistent widget, so a tick never rebuilds the Statusline. `RADE_SYSTEM_MONITOR=0` turns it off (the E2E harness, so goldens stay fixed); `RADE_PROC_ROOT` / `RADE_SYS_ROOT` point it at fixtures.

AC:
- R116-a [M42] (unit+e2e): Given `/proc/stat` samples 34 % busy apart, meminfo 61 % used and `net/dev` with `lo`, `docker0` and `enp1s0` (the only one with a `device`). When the cell is formatted. Then it reads `cpu  34% · mem  61% · ↓1.2M ↑312K` (with a GPU: `gpu   7%` before the network), only `enp1s0` counts, and AMD's sysfs busy % is read; in the app over the same fixtures the cell shows `cpu  34% · mem  61% · ↓… ↑…`.
- R116-b [M42] (unit+e2e): Given CPU samples 92, 93, 95, 88, 84, 84 % and memory 86, 96, 82, 79, 79 %. When the levels are computed. Then CPU is high from the 3rd to the 5th sample and normal at the 6th; memory is high, critical, high, high, normal; in the app memory at 96 % turns the cell `err` and back.
- R116-c [M42] (unit+e2e): Given the Statusline with the monitor, at decreasing widths. When it is fitted. Then the monitor drops only after both hints and before ports and identity.
- R116-d [M42] (unit+e2e): Given process stats grouped by session. When two scans 2 s apart are compared. Then a Pane whose process used 200 ticks at 100 Hz shows 100 % and its RSS; in the app a Pane running a busy loop leads BY PANE with ≥ 80 %, an `outside RADE` line follows, and a click on the row focuses it.
- R116-e [M42] (e2e): Given the fake root, then the Statusline hidden, then `system_monitor = false`. When ticks run. Then a tick costs ≤ 1 ms on the worker and the UI thread (≤ 50 ms on the worker with the popover open on the real /proc); hidden, no sample is taken; off, there is no cell.

## R117 — Agent Overview (M43, ADR-0039)

A full-window view of every Agent in every Workspace, most urgent first, to jump straight to one (design overview-mode-proposal.md concept A; spec [g-overview.md](design/spec/g-overview.md)). Only one Workspace is shown at a time (R23.4); Agent Overview is the one place that lists them all.

1. **Open and close.** `Ctrl+Shift+Space` from anywhere (a Terminal included, R17.3), the titlebar button right of `+`, the palette's `Agent Overview` action, a click on the switcher's waiting `!` (filtered to Needs you) or on a Statusline Agent count (filtered to that status). It is modal like the palette (other RADE keys don't fire; IBus works in its entry). `Esc` clears the filter, else closes, and the keyboard returns to the Pane that had it; so does a click on anything but a card, the `+n more` tile, a chip, the filter field or the group toggle (as macOS Mission Control closes).
2. **What is listed.** Every Agent Pane (a Preset and an Agent Status, R10.1) of every Workspace, in four bands: **Needs you** (`waiting`, then `error`, then `interrupted`; newest status first within each), **Finished, not seen** (`done` with an Unread Notification; newest first), **Running** (newest status change first; `quiet <n>m` once its screen has not changed for 10 minutes), **Quiet** (`done` and read, `exited`; one-line rows, all of them while they fit under the other bands, else as many whole rows as fit (at least one) ending in a `+<n> more ▾` tile; the band's title counts all). The Agents of one Fan-out are one card with a row each; a Subagent's card names its Parent (`↳ planner`). While open the order is frozen: statuses update in place, cards move only on the next open.
3. **Card.** the agent's glyph coloured by its status (R115; a Fan-out card shows an LED for the group), name and model; the status word and its age (`needs input · 4m`); the Workspace's color and name, then `Project / link / Worktree` (Scratch for none); the last 3 non-blank lines of its screen (not for Quiet; read once a second, only while open and only for cards on screen); the last Notification text; `↑ahead · n changed` (when the shown Workspace's git facts have it); the Agent Account it runs under, if any.
4. **Keyboard.** Typing filters: every word of the query must appear in the card's name, Preset, model, Workspace, Project, link, Worktree, status words (`needs input`, `failed`, `error`, …) or Notification text, case and accents ignored. Arrows move through the grid, `Tab` / `Shift+Tab` to the next / previous band, `Enter` jumps, `Shift+Enter` jumps and Zooms, `Ctrl+Alt+1..9` shows one Workspace (again: all), `Alt+G` groups by status ⇄ by Workspace (kept in `state.json`). **Hold to flip:** while `Ctrl+Shift` stay down, each further `Space` selects the next card, and releasing `Ctrl` after two or more presses jumps; after one press it stays open. It opens on the first Needs-you card, else on the Agent focused before.
5. **Jump** goes where a Notification click goes (R3.3, R23.6): the Agent's Workspace, Source, Worktree and Tab are shown, the Pane is focused (a Title Bar Zoomed) and its Notifications are read; a Fan-out card shows its Fan-out Tab.
6. **Motion** (ADR-0039, amending ADR-0036 §6): in 200 ms (the window behind shrinks to 94 % under the scrim, cards fade and rise 6 px, 15 ms apart for the first 12), out by a jump 220 ms (the layer fades while the window behind scales back to full size), out by `Esc` 140 ms. With GNOME's Reduce Animation (`gtk-enable-animations` off) nothing moves.
7. **Empty and large.** No Agent: one mono line `No Agents running · Ctrl+Shift+P > Spawn Agent`. No match: `No Agent matches "<query>" · Esc clears`. Running shows 8 cards then `+<n> more ▾`; Needs you and Finished never fold. Cards at least 300 px wide: 4 columns at 1440 px, 3 at 1280 px, 6 at 2000 px (at most 8), 2 at text scale 200 %; never a horizontal scroll.
8. **Budgets.** With 40 Agents the first frame is shown within 50 ms of the key, and each animation frame's own work stays within 16.7 ms (p95).
9. **Preview.** A floating view of one card, to read what its Agent is saying without jumping (plan M43-preview.md, mockup `#p`). Header: glyph, name, model, status · age; the Workspace, `Project / link / Worktree` and `↳ Parent`. Body: up to 24 lines of its screen (mono 11.5 px on `bg0`, the agent CLI's input box and footer cut, runs of blank lines kept as one, long lines ellipsized); a Fan-out card: each member's last 6 lines under its name. Foot: the last Notification, git facts, the account, `↵ jump · Alt+P hide`. Every card has one, Quiet rows included.
   - **Place:** an overlay; no card moves. Beside its card, top-aligned, on the right when it fits, else the left; when neither fits, below the card's row, else above it; when its content fits neither, beside after all on the roomier side, narrower (≥ 300 px); else the larger of below / above, shortened. Always inside the body, never over its own card (it may cover others). Width min(600 px, 46 % of the window); height its content within the room left.
   - **When:** the pointer resting 350 ms on a card (it stays while the pointer is on the card or the Preview, and goes 150 ms after it leaves both); the keyboard selection resting 250 ms (on open, arrows, `Tab`, the hold-to-flip). Hidden while a filter is typed (back after a pause). `Alt+P` turns keyboard Previews off / on (kept in `state.json`); hover always works.
   - **Live:** while shown, that one Pane is re-read once a second; its body stays at the newest line unless scrolled up. A click on it jumps like a click on its card and counts as part of the card for R117.1; its body scrolls under the pointer; its text can't be selected. In: 120 ms fade + a 4 px slide from the card; none under Reduce Animation.

AC:
- R117-a [M43] (unit): Given 16 Agents in 5 Workspaces (the mockup's data: 2 waiting, 1 error, 1 interrupted, 2 done unread, 6 running incl. a Fan-out of 3, 4 done read or exited). When the Overview is built. Then the bands are Needs you, Finished, not seen, Running, Quiet with waiting before error before interrupted, newest first within each status, the Fan-out is one card of 3 rows, a Subagent card names its Parent, and Quiet shows all 4 when they fit, or whole rows and `+n more ▾` when the app has room for fewer.
- R117-b [M43] (unit): Given an open Overview. When an Agent in Running turns `waiting` and the Overview is refreshed, then reopened. Then the refreshed card keeps its place with its new status, and on reopening it is first in Needs you.
- R117-c [M43] (unit): Given the Overview of R117-a. When it is filtered by `mig`, `globex`, `err`, `fixtures`, `viet` and by Workspace 2. Then each lists exactly the matching Agents (name, Workspace, status word, Notification text, accents ignored) and the Workspace filter lists only that Workspace's Agents.
- R117-d [M43] (unit): Given the Overview of R117-a. When the selection moves by arrows, `Tab`, `Shift+Tab`, and a flip of 1 and of 3 `Space` presses ends. Then arrows wrap across bands, `Tab` lands on each band's first card, a flip of 1 stays open and a flip of 3 jumps to the third card; with nothing in Needs you it opens on the Agent focused before.
- R117-e [M43] (unit): Given a running Agent whose screen last changed 599 s ago, then 600 s ago. When its card is built. Then the first has no quiet tag and the second reads `running · quiet 10m`.
- R117-f [M43] (e2e): Given Workspaces A and B with fake Agents waiting, error, done and running, B shown, and the keyboard in a Terminal running `cat -v`. When `Ctrl+Shift+Space` is pressed, the A Agent `tests` is chosen with `Enter`, then the Overview is opened again and closed with `Esc`. Then the Terminal prints nothing, `debug.ui overview` lists the bands of R117-2, `Enter` shows Workspace A with `tests` focused and its Notification read, and after `Esc` typing reaches the focused Terminal.
- R117-g [M43] (e2e): Given the Overview of R117-f closed. When `Ctrl+Shift+Space` is pressed 3 times with `Ctrl+Shift` held and `Ctrl` is released, then pressed once and released. Then the third card's Agent is focused, and after the single press the Overview is open.
- R117-h [M43] (e2e): Given a running Agent that printed `one`, `two`, `three` last. When the Overview opens. Then its card's tail is those 3 lines and no Quiet row has a tail.
- R117-i [M43] (e2e): Given `gtk-enable-animations` off. When the Overview opens and closes. Then `debug.ui overview` reports 0 ms for both and the first frame shows the cards at full opacity.
- R117-j [M43] (e2e): Given no Agent, then 40 Agents in 5 Workspaces. When the Overview opens at 1440 × 900, at 1280 × 800 and at text scale 200 %. Then the first shows the one empty line, and with 40 the columns are 4, 3 and 2, `hscroll` and `clipped` are 0, the first frame is within 50 ms and the animation frame work p95 within 16.7 ms.
- R117-k [M43] (e2e): Given an Agent waiting in a hidden Workspace. When the Overview is opened from the titlebar button, the palette's `Agent Overview`, the switcher's `!` and the Statusline's `running` count. Then each opens it, the `!` filtered to Needs you and the count to Running.
- R117-l [M43] (unit): Given the default keybindings. When the R17.3 collision test runs. Then `overview` is `<Ctrl><Shift>space` and collides with no GNOME, Ghostty or IBus default.
- R117-m [M43] (e2e): Given the Overview of R117-f. When screenshots are taken in dark and light at 1440 × 900. Then they are saved under `target/e2e-m43/` and looked at against the mockup `#a` / `#a-light`.
- R117-n [M43] (unit): Given a 1440 px Overview's body and a 900 px one at 200 %. When a Preview is placed for a card in column 1, in the last column, low on the screen, taller than the body, and in a 2-column grid where neither side fits (content that fits below, above, neither, and one column); and when a 60-line screen ending in an input box is cut for it. Then it is right, left, pushed up into the body, as tall as the body, below, above, beside narrower (≥ 300 px), and above shortened, always inside the body and never over its card; the text is the last 24 output lines, blank runs as one, none at the ends.
- R117-o [M43] (e2e): Given the Overview of R117-f and an Agent that printed 60 lines. When the selection rests, the pointer rests on that Agent's card, on a last-column card and on a Quiet row, the Agent prints a line, the pointer leaves, a filter is typed, `Alt+P` is pressed, the Preview is clicked, and the window is 900 px at text scale 200 %. Then the Preview shows after 250 ms / 350 ms (not before), right of a column-1 card and left of a last-column one, beside and narrower at 900 px, never over its card; 24 lines ending with the newest, the new line within 2 s; gone 150 ms after the pointer leaves; hidden while typing and back after a pause; `Alt+P` stops keyboard Previews (saved in `state.json`) while hover still shows them; a click on it stays open per R117.1 and a Preview click jumps to its Agent; screenshots dark and light against the mockup `#p`.

## R118 — Review: context and reading (M44, ADR-0040)

The Review Pane tells whose work it shows and lets the owner read it file by file (plan M44-review-loop.md; spec [h-review.md](design/spec/h-review.md)).

1. **Header context.** Line 1:
   - the branch and its base (`feat ← main`);
   - the Worktree's Agent: its glyph (R115) and name; with several, the one whose status changed last, then `+n`;
   - its **task**: the prompt RADE spawned it with (`rade spawn … "<prompt>"`, the `+` menu, Fan-out; kept on the Pane in `state.json`), else its session title (R104.7), whitespace collapsed to one line (at most 200 characters) with an ellipsis;
   - `n commits ▾`: a popover of `git log <base>..HEAD` (short SHA, subject, age).

   Line 2: the scope control (R119.2), the whitespace toggle, `k/n viewed` with a meter, the Impact toggle, Refresh and Close.

   With no Agent in the Worktree, the Agent and task are left out.
2. **Files rail.** The changed files as a folder tree in git's order (the diff's order, so `N` / `P` follow it); a folder whose files all sit in one child folder is joined with it (`src/ui/`). Above it, a filter field (case-insensitive substring of the path) and *Hide viewed*. Each file row shows:
   - its status letter;
   - its name, with the folder dim when filtered flat;
   - a viewed tick or an unread dot;
   - a comment count;
   - `+/−`.

   **Lock and generated files** go in a last group, folded in the rail and in the diff: `Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `poetry.lock`, `uv.lock`, `go.sum`, `*.min.js`, `*.min.css`, `*.snap`, `*.map`, and paths marked `linguist-generated` in the repository's `.gitattributes`.
3. **Viewed.** A checkbox on each file's header in the diff and the `v` key mark a file as read; the rail shows a tick (viewed) or a dot (unread). Viewing a file folds it and moves to the next unread file. The mark is kept per Worktree in `state.json` as path → a hash of the file's status, old path and content in the working tree (the base is fixed, so new content is a new diff; hiding whitespace does not change it), so a file that changes is unread again. The header shows `k/n viewed`.
4. **Inline comments.** Selecting lines and pressing `c` (or the Comment button) opens the composer as a row right under the last selected line: a multi-line entry, where `Enter` adds, `Shift+Enter` makes a new line and `Esc` cancels. A comment shows as a card under its last line. Drafts have Edit and Delete; sent comments are described in R119.3. The list under the diff is gone. Re-anchoring and `outdated` work as in R13.2; an outdated comment is a card at the top of its file.
5. **Keys** of the Review Pane, while the diff list has the focus (never while typing in an entry):

   | Key | Action |
   |---|---|
   | `n` / `p` | next / previous hunk |
   | `N` / `P` | next / previous file |
   | `]` / `[` | next / previous comment |
   | `v` | viewed |
   | `c` | comment |
   | `w` | whitespace |
   | `i` | Impact |
   | `Ctrl+Enter` | the primary button |

   The footer shows the hints. They are plain keys of the Review Pane only (like `c` before), so they never collide with RADE's or Ghostty's global keys (R17.3).
6. **Whitespace.** The toggle (`w`) diffs with `-w`; hunks that only change whitespace disappear. It is kept per Review Pane while open.
7. **Impact rail.** R88.2's section is a rail on the right of the diff (320 px), not a band above it. Below 1100 px of Pane width it shows as an overlay over the diff's right side.
8. **Footer.**
   - Left: the key hints and `n drafts`.
   - Right: `Discard…`, which asks (`Discard n draft comments?`), and one primary button: `Send to Agent (n)`, which becomes R119.4's *Request changes (n)* / *Approve ▾*.
   - `Clear` is gone.

AC:
- R118-a [M44] (unit): Given a Worktree whose Agent was spawned with the prompt `add  OTP⏎login`, one whose Agent has only the session title `Fix flaky test`, one with two Agents, and one with none. When the Review head is built. Then the tasks are `add OTP login` and `Fix flaky test`, the two-Agent one names the Agent whose status changed last plus `+1`, the last one has no Agent, and an exited Agent is never the head's.
- R118-b [M44] (unit): Given changed paths `src/ui/a.rs`, `src/ui/b.rs`, `src/main.rs`, `README.md`, `Cargo.lock`, `web/app.min.js`, `gen/x.rs` with `gen/** linguist-generated` in `.gitattributes`. When the rail tree is built. Then it is README.md, `src/` → (main.rs, `ui/` → a, b), then a folded generated group of 3 (Cargo.lock, gen/x.rs, web/app.min.js); `a/b/c/one.rs` + `a/b/c/two.rs` give one folder `a/b/c/`; a filter lists matches flat.
- R118-c [M44] (unit): Given 3 files of which 2 are marked viewed. When one viewed file's content changes. Then that file is unread, the other stays viewed, the progress is `1/3 viewed`, a file no longer changed loses its mark, and the same file with whitespace hidden hashes the same.
- R118-d [M44] (unit): Given rows of 3 files (the second folded), hunks and 2 comments. When `n`, `p`, `N`, `P`, `]`, `[` are applied from a row. Then each lands on the next / previous hunk, file header or comment card, skipping the folded file's hunks, without wrapping.
- R118-e [M44] (e2e): Given a Review of a Worktree with 4 changed files, one only in whitespace, and a fake Agent spawned with a prompt. When lines are selected and `c` is pressed, text typed and `Enter` pressed, `w` is pressed, `v` is pressed twice, `i` is pressed, and `Discard…` is clicked. Then:
  - the composer row sits right under the selection and the card under the commented line;
  - the whitespace-only file disappears;
  - the header shows the task and `2/3 viewed`;
  - the Impact rail sits right of the diff;
  - Discard asks before dropping the draft.

  Spec H rows are checked in dark and light (`tests/e2e/spec_h.py`).

## R119 — Review: Rounds and conclusion (M44, ADR-0040)

1. **Rounds.** *Request changes* sends the draft comments like R13.2, with the prompt headed `Review round <n>` and any follow-ups quoting the earlier comment, and records a **Round**: `{n, at, snapshot}`.
   - `snapshot` is a commit of the Worktree's files at that moment, including untracked files (`merge::snapshot`), kept by the ref `refs/rade/review/<worktree name>/<n>` (the git worktree's name: unique in its repository).
   - At most 20 Rounds per Worktree: the oldest one and its ref are dropped.
   - Removing the Worktree deletes its refs.
   - The sent comments stay, with their Round.
2. **Since round n.** The header's scope control offers `All changes <k>` and `Since round <n> <j>`. The second diffs the last Round's snapshot against the working tree. It is chosen when the Review opens if a Round exists and something changed since. Files changed since the Round are unread again (R118.3). When nothing changed: `Nothing changed since round <n>` with a link to *All changes*.
3. **Comment states.** `draft` → `sent` (Round n). Once an Agent of the Worktree has ended a turn (`done`, `waiting`, `error`) in or after that Round's second, a sent comment reads `code changed` when its lines with their context are no longer in the file (or it is outdated), else `untouched`. The owner can **Resolve** a sent comment (it stays as a one-line card, dimmed, with Reopen) or **Follow up**, which opens a draft on the same lines (quoting them as they are now) whose prompt entry starts `Re: round n, <path:line>: "<first line>"`. A sent comment that is not resolved is **open**.
4. **Conclusion.** The footer's primary button is *Request changes (n)* while there are drafts, else *Approve ▾*. Approve's menu:

   | Item | Shown | Does |
   |---|---|---|
   | Ask Agent to commit | the Worktree has an Agent that has not exited | Sends it `Approved (review round <n>). Commit these changes following this repository's conventions. Do not push.` |
   | Ask Agent to commit and open a PR | same | The same prompt, ending `Then push the branch and open a pull request against <base>.` instead of `Do not push.` |
   | Commit myself… | always | Opens the Inspector's Changes page (R21) with the message box focused |
   | Create PR… | always | Opens R21.6's dialog |
   | Choose as Winner… | in a Fan-out's Worktree | Opens R15.4's dialog with this Worktree selected |

   Choosing any item records a Round in state `approved`. A Round's state is `changes requested`, `approved`, or `changed after approval` once the files differ from an approved snapshot. The header shows it.
5. **Ways in.**
   - When an Agent's turn ends (`done`) in a Worktree under review (not a Source's main checkout, or one with Rounds or comments), a Toast `<agent> finished in <worktree>` with a **Review** button is raised, and its Inbox row (and a `Ready for review` row) get a **Review** button. They open the Review showing what changed since the last Round when there is any.
   - The Agent Overview's selected card opens its Worktree's Review on `Alt+R` (plain keys type into its filter, R117.4).
   - The Navigator's Worktree row shows `R<n> · <k> open` while comments are open, `approved` after an approving Round, or `R<n> · changed` once an open Review has seen the files change since.
   - The palette gains `Review: Since Last Round`.
6. **For Agents.**
   - `rade review status [--json]` (`review.status`) gives the Rounds (n, at, state), the comments with `path:line`, text, Round, state and resolved, `viewed` `k/n`, and the Agent summary.
   - `rade review request [--summary "<text>"]` (`review.request`): the calling Agent says the work is ready. The summary (at most 3 lines shown, Markdown stripped) shows under the header's task until the next Round, and the owner gets a `Ready for review` Notification with the Review action.
   - `review.list` / `review.send` gain `round`, `state`, `resolved` per comment and `round` in the send result.
   - The `rade` skill tells Agents to read `rade review status` after a Review prompt and to call `rade review request` when done.
7. **Restore.** Rounds, comment states, viewed marks and the summary are in `state.json` (additive fields, no migration) and survive Session Restore. The refs survive in the repository.

AC:
- R119-a [M44] (unit): Given a git Worktree with 2 draft comments. When *Request changes* is applied 21 times (with a new draft each time). Then rounds 2–21 exist, `refs/rade/review/<name>/1` is gone and `…/21` points at a commit of the working tree including an untracked file, the comments carry their Round, and removing the Worktree's refs leaves none.
- R119-b [M44] (unit): Given Round 1 over 5 files. When 2 files are edited and one is added. Then the diff since Round 1 lists exactly those 3 files and their viewed marks are cleared.
- R119-c [M44] (unit): Given 2 comments sent in Round 1. When the Agent's turn ends after editing the lines of the first only. Then the first is `code changed`, the second `untouched` (a file gone: `code changed`); a follow-up on the second composes a prompt headed `Review round 2` quoting it under `Re: round 1`; a turn ending in the Round's own second counts.
- R119-d [M44] (unit): Given Worktrees with a live Agent, with none, and in a Fan-out. When the Approve menu is built. Then the items are as in R119.4's table, and the Ask prompts name the Round and the base.
- R119-e [M44] (e2e): Given a fake Agent in a Worktree. When a comment is sent with *Request changes*, the Agent edits 2 files and its turn ends, the Toast's Review action is clicked, a comment is resolved, *Approve ▾ → Ask Agent to commit* is chosen, then `rade review status --json` is run by the Agent and `rade review request --summary "Done: OTP"`. Then:
  - the Review opens in `Since round 1` with those 2 files;
  - the comment states show;
  - the Agent receives the commit prompt;
  - the Round is `approved` and the Navigator row says `approved`;
  - the status JSON lists the Rounds and comments;
  - the header shows the summary and a `Ready for review` Notification exists.
- R119-f [M44] (e2e): Given the state of R119-e. When the app is killed (`-9`) and relaunched. Then the Rounds, comment states, viewed marks and summary are back.
- R119-g [M44] (e2e): Given the Review of R119-e in round 3. When screenshots are taken in dark and light at 1440 × 900. Then they match golden H within 0.5 % (`tests/e2e/golden/m44/`, approved by the owner).

## R120 — Changes page, VS Code model (M45, ADR-0045)

Amends R21.1–R21.5, R21.8, R21.9 and R107.1. One page: a card per Source link of the focused Project (a checkout outside every Project: one card for it, header `SOURCE · <NAME>`); there is no single-checkout Details page.

1. **Card.** Header: fold chevron, link name, its Worktree or branch, meta (`↑3 · 4`), a sync button (`↑a ↓b`, own upstream) or Publish button (no own upstream), a `…` menu (Fetch, Pull, Push…, Sync…, Publish Branch…, Create Pull Request…, Stage All Changes, Unstage All Changes, Discard All Changes…). Body: the commit area (R121), then the groups Merge Changes, Staged Changes, Changes (untracked files in Changes as `U`); an empty group is left out; a clean card folds to its header unless it shows an action button.
2. **Rows.** File icon, name, folder (list view), `+a −d` against the group's side (staged: index vs `HEAD`; Changes: working tree vs index; untracked: its lines), status letter right-most (`M` `wait`, `A`/`U` `add`, `D` `del` + struck name, `R`/`C` `run`, `!` `err`). Hover, selection or focus swaps the counts for actions: Open File; Changes: Discard Changes, Stage Changes; Staged: Unstage Changes; Merge: Mark as Resolved (stage). Group headers: count and Stage All / Unstage All / Discard All.
3. **Click and keys.** A click selects the row and opens its diff in the Diff Pane (R122), focus staying in the list; a double click focuses the Diff Pane; Ctrl / Shift extend the selection. Enter opens, Space stages / unstages the selection, Delete discards it (asks), Ctrl+A selects all.
4. **Discard.** Tracked files: their unstaged changes (`git restore --worktree`). Untracked files go to the trash; if that fails a second dialog offers to delete them permanently. One dialog names the count.
5. **Context menu.** Changes (tracked): Open Changes, Open File, Open File to the Side, Stage Changes, Discard Changes…, Copy Path, Copy Relative Path. Changes (untracked): Open Changes, Open File, Open File to the Side, Stage Changes, Delete File…, Add to .gitignore, Copy Path, Copy Relative Path. Staged: Open Changes, Open File, Open File to the Side, Unstage Changes, Copy Path, Copy Relative Path. Merge: Open File, Open File to the Side, Mark as Resolved, Copy Path, Copy Relative Path. Add to .gitignore appends `/<path>` to the repository's `.gitignore`.
6. **View.** List or tree (compact folders, folders first), sort by path, name or status (list), from the page header; kept in `[scm] view` / `[scm] sort`.

AC:
- R120-a [M45] (unit): Given a status with a conflicted file, a file staged only, a file staged and modified again, a modified file and an untracked file. When the card's groups are built. Then they are Merge Changes (1 row, `!`), Staged Changes (2 rows, the index letters), Changes (3 rows: two `M`, one `U`), in that order, and a status with no conflict has no Merge Changes group.
- R120-b [M45] (unit): Given changed paths `src/ui/a.rs`, `src/ui/b.rs`, `src/main.rs`, `README.md`, `a/b/c/one.rs`. When rows are built as a tree. Then they are `a/b/c/` → one.rs, `src/` → `ui/` → a.rs, b.rs, then main.rs, then README.md; as a list sorted by name: `a.rs`, `b.rs`, `main.rs`, `one.rs`, `README.md` (case ignored); by status: the `U` rows after the `M` rows, each by path.
- R120-c [M45] (e2e): Given a checkout with a modified, a staged and an untracked file. When the modified row is clicked. Then a Diff Pane `app.rs (Working Tree)` opens right of the focused Pane, keyboard focus is still in the Changes list and nothing was staged; a click on the staged row shows `util.rs (Staged)` in the same Diff Pane.
- R120-d [M45] (e2e): Given 2 modified rows and 1 untracked row selected. When Space is pressed in the list. Then the 3 are staged; Space on the 3 staged rows unstages them; Delete on the 3 asks once (`Discard changes in 3 files?`), and confirming restores the 2 tracked files and moves the untracked one to `$XDG_DATA_HOME/Trash/files/`.
- R120-e [M45] (e2e): Given an untracked `app.log`. When its context menu is read and Add to .gitignore activated. Then the items are R120.5's untracked list, `.gitignore` ends with `/app.log` and the row is gone.
- R120-f [M45] (e2e): Given the default config. When View as Tree is clicked, then Sort by Name chosen. Then `config.toml` holds `view = "tree"` and `sort = "name"` under `[scm]` with its other lines unchanged, and after a relaunch the page is still a tree.
- R120-g [M45] (e2e): Given scene A in dark and light. When `debug.ui geometry` measures the Changes page. Then rows A.58–A.69 and A.81–A.86 of `docs/internals/design/spec/a-workspace.md` are within tolerance.
- R120-h [M45] (e2e): Given a Terminal in a repository that is in no Project. When Changes is shown. Then one card, header `SOURCE · SHOP`, `debug.scm` `mode` = `source`; no op returns a `checkout` page.

## R121 — Commit and sync (M45, ADR-0045)

Amends R21.4 and R21.5.

1. **Message box.** 1–10 lines, placeholder `Message (Ctrl+Enter to commit on ‘<branch>’)`; Up on the first line / Down on the last line cycle through the last 20 commit messages of the checkout; a first line over 72 characters shows `Subject: <n> characters (keep it ≤ 72)`.
2. **Commit ▾.** Commit (Ctrl+Enter), Commit (Amend), Commit & Push, Commit All, Undo Last Commit. With a message, nothing staged and changes present, Commit asks `Stage all <n> changes and commit?`. Amend with an empty box keeps the last message. Amend and Undo Last Commit are disabled while HEAD is on a remote branch. Undo Last Commit keeps its changes staged and puts its message in the box. Commit & Push asks Push's confirmation after committing.
3. **Action button.** A card without changes shows `Publish Branch` (no own upstream) or `Sync Changes ↑a ↓b` (own upstream, something to pull or push). Sync asks once, pulls `--ff-only`, then pushes; a failed pull pushes nothing.
4. **Fetch.** From the card's `…` menu and the page's `…` (Fetch All). `[scm] autofetch = true` fetches every card every `autofetch_period` seconds (default 180, 60–3600); off by default (ADR-0045 amends ADR-0013).

AC:
- R121-a [M45] (unit): Given a repository with two commits. When Commit (Amend) runs with an empty message, Commit All with an untracked file present, and Undo Last Commit. Then the amend keeps the subject, Commit All's commit holds the untracked file, and Undo leaves HEAD at the first commit with the undone files staged and returns the undone message; on a root commit Undo empties HEAD and stages everything.
- R121-b [M45] (unit): Given the card states (staged, changes, message, HEAD pushed, has HEAD). When the commit menu is built. Then Commit is enabled iff a message and any change; Amend iff HEAD exists and is not pushed; Undo iff HEAD exists and is not pushed; Commit All iff any change and a message; and the smart prompt is asked iff message, no staged, changes.
- R121-c [M45] (unit): Given a first line of 72 characters, then 73. When the warning is built. Then none, then `Subject: 73 characters (keep it ≤ 72)`; and the history cursor walks newest → oldest on Up and back to the draft on Down.
- R121-d [M45] (e2e): Given 2 unstaged changes and a message. When Commit is clicked. Then the dialog `Stage all 2 changes and commit?` shows, and confirming commits both files with the message.
- R121-e [M45] (e2e): Given a commit to make on a branch with an upstream. When Commit & Push is chosen and the Push confirmation confirmed. Then the bare remote has the commit and Undo Last Commit is disabled.
- R121-f [M45] (e2e): Given a clean branch without upstream. Then the action button reads `Publish Branch`; publishing sets `origin/<branch>`. Given another clone pushed one commit and a local commit exists. When Fetch runs. Then the header shows `↑1 ↓1` and the action button `Sync Changes ↑1 ↓1`; Sync with a fast-forwardable remote pulls then pushes; with a diverged remote the card shows `Sync: ` and git's message and the remote is unchanged.
- R121-g [M45] (e2e): Given the default config. When the auto-fetch timer ticks (`debug.scm autofetch_tick`). Then nothing is fetched; with `[scm] autofetch = true` the tick fetches and the behind count updates.

## R122 — Diff Pane (M45, ADR-0045)

1. **Open.** From a Changes row or its menu: the active Tab's Diff Pane shows the file (else one opens right of the focused Pane, else in a new Tab). Header: file icon, name, folder, `(Working Tree)` / `(Staged)` / `(Untracked)`, Previous / Next Change, layout toggle, Show Full File toggle, Stage File or Unstage File, Discard File (not staged), Open File, Close. Esc closes it.
2. **Rows.** Inline (old / new line numbers, sign, highlighted code) or side by side (old left, new right, removed and added lines paired in order, filler where one side has none); `auto` = side by side from 900 px. Three lines of context; Show Full File shows the whole file.
3. **Hunks and lines.** On a modified text file each hunk header has Stage Hunk / Unstage Hunk / Discard Hunk…; selected lines can be staged, unstaged or discarded (`Stage Selected Lines` in the header and the row menu). Discard asks.
4. **Moves.** F7 / Shift+F7 = next / previous change, wrapping.
5. **Freshness.** Every 2 s while on screen and after its own actions; no longer changed → `No changes` and a button to the other section when it has the file. Kept across a restart (`{"type": "diff", "root", "path", "section"}`).

AC:
- R122-a [M45] (unit): Given a hunk `ctx, -a, -b, +A, ctx, +C`. When paired side by side. Then the pairs are (ctx, ctx), (-a, +A), (-b, —), (ctx, ctx), (—, +C).
- R122-b [M45] (unit): Given a two-hunk edit of a ten-line file and the second added line of hunk 1 selected. When the lines patch is made and applied for stage, then the index's patch for unstage, then for discard. Then the index holds only that line; unstaging it empties the index diff; discarding a selected removed line restores only that line in the working tree.
- R122-c [M45] (unit): Given rows with change blocks starting at rows 4, 10, 30. When moving next from row 0, 10, 30 and previous from 4. Then 4, 30, 4 (wrap), 30 (wrap).
- R122-d [M45] (e2e): Given `src/app.rs` with two changed hunks shown in the Diff Pane. When Stage Hunk is clicked on hunk 2, then line `CHANGED 2` selected and Stage Selected Lines clicked. Then `git diff --cached` holds hunk 2 and then also only `CHANGED 2`; Discard Hunk… asks before anything changes.
- R122-e [M45] (e2e): Given the Diff Pane 1000 px wide with layout `auto`. When it is measured and toggled. Then it is side by side; at 600 px inline; the toggle forces inline at 1000 px; Show Full File shows every line of the file; F7 from the last change goes to the first.
- R122-f [M45] (e2e): Given a Diff Pane on `app.rs (Working Tree)`. When the app is relaunched. Then the Pane is back with the same file; after `git add src/app.rs` it shows `No changes` and `Open Staged Changes`, which switches it to `app.rs (Staged)`.

## R123 — Quick Diff (M45, ADR-0045)

1. A Viewer of a tracked file in a git repository marks lines changed against the index in its gutter: added (`add` bar), modified (`run` bar), deleted (`del` triangle on the line after the deletion). `[scm] quick_diff = false` turns it off.
2. A click on a mark opens a peek with that change's hunk and Stage Change, Revert Change… (asks), Open Changes (the Diff Pane). One popover at a time (R90).
3. Marks follow the file: on reload and every 3 s while the Viewer is on screen.

AC:
- R123-a [M45] (unit): Given a `-U0` diff with a one-line modification at 2, two added lines at 5–6, a deletion after 8 and a deletion at the top. When marks are built. Then (2 modified), (5 added), (6 added), (9 deleted), (1 deleted).
- R123-b [M45] (e2e): Given a Viewer on a modified tracked file. When it is inspected. Then `debug.viewer info` lists its marks; an edit on disk updates them within 5 s; the peek on a mark shows its hunk and Stage Change stages it (the mark goes); with `quick_diff = false` there are no marks; an untracked file has none.

## R124 — Voice prompts (M46, ADR-0046)

Speak to an Agent Pane instead of typing: RADE transcribes, a read-only voice agent writes a short, clear prompt from what was said, the session and the checkout, the owner revises it by speaking again and sends it.

1. **Open and talk.** On an Agent Pane, hold `F8` (keybinding `voice`) to talk and let go to draft; a tap toggles talking on and off; the Pane header's mic button toggles too. A Voice Composer opens at the bottom of that Pane: what you say appears live, then the Voice Draft streams in. One Voice Composer at a time. Other Panes: a Toast says voice works on Agent Panes.
2. **Speech to text.** Audio is recorded by `[voice] record_command` into memory only and sent to an OpenAI-compatible transcription endpoint (`[voice] stt_url`, default a local whisper.cpp server), with names from the session and the checkout as the vocabulary hint. Live words while talking (`live_preview`), a final pass after. A take with less than 300 ms of sound above speech's bar (`[voice] min_speech_level`, raised by a noisy room) is not transcribed (`Didn't catch that`), since Whisper makes sentences up on silence; known made-up outros are dropped from transcripts.
3. **Understanding.** The voice agent (Claude Code headless by default, Codex optional) gets the Pane's session tail (transcript or rollout; the screen when there is none), git status, the rules files, the checkout's layout and the Viewer's file, and may read, search and list files of the checkout (no writing, no editing, no other tools), within `max_turns` steps and `timeout_secs` per turn.
4. **Draft.** One goal sentence, up to 5 bullets, `path:line` references, the owner's language (or English), fillers and repeats removed, nothing invented; what it could not resolve is marked `[?]`.
5. **Revise.** Holding `F8` again with a draft speaks a revision ("drop the second point", "say it in English"); the whole draft is rewritten in the same voice agent process and the changed words are marked. The draft can also be edited by hand.
6. **Send.** Enter sends the draft to the Agent (bracketed paste + Enter); Alt+Enter pastes it without Enter; Esc cancels a take, stops a draft or discards (asking once after edits).
7. **Nothing kept.** The voice agent runs without session persistence, without the user's hooks and without the Pane's `RADE_*` variables; it is killed when the composer closes. No audio file is written.
8. **Settings.** `[voice]` in the user's `config.toml` only; the Settings page's Voice category has agent, tools, max steps, timeout, max recording, live words, draft language, speech language.

AC:
- R124-a [M46] (unit): Given a Claude config dir with `projects/-home-u-ws-shop/<id>.jsonl` holding 9 exchanges with tool calls and tool results. When the context is built for cwd `/home/u/ws/shop` and session `<id>`. Then it holds the last 6 exchanges' user and assistant texts only, no tool input or result, each message ≤ 2,000 chars, the whole tail ≤ 16,000 chars with the newest kept; a session file elsewhere under `projects/*/` is found by the fallback.
- R124-b [M46] (unit): Given a Codex home with two rollouts, one named with the session id. When the context is built. Then the session tail comes from that rollout's user / assistant messages; with no session id the bundle holds the screen text's last 120 lines with trailing blank lines trimmed.
- R124-c [M46] (unit): Given a checkout with `CLAUDE.md`, 3 changed files and 500 tracked files. When the context is built. Then it has the branch, `git status --short`, `git diff --stat HEAD`, the last 5 subjects, `CLAUDE.md` (≤ 6,000 chars) and a layout of ≤ 3,000 chars of depth-2 directories with counts.
- R124-d [M46] (unit): Given a session tail naming `handleSubmit`, `use_totals`, `promo.ts`, `the`, `Fix` and changed path `src/checkout/applyPromo.ts`. When the vocabulary is built. Then it is `applyPromo promo.ts use_totals handleSubmit` order-by-newest, without plain words, ≤ 800 chars.
- R124-e [M46] (unit): Given the default `[voice]` and a Pane with account `acme-work`. When the Claude command is built. Then argv is §T4's exact list with `--max-turns 6` and `--tools Read,Grep,Glob`; `max_turns = 3` and `tools = "none"` give `--max-turns 3` and `--tools ""`; the env has the account's `CLAUDE_CONFIG_DIR` and no `RADE_PANE_ID`, `RADE_SOCKET` or other `RADE_*` key.
- R124-f [M46] (unit): Given the recorded stream fixtures `crates/rade-core/tests/fixtures/voice/claude-stream.jsonl` (message 1 ends `error_max_turns` after 3 Reads, message 2 answers `applyPromo`) and `codex-exec.jsonl`. When parsed. Then message 1 gives activities `Read src/checkout/useTotals.ts`, `Read src/checkout/promo.ts`, `Read src/checkout/CheckoutForm.tsx` and a max-turns end, message 2 gives deltas `appl`, `yPromo` and final text `applyPromo`; Codex gives `Ran cat src/checkout/useTotals.ts` and its last agent message as final text; a final text wrapped in a code fence is unwrapped.
- R124-g [M46] (unit): Given a Voice Composer model. When F8 is pressed and released after 400 ms, pressed and released after 100 ms twice, auto-repeat presses arrive, Esc is pressed in each state, and a turn passes `timeout_secs`. Then the states and actions follow D2, D11, D13 and D17 (table in `voice/composer_tests.rs`).
- R124-h [M46] (unit): Given draft `Fix A in x.ts.\n- add a test` and revision `Fix A in x.ts.\n- keep the API`. When changed spans are computed. Then exactly the words `keep the API` are marked.
- R124-i [M46] (unit): Given `stt_key_command = "echo sk-test"`. When the STT request is built. Then the key appears only in the curl config text (never in argv), the form has `model`, `prompt` and `response_format`, `language` only when not `auto`; `{"text":" ừ thì sửa cái này "}` parses to `ừ thì sửa cái này` and a partial's last 3 words are tentative.
- R124-j [M46] (unit): Given defaults. When `[voice]` is read and the Settings rows are listed. Then the defaults are D-table values, the Voice rows exist with §T2's bounds, and a `.rade.toml` with `[voice]` is explained as `ignored (user config only)`.
- R124-k [M46] (e2e): Given an Agent Pane whose fake Claude session file holds `handleSubmit`, a fake STT and a fake voice agent. When F8 is held for 3 s and released. Then the composer shows `Listening` and at least 2 live partials while held, the Statusline shows `REC`, the STT `prompt` contained `handleSubmit`, the voice agent's first message held the session text and `git status`, the draft streamed in with activity `Read …` and the state is `Ready`.
- R124-l [M46] (e2e): Given a Ready draft edited by hand. When F8 is held again and released. Then the draft was `stale` while held, the agent's `REVISE` message carried the edited draft, the new draft is shown with `changed` marks, the header says `revision 1` and the same voice agent process answered (one pid).
- R124-m [M46] (e2e): Given a Ready draft on a sink Agent Pane (`cat -v`). When Enter is pressed. Then the Pane received the draft in bracketed paste followed by Enter, the composer is closed and the voice agent process is gone; Alt+Enter on another draft pastes without Enter.
- R124-n [M46] (e2e): Given the fake voice agent logging its env and running `rade status --hook`. When a draft is made. Then its env has no `RADE_PANE_ID` / `RADE_SOCKET`, the Pane's status did not change, and no file was created under `$XDG_RUNTIME_DIR`, `$TMPDIR` or the Claude config dir. With `REAL=1` and `claude` installed: a real Haiku draft leaves no new `.jsonl` under the account's `projects/`.
- R124-o [M46] (e2e): Given `timeout_secs = 10` and a voice agent that hangs after one delta. When a take is drafted. Then after 10 s (±2 s) the state is `Timed out` with that delta kept as the draft; changing Voice › `Max steps` to 3 on the Settings page writes `max_turns = 3` under `[voice]` (other lines unchanged) and the next voice agent's argv has `--max-turns 3`.
- R124-p [M46] (e2e): Given no STT server. When a take ends. Then the state is `Can't transcribe` with `No speech-to-text server at 127.0.0.1:<port>`; a failing `record_command` gives `Can't record: …`; `Set up voice…` opens the Settings page filtered to `voice`.
- R124-q [M46] (e2e): Given a Shell Pane and an Agent Pane. When F8 is pressed on the Shell Pane, the Agent Pane's mic button is clicked twice, and F8 is pressed while another Pane's composer is open. Then a Toast `Voice works on Agent Panes`; the click toggles a take on then off; the open composer moves to the focused Agent Pane only after it is closed (the press is ignored while it is open on another Pane and a Toast `Voice is open on <pane>` shows).
- R124-r [M46] (e2e): Given scenes I-a, I-c and I-e in dark and light at 1440 × 900. When `debug.ui geometry` measures them. Then every row of `docs/internals/design/spec/i-voice.md` is within tolerance and the screenshots match the approved goldens `tests/e2e/golden/m46/`.
- R124-s [M46] (unit): Given takes of silence, of room noise at -50 dBFS, of 0.5 s of speech within room noise, of a 100 ms blip, of steady noise at -26 dBFS and of steady loud speech at -10 dBFS. When their speech is measured. Then only the 0.5 s take and the loud one hold speech; and Whisper's made-up outros (`Hãy subscribe cho kênh Ghiền Mì Gõ…`, `Cảm ơn các bạn đã theo dõi!`, `Thanks for watching!`) are dropped from a transcript while `thêm nút subscribe vào trang settings` stays.
- R124-t [M46] (e2e): Given a silent microphone. When F8 is held and released. Then the composer says `Didn't catch that` and no speech-to-text request was made. Given the composer open, when the Pane is focused again (a model sync), then the keyboard stays in the card; a press on the draft box lands in the card (GTK's pick), not on the Terminal, and typing edits the draft.

## R127 — Updates (M48, ADR-0048)

The owner's request of 2026-10-26: see the version, learn about a newer release, install it from the app, restart when it suits.

1. **Who and why.** RADE ships as a `.deb` on GitHub Releases with no apt repository, so `apt upgrade` never brings a new RADE. Many Agents run in it at once: nothing may interrupt them — no modal, no automatic install, no automatic restart.
2. **Check.** One `GET https://api.github.com/repos/richard-plg/rade/releases?per_page=20` (no token, `User-Agent: rade/<version>`, nothing else sent) 60 s after start and then at most once a day (an hour after a failed check), from the Installed flavor only; `[update] check = false` stops the automatic checks, `RADE_UPDATE_CHECK=0` stops all of them. The answer and the time are cached in `~/.cache/rade/update/update.json`, shared with `rade version`.
3. **Which release.** The highest non-draft release above the running version with `rade_<v>_amd64.deb` and `SHA256SUMS` assets; `[update] channel = "stable"` (default) skips pre-releases, `"rc"` includes them. Versions compare per SemVer (`1.1.0-rc.1 < 1.1.0`, build metadata ignored). Kind: major / minor / patch; security when a release in between lists `### Security` (a `Security: yes` commit footer, git-cliff).
4. **Where it shows.** A Statusline cell only while there is something to say (`1.2.0`, `1.2.0 security`, `2.0.0 major`, `1.2.0 42%`, `restart for 1.2.0`, `update failed`), dropping after the monitor and before ports; one Activity row per version; one Toast only for a security release, held while an Agent waits; **Settings › Updates** with the About block (version, status, last check, Install / Skip This Version / Release notes / Check now) and the rows Check for updates and Update channel; palette **About RADE**, **Check for Updates**, **Restart to Finish Update…**.
5. **Install** (a click, `.deb` installs only): download into the cache with progress and Cancel, SHA-256 equal to `SHA256SUMS` and to the API's asset digest, then `pkexec /usr/bin/apt-get install -y --no-remove <deb>` (the system's password dialog). Cancelling the dialog goes back to Install; a failure says why. The running app keeps working on its loaded files; the new `rade` CLI talks to it.
6. **Restart.** The cell turns `restart for <v>` (also when the owner installed the package by hand). A click asks first, counting running Agents (they stop and come back interrupted through Session Restore) and waiting ones; Restart saves, quits and a helper starts the new binary once the old process is gone. Not now keeps everything running. The first start on the new version shows `Updated to RADE <v>` once.
7. **Other installs.** A Devel build never checks (About: `Development build`). A RADE not installed from the `.deb` is told about updates and gets Download (the release page) instead of Install.
8. **CLI.** `rade version [--check] [--json]`: `rade <cli>`, then `app <v> (pid <n>)` (`· restart RADE to finish updating` when it differs) or `app older than 1.2.0 (no app.info) · restart RADE to finish updating` or `app not running`, then `update <v> available (<kind>[, security]) · checked <ago>` / `update <v> skipped · checked <ago>` / `up to date · checked <ago>` / `not checked yet` / `last check failed: <error> · <ago>` (an offer known from the cache wins over a failed check's line; `<ago>` = `just now` / `5m ago` …). Socket `app.info` gives the app's version and pid. A call that fails with `unknown method` adds `rade: this rade is <cli>, the running RADE app is <v|older>: restart RADE to finish updating`.

AC:
- R127-a [M48] (unit): Given version strings `1.2.0`, `1.1.0`, `1.1.0-rc.1`, `1.1.0-rc.2`, `1.1.0-rc.10`, `v1.1.0`, dpkg `1.1.0~rc.1`, `1.1.0+git202610261200.abc1234`, `1.1`, `x`. When they are parsed and compared. Then `1.1.0-rc.1 < 1.1.0-rc.2 < 1.1.0-rc.10 < 1.1.0 < 1.2.0`, the tag and dpkg forms equal their SemVer forms, the build form equals `1.1.0`, the deb name of `1.1.0-rc.1` is `rade_1.1.0~rc.1_amd64.deb`, and `1.1` and `x` are not versions.
- R127-b [M48] (unit): Given the release-list fixture (1.0.0, 1.0.1, 1.1.0, 1.2.0-rc.1 as pre-release, a 1.3.0 draft, a 1.2.1 without the `.deb`, a 1.2.2 whose `.deb` URL is on another host). When the offer is picked for 1.0.0 on `stable`, for 1.0.0 on `rc`, for 1.1.0 on `stable`. Then it is 1.1.0 (minor) with notes `# What's new since RADE 1.0.0`, `## RADE 1.1.0 · 2026-10-26` before `## RADE 1.0.1 · 2026-10-25` and no `Install:` line; then 1.2.0-rc.1; then none.
- R127-c [M48] (unit): Given the fixture with `### Security` in 1.0.1's body, and a 2.0.0 added. When offers are picked for 1.0.0, 1.0.1 and 1.1.0. Then from 1.0.0 the offer is security, from 1.0.1 it is not, and 2.0.0 from 1.1.0 is major.
- R127-d [M48] (unit): Given no cache, a good check 23 h 59 min ago, 24 h ago, a failed check 59 min and 60 min ago, a broken `update.json`. When `due` is asked. Then due, not due, due, not due, due, due (the broken file reads as empty); a written cache reads back equal and is written through a temp file + rename.
- R127-e [M48] (unit): Given a file, its `SHA256SUMS` line and the asset digest, then a wrong line, a wrong digest, a file missing from `SHA256SUMS`, no digest; and asset URLs on `github.com/richard-plg/rade/releases/download/<tag>/`, on another repo, on `127.0.0.1:<port>` with and without `RADE_UPDATE_URL` on that port. When verified / allowed. Then only the matching file passes (no digest: `SHA256SUMS` alone decides); only the right GitHub URL is allowed, and the local one only with the override.
- R127-f [M48] (unit): Given the Installed flavor at `/usr/lib/rade/rade-app` with a dpkg version, at `~/.cargo/bin/rade-app`, the Devel flavor, the Devel flavor with `RADE_UPDATE_URL`, and `RADE_UPDATE_INSTALL`; install exits 0, 126, 127, 100 with `Could not get lock`, 100 with `E: Broken package`, and `/usr/lib/rade/rade-app (deleted)`. When the install kind, the install outcome and the restart path are worked out. Then `deb`, `other`, `devel`, `other`, `deb`; done, cancelled, `Not authorized to install software.`, `Another package manager is running. Try again in a minute.`, `E: Broken package`; and `/usr/lib/rade/rade-app`.
- R127-g [M48] (unit): Given every phase of §5 with and without security / major / skipped, for kinds `deb` and `other`. When the cell and the About block are built. Then their texts, icons, classes and buttons are §5.1 / D16's, and the cell is absent in `devel`, `off`, `idle`, `checking` (without an offer) and for a skipped offer.
- R127-h [M48] (unit): Given Agent facts with 0 running and 0 waiting, 1 running, 3 running and 5 waiting (`a` … `e`). When the restart body is built. Then it is D20's text for each (`… waiting for you: a, b, c +2. Session Restore reopens them as interrupted.`).
- R127-i [M48] (e2e): Given a fake GitHub serving a release one minor above the running version, `RADE_UPDATE_DELAY=2`. When the app starts. Then after one request (User-Agent `rade/<version>`, no Authorization) the cell reads `<v>`, Activity has `RADE <v> is available`, Settings › Updates shows `RADE <v> is available` with Install and Skip This Version; a relaunch within the day sends no request and shows the cell from the cache; with `[update] check = false` a relaunch sends no request even when due (cache aged 25 h by `debug.update`'s `age` op), the cell still shows from the cache, and Check now sends one; with `RADE_UPDATE_CHECK=0` nothing is sent and Check now is off.
- R127-j [M48] (e2e): Given the fake release marked security and an Agent Pane waiting. When the check finds it, then the Agent stops waiting, then the app relaunches. Then no Toast while it waits, one `RADE <v> fixes a security issue` Toast after, none after the relaunch; Skip This Version hides the cell and survives a relaunch, and a higher release later shows again.
- R127-k [M48] (e2e): Given the fake release with a 2 MB `.deb`. When Install is clicked with the fake installer exiting 0, then (fresh = app stopped, `dpkg-version` back to the running version, `$XDG_CACHE_HOME/rade-dev/update` removed, app started) with a wrong `SHA256SUMS`, with the installer exiting 126, and 1. Then `debug.update`'s `phases` log reads `available, downloading, verifying, installing, restart_pending` and the cell was seen as `<v> <pct>%` and `<v> installing` (slow fake: `slow` file + `install-sleep`) before `restart for <v>` and the installer got the `.deb` path whose SHA-256 matches, the `.deb` is deleted afterwards; the wrong sums show `Download didn't match SHA256SUMS. Nothing was installed.` without calling the installer; 126 returns to Install with no error; 1 shows `update failed` with the installer's last line.
- R127-l [M48] (e2e): Given restart pending with one Agent running and one waiting. When the cell is clicked (`debug.update cell_click`), Not now, then Restart. Then the dialog reads D20's two lines; Not now keeps the same pid; Restart ends the old pid and within 20 s a new app answers `app.info` with another pid, `rade list` shows the same Panes (both Agents `interrupted`), and `rade version` reads `app <v> (pid <new>)`.
- R127-m [M48] (e2e): Given the fake installed-version query printing a version above the running one (a hand install). When a check runs. Then the cell reads `restart for <that version>` with no download.
- R127-n [M48] (e2e): Given no app, then the app, then a fake socket answering `unknown method` to every call. When `rade version`, `rade version --json`, `rade version --check` and `rade list` run. Then the texts of item 8 (`app not running`, `app <v> (pid <n>)`, `update <v> available (minor) · checked just now`), the JSON keys of API.md, exactly one request for `--check` (which then reads `checked just now`), and the `restart RADE to finish updating` line on stderr for the fake socket.
- R127-o [M48] (e2e): Given the Devel build without `RADE_UPDATE_URL`, and a non-`deb` install (no `RADE_UPDATE_INSTALL`, URL set). When Settings › Updates opens. Then `Development build` with no request; then `RADE <v> is available` with Download (opens the release page, `debug.update` records the URI) instead of Install.
- R127-p [M48] (unit+e2e): Given cells Workspace, Worktree, counts, ports, update, identity, a monitor and two hints with widths that fit only after dropping 1, 3, 4 and 5 cells; and in the app the cell in each phase and the About block in `available` / `restart_pending`, dark and light, and the window at 1280 px (`debug.ui layout`) with the cell `available`. When `statusline_fit` runs and screenshots are taken. Then the drop order is hints → monitor → update → ports → identity; in the app at 1280 px `statusline.update` is the cell's text (not dropped, not clipped) and the images are written and looked at.

## R125 — Agent Accounts: env, user accounts, detection (M47, ADR-0047)

1. **Env per account.** `config_dir` may be left out or empty: the account uses the agent's default login (`~/.claude` / `~/.codex`) and its Panes get `CLAUDE_CONFIG_DIR` / `CODEX_HOME` removed rather than set (owner decision 2026-10-25). `[[account]]` takes `env_file` (`~` expanded) and `[account.env]` (one key per variable). `env_file` is a dotenv file: `KEY=VALUE`, an optional leading `export `, `'…'` (literal) or `"…"` (`\n`, `\"`, `\\`) values, `#` comment lines, blank lines; no `$VAR` interpolation, no multi-line values; any other line is an error naming the file and line. Keys are `[A-Za-z_][A-Za-z0-9_]*`; the agent's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` and `RADE_*` are refused (validation error naming the file and key). A Pane with the account gets, later winning: the Workspace's `[env]`, then `env_file`, then `[account.env]`, then the config-dir variable and `RADE_ACCOUNT` (R108.2).
2. **Read, never stored.** `env_file` is read when a Pane is spawned and when it is restored. A missing, unreadable or broken file refuses the spawn with `USAGE` naming the file (and line), no Pane; at Session Restore or resume (R19.3) the Pane comes back as a plain shell in its cwd that prints `rade: not resumed: <reason>` (as R108.7). No value of `env_file` is written to `state.json`, logs, Activity, `debug.*` answers, `account.list` or Settings; only its keys are shown.
3. **User accounts.** `$XDG_CONFIG_HOME/rade/accounts.toml` holds `[[account]]` tables of the same shape (the file is shared by the installed and dev builds, like `config.toml`). Every Workspace may use them besides its own (`scope`: `workspace` | `user`), unless its `workspace.toml` sets top-level `accounts = "own"` (`"all"` is the default; another value is a validation error): then `--account <user account>` there → `FORBIDDEN` (exit 7). One (agent, name) in both files → a validation error naming both files. The default per agent: the Workspace's `default = true`, else the user file's, else the first listed, Workspace accounts listed first. A Workspace with no account of its own for an agent whose `[env]` sets that agent's `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (R23, `rade workspace setup`) keeps that login by default: the user account on that dir is its default, and with none on it the agent has no default account (its Panes keep `[env]`); `--account` still picks any user account. `rade setup` treats user accounts' `config_dir` like Workspace ones (R108.5). `rade account usage` lists a user account once. Two accounts on one login (a written `~/.claude` and a default-login account) are one quota: the forecast Toast (R110.4) never offers one instead of the other, and Settings says so on both.
4. **Detection.** When the session-start hook reports an invocation (R11.2) in a Pane without an account, for a Preset of kind `claude` / `codex`: its recorded `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (unset → `~/.claude` / `~/.codex`) is compared by `realpath` with the accounts the Pane's Workspace may use; one match — several: those whose dir is written when the variable is set, those on the default login when it is unset, then the default among them, else none — becomes the Pane's account with `account_detected: true` in `state.json`. It counts as the Pane's account for the Statusline, usage, Subagents (R108.2) and restore, injects nothing into the running Agent, and is cleared when that Agent ends or its restore is refused. A Pane with an account is never re-detected.
5. **CLI / socket (additive).** `rade account add … [--user] [--env-file <path>] [--env KEY=VALUE]…`; `rade account set <name> [--agent A] [--user] [--env-file <path> | --no-env-file] [--env KEY=VALUE]… [--unset-env KEY]…` (`account.set`); `rade account list` shows `user` / `workspace`; `account.list` rows gain `scope`, `env_file` and `env_keys` (keys of `[account.env]` and of the file, never values) and `pane.list` / `rade list` rows gain `account_detected`.
6. **Detailed limits** (R109.5) for an account whose env sets `CLAUDE_CODE_OAUTH_TOKEN`: not requested; the Usage page and `usage.get` say `not available with a token login`.

AC:
- R125-a [M47] (unit): Given a dotenv text with `export A=1`, `B='x $y'`, `C="a\nb"`, a comment, a blank line, then a text with a line `oops`. When each is parsed. Then the first gives A=1, B=`x $y`, C=a⏎b, and the second an error naming line 1.
- R125-b [M47] (unit): Given Workspace `[env]` `A=ws`, `B=ws`, an account with `env_file` setting `B=file`, `C=file` and `[account.env]` `C=inline`, and separately an account whose `env` sets `CLAUDE_CONFIG_DIR` and one whose file sets `RADE_X`. When the Pane env is built and the files are validated. Then A=ws, B=file, C=inline, then the config dir and `RADE_ACCOUNT`; the two others give validation errors naming the file and the key.
- R125-c [M47] (e2e): Given a temp HOME with user account `clw` (claude, `~/.claude-work`, `env_file` with `CLAUDE_CODE_OAUTH_TOKEN=tok-…`). When `rade spawn --agent claude --account clw --cmd env` runs. Then the Pane has the token, `CLAUDE_CONFIG_DIR=$HOME/.claude-work` and `RADE_ACCOUNT=clw`, and the token string is in none of `state.json`, `rade account list --json`, `rade debug state`.
- R125-d [M47] (e2e): Given `clw` with its env file removed. When it is spawned, then a Pane on `clw` is restored after SIGKILL, then the file is put back and it is spawned again. Then the first exits 2 naming the file and creates no Pane, the restored Pane is a plain shell that printed `rade: not resumed:` naming the file, and the last Pane has the token.
- R125-e [M47] (unit+e2e): Given user accounts `cl` (default) and `clw`, Workspace `acme` with its own `acme-work` (claude, default) and Workspace `corp` with `accounts = "own"`. When the lists are built and `--account clw` is spawned in each. Then `acme` lists `acme-work` (default), `cl`, `clw`; `default` lists `cl` (default), `clw`; the spawn in `corp` exits 7 naming `clw` and `corp`; one name in both files is a validation error naming both files.
- R125-f [M47] (e2e): Given a temp HOME with user account `clw` at `~/.claude-work`. When `rade setup --dry-run` runs. Then its diff installs the hooks into `~/.claude-work`.
- R125-g [M47] (unit+e2e): Given user accounts `cl` (`~/.claude`) and `clw` (`~/.claude-work`) and a shell Pane. When a fake claude Agent started by hand there reports `CLAUDE_CONFIG_DIR=~/.claude-work`, it spawns a Subagent, and it ends; and when another reports no `CLAUDE_CONFIG_DIR`. Then the Pane has account `clw` with `account_detected: true`, the Subagent has `RADE_ACCOUNT=clw`, the account is cleared at the end, and the second gets `cl`.
- R125-h [M47] (unit): Given `accounts.toml`. When it exists next to a `config.toml` without new keys. Then `Config` parses `config.toml` unchanged (no account key in it), and an `accounts.toml` with an unknown top-level key still loads its accounts.
- R125-i [M47] (unit): Given accounts `cl` with the token in `[account.env]` and `clw` with it in `env_file`. When the Detailed limits request is decided. Then neither is requested and both say `not available with a token login`.
- R125-j [M47] (unit+e2e): Given user accounts `cl` (`~/.claude`, default), `clw` (`~/.claude-work`), `clv` (`~/.claude-vm25`) and Workspace `viact` with `[env] CLAUDE_CONFIG_DIR = "~/.claude-work"` and no account. When its accounts are listed and an Agent is spawned there without `--account`, then `[env]` names a dir no account has. Then `clw` is viact's default and the Pane has `RADE_ACCOUNT=clw`; then there is no default and the Pane keeps `CLAUDE_CONFIG_DIR` from `[env]`, while `--account clv` still works.
- R125-k [M47] (unit): Given `cl` (`~/.claude`) and `clp` (no dir) and `clw` (default). When hand-started Agents report `CLAUDE_CONFIG_DIR=$HOME/.claude` and no variable, a Preset prefix `CLAUDE_CONFIG_DIR=~/.claude-work` is checked against `clp`, and the forecast Toast is built for `cl` running hot. Then they are detected as `cl` and `clp`; the refusal names `~/.claude (default login)` and says to remove the prefix from `[agents.claude] command`; and the Toast offers `clw`, never `clp`.

## R126 — Settings → Accounts (M47, ADR-0047)

Owner review 2026-10-25: a list of accounts, each with its environment, easy to add to; the config dir is just one of its variables.

1. **Master–detail.** The Settings page (R112) gains **Accounts**: on the left a list of every account the shown Workspace may use, grouped by agent (`CLAUDE CODE`, `CODEX`), each row its name, its config dir (muted; `default login` when empty), `✓` on the default, a `.env · n vars` badge and a lock on a Workspace's own account; **+ New account** at the bottom. On the right the selected account. Below a page width of 700 px the list sits above the detail. Searching the page matches account names, agents and variable keys.
2. **Detail.** Header: name, agent, `yours · every Workspace` or `from <file>`, a *Default* switch, a `Saved` mark after each write, and a menu (*Duplicate*, *Delete…*, *Open accounts.toml*). One **Environment** table in the order an Agent gets it: first the login row `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (pinned, not removable, a folder chooser; empty = the agent's default login `~/.claude` / `~/.codex`), then the `.env file` row (path + chooser) with its keys under it, read-only, values shown as `••••` and `from .env` (a key also set below is struck through, `overridden`), then the account's own variables `KEY` · value · ✕. A value whose key contains `TOKEN`, `SECRET`, `KEY` or `PASSWORD` is a password entry with a peek button. **+ Add variable** inserts an empty row in place with the cursor in `KEY` (Tab → value, Enter saves, Esc drops it); **Paste KEY=VALUE lines** adds every variable of the clipboard's dotenv text at once. A refused key (R125.1) shows its error under that row and writes nothing. A Workspace's own account is read-only, with `Declared in <file>` and *Open file*.
3. **New account.** **+ New account** adds a draft row to the list, selected, with the cursor in its name; the detail asks for the agent and the login row; *Create* (or Enter) writes it, Esc drops the draft. Nothing is written before *Create*.
4. **Writes keep the place.** Each change is one `toml_edit` write of `accounts.toml` (atomic, parsed first, comments and other bytes kept, as R112.3). A value edit commits on Enter or when focus leaves and redraws nothing else; a structural change (a row added or removed, the env file, default, a new or deleted account) redraws only the list and the detail, keeping the page's scroll position and putting the cursor where the work continues. Running Panes keep their env; the detail says `New Agents use these settings`.
5. **Debug.** `debug.ui {"op": "app_settings_accounts", …}` (test mode) returns the list, the selection, the detail's Environment rows (key, source `login` | `file` | `own`, overridden, secret; never a value) and the page's scroll position, and drives selection, drafts and edits.

AC:
- R126-a [M47] (unit): Given an `accounts.toml` with comments and two accounts. When an account is added (also without a config dir), its config dir changed and cleared, a variable set, changed and unset, an env file set and cleared, the default moved and an account deleted. Then each write keeps the comments and other bytes, the last unset removes an emptied `[account.env]`, clearing the config dir removes the key, and a refused key or a broken file writes nothing.
- R126-b [M47] (e2e): Given a temp HOME with user accounts `cl`, `clw` (an env file) and Workspace account `acme-work`. When Settings → Accounts opens, `clw` is selected, a variable `FOO=bar` is added, `RADE_X` is tried, two lines are pasted, a new account `cx` is drafted and created, and a new Pane is spawned on `clw`. Then the list shows `cl ✓`, `clw` with `.env · 1 vars`, a locked `acme-work`; `clw`'s Environment rows are the login row, the file's `CLAUDE_CODE_OAUTH_TOKEN` (from .env, no value) and `FOO`; the page's scroll position is unchanged by the edits; `RADE_X` shows an error and writes nothing; the pasted variables are written; `cx` is written only on Create and becomes selected; the new Pane has `FOO=bar`; screenshots in dark and light.
- R126-c [M47] (unit): Given an account with `env_file` keys `A`, `B`, own variables `B`, `C` and no config dir. When its Environment rows are built. Then they are the login row (empty, `default login ~/.claude`), `A` and `B` from the file with `B` overridden, then `B` and `C`, none carrying a file value.

## R128 — One Agent per Pane (M49, ADR-0049)

The owner's review of 2026-10-29: a Claude Pane sometimes shows the Codex glyph, and after a restart resumes the wrong thing. In 1.0–1.3 a `codex exec` run by a Pane's Claude (an image skill's `tools/img.sh`) whose parent had gone (`setsid -f`, `nohup … &`) passed the CLI's process-tree check (R10, `nested_agent`), relabelled the Pane `codex`, replaced its session and its recorded invocation, and Session Restore then resumed the Codex job instead of Claude.

1. **Reporter.** `rade status --hook` also sends `reporter` = the nearest ancestor inside the Pane (its environment has the Pane's `RADE_PANE_ID` / `RADE_SOCKET`) running a Preset's program other than a shell (`binary`, its base name), and whether that run is one-shot (`headless`: Codex `exec`, Claude `-p` / `--print`). Nearest first, so an orphaned Agent is still named.
2. **One Agent per Pane.** While a Pane has a live Agent (a Preset, status not `exited`), a report with a `reporter` is dropped whole — status, session id, Preset, invocation, account detection (R125.4) — when it comes from another kind of agent: the kind (as the glyphs, R115.1) of the Preset it names, else of its `reporter`, differs from the Pane's, unless that kind runs in the Pane's foreground when the report arrives (read then, as R115.4 reads it). A one-shot `reporter` is dropped in a Pane whose own command or recorded invocation is not one-shot. Same kind (`/clear`, `/resume`, a Preset alias), no reporter (`rade status` by hand, an older CLI), or a Pane or report of unknown kind (a wrapper Preset such as `zsh -ic 'clv'` before its invocation is recorded): taken as before. `pane.set_status` then answers `{"pane", "ignored": "another agent" | "one-shot run"}`.
3. **Activity.** The first dropped report of each (Pane, session) adds `<pane> ignored a <agent> session (started by its Agent)` (kind `ignored`, robot icon); a click opens the Pane.
4. **Restore repair.** A Pane whose recorded invocation is one-shot comes back as a plain shell in its cwd that prints `rade: not resumed: the last session recorded here was a one-shot codex run (codex exec)` (or `claude -p`), its Preset, session and invocation cleared, its name kept: state written by 1.0–1.3 never re-runs a Codex job.

AC:
- R128-a [M49] (unit): Given a Claude Pane with session `S1` running. When reports arrive from a one-shot Codex (session start `S2`, then `done`), from an interactive Codex (same), from Claude (`S3`), then `exited` and Codex `S4`. Then the Codex ones are dropped as `one-shot run` / `another agent`, the Pane keeps `claude` / `S1` / `running`, then takes `S3`, then after `exited` becomes `codex` / `S4`.
- R128-b [M49] (unit): Given a Claude Pane whose foreground is `codex` (then `claude`, then nothing), a one-shot Pane (`codex exec` invocation), a Pane of wrapper Preset `clv` = `zsh -ic 'clv'`, and reports with no reporter, an unknown Preset or an unknown reporter. When reports arrive. Then Codex takes the first Pane only while `codex` is in its foreground, the one-shot Pane takes its own one-shot report, `clv` takes Claude's reports (and drops Codex's once its `claude` invocation is recorded), and the others are taken.
- R128-c [M49] (unit): Given `codex exec …`, `codex --no-daemon exec …`, `codex -m x exec`, `node …/codex.js exec`, quoted `'codex' 'exec'`, `claude -p`, `CLAUDE_CONFIG_DIR=… claude --print`, `claude --print=json`, and `codex`, `codex resume X`, `claude --resume X`, `claude 'fix exec bug'`, `zsh -ic 'clv …'`, `bash agent.sh -p`. When `is_headless` reads them. Then the first eight are one-shot and the others are not.
- R128-d [M49] (unit): Given a Pane whose invocation is `codex --no-daemon exec … -C /x/img '$imagegen …'` and a Claude Pane with `CLAUDE_CONFIG_DIR=~/.claude-work claude --dangerously-skip-permissions <prompt>`. When Session Restore prepares them. Then the first is a shell with the notice, no Preset, session or command, its name kept; the second runs `CLAUDE_CONFIG_DIR='…' 'claude' '--dangerously-skip-permissions' --resume '<id>'`.
- R128-e [M49] (e2e): Given three Claude Panes (`claude` = tests/e2e/fake-agent.sh under that name) whose Agent runs a script starting `codex --no-daemon exec …` (the fake under that name) in the foreground, in the background with `wait`, and orphaned with `setsid -f`. When each Codex reports session start, `running` and `done`. Then every Pane keeps `claude`, its session, a `claude` invocation and the Claude glyph, and Activity has one `ignored` line, for the orphaned run.
- R128-f [M49] (e2e): Given R128-e. When the app is killed with SIGKILL and started again. Then each Pane runs `claude … --resume <its session>` and no `codex`.

## R129 — Files actions (M49)

The owner's review of 2026-10-29: the Files tree had no way to copy a path, reveal a file, refresh or see where its root is. Amends R8. New / Rename / Delete stay out (Non-goal: editing files in the app).

1. **Header** (after the title): `Show ignored` (as before), **Refresh** (`Refresh`: relists the tree and its git status, keeping expanded folders and the scroll), **Collapse All** (`Collapse All`).
2. **Root path.** The title is the root folder's name; its tooltip is the full path; a click copies the full path and shows the Toast `Copied <path>`.
3. **Row menu**, for files and folders: files `Open`, `Open to the Side`; both `Copy Path`, `Copy Relative Path` (relative to the tree's root), `Insert Path in Agent`; both `Reveal in Files`, `Open Terminal Here` (a file: its folder). `Insert Path in Agent` types the relative path, shell-quoted as a paste (R5), and a space, without Enter, into the active Tab's focused Pane when it is an Agent Pane, else the active Worktree's most recent Agent Pane; disabled when there is none. `Reveal in Files` opens the system file manager with the item selected (`org.freedesktop.FileManager1.ShowItems` via GTK); a failure shows `Couldn't open the file manager`. `Open Terminal Here` opens a new Tab in the active Worktree whose Terminal starts in that folder.
4. **Drag** a row onto a Terminal: the R5 drop (its path, quoted).
5. **Keys** on the focused row: `Ctrl+Alt+C` Copy Path, `Ctrl+Shift+Alt+C` Copy Relative Path; `F5` Refresh while the Files page has focus.

AC:
- R129-a [M49] (unit): Given a root and nested files, the root itself and a path outside it. When `relative_path` and `insert_text` run. Then nested paths are relative, the root is `.`, outside is none, and `a b.rs` becomes `'a b.rs' ` quoted as a paste.
- R129-b [M49] (e2e): Given the Files page on a temp repo. When `debug.ui files` reads the header and the menus of a file and a folder are read. Then the header has Refresh, Collapse All and Show ignored; the file menu has the seven items in order and the folder menu the five.
- R129-c [M49] (e2e): Given a file row. When Copy Path, Copy Relative Path and a click on the title run. Then the clipboard holds the absolute path, the relative path, then the root, and the Toast says `Copied <root>`.
- R129-d [M49] (e2e): Given a fake Agent Pane, then none. When Insert Path in Agent runs on `src/a b.rs`. Then the Agent's input shows the quoted path and a space and no prompt was submitted; with no Agent Pane the item is disabled.
- R129-e [M49] (e2e): Given file watches off (test mode). When a file is created, then Refresh, then Collapse All. Then the file appears only after Refresh with expanded folders kept, and only top-level rows remain.
- R129-f [M49] (e2e): Given `src/` and a fake `org.freedesktop.FileManager1` on the private bus. When Open Terminal Here and Reveal in Files run. Then a new Tab's Pane has cwd `<repo>/src`, and `ShowItems` was called with the file's URI. Screenshots of the page and a menu, dark + light.

## R130 — Staged Session Restore (M50, ADR-0050)

The owner's report of 2026-10-30: after the update to 1.4.0 the window showed "Not Responding" at every launch. Their state held 62 Agent Panes in 7 Workspaces (12 more than 1.2.0 resumed, since R125's user accounts); Session Restore realized every Terminal in one main-loop callback, blocking the window for ~5.5 s on a headless run of that state (3.5 s for a single socket call) while 62 agents started at the same instant (4–5 min of CPU in 16–21 s, a 12–15 GB peak). Amends R11.2 and the "started even if never shown" rule (R2.8) for restored Panes only.

1. **Waiting Panes.** Every Terminal Pane of a launch's restored state waits in a queue (in memory only: state.json, the socket and `rade list` are unchanged). A waiting Pane has its widget but no surface and no process; its model (Agent, session, invocation, account, status) is what R11.2 made it.
2. **Order.** The focused Pane, the rest of the Tab on screen, the shown Worktree's other Tabs, the shown Workspace (its active Source first), then the other Workspaces in their order. Panes of paused Sources run nothing (R19.3) and are not queued; Viewer / Review / Diff / Browser Panes are built at once.
3. **Pace.** The queue starts once the window is shown, one Terminal per main-loop turn at low priority (input, frames and the compositor's ping first), waiting `AGENT_GAP_MS` = 300 ms after each Agent; shells follow at once. Everything still runs in the background (R23: the other Workspaces keep running).
4. **Out of turn.** A Pane that comes on screen (a Tab, Worktree or Workspace switch, a focus, the Overview) starts as GTK shows it; a socket call that targets a waiting Pane (`send`, `read`, `wait`, `focus`, and everything else that needs its Terminal) starts it first. A waiting Pane that is closed never runs; a quit keeps the waiting Panes' sessions for the next launch.
5. **Panes made after launch** (spawn, split, new Tab, a paused Source resumed) start at once, as before.

AC:
- R130-a [M50] (unit): Given Workspaces `default` (Source `x`), `a` (`api`, `web`) and `b` (`ops`) with Agents, a shell Tab and a Viewer in `web`, Workspace `a` shown with `web`'s second Agent focused, then `api` paused. When `order` runs. Then it is the focused Agent, its Tab's other Agent, `web`'s shell Tab, `api`'s Agent, then `x`'s and `ops`'s; the Viewer is not in it; with `api` paused its Agent is not either.
- R130-b [M50] (unit): Given a queue of 5 Panes, the third a shell. When `next` runs at 0, 100, 300, 600, 600 and 900 ms and the fourth is promoted at 150 ms. Then the first starts at 0, 100 waits 200 ms, the second starts at 300, the shell at 600 and the fifth right after it, then the queue is done; the promoted Pane leaves the queue once.
- R130-c [M50] (e2e): Given 62 fake Claude Agents with a turn in 7 Workspaces (one Source paused), `w3` shown. When the app is relaunched while `tests/e2e/probe-latency.py` calls `pane.list` every 50 ms. Then after the first frame no call waits over 250 ms (a `perf_check`), the queued Agents of `w3` start before any other Workspace's, and every Agent of an unpaused Source runs `claude --resume <its session>` within 35 s.
- R130-d [M50] (e2e): Given R130-c's relaunch with the queue still running. When `rade send` targets a waiting Pane of `w5` and `w4` is switched to with one of its Panes focused. Then the text reaches `w5`'s Agent and no Pane of the Tab shown in `w4` is still waiting a second later.
- R130-e [M50] (e2e): Given a relaunch with 20 or more Agents still waiting after 1 s. When the app quits and starts again. Then state.json holds the same session ids as before, and each Agent that was waiting resumes.
- R130-f [M50] (e2e): Given a paused Source and a waiting Pane closed right after launch. When the queue has run. Then neither the paused Source's Agents nor the closed Pane started a process.

## R131 — A folder without git in a Project (M51)

The owner's report of 2026-10-30: a new folder added in New Project could not be linked ("not a git repository") and nothing helped. CONTEXT.md lets a Source be a plain folder, but a Source link needs a base branch.

1. **Nothing is left behind.** A folder or `owner/repo` added in New Project or Add Source is registered as a Source for the dialog's plan; on Cancel (or when its row is not saved) it is forgotten again. A cloned folder stays on disk.
2. **Initialize.** A row whose plan is refused with `not a git repository` shows `Not a git repository` and an **Initialize** button: one confirmation naming the folder and the command, then only `git init -b <the row's base>` — no file added, no commit — and the row is planned again.
3. **A repository without commits** (`git init`, HEAD unborn): a link whose base is the unborn branch uses the main checkout (it is on its base) and can be created. What needs a commit is refused with `USAGE` `<folder> has no commits yet: commit once, then try again` and 0 commands: a Project checkout for another base, a new Worktree (`git worktree add` has nothing to start from).
4. **The CLI names the fix**: a plain folder's link is refused with `<folder>: not a git repository (run: git init -b <base>)` (`project plan` exits 2, R98.3); the CLI never runs git for it.

AC:
- R131-a [M51] (unit): Given an empty folder and a `git init -b main` folder with an untracked file. When they are observed and links at `main` on both and at `develop` on the second are resolved. Then the first is not git and is refused with the `git init` hint, the second has `unborn = main`, its `main` link uses the main checkout with 0 commands, and its `develop` link is refused `has no commits yet`.
- R131-b [M51] (e2e): Given New Project with a new empty folder added. When the row is read and the dialog cancelled. Then the row shows Initialize, Create is disabled, and after Cancel the folder is no Source and no Project file exists.
- R131-c [M51] (e2e): Given that row again with `a.txt` in the folder. When Initialize is confirmed and Create pressed. Then HEAD is `refs/heads/main` with no commit, `a.txt` is untracked, the row plans the main checkout, and the link is written with the Source registered.
- R131-d [M51] (unit+e2e): Given that Project on a repository with no commit. When a Worktree is added on its link, then again after one commit. Then the first is refused `has no commits yet: commit once, then try again` with nothing created, and the second works.
- R131-e [M51] (e2e): Given a plain folder registered as a Source. When `rade project plan` links it. Then the output names `not a git repository (run: git init -b main)` and no file is written.

## R132 — Worktrees between Projects, Scratch (M51)

From the owner's UX review of 2026-10-30.

1. **Move to Project asks about the base.** When the target Project's link has another base than the one the Worktree compares against (its recorded base, else its link's, else the Source's main branch), one question: `Move “<worktree>” to <Project>?`, buttons `Keep comparing against <old>` (default) and `Compare against <new>` (re-records the Worktree's base; commits are never rebased). Same base, or Scratch: no question, the base is kept. `rade worktree move <source> <worktree> --project <p> | --scratch [--rebase-to-link]` (method `worktree.move`) does the same; a Project that doesn't link the Source, or a main checkout, is refused with `USAGE`.
2. **Suggestions only on a real match.** A Scratch Worktree's `→ <Project>?` (R104.1): a Project linking its Source whose base is the branch itself, or one of whose `/`-separated parts is the Project id, alone or before `-` / `_` / `.` (`shop/coupons`, `feat/shop-x`; not `shopping-cart`, not `maintenance` for `main`); two Projects match → no suggestion. A Source only one Project links still suggests it.
3. **Words.** The Move to Project submenu's last item is **Scratch** (✓ when the Worktree is in none), never "No Project"; `scripts/check-removed.sh` fails on a `"No Project` string literal in code.
4. **Close Panes in Scratch…** replaces Remove from Scratch… (R104.5), with the title `Close <n> Pane(s) of “<source>”?`; what it does is unchanged.
5. **Project home's Open terminal** with several links shows them (`<source> · <base>`, the one used last first) instead of opening on the first link; with one link it opens at once. The button looks the same (spec B).

AC:
- R132-a [M51] (unit+e2e): Given Projects linking a Source at `master` / `develop` and ids `beacon`, `atlas`, `shop24`. When branches `masterclass`, `master-old`, `feat/beaconing`, `beacon`, `feat/beacon_x`, `beacon/atlas-x`, `shopping-cart`, `shop24/fix` are suggested. Then only `beacon`, `feat/beacon_x` and `shop24/fix` get a Project, `beacon/atlas-x` none (two match).
- R132-b [M51] (e2e): Given `checkout-v2` in Shop (develop) and Shop24 linking the Source at `release/2.4`. When it is moved to Shop24 (Cancel, then Compare against), back with `rade worktree move` without and with `--rebase-to-link`, then to Scratch. Then the dialog names both bases and buttons Cancel / Keep / Compare, Cancel moves nothing, the records go `shop24@release/2.4`, `shop@release/2.4`, `shop@develop`, `null@develop`, and a Project not linking the Source or the main checkout is refused (exit 2).
- R132-c [M51] (e2e): Given that Worktree's row. When its menu is read and check-removed runs on a tree with a planted `"No Project{}"` label. Then the submenu is `Shop ✓`, `Shop24`, `Scratch`, and the script fails.
- R132-d [M51] (e2e): Given a shell in a linked Source's main checkout under Scratch. When its row's menu is read and the item activated. Then the item is `Close Panes in Scratch…` and the dialog title `Close 1 Pane of “shared-ui”?`.
- R132-e [M51] (unit+e2e): Given Project Trio with 3 links and Solo with 1. When Trio's home is read, a Terminal opened on `shop-api`, the home read again and Open terminal clicked on each. Then the menu is the 3 links in table order, then `shop-api` first; the Terminal runs in shop-api's checkout; on Trio the click shows the menu and opens nothing, on Solo it opens a Terminal.

## R133 — Remove a Project, prune Project checkouts (M51)

1. **Remove Project…** (Project menu; `rade project remove <p> [--dry-run] [--keep-checkouts]`, method `project.remove`). One confirmation lists, paths relative to the Workspace root: the Project's folder (deleted with `project.toml`; "This can't be undone"), its Project checkouts that no other link uses and that are clean (removed with `git worktree remove`, never forced), those with uncommitted changes (kept, named), the Panes running in a removed checkout (closed), and its Worktrees (moved to Scratch with their base, never removed). Its records and place in the Project order go.
2. **Prune.** `rade project prune [--workspace <ws>] [--dry-run]` (method `project.prune`) and the palette's **Prune Unused Project Checkouts…** remove every clean git worktree under `<folder>/.projects` that no link of the Workspace resolves to; dirty ones are listed and kept. `rade project link remove` / `set-base` print `1 Project checkout no longer used: <path> (rade project prune)` when they leave one behind.
3. **Back to base** refused because the base is checked out in an unused Project checkout adds `— an unused Project checkout; rade project prune removes it`.

AC:
- R133-a [M51] (unit+e2e): Given Project checkouts of Shop24 (clean) and an old Project (dirty), an ordinary Worktree, and Shop's checkout still in use. When the orphans are listed; and, in the app, Store's remove is dry-run with a clean unshared checkout, a checkout shared with Billing, a Pane in the first and a Worktree. Then the orphans are the two `.projects` folders with their dirty counts, and the dry run removes only the unshared checkout, closes 1 Pane, moves 1 Worktree, and writes nothing.
- R133-b [M51] (e2e): Given that plan. When Remove Project is confirmed. Then the Project's folder is gone, its unshared checkout is removed from disk and `git worktree list`, the shared one stays, the Pane is closed, the Worktree is in Scratch with its base and the Navigator has no row for it.
- R133-c [M51] (e2e): Given Billing's link moved to another base and Rel's dirty checkout. When `set-base` runs, Rel is removed, the palette's Prune… is opened and `rade project prune` runs with and without `--dry-run`. Then `set-base` reports the unused checkout, Rel's is kept with why, the palette's dialog and the dry run list the clean one and keep the dirty one, and prune removes only the clean one.
- R133-d [M51] (e2e): Given Store. When Remove Project… is chosen from its menu. Then the dialog `Remove Project “Store”?` says it can't be undone and lists the checkout, the Panes and the Worktree moving to Scratch.

## R134 — Ctrl+click on a wrapped path or URL (M51)

The owner's report of 2026-10-30: in a narrow Pane a path or URL continues on the next row, and Ctrl+click opened only the clicked row's piece (or nothing).

1. Ctrl+click reads the clicked row and up to 8 rows above and below. While the token ends its row within 3 columns of the Terminal's width — a soft wrap, or Claude Code / Codex breaking a long token at the width with the next row indented — the next row's first word is appended; the same backwards. A URL takes only pieces that look like more of a URL (`/ ? = & % # . - _`), and a URL on the row above is joined only to such a piece.
2. Of those joins the longest that opens something wins — a URL, or a path that exists (R7.1 resolution) — else what Ghostty matched, else the clicked word (`rade_core::link::token_across`).
3. Ghostty's own match is held during the click and opened only when no longer join opens.

AC:
- R134-a [M51] (unit): Given rows with a soft-wrapped path, a TUI-wrapped URL with an indent, a path over 3 rows, two edge words that open nothing, a short row under a wider one, a URL followed by plain words, and a short row ending in a URL above a plain word (m4's OSC 8 line). When `token_across` runs on a cell of each. Then the whole path / URL comes back (from either row), the clicked word alone where nothing opens or the row is not at the edge, and the URL without the plain words.
- R134-b [M51] (e2e): Given a Terminal that prints an existing path longer than its width, and a TUI-shaped text breaking a path and a URL 2 columns short of the width with an indent. When `rade debug click` Ctrl+clicks a later row of the path, each row of the URL and the second row of the wrapped path. Then the Viewer opens the whole path each time and the stand-in browser gets the whole URL each time.

## R135 — Selected text in the Viewer is visible (M51)

The owner's report of 2026-10-30: text in the Viewer (Markdown, HTML, code) "can't be selected or copied". Reproduced on a headless GNOME Shell (Wayland) with real pointer and key input: drag, double-click, Ctrl+C and the context menu's Copy all worked, but the selection was drawn almost in the page's color (`term-sel` #19403a, an opaque Terminal color, which WebKit and GtkSourceView draw translucent: (12, 25, 25) on (7, 9, 11)).

1. Selections in the Viewer, Review and Diff Panes use the token `view-sel`: the accent, translucent (dark `#4fe3c852`, light `#0b8f7b40`), for `::selection` in Markdown and the GtkSourceView schemes' `selection` and `selection-unfocused`. Text on it keeps ≥ 4.5:1 (`contrast.py`). The Terminal keeps `term-sel`.

AC:
- R135-a [M51] (e2e): Given a Markdown and a Rust file in the Viewer, dark. When a paragraph / a line is selected and Copy runs. Then the shot has 150 or more pixels more of the selection color than before, and the pasted clipboard is the selected text.
- R135-b [M51] (unit): Given both themes. When `view-sel`, the scheme XML and the Markdown page CSS are read. Then `view-sel` is `#rrggbbaa` with alpha 0x30–0x70, not `term-sel`, and it is the scheme's `selection` and `selection-unfocused` and the page's `--sel`.

## Non-goals (v1)

- Editing files in the app (Viewer is read-only).
- Linear / issue-tracker integration. (Commit / push / PR became R21 in M14, human-confirmed: ADR-0013.)
- Processes surviving app restart (PTY daemon) — designed for (ADR-0005), not built; the owner dropped it on 2026-10-02 after reviewing the design (Session Restore with resume stays the answer).
- X11 support, Flatpak/Snap packaging, macOS/Windows.
- MCP server, SSH remote, mobile — later candidates (the Browser Pane and port detection became R18, M12).
- New branch across sources (one branch name cut in several links of a Project from each link's base) — later (design §8, M33 plan §2.1).
- PR status on Worktrees (`#482 ✓` through the Workspace's `gh`) — later (design §8, M33 plan §2.1).
