# RADE

A Linux desktop Agentic Development Environment: a GPU terminal workspace where humans and coding agents open, organize, and drive terminal panes per repository, grouped by working identity.

## Language

### Organization

**Workspace**:
A working identity (a company, personal work) rooted at a folder such as `~/workspaces/globex`: its `.rade/workspace.toml` gives every Pane in it the same environment (GitHub CLI config, git identity) and its Agent Accounts, and its Sources live under it. One window shows one Workspace; hidden ones keep running. `default` (rooted at `$HOME`) always exists.
_Avoid_: Tenant, profile, account (and a Conductor-style "workspace" meaning a checkout)

**Source**:
A git repository (or plain folder) registered in RADE, in exactly one Workspace and in any number of its Projects (once per Source link). (Called Project before M15.)
_Avoid_: Project, repo, folder

**Project**:
A named set of Source links of one Workspace that ship together (e.g. Shop = `shop-web`, `shop-api`, `shared-ui`, each at `develop`), defined by `<root>/.rade/projects/<id>/project.toml`; each link has exactly one base, so the same Sources at prod are a second Project. A row in the Navigator with its links under it. (Before M15 "Project" meant what is now a Source.)
_Avoid_: using it for a single repository; Group, Product

**Source link**:
One Source in one Project with exactly one base branch (`[[source]] id`, `base`, `checkout` in `project.toml`); the Project's unit of work. Its Worktrees are cut from that base and its Review, Changes, `+/−` counts and PRs compare against it. A Source in two Projects has two links, possibly at two bases.
_Avoid_: Named Base (retired), binding, mapping, membership

**Project checkout**:
A durable git worktree at `<root>/.worktrees/.projects/<project>/<source>` on a link's base, made by RADE (on first use, after showing the plan) when the Source's main checkout is not on that base or another link needs it at another base. Links with the same Source and the same base share one, labelled **shared**. Not the same as **also in**: another Project links the same Source at any base (the Navigator's link marker), whether or not it shares the checkout. A link's checkout is chosen once and recorded; a HEAD change later is Base drift, not a new choice. RADE never switches, stashes or resets a checkout with changes.
_Avoid_: Mirror, clone, environment checkout

**Base drift**:
A link's checkout whose HEAD is not the link's base (e.g. `shop-api` on `feat/checkout-v2` while Shop's base is `develop`): a warning on its Navigator row, a chip on Project home and a card in Changes, with **Back to base** (only when clean), **Keep** and **Move to Worktree…**.
_Avoid_: Off-branch, divergence, out of sync (that is ahead/behind)

**Config Layer**:
One file that sets configuration, in precedence order: built-in → user `config.toml` → `workspace.toml` → `project.toml` → the Source's `.rade.toml` (which counts only once trusted). `rade config explain` shows which layer set a value.
_Avoid_: Profile, override file, scope

**Project home**:
The main-area page of one Project: its Source links (base, checkout, HEAD, ahead/behind, clean / drifted / also in), its Worktrees, recent Activity and Open terminal. Opened from the Project's Navigator row.
_Avoid_: Tasks page (retired), dashboard, overview (that is Agent Overview; the breadcrumb reads `Shop / Home`)

**Project settings**:
The link table over a Project's `project.toml` (Source, base, checkout) plus its name and Agent; every edit is one key of the file. New Project, Add Source and Change base show the checkout plan for each link before Create.
_Avoid_: Project config dialog, preferences

**Paused**:
A Source whose processes RADE stopped on request: its Worktrees, Tabs and Panes stay (each Pane shown as a placeholder) and nothing runs or opens in it until Resume starts its Panes again the way Session Restore would.
_Avoid_: Suspended, sleeping, hibernated, stopped

**Scratch**:
The always-present place, one per Workspace, for terminals that belong to no Project; rooted at the Workspace root (`$HOME` for `default`). Linked Worktrees with no recorded Project show here too. (Was General, and No Project for Worktrees, before 1.0.0.)
_Avoid_: General, No Project, default source, misc

**Worktree**:
One checkout of a Source (the main checkout, a Project checkout or a git worktree) with its own Tabs; every Pane runs inside exactly one Worktree. A linked Worktree belongs to the Source link its recorded `project` names (never placed by its branch); one with none is in **Scratch**.
_Avoid_: Branch, workspace, environment

**Main checkout**:
A Source's own folder (its first Worktree, not a git worktree). A link uses it when it is on the link's base and no other link needs it at another base; it shows under each link that uses it and is never switched by RADE.
_Avoid_: Root worktree, primary clone, default worktree

**Tab**:
A named page inside a Worktree holding one Layout of Panes.
_Avoid_: Surface, window, page

**Pane**:
One visible region of a Tab showing a Terminal, a Viewer, a Review, or a Browser Pane.
_Avoid_: Split, surface, panel

**Layout**:
The arrangement of Panes inside a Tab; either user-arranged splits or the automatic Agent Grid.
_Avoid_: Split tree, tiling

**Agent Grid**:
The automatic Layout used when an Agent spawns Subagents: parent on the left, up to 8 children tiled on the right, any further children collapsed to Title Bars.
_Avoid_: Auto layout, tiling mode

**Title Bar**:
The collapsed one-line form of a Pane showing its caption: Agent Status LED, name, Preset, Agent Account and Unread count.
_Avoid_: Mini pane, stub

**Zoom**:
Temporarily showing one Pane over its whole Tab; toggling restores the Layout.
_Avoid_: Maximize, fullscreen

### Terminals and agents

**Terminal**:
A Pane running a process under a pseudo-terminal, rendered by Ghostty.
_Avoid_: Shell, console, surface

**Viewer**:
A read-only Pane that displays a file; Markdown files are rendered.
_Avoid_: Editor, preview, file pane

**Review**:
A Pane showing a Worktree's changes against its recorded base (its Source link's base unless it was cut from another ref) (every changed file in one scrolling diff, with a jump list of the files) where the user reads it file by file (Viewed), writes Review Comments in Rounds and ends with Request changes or Approve (a hand-off to Changes, Create PR, Choose Winner or the Agent, ADR-0040).
_Avoid_: Diff view, PR view, code review

**Diff Pane**:
A Pane showing one file's uncommitted changes (working tree vs index, or index vs `HEAD`), opened from the Changes page; hunks and lines can be staged there.

**Quick Diff**:
The Viewer's gutter marks of lines changed against the index.

**Browser Pane**:
A Pane showing a web page (usually a local dev server) that the user browses and an Agent drives through `rade browser` (snapshot with element refs, click, fill, eval) to check its own web changes.
_Avoid_: Web view, preview, embedded browser

**Browser Annotation**:
A comment the owner attaches to an element or a dragged region of a web page in annotate mode (a Browser Pane, or a headed Chromium), kept with its page URL, selector, ref, text, box, cropped screenshot and the page's recent console errors until sent to an Agent as one prompt.
_Avoid_: Review Comment (that is on a diff line), markup, pin

**Chromium engine**:
`rade browser --engine chromium`: the same `rade browser` commands and refs on a real Chromium that RADE starts outside its window (headless, or `--headed` with the annotation toolbar), for pages WebKit can't run and for trusted input. The Browser Pane stays WebKit.
_Avoid_: Chrome Pane, CEF, embedded Chromium

**Agent**:
A coding-agent CLI (e.g. Claude Code, Codex) running in a Terminal, identified by its Agent Preset.
_Avoid_: Bot, AI, assistant

**Agent Preset**:
A named entry in the registry describing how to launch, resume, and observe one kind of Agent (including the flag that picks its model).
_Avoid_: Agent config, profile, account (that is which login it runs under)

**Agent Account**:
One login of one kind of Agent: agent + config dir (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`) + name (`acme-work`), declared per Workspace in `workspace.toml` `[[account]]` or for every Workspace in `accounts.toml` (a Workspace with `accounts = "own"` uses only its own), with optional extra env (`[account.env]`, an `env_file` read at spawn); one default per agent; a name is unique per Workspace and agent. A Pane where an Agent was started by hand gets the account whose config dir it runs with (detected). A Pane runs under exactly one, and its config dir is never overridden by `[env]` or a command prefix (a prefix naming another dir is refused), gets `RADE_ACCOUNT`, and a Subagent inherits its Parent's. Only the Pane's own Workspace's accounts and the user accounts it allows can be used (`FORBIDDEN` otherwise).
_Avoid_: Profile, user, identity (that is the Workspace's), subscription

**Subagent**:
An Agent whose Terminal was spawned by another Pane through the RADE CLI; it records its Parent.
_Avoid_: Child agent, worker

**Parent**:
The Pane that spawned a Subagent.
_Avoid_: Owner, caller

**Agent Status**:
The observed state of an Agent: `running`, `waiting` (needs human input), `done` (turn finished), `error`, `exited`, or `interrupted` (its turn was cut off by a RADE quit or crash; Session Restore resumed it idle, and it needs a prompt to go on — not finished).
_Avoid_: State, phase

**Session Restore**:
Recreating Sources, Worktrees, Tabs, Layouts, working directories, and resuming Agents after RADE restarts; processes themselves do not survive.
_Avoid_: Persistence, reattach

**Voice Composer**:
The card at the bottom of an Agent Pane where the owner dictates: live words, then the Voice Draft, revised by speaking again, sent with Enter (R124).
_Avoid_: Dictation box, voice modal

**Voice Draft**:
The prompt RADE's voice agent writes from what the owner said, the Pane's session and the checkout; it reaches the Agent only when sent.
_Avoid_: Transcript (that is the raw words)

### Attention

**Notification**:
A message raised by a Pane (escape sequence, bell, hook, or `rade notify`) that targets that exact Pane.
_Avoid_: Alert, toast, event

**Unread**:
A Notification whose Pane has not been focused since it arrived.
_Avoid_: New, pending

**Inbox**:
The list of Notifications in a popover opened from the foot of the Navigator, newest first, with "Mark all read" and Clear.
_Avoid_: Notification panel, notification center, bell

**Toast**:
A transient in-app message at the bottom of the window — "<Agent> needs input · Show", or a limit forecast — raised only when a waiting Agent's Pane is not on screen (or a forecast fires); it never covers a button. Not a Notification: nothing is stored.
_Avoid_: Popup, banner, alert

**Activity**:
The Inspector tab listing recent events of the shown Workspace — an Agent waiting or done, a PR opened, a commit, an account limit passing 80 % / blocked / reset — kept in memory only (nothing written, gone on restart).
_Avoid_: Event Journal (retired), history, audit log, feed

### Navigation

**Navigator**:
The left column: the shown Workspace's Projects as a tree Project → Source link (base, drift and also-in markers) → Worktree → Agent (LED), then Scratch, with the Inbox at its foot; a folded row shows its children's counts. (Was the sidebar before 1.0.0.)
_Avoid_: Sidebar, tree view, explorer

**Inspector**:
The right column, collapsed with `Ctrl+Shift+E` (or `Ctrl+B` outside a Terminal): text tabs Changes, Files (the file tree) and Search, plus Activity as an icon. (Was the Right Panel before 1.0.0.)
_Avoid_: Right Panel, drawer, file pane

**Statusline**:
The tmux-like bar at the bottom of the window, cells split by hairlines: Workspace, Project, Worktree with ahead/behind, Agent counts per status, listening ports, the git identity (`richard@acme.io`, warned when a checkout's email differs), the focused Agent's Tightest limit and key hints. (Was the Status Bar before 1.0.0.)
_Avoid_: Status Bar, footer, info bar

**Changes**:
The Inspector tab for git state, grouped by Project with one group per Source link (Staged, Changes and Untracked files with stage / unstage / discard per file and per hunk, a commit box per link, the Project's total `+/−`), Pull / Push and Create PR against the Worktree's recorded base, and Base drift cards. RADE runs git or `gh` only when the user clicks (ADR-0013).
_Avoid_: Source control view, SCM, git panel

**Draft**:
Text an Agent writes on request for a commit message or a pull request's title and body (Generate); it only fills the box, the user edits it and confirms.
_Avoid_: Suggestion, auto-commit, AI message

**Search**:
The Inspector tab that searches files of the current Source link or the whole Project (regex, case, whole word, include / exclude globs, `.gitignore` applied); a result opens the Viewer at its line. Not the Terminal's search bar (Find in Terminal).
_Avoid_: Find in files, grep, global search

**Impact**:
A diff's changed, added and removed symbols (functions, methods, types) with their callers outside the diff, from the Language Server plus text matches in tests (and for removed ones, uses still in the tree); shown in the Review Pane, printed by `rade code impact`, and added to the Judge's prompt.
_Avoid_: Blast radius, change analysis, dependents

**Language Server**:
A program (rust-analyzer, typescript-language-server) RADE starts on the first code navigation in a Worktree root and asks where a name is defined, used and called (LSP); one per root and language, shared by the owner's Viewer and every Agent (`rade code`), stopped when idle. Answers that come from text search instead are labelled **text matches**.
_Avoid_: LSP (the protocol), indexer, code intelligence plugin

**Agent Overview**:
The full-window view (`Ctrl+Shift+Space`) of every Agent in every Workspace, in bands Needs you → Finished, not seen → Running → Quiet, each card showing where the Agent runs and its last screen lines; `Enter` jumps to it, switching Workspace if needed.
_Avoid_: Overview (alone), Mission Control, dashboard, switcher (that is the Workspace switcher)

**Command Palette**:
The `Ctrl+Shift+P` (and, outside a Terminal, `Ctrl+K`) overlay (also the titlebar's command field) that finds Panes, Tabs, Worktrees, Source links, Projects, files and actions by fuzzy search and runs one; an action needing input shows an inline form there.
_Avoid_: Quick open, launcher, command bar

**Update**:
A newer RADE release on GitHub Releases that RADE found and can install from the app; it takes effect when RADE restarts (R127).
_Avoid_: Upgrade (apt's word for the package step), patch

### Usage

**Limit**:
One quota of an Agent Account with its own window and reset time — Claude `five_hour`, `seven_day`, `spend_limit`; Codex `primary` / `secondary` per `limit_id` (e.g. Spark 5-hour) — kept as a list `{account, group, name, window, used %, resets_at, source}`, never a fixed set.
_Avoid_: Quota (in UI text), rate limit (that is the API's), cap

**Tightest limit**:
An Agent Account's Limit with the highest used % (`81% Spark 5-hour`); what the account table sorts by, the `+` menu shows and the Statusline cell shows for the focused Agent's account.
_Avoid_: Worst limit, main limit

**Usage page**:
The list–detail page of Agent Accounts: the account table (Tightest limit, tokens, last update), Next resets with forecasts, and the selected account's every Limit and tokens by day, Project and model. Opened from the Statusline cell, the titlebar or the palette (`> usage`).
_Avoid_: Billing page, dashboard, cost view

**Usage store**:
RADE's own record of Limit samples and token totals, one file per agent and config dir (`$XDG_STATE_HOME/rade/usage/<agent>-<sha8>.jsonl`), filled from the Claude statusline JSON and by tailing Agent logs read-only; kept 90 days.
_Avoid_: Usage log, metrics DB, Event Journal

**Detailed limits**:
An opt-in per Claude Agent Account (off by default): RADE asks Claude's usage endpoint at most once per 5 minutes for the per-model weekly Limits the statusline lacks, reading the login token only at call time and never storing it; it turns itself off on an unexpected answer.
_Avoid_: Full usage, API usage, scraping

### Parallel work

**Fan-out**:
Sending one prompt to N Agents, each in its own new Worktree of one Source link, shown together in one Fan-out Tab of the link's checkout (the Agents still run in their own Worktrees).
_Avoid_: Broadcast, multi-run, race

**Judge**:
An Agent spawned, read-only, to compare Fan-out results and recommend a Winner.
_Avoid_: Reviewer, evaluator

**Winner**:
The Fan-out Worktree the user chose to keep; its result is staged on the Source's current branch (squash, never committed by RADE) or its Worktree is kept.
_Avoid_: Best result, pick

**Review Comment**:
A user note attached to a line of a Worktree diff, batched and sent to that Worktree's Agent as a prompt. It follows its line when the file changes; when the line is gone it is **outdated** (still sent, with the code it quoted).
_Avoid_: Annotation (that is a Browser Annotation, on a web page), PR comment

**Round**:
One pass of the owner over a Worktree's Review: it starts when they send comments (*Request changes*) or approve, and records a snapshot of the files then, so the next pass shows only what changed since (`Since round n`). A sent Review Comment keeps its Round; it is **open** until the owner resolves it.
_Avoid_: Iteration, pass, review cycle

**Viewed**:
A file of a Review the owner marked as read; it is unread again when its diff changes.
_Avoid_: Seen, read (those are for Notifications)

### Retired terms

Retired in 1.0.0 (ADR-0033, ADR-0034): Named Base, Task, Task template v2, Definition of Ready, Task Run, Task Agent, Verify command, Verified Run, Confirmed verify command, Plan gate, Agent Profile, Free Tab, Live Task, Tasks page, Task Detail, Description (of a Task), To do / Needs info, Run dialog, Preset (of the Run dialog), Start here, Brainstorm, Event Journal, Digest, Flow metrics, Lessons, Report, Report Version, Report Style, Batch, Batch Policy, Orchestrator, Reviewer (of a Batch), Integration Branch, Initiative, Initiative Agent, Milestone (of an Initiative), Acceptance Agent, Initiative Branch, Code page, Attention Ring (ADR-0036), No Project, General (now Scratch), Right Panel (now Inspector), Status Bar (now Statusline), sidebar (now Navigator).
