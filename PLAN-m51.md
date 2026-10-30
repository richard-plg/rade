# PLAN M51 — Projects and Scratch fixes, Project lifecycle, Tab titles

Status: **implemented** (approved 2026-10-30: Q1 delete, Q2 `git init` only, Q3 24, Q4 in; branch `m51-projects-scratch` from `m50-staged-restore`, released together as **1.5.0**). Checks: docs/internals/checks/M51.md. PRD text: R131–R135 and the R103.2 amendment (the PRD is the source of truth; this plan keeps the reasoning).

**Built differently from this plan:**
- **R131.1 — no pending rows.** The dialog's plan (`project.plan`) needs a registered Source, so a folder is still registered when added; the dialog remembers what it added and forgets each one on Cancel or when its row isn't saved. Same outcome for the user.
- **R131.3 — no "Create first commit".** Q2 = `git init` only; a repository with no commit links on its main checkout (the unborn branch is its base) and a Worktree / Project checkout says to commit first.
- **R131.4 — `rade project add` still writes** a link on a plain folder (scripts rely on it writing the file); `rade project plan` and `project show` carry the `git init` hint and `plan` exits 2.
- **R132.2 — no `<base>/` prefix rule, no fork-point rule**: git can't have `develop/x` next to `develop`, and the fork point needs git per row; the rule is the base itself or the Project id as a word. A Source only one Project links still suggests it (the old `only_project` fallback).
- **R132.5 — the button looks the same** (no split arrow: golden B stays); a click with several links opens the menu.
- **Method names**: `worktree.move` (not `worktree.set_project`).
- **R133.1 — no Fan-out refusal**: Fan-outs belong to a Source, not a Project.
- **R133.2 — Prune's UI is a palette action** (*Prune Unused Project Checkouts…*), not a Workspace menu item.
- **R134 — edge rule**: within 3 columns of the Terminal's width only. "The block's widest row" failed with a long shell command on screen, and "as far right as the rows next to it" joined m4's OSC 8 click into `…?q=1clearosc8-guide`; Claude Code / Codex break long tokens at the width anyway. A URL takes only URL-looking pieces, in both directions.
- **R135 — the cause was visibility, not input.** Reproduced on a headless GNOME Shell (Wayland) with Mutter RemoteDesktop input and under Xvfb with XTest: drag, double-click, Ctrl+C and the context menu's Copy all worked; the selection was drawn as (12, 25, 25) / (1, 14, 8) on the page's (7, 9, 11). Fix: token `view-sel`. The keyboard (Ctrl+C reaching a Terminal) was never the problem, so R135's old AC c is gone.
- **Also seen, not fixed**: an HTML file without its own styles renders black text on the dark Viewer (WebKit's default colors on RADE's page background) — a follow-up.

Source: the owner's UX review of 2026-10-30
(Projects / Scratch, Vietnamese conversation) and four owner reports from the same day:
a new folder added in New Project fails because it has no git; Tabs all read `claude`;
Ctrl+click on a path or URL wrapped over several rows opens only the first row; text in the
Viewer (Markdown, HTML, …) can't be selected and copied.
Target release **1.5.0** (MINOR: new CLI command `rade project remove` and socket method
`project.remove`, compatible additions only, ADR-0041).

R-IDs: **R131** (Source folders without git), **R132** (Worktree ↔ Project moves, Scratch),
**R133** (Project lifecycle), **R103.2 amendment** (Tab label = the lead session's title),
**R134** (Ctrl+click on a wrapped path / URL), **R135** (select and copy in the Viewer).

---

## 1. Findings (verified in code, 2026-10-30)

**F1 — a folder without git can't join a Project, and nothing helps.**
New Project → Add source… → A folder… → an empty folder:
- `project_dialogs.rs:1047` `add_folder` registers the folder as a Source **at once** and saves
  state, before Create. Cancel keeps the Source in the Workspace.
- `checkout.rs:327` refuses the link: `<path>: not a git repository` (red plan text).
- `project_dialogs.rs:870–882`: any refused included row disables **Create Project**.
- No fix is offered: the user must leave, `git init`, commit, come back.
- CONTEXT.md says a Source is "a git repository (or plain folder)", and the dialog offers
  "a folder": the UI invites what the link model refuses.
- **Unverified**: a folder with `git init` but no commit (no branch, no `refs/heads`). The base
  dropdown and the Project checkout's `git worktree add` probably fail late. Task 0 reproduces it.

**F2 — Move to Project keeps the old base.** `worktree.rs:127` `set_worktree_project` sets only
`project`. A Worktree moved from Shop (`develop`) to `Shop · 2.4` (`release/2.4`) is listed
under 2.4 but its Review, `+/−` and Create PR still compare against `develop`. No confirmation.

**F3 — the `→ <Project>?` suggestion matches loosely** (`design.rs:90` `suggest`):
`branch.starts_with(base)` (base `main` ⇒ `maintenance-x`; `develop` ⇒ `develop-old`), any
branch part starting with the Project id (`shop` ⇒ `shopping-cart`), and the first Project by
id wins. One click on a wrong suggestion moves the Worktree (F2 applies too).

**F4 — the retired word "No Project"** is a menu item: `sidebar.rs:932` (Move to Project
submenu). CONTEXT.md retired it for **Scratch**; `scripts/check-removed.sh` doesn't list it.

**F5 — "Remove from Scratch…" closes Panes and stops Agents**, but reads like hiding a row
(`sidebar.rs:948`, R104.5).

**F6 — no way to remove a Project.** CLI: `list/show/add/plan/link/set`; Project menu
(`projects.rs:689`): settings, add source, new Worktree, open file, customize. The guide tells
users to create extra Projects (`Shop · 2.4`), so they only accumulate.

**F7 — Project checkouts are never cleaned up.** `link remove`, `set-base` and a deleted
`project.toml` leave `<root>/.worktrees/.projects/<p>/<s>`; it is skipped by `worktree.list`
and there is no prune. An orphan **still holds its branch**, so `to-base` / `git switch <base>`
elsewhere is refused with "checked out in another checkout", naming a folder the UI never shows.

**F8 — Project home's primary "Open terminal" opens on the first link silently**
(`home_view.rs:384`); with 3 links the user can't tell which repo they are in.

**F9 — Tabs show the Preset, not the session.** `tabs.rs:470–476`: the lead Pane's label is
`p.name` else `p.agent_name()` (`claude`). Every Claude Tab reads `claude`; the session title
(`Tính năng projects trên SCRATCH`) is only in the Pane header and the Navigator
(`title.rs:138` `session_title`, R104.7). With several Tabs, finding one means clicking each.

**F10 — Ctrl+click reads one row.** `terminal/links.rs:46` `row_text` reads the clicked viewport
row only, and `link::word_at` cuts the word there. A path or URL longer than the Pane is wide
continues on the next row(s), so only the first piece is opened (or nothing: a cut path doesn't
exist). Two shapes:
- **Soft wrap** (a shell printing a long line): Ghostty marks the row as wrapped; reading the
  rows as one selection joins them without a newline.
- **Hard wrap by the TUI** (Claude Code, Codex): they break lines themselves and write each row
  separately, often with an indent on the continuation row. Ghostty sees two lines; only a
  heuristic can join them.
Ghostty's own URL matcher (the `OPENED` path) runs first; whether it handles soft wraps is checked
in Task 0.

**F11 — the Viewer's text can't be selected / copied** (owner report; Markdown, HTML view, …).
Not found by reading: the Viewer has no `user-select` CSS, its key handler (`viewer.rs:583`
`on_key`) doesn't take Ctrl+C, and the text view is `set_editable(false)` (selectable). Suspects:
the root's capture-phase click (`viewer.rs:567`: `code_pop::close` + `focus()` on every press,
during a WebView drag-select), the window-level key handling sending Ctrl+C / Ctrl+Shift+C to the
last Terminal, and no context menu (Copy) on the WebView. Task 0 reproduces each content kind
(code, Markdown, HTML, plain text) with a drag + Ctrl+C + `wl-paste` before any fix.

---

## 2. Design

### R131 — Source folders without git (New Project / Add Source)

1. **Nothing is registered before Create.** A folder or `owner/repo` picked in New Project or
   Add Source is a *pending* row of the dialog; the Source is added (`add_source_in`) only when
   Create / Add runs. Cancel adds nothing. (A clone of `owner/repo` still happens at pick time —
   it needs the branches for the base list — and its folder is kept on Cancel, but no Source is
   registered.)
2. **Not a git repository** → the row's plan text reads `Not a git repository` with an
   **Initialize** button. Initialize (one confirmation naming the folder and the command) runs
   only `git init -b <base>` (base from the row, default `main`); no commit, no `git add`. The
   row re-plans.
3. **An unborn branch is a valid base for the main checkout.** `observe` records an unborn HEAD
   (`git symbolic-ref HEAD` with no commit) as `unborn = Some(<branch>)`. A link whose base is that
   branch resolves to the **main checkout** (it is on the base) and Create is enabled; this covers
   both a fresh Initialize and a folder that was `git init`ed by hand. What needs a commit says
   so instead of failing late:
   - a **Project checkout** or a **new Worktree** on that link → refused with `USAGE`
     `<folder> has no commits yet: commit once, then try again`, 0 commands (`git worktree add`
     can't start from an unborn branch);
   - Project home / `project show`: HEAD `<branch> (no commits)`, ahead/behind `–`, chip `clean`
     or `n changes`; Review and Changes compare against the empty tree.
   An unborn branch that is not the link's base is refused like any other branch mismatch (R98.1).
4. The CLI keeps refusing a plain folder (`rade project add` / `link add` with `USAGE`), now with
   the fix in the message: `<folder>: not a git repository (run: git init -b main)`. The CLI never
   runs git for you.
5. CONTEXT.md **Source**: "a git repository (or plain folder; a plain folder can't be linked to
   a Project until it is a git repository)".

AC:
- R131-a [M51] (unit): `checkout::observe_one` on an empty folder → `git = false`; on
  `git init -b main` with no commit → `git = true`, `unborn = Some("main")`. Plans: the plain
  folder is refused with the hint, 0 commands; the unborn repo at base `main` → main checkout,
  0 commands; at base `develop` → refused; a second link at another base (needs a Project
  checkout) → refused `has no commits yet`.
- R131-b [M51] (e2e): Given New Project with a new empty folder picked. When the dialog is
  cancelled. Then `rade source list` doesn't list it and `state.json` is unchanged.
- R131-c [M51] (e2e): Given the same row with a file `a.txt` in the folder. When Initialize is
  confirmed. Then `git -C <folder> symbolic-ref HEAD` is `refs/heads/main`, `git log` has no
  commit, `a.txt` is untracked, the row's plan is `main checkout · on main`, Create is enabled,
  and Create writes the link and registers the Source.
- R131-d [M51] (e2e): Given that Project. When New Worktree… runs on the link. Then it is refused
  with `has no commits yet`, nothing created; Project home shows HEAD `main (no commits)`; after
  one commit by hand, New Worktree works.
- R131-e [M51] (e2e): `rade project add X --source <empty>` exits 2 with the hint text, no file written.

### R132 — Worktree ↔ Project moves and Scratch

1. **Move to Project asks about the base** when the target link's base differs from the
   Worktree's recorded base: dialog `Move “checkout-v2” to Shop · 2.4?` with
   `Keep comparing against develop` (default) / `Compare against release/2.4` (re-records the
   base; commits are not rebased). Same base → no dialog. Moving to Scratch keeps the base.
   CLI: `rade worktree move <source> <worktree> --project <p>|--scratch [--rebase-to-link]`
   (method `worktree.set_project`).
2. **Suggestion only on a real match** (`design::suggest`): the branch's upstream is
   `<remote>/<base>`-tracked from a link's base, or the branch was created from a commit that is on
   exactly one linked base (`merge-base --is-ancestor` of the fork point), or a `/`-separated
   segment **equals** the Project id. No prefix matches. Two Projects match → no suggestion
   (the submenu still lists both).
3. The submenu's last item is **Scratch** (✓ when none), never "No Project"; add `No Project` and
   `General` to `scripts/check-removed.sh`'s UI-string check.
4. **"Remove from Scratch…" → "Close Panes in Scratch…"**; the dialog title
   `Close 2 Panes of “shared-ui”?`, body naming the Agents it stops. Behaviour unchanged (R104.5).
5. **Project home's "Open terminal"**: with one link it opens there; with more, it is a split
   button whose menu lists the links (`shop-web · develop`), the last used one first and default.

AC:
- R132-a [M51] (unit): `suggest` cases: `maintenance-x`/base `main` → none; `shopping-cart`/
  Project `shop` → none; `shop/coupons` → `shop`; a branch forked from `develop` where only Shop
  links `develop` → Shop; two Projects on `develop` → none.
- R132-b [M51] (e2e): Given `checkout-v2` (recorded base `develop`) in Shop. When moved to
  `Shop · 2.4` choosing Compare against `release/2.4`. Then `rade worktree list --json` has
  `project: shop-2-4`, `base: release/2.4`, and Review's comparison base is `release/2.4`;
  choosing Keep leaves `base: develop`.
- R132-c [M51] (e2e): `debug.menu` of a Worktree row's Move to Project submenu has `Scratch`,
  no `No Project`; `scripts/check-removed.sh` fails on a planted `"No Project"` string.
- R132-d [M51] (e2e): Scratch row menu reads `Close Panes in Scratch…`, the dialog text as in 4.
- R132-e [M51] (e2e): Project home with 3 links: `debug.ui home` lists the Open terminal menu with
  3 links; activating `shop-api` opens a Terminal whose `pwd` is shop-api's checkout.

### R133 — Project lifecycle: remove, prune checkouts

1. **Remove Project** (Project menu `Remove Project…`, Project settings, palette;
   `rade project remove <p> [--keep-checkouts]`, method `project.remove`). One dialog lists what
   happens:
   - `<root>/.rade/projects/<id>/` is **deleted** (`project.toml` and the folder; the dialog
     says it can't be undone, and names the file so the user can copy it first);
   - each Project checkout no other link uses: **removed** when clean (`git worktree remove`),
     **kept and listed** when dirty or with unpushed commits;
   - its Worktrees go to **Scratch** with their recorded base (never removed);
   - Panes running in a removed checkout are listed and closed after confirmation.
   Refused while a Fan-out of the Project runs.
2. **Prune Project checkouts**: `rade project prune [--dry-run]` and a Workspace menu item list
   every `.worktrees/.projects/*/*` git worktree that no current link resolves to (no record or a
   record of a removed link / old base), and remove the clean ones; dirty ones are listed, never
   touched. `link remove` and `set-base` print `1 Project checkout no longer used: <path> (rade project prune)`.
3. When `to-base` is refused because the base is checked out elsewhere and that place is an
   orphaned Project checkout, the message adds `— an unused Project checkout; rade project prune removes it`.

AC:
- R133-a [M51] (unit): the remove plan for Shop with a clean shared-ui Project checkout also used
  by Billing (kept), a clean shop-api one (removed), a dirty one (kept, listed), 2 Worktrees
  (→ Scratch); 0 commands touch a dirty checkout.
- R133-b [M51] (e2e): `rade project remove shop` → `<root>/.rade/projects/shop/` gone, Navigator has
  no Shop, `checkout-v2` under Scratch with base `develop`, the clean checkout's folder gone and
  `git worktree list` without it.
- R133-c [M51] (e2e): after `link set-base shop shop-api main` the old checkout is reported;
  `rade project prune --dry-run` lists it, `prune` removes it, a dirty one stays.
- R133-d [M51] (e2e): Remove Project… dialog text (`debug.ui dialog`) lists the 4 groups.

### R103.2 amendment — Tab label = the lead session's title

The Tab's name = the name the user gave (rename), else the **lead Agent Pane's** label (the
Fan-out grid owner, else the first Agent Pane — the same lead as R115.3), where an Agent Pane's
label is its user-given Pane name, else its **session title** (`Pane::session_title`, R104.7),
else its Preset; else a Review / Browser / Diff label; else the Worktree's name. Never `Tab <n>`.
- Shown ellipsized at **24 characters** (`…`); the full title stays in the Tab tooltip (R1).
- Follows the title live with R1's throttle (4/s per Pane); a Tab rename still wins forever.
- The Tab's Agent count (`2`) and glyphs (R115.3) are unchanged.

AC:
- R103-l [M51] (unit, `chrome::tab_label` + a new `chrome::lead_label`): lead Pane titled
  `Tính năng projects trên SCRATCH` → that text; titled `Claude Code` → `claude`; Pane renamed
  `planner` → `planner`; Tab renamed `api` → `api`; a 40-char title → 23 chars + `…`.
- R103-m [M51] (e2e, `tests/e2e/titles.sh`): two Tabs each with a fake `claude` setting OSC 2
  `Fix login` / `Projects on SCRATCH`, a third with a shell. When `debug.ui chrome` reads the tab
  strip. Then the labels are `Fix login`, `Projects on SCRATCH` and the Worktree's name;
  screenshot `target/e2e-titles/tabs-session.png` (dark + light).

### R134 — Ctrl+click on a path or URL wrapped over several rows

1. **Read around the click.** Ctrl+click (and the Ctrl-hover underline, if any) reads the clicked
   row and up to 8 rows above and below as one Ghostty selection, so soft-wrapped rows come back
   joined; the clicked cell's offset in that text is the length of the selection from the block's
   start to the cell. The token is the whitespace-delimited word around that offset
   (`link::word_at`), then trimmed of trailing `.,;:)]` as today.
2. **Hard-wrapped rows (TUIs).** When the token touches the right edge of its row's text (its last
   non-blank cell is within 2 columns of the Pane's last column, or of the widest row of the
   block), the next row's first word (after its indent) is appended, repeatedly while that holds;
   the same backwards when the token starts at the row's first non-blank cell and the row above
   ends at the edge. Each join is kept only if the result is a **URL**, or a **path that exists**
   (`link::open_target`); otherwise the longest candidate that resolves wins, then the clicked
   row's word as today. Pure: `rade_core::link::token_across(rows, row, col, cols, exists)`.
3. Ghostty's own match (URLs, OSC 8 hyperlinks) still wins when it opens something.

AC:
- R134-a [M51] (unit): `token_across` — a soft-wrapped path (one joined line), a URL hard-wrapped
  at column 60 with a 2-space indent on the next row, a path wrapped into 3 rows, a click on the
  2nd row of each → the whole token; two words at the edge that don't form an existing path → the
  clicked row's word only; a token that doesn't reach the edge → no join.
- R134-b [M51] (e2e): a Terminal 50 columns wide prints a 120-character existing path (soft wrap)
  and `fake-agent.sh` prints a URL and a path hard-wrapped with an indent; `debug.ui
  ctrl_click_cell` on a cell of the 2nd and 3rd row opens the Viewer on that file
  (`debug state` shows it) / logs the full URL in the test browser hook.

### R135 — Select and copy in the Viewer

The Viewer is read-only, never unselectable. In every content kind — code / text (GtkSourceView),
Markdown and HTML (WebKit), PDF where WebKit allows:
1. Drag selects; double-click a word, triple-click a line / paragraph; `Ctrl+A` selects all.
2. `Ctrl+C` and `Ctrl+Shift+C` copy the selection (as plain text from WebKit, Markdown's rendered
   text, not its source) while the Viewer has focus; never sent to a Terminal.
3. A right-click menu with **Copy** (and **Select All**) on the text and on the WebView; links keep
   Ctrl+click / click as now.
4. A click that focuses the Viewer never clears or interrupts a selection being made.
The fix follows Task 0's root cause; this section states the behaviour.

AC:
- R135-a [M51] (e2e): for a `.rs`, a `.md` (with a code block and a Mermaid diagram) and a `.html`
  file: a drag over a known span (`debug.viewer select` driving real pointer events in the
  headless seat, or `debug.ui drag`) + `Ctrl+C` → `wl-paste` equals the span's text; `Ctrl+A`,
  `Ctrl+C` → the whole rendered text.
- R135-b [M51] (e2e): the right-click menu of each (`debug.menu viewer`) has Copy, Select All;
  Copy gives the same `wl-paste`.
- R135-c [M51] (e2e): with a Terminal Pane beside the Viewer, `Ctrl+C` in the Viewer sends nothing
  to the Terminal (its fake shell logs no `^C`).

---

## 3. Tasks (tests first; one commit each)

0. **Repro** (no fix): E2E for F1 (empty folder, `git init` without commit), F9 (Tab labels),
   F10 (soft- and hard-wrapped path / URL; does Ghostty's matcher join soft wraps?) and F11
   (select + copy in each Viewer kind; find the root cause); record what fails in
   `docs/internals/checks/M51.md`.
1. `fix(R103.2)`: Tab label from the lead session title — `rade-core/src/chrome.rs`,
   `rade-app/src/tabs.rs`; R103-l, R103-m.
2. `fix(R131)`: pending Source rows, Initialize (`git init` only), unborn base, CLI hints —
   `checkout.rs` (`unborn`), `project_dialogs.rs`, `rade-cli/src/project.rs`; R131-a…e.
3. `fix(R132)`: suggest rules, Move to Project dialog + `worktree move`, Scratch wording,
   check-removed list — `design.rs`, `worktree.rs`, `sidebar.rs`, `handlers.rs`,
   `scripts/check-removed.sh`; R132-a…d.
4. `feat(R132)`: Project home Open terminal link menu — `home_view.rs`; R132-e.
5. `feat(R133)`: `project.remove` + dialog, `project prune` — `rade-core/src/project.rs`,
   `checkout.rs`, `rade-app/src/projects.rs`, `rade-cli/src/project.rs`, docs/API.md; R133-a…d.
6. `fix(R134)`: Ctrl+click across wrapped rows — `rade-core/src/link.rs` (`token_across`),
   `rade-app/src/terminal/links.rs`; R134-a, R134-b.
7. `fix(R135)`: select and copy in the Viewer (from Task 0's cause) — `viewer.rs`,
   `markdown_view.rs`; R135-a…c.
8. Docs: PRD (R131–R135, R103.2 amendment), CONTEXT.md (Source), docs/API.md
   (`project.remove`, `project.prune`, `worktree.set_project`), `docs/features/projects-and-worktrees.md`,
   `docs/guides/organizing-workspaces-and-projects.md`, `docs/features/terminals-and-layout.md`
   (Tab names, Ctrl+click), `docs/features/viewer-files-search.md` (copy), skills `rade-config` (remove / prune); re-run `scripts/demo/` scenes with Tabs and
   New Project; `tests/e2e/m51.sh` + CI shard; `scripts/trace.sh`; `docs/internals/checks/M51.md`.

## 4. Owner decisions (2026-10-30)

- **Q1** Remove Project deletes the Project's folder (R133.1).
- **Q2** Initialize runs `git init -b <base>` only (R131.2–3).
- **Q3** Tab labels ellipsized at 24 characters (R103.2).
- **Q4** F8 (Open terminal link menu) and Prune (R133.2) are in M51.
