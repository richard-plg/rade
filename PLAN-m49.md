# M49 — Pane identity hardening, Files actions, Tab glyphs (implementation plan)

Status: **implemented** (approved 2026-10-29, Q1–Q3 = defaults; results and deviations in docs/internals/checks/M49.md — the R128 rules ended up kind-based with a `reporter` from the CLI, see PRD R128 / ADR-0049). Base: `main` @ v1.3.0 (957592b).

Source: the owner's UX review of 2026-10-29:
1. Files lacks open folder / file path / refresh.
2. "`rade spawn` of claude sometimes shows the Codex icon."
3. "Resume after closing the app sometimes fails / does not follow the account."

Everything below is fixed unless listed under §7 Open questions. **Implementers make no product or
design decision**: if something isn't covered or the code contradicts this plan, stop and ask.

---

## 0. Rules for whoever implements a task

- Read `CLAUDE.md` first: Conventional Commits with R-IDs, tests first for core logic,
  `cargo clippy --workspace --all-targets` clean, `cargo test --workspace` green.
- Only your task's files. No drive-by refactors.
- Core tests named after the AC: `fn r128_a_…`; E2E checks `check "R128-c …"` (`scripts/trace.sh`).
- After each task: `cargo build --release && cargo clippy --workspace --all-targets && cargo test
  --workspace`, then commit with the given message.
- Never run `rade setup` for real, never push, never touch `~/.local/share/rade/state.json`.

## 1. Scope, IDs, release impact

- Milestone **M49**; PRD **R128** (Agent identity of a Pane), **R129** (Files actions); **R115.3
  amended** (Tab glyphs). ADR **0049** (one Agent per Pane: who may relabel it).
- Release impact: **MINOR** (new UI actions; `debug.ui` shapes grow). The agent-facing CLI/socket
  contract is unchanged (`pane.set_status` keeps its params; it may now answer
  `{"ignored": "<reason>"}` instead of `{}` — additive, documented in docs/API.md).
- Out: New / Rename / Delete file (PRD Non-goal "Editing files in the app").

---

## 2. Findings this plan rests on (verified 2026-10-29)

**F1 — a nested Agent took over Claude Panes.** In the owner's state (`rade` 1.2.0, installed
2026-10-26, so the `nested_agent` guard of c5b7166 was in place), Pane `p_1301` (`b1-k2`, command
`zsh -ic 'clv "…"'`, `clv` = `CLAUDE_CONFIG_DIR=~/.claude-vm25 claude`) records
`agent = "codex"`, the session id of a `codex exec` run, and invocation
`codex --no-daemon exec … -C …/img "$imagegen …"` (from `tools/img.sh`, which the livestream skill
runs from Claude's Bash tool). `p_1285`, `p_468`, `p_667` are the same shape (all `codex … exec …`).
`p_1285`'s restart command is already `codex … -C …/img resume 7d6a…`: a restore brought that
Claude Pane back as a Codex image job. This is both symptoms 2 (glyph = `agent` first,
`design.rs` `Pane::mark`) and 3 (`agents.rs` `restore_command` = `agent` + `invocation` +
`session_id`).

**F2 — the guard works when the process chain is intact, and is blind when it isn't.**
- Repro A (passes): fake `claude` parent → real `codex --no-daemon exec -s workspace-write` with a
  fake `RADE_SOCKET`: 0 reports. Control without the `claude` parent: 3 reports
  (`agent: codex`, `running`, `done`, each with Codex's session id).
- Repro B: from an orphaned process (`(nohup … &)`) the ancestor walk ends at `systemd --user`
  (2919, environ unreadable) → `nested_agent` sees one Agent → the report goes through.
- Claude's `run_in_background` keeps the chain (checked). **Which exact shape orphaned the
  `codex exec` in `p_1301` is not reproduced yet** → Task 1 pins it with an E2E; the fix in Task 2
  does not depend on it (it doesn't trust the process tree alone).

**F3 — accounts on restore are fine for the owner's setup.** Every Pane has `account = null` (aliases,
not Agent Accounts); restart commands keep the right `CLAUDE_CONFIG_DIR` (`.claude`,
`.claude-work`, `.claude-vm25`) + `--resume <id>`. The "wrong account" seen was F1 (a Codex job
instead of Claude). R125.4 account detection reads the same polluted invocation, so F1 also
attaches a Codex account to a Claude Pane → fixed by the same filter.

**F4 — Tab glyph = the lead Agent only** (`tabs.rs` ~448): Codex spawning Claude into its own Tab
keeps the Codex glyph on the Tab. Correct per R115.3 but reads as a bug.

**F5 — Files** (`files.rs`): header = name + "Show ignored"; menu only on files (folders return
early, ~270) with Open / Open to the Side; no copy path, reveal, terminal here, refresh, collapse,
drag, full root path.

---

## 3. R128 — Agent identity of a Pane (PRD text to add)

1. **One Agent per Pane.** A hook report changes a Pane's Agent identity (`agent`, `session_id`,
   `invocation`, detected account) only when one of these holds:
   a. the Pane has no live Agent: no `agent`, or status `None` / `exited`, or no `session_id`;
   b. the report names the Pane's own Preset (same agent: `/clear`, `/resume`, a restart);
   c. the Pane's foreground program (R115.4 `detected`) is now the reported Preset (an Agent
      started by hand after another one ended without an `exited` report).
   Otherwise the **whole report is dropped** (status included: it describes the other Agent),
   `pane.set_status` answers `{"ignored": "another agent"}`, and Activity gets one line per Pane
   and foreign session: `Ignored <preset> session in <pane name> (started by its Agent)`.
2. **Status-only reports from another session.** A report without a Preset whose `session_id`
   differs from the Pane's recorded one, while the Pane has a live Agent, is dropped the same way
   (Codex's `running` / `done` carry their own session id).
3. **Headless runs never become the Pane's Agent.** An invocation that is a one-shot run —
   Codex `exec` (first non-flag argument), Claude `-p` / `--print` — never replaces a recorded
   interactive invocation, whatever 1 says; `rade_core::preset::is_headless(preset, argv)`.
4. **Session Restore never re-runs a headless invocation.** A Pane whose recorded invocation is
   headless comes back as a plain shell in its cwd that prints
   `rade: not resumed: the last session recorded here was a one-shot <agent> run (codex exec)`
   (`Pane.notice`, as R108.7 refusals), keeping its name; `agent` / `session_id` / `invocation`
   are cleared. This repairs state written by 1.0–1.3 (`p_1285`, `p_1301`, `p_468`, `p_667`).
5. The process-tree guard (`nested_agent`) stays as the first filter in the CLI.

AC:
- R128-a (unit, core): Given a Pane `claude` + session `S1`, status `running`. When reports arrive:
  `agent codex, S2` → dropped; `status done, S2` (no agent) → dropped; `agent claude, S3` → taken
  (S3); after `exited`, `agent codex, S4` → taken. Then `agent`/`session_id`/`status` are exactly
  those, and one Activity line was produced for S2 (not two).
- R128-b (unit, core): Given a shell Pane whose `claude` ran then ended without an `exited`
  report and `detected = codex`. When `agent codex, S5` arrives. Then it is taken (rule 1c).
- R128-c (unit, core): `is_headless`: `codex exec …`, `codex --no-daemon exec …`,
  `codex -m x exec`, `claude -p hi`, `claude --print` → true; `codex`, `codex resume X`,
  `codex --yolo`, `claude --resume X`, `claude "fix exec bug"` → false.
- R128-d (unit, core): Given the 4 polluted Panes copied (trimmed) from the owner's state into
  `crates/rade-core/tests/fixtures/state-1.3.0-nested-codex.json`. When `prepare_restore` runs.
  Then each comes back a shell with the notice, no restart command; the healthy `claude` Panes of
  the fixture keep `CLAUDE_CONFIG_DIR=… claude --dangerously-skip-permissions --resume <id>`.
- R128-e (e2e, `tests/e2e/m49.sh`): Given a Pane spawned with
  `rade spawn --agent claude --cmd "zsh -ic 'fake-claude'"` (fake-agent.sh as `claude`), whose
  fake Claude runs `tools/img.sh`-shaped script that starts `fake-agent.sh codex exec` (a) in the
  foreground, (b) via `run_in_background`-like `&` + `wait`, (c) orphaned via `setsid`/`nohup &`.
  When each fake Codex reports SessionStart / running / done. Then `rade list --json` keeps
  `agent: claude` and the Claude session in all three, Activity has the Ignored line for (c) only
  (a, b stopped by the CLI guard), and `debug.ui pane` shows the Claude glyph.
- R128-f (e2e): kill -9 the app after R128-e, relaunch: the Pane resumes the Claude session (fake
  agent's `--resume` arg logged), not Codex.

---

## 4. R129 — Files actions (PRD text to add)

1. **Header** (right to left after the title): `Show ignored` (as now), **Refresh**
   (`view-refresh-symbolic`, tooltip `Refresh`: relists the tree and git status, keeps expanded
   folders and scroll), **Collapse All** (`ph-arrows-in-line-vertical-bold-symbolic` if bundled,
   else `view-restore-symbolic`; tooltip `Collapse All`).
2. **Root path.** The title keeps the folder name; its tooltip is the full path; clicking it copies
   the full path and shows Toast `Copied <path>`.
3. **Row menu** (files **and folders**), sections:
   - files: `Open`, `Open to the Side` | both: `Copy Path`, `Copy Relative Path`,
     `Insert Path in Agent` | both: `Reveal in Files`, `Open Terminal Here` (folders; for a file:
     its folder).
   - `Copy Relative Path` is relative to the tree's root.
   - `Insert Path in Agent` types the relative path, shell-quoted via `paste::escape_paths`,
     followed by a space, **without Enter**, into the Agent Pane focused last in the active Tab
     (else the active Worktree's most recent Agent Pane); disabled (greyed) when there is none.
   - `Reveal in Files` = `gtk::FileLauncher::open_containing_folder` (file selected in the file
     manager); a failure → Toast `Couldn't open the file manager`.
   - `Open Terminal Here` = a new Tab in the active Worktree whose Terminal starts in that folder
     (`State::new_tab_with` with the folder as root).
4. **Drag** a row onto a Terminal = the R5 drop (text/uri-list → quoted path); the row is a
   `gtk::DragSource` offering `gdk::FileList` + plain text.
5. **Keyboard** on the focused row: `Ctrl+Alt+C` Copy Path, `Ctrl+Shift+Alt+C` Copy Relative
   Path (VS Code's keys), `F5` Refresh while the Files page has focus.

AC:
- R129-a (unit, core `filetree.rs`): `relative_path(root, p)` for nested files, the root itself
  (`.`), a path outside the root (None); `insert_text` quotes `a b.rs` → `'a b.rs' `.
- R129-b (e2e, m49.sh): Given the Files page on a temp repo. When `debug.ui files` reads the
  header and `debug.menu files <path>` the menus of a file and a folder. Then the header lists
  `refresh`, `collapse`, `ignored`; the file menu has the 7 items in order, the folder menu the 5.
- R129-c (e2e): Copy Path / Copy Relative Path → `wl-paste` (headless clipboard) equals the
  absolute / relative path; title click → clipboard = root, Toast text.
- R129-d (e2e): Insert Path in Agent with a fake Agent Pane → its screen shows `src/a\ b.rs `
  (no newline: the fake agent logs no submitted line); with no Agent Pane the item is disabled.
- R129-e (e2e): a file created with inotify watches exhausted (`RADE_FILES_NO_WATCH=1`, test only)
  is absent, then present after Refresh; expanded folders stay expanded; Collapse All → only
  top-level rows.
- R129-f (e2e): Open Terminal Here on `src/` → a new Tab whose Pane cwd (`rade list --json`) is
  `<repo>/src`. Reveal in Files with a fake `org.freedesktop.FileManager1` on the private D-Bus →
  `ShowItems` called with the file URI.
- Screenshots: Files page + menu, dark + light → `target/e2e-m49/`.

---

## 5. R115.3 amendment — Tab glyphs

A Tab with Agents of more than one kind shows the lead Agent's glyph followed by each other
distinct kind's glyph in Pane order, at most 3 glyphs (12 px for the extra ones, 2 px apart), each
coloured by the worst status of that kind in the Tab; the Tab tooltip lists `name · agent ·
status` per Agent Pane. One kind → unchanged.

AC:
- R115-f (unit, core `chrome.rs`): `tab_marks(panes, grid)`: [codex lead, claude, claude] →
  [Codex, Claude]; 4 kinds → first 3; no Agents → []; worst-of status per kind.
- R115-g (e2e, m49.sh): Codex fake Agent spawns `--agent claude` into its Tab → `debug.ui tabs`
  shows marks `["codex","claude"]`; screenshot dark + light.

---

## 6. Tasks (in order)

| # | Task | Files | Commit |
|---|------|-------|--------|
| 1 | **Repro first**: `tests/e2e/m49.sh` part `identity` with cases R128-e (a)(b)(c) + R128-f; fixture script `tests/e2e/fake-img.sh`. Expected now: (c) fails (Pane relabelled codex). Record the result in the PR description. | tests/e2e/m49.sh, tests/e2e/fake-img.sh | `test(R128): nested codex exec relabels a Claude Pane (failing)` |
| 2 | R128.1–.3 core: `State::set_status` returns `Applied \| Ignored(reason)`; rules 1a–c, 2, 3; `preset::is_headless`; Activity line (new `activity::Kind::Ignored`). Unit tests R128-a/b/c first. | rade-core: agents.rs, preset.rs, activity.rs (+ tests) | `fix(R128): an Agent started by a Pane's Agent no longer takes over the Pane` |
| 3 | R128.1 app: `pane.set_status` handler — skip `set_invocation`, `accounts::detect`, `turn_ended` when ignored; answer `{"ignored": …}`; push the Activity line. docs/API.md note. | rade-app: handlers.rs, activity_view.rs; docs/API.md | `fix(R128,R125.4): ignored reports set no invocation or account` |
| 4 | R128.4 restore repair + fixture test R128-d. | rade-core: agents.rs (`restore_pane`), tests/fixtures/ | `fix(R128,R11.2): never resume a one-shot codex exec / claude -p run` |
| 5 | R115.3 Tab glyphs: `chrome::tab_marks` (unit R115-f), `tabs.rs` renders up to 3 glyphs + tooltip, `debug.ui tabs` gains `marks`. | rade-core: chrome.rs; rade-app: tabs.rs, agent_glyph.rs, style.css | `feat(R115): a Tab shows every kind of Agent it holds` |
| 6 | R129.1–.2: Refresh, Collapse All, title tooltip + copy; `RADE_FILES_NO_WATCH` (test mode only). | rade-app: files.rs | `feat(R129): Files Refresh, Collapse All and the root path` |
| 7 | R129.3: menus for files and folders, Copy (Relative) Path, Insert Path in Agent, Reveal in Files, Open Terminal Here; `filetree::relative_path` (unit R129-a); `debug.menu files`. | rade-core: filetree.rs; rade-app: files.rs, debug.rs | `feat(R129): copy, insert, reveal and terminal-here from the Files tree` |
| 8 | R129.4–.5: drag source, keys. | rade-app: files.rs, keys.rs | `feat(R129): drag a file into a Terminal, copy-path keys` |
| 9 | E2E rest of m49.sh (R129-b…f, R115-g, screenshots), PRD R128/R129/R115.3 text, ADR-0049, MILESTONES M49, docs/features/viewer-files-search.md + agents.md, docs/keybindings.md, CLAUDE.md command line for m49.sh, `scripts/demo/` files scene re-run + `NO_RUN=1 scripts/demo/collect.sh`, `docs/internals/checks/M49.md`, `scripts/trace.sh`. | docs/, tests/e2e/m49.sh, CLAUDE.md, scripts/demo | `docs(M49): checks, PRD, ADR-0049, user docs` |

Tasks 2–4 (identity) and 5–8 (UI) touch disjoint files after Task 1 and may run in parallel
worktrees. A `reviewer` pass (opus) on 2–4 before commit — it changes Session Restore.

## 7. Open questions (owner)

- **Q1 — "open folder" in Files.** Default in this plan: `Reveal in Files` (system file manager) +
  `Open Terminal Here`. Alternative: an `Open Folder…` header button that picks a folder and adds
  it as a Source (the existing Add Source flow). Keep the default, add the alternative too, or
  replace?
- **Q2 — Insert Path format.** Default: relative path, shell-quoted, trailing space. Alternative:
  `@relative/path` (Claude Code / Codex file mentions). Pick one.
- **Q3 — Dropped reports visibility.** Default: one Activity line per foreign session. Also a
  Toast? (Default: no.)
