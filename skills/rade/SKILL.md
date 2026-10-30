---
name: rade
description: Spawn and drive other coding agents (Claude Code, Codex, Antigravity, OpenCode) in RADE terminal panes - delegate a task or a review to a subagent, wait for it, read its answer, send follow-ups, check web pages in a RADE Browser Pane (open a dev server URL, snapshot, click, fill, console errors, screenshot), and trace code with `rade code` (callers, references, definition through the language server). Use when $RADE_SOCKET is set and the user wants to delegate, parallelize, get a second opinion, have another agent (e.g. codex) review work, verify a web change in the browser, find who calls a function before changing it, or start an agent later or on a schedule (`rade schedule`: cron or a given time, in a Project or a folder).
---

# Driving other agents with `rade`

Works only inside RADE. Check first: `[ -n "$RADE_SOCKET" ] && rade list >/dev/null`. If that fails, RADE is not running: say so and do the work yourself.

## The loop
1. **Spawn** a named subagent with a prompt. It opens next to you in your Tab (Agent Grid) so the user can watch:
   `rade spawn --agent codex --name rev "Review the uncommitted changes (git diff). Reply with a numbered list of problems, most severe first."`
2. **Wait** until its turn ends: `rade wait rev --until done --timeout 1200`. It prints the status:
   - `done`: turn finished, go read.
   - `waiting`: it needs input or a permission. `rade read rev` to see the question; answer with `rade send`, or ask the user.
   - `error` / `exited`: it failed; read the screen to see why.
   - After a RADE restart an agent that was mid-turn shows `interrupted` in `rade list` (a wait keeps waiting): `rade send rev "continue" --enter`.
3. **Read** the answer: `rade read rev --lines 200` (or `--scrollback` for everything, at least the last 10,000 lines).
4. **Follow up**: `rade send rev "Also check the tests" --enter`, then wait again.
5. **Close** when finished: `rade close rev`.

## Notes
- Presets: `claude`, `codex`, `agy` (Antigravity), `opencode`. Any other program: `rade spawn --cmd "cargo test" --name tests`, then `rade wait tests --until exited` or `--until '/test result/'` (a `/regex/` over the screen).
- `rade list --tree` shows panes, their parents and status. Names resolve in your worktree first; if a name is ambiguous (exit 4), use the pane id that `spawn` printed.
- Limits: at most 8 live subagents per agent and depth 3 (exit 5 = limit). Exit 6 = `wait` timed out: `rade read` it to see what it is doing (a first-run trust or login prompt never reports `done`).
- Prefer `wait` over polling `read`. `--until idle` = no output for 5 s.
- Long prompts: write them to a file and prompt "Read /path/task.md and do what it says".
- Best of N: `rade fanout --agents codex,codex,claude "task"` gives each Agent its own git worktree (one Fan-out Tab), `rade fanout wait fanout-1`, `rade fanout judge fanout-1` (a read-only Judge compares the diffs), `rade fanout wait fanout-1 --until judged`, then leave the choice to the user or `rade fanout choose fanout-1 [<label>] --cleanup` (stages the Winner on the current branch, never commits). `rade close` refuses a Fan-out agent (end the Fan-out with `rade fanout discard fanout-1`).
- A source the user paused (`rade source list` says `paused`) refuses spawn and send (exit 2): ask before `rade source resume <id>`. `rade rename <pane> <name>` names a pane; `rade tab list|rename|close` handles tabs.
- Workspaces: you run in one (`$RADE_WORKSPACE`, e.g. a company or personal identity, with its own `gh` / Claude account in your env). `rade list` shows your Workspace's panes (`--all` for every one); never act in another Workspace unless the user asks. `rade source add owner/repo` clones into your Workspace with its `gh` account and registers it (`rade source add <path>` registers a folder). Projects group Sources with Named Bases (`dev`, `prod`): `rade project list|show`, `rade worktree add <src> <name> --project <p> [--base prod]`; to change Project or config files use the `rade-config` skill.
- Agent Accounts: a Workspace can declare several Claude Code / Codex logins (`rade account list` shows them, `✓` = default; your own is `$RADE_ACCOUNT`). A Subagent you spawn uses your account; pick another of your Workspace with `rade spawn --agent claude --account acme-team …`. An account of another Workspace, or a command that points `CLAUDE_CONFIG_DIR` / `CODEX_HOME` elsewhere, is refused (exit 7, `FORBIDDEN`): don't work around it. A name a Claude and a Codex account share needs `--agent` (exit 4 otherwise). `rade account usage` prints each account's tightest limit and next reset (`acme-work  claude  62% 5-hour  resets in 1h 12m`); `rade account usage <name> --json` lists every limit and the tokens per day / Project / model. Near a limit, prefer another account of your Workspace for new Subagents (`--account`), never one of another Workspace. Accounts marked `(user)` come from the user's `~/.config/rade/accounts.toml` and are usable in every Workspace that doesn't set `accounts = "own"`. An account may load a `.env` file and extra variables (`rade account list --json` shows `env_keys`, never values); never print, copy or ask for those values.
- Show the user a file: `rade open path/to/file.rs:42` (or a `.md`, rendered) opens it in a read-only Viewer beside you without taking focus.

## When the user reviews your work: `rade review`
The user reads your worktree's diff in RADE's Review and sends you comments as one prompt headed `Review round <n>` (each `path:line`, the quoted code, the comment; a `Re: round <n>` line quotes an earlier comment they follow up). Then:
```sh
rade review status            # Round 2 · changes requested · 3 open · 4/7 viewed, then each open comment: path:line  state  text
rade review status --json     # rounds, comments (round, state: sent / changed / untouched, resolved), viewed, summary
rade review request --summary "Done: OTP login; tests in tests/otp.rs. Not done: rate limit (needs a decision)."
```
- Address every open comment, then run `rade review request --summary "…"` (what you changed, what you did not and why, 1–3 lines): the user gets a *Ready for review* notification and sees your summary in the Review header.
- `code changed` / `untouched` only says whether the commented lines changed since the comment was sent, not whether you did it right; the user resolves comments, not you.
- An `Approved (review round <n>)` prompt asks you to commit (and maybe push and open a PR): follow the repository's commit conventions; do not push unless it says so.

## Trace code before you change it: `rade code`
RADE runs the real language server (rust-analyzer, typescript-language-server) for your worktree and shares it with the user and other agents. Positions are `path:line:col` (1-based, relative to your cwd):
```sh
rade code callers src/auth.rs:88:12   # every call of this function (--depth 2: and who calls those), then uses that are not calls: run it before changing a signature
rade code refs    src/auth.rs:88:12   # every use (types, constants, fields too)
rade code def     src/main.rs:14:20   # where a name is defined
rade code callees src/auth.rs:88:12   # what it calls
rade code impl    src/store.rs:5:11   # implementations of a trait / interface
rade code symbols src/auth.rs         # a file's functions and types
rade code symbols --query verifyOtp   # find a function / type by name anywhere in the Worktree
rade code hover   src/auth.rs:88:12   # its signature, type and docs
rade code impact                      # your diff vs its base: each changed function and its callers outside the diff
```
Each prints `path:line:col<TAB>name<TAB>line`; `--json` for the full answer. The `#` header says where it came from: the server's name, or **text matches** (whole-word search) when the server is still indexing, missing (the header has the install command: tell the user, never install it yourself), off for the repo, or crashed (`rade code restart <file>`). Text matches can include unrelated names: check them. It waits up to 20 s for an indexing server (`--wait`). Before you say you are done with a change to existing code, run `rade code impact` and check each listed call site still works (`test` = in a test file, `text match` = found by name only); as a Reviewer you get the same list in your prompt.

## Verify your web change in the Browser Pane
`rade browser …` drives a Browser Pane beside you (WebKit; the user sees it too). Refs come from the last snapshot and stay valid until the page navigates.
```sh
rade browser ports                      # dev servers running in RADE panes: port, pane, source
rade browser open 5173                  # = http://localhost:5173/, waits for the load; prints pane, url, title
rade browser snapshot                   # page as text: [ref=e3] button "Save", textbox "Email" value="", text "…"
rade browser fill e2 "me@example.com"   # set a field by ref (inputs, textareas, selects)
rade browser click e3                   # click by ref; waits if it navigates
rade browser press Enter                # keys go to the focused element: Enter, Tab, Shift+Tab, Escape, a …
rade browser wait --text "Saved"        # or --ref e7, or --load (exit 6 on timeout, default 30 s)
rade browser console                    # console messages + JS errors since the last call
rade browser eval "document.title"      # any JS in the page (expression or statements with return) → JSON
rade browser screenshot /tmp/shot.png   # then look at the PNG
```
Loop: change code → `open`/`reload` → `snapshot` → act → `wait --text` → `console` (no new errors?) → `screenshot` and look. `snapshot -i` lists only elements with refs. Exit 4 = no Browser Pane / unknown ref (take a new snapshot), 2 = bad input or a JS error.

### A real Chromium: `--engine chromium`
The Browser Pane is WebKit: no WebRTC, File System Access or WebGPU, and some media sites (YouTube) crash it — and RADE with it. For those, or when you need trusted clicks and keys, add `--engine chromium` to any command above (not `ports`): the same commands, refs and output, on a headless Chromium RADE starts with a throwaway profile (Playwright's Chrome for Testing, `chromium` or `google-chrome`; `[browser] chromium` in config.toml picks one; none → `npx playwright install chromium`).
```sh
rade browser --engine chromium open 5173      # starts Chromium on first use
rade browser --engine chromium snapshot       # same [ref=eN] lines
rade browser --engine chromium click e3       # a trusted mouse click
rade browser --engine chromium close          # quit it (also quits after 30 idle minutes)
```
`--headed` opens a real window instead (the user can watch and annotate there); close a running one first to switch. For checks that must keep passing, prefer Playwright tests in the repo (`npx playwright test`).

### The user's Browser Annotations
The user can switch on annotate mode (the Browser header's pencil, `Ctrl+Shift+A`; the toolbar in a headed Chromium), click an element or drag a region and comment. Send to Agent pastes them to you as one prompt: per annotation the page URL, a CSS selector, a ref, the element's text, its box, a cropped screenshot path (open it and look) and the page's console errors. Find the element by the selector in the code, fix it, check it in the browser, and say what you changed per annotation. `rade browser annotations` shows pending ones; `rade browser annotate on` turns the mode on when you ask the user to point at something.

## Start an agent later or on a timetable: `rade schedule`
A Schedule starts an Agent with a prompt at cron times (repeating) or once at a given time, in a Project link's checkout (`--project`) or a folder (`--dir`, runs in Scratch). RADE runs it only while the app is open; each run is a new Pane `<name>-<n>` in the Tab `⏱ <name>`. Use it when the user asks for something recurring ("every weekday at 9 check the dependency updates") or deferred ("tonight at 23:00 run the full suite and report").
```sh
rade schedule next "0 9 * * 1-5" --count 3                      # check an expression first (local time, no app needed)
rade schedule add deps --project shop --cron "0 9 * * 1-5" "Check outdated dependencies and open a short report"
rade schedule add nightly --dir ~/notes --at "2026-11-02 23:00" --agent codex --prompt-file prompt.md
rade schedule list [--all] [--project shop | --scratch] [--json] # on/off, when, agent (account), next time, last Run
rade schedule show deps [--json]                                 # every field, the next 5 times, the newest Runs
rade schedule edit deps --cron "30 8 * * *" [--account acme-work | --default-account] [--rename new] ["new prompt"]
rade schedule on deps · rade schedule off deps                   # off keeps its Panes
rade schedule run deps                                           # one run now
rade schedule runs deps [--limit 20] [--json]                    # n, time, outcome, status, pane, session
rade schedule resume deps [<n>]                                  # its open Pane is focused, else its session is resumed
rade schedule remove deps [--close-panes]
```
- Cron: 5 fields `minute hour day-of-month month day-of-week` or `@hourly` `@daily` `@weekly` `@monthly`, local time. `--at "YYYY-MM-DD HH:MM"` runs once, then the Schedule is `done` (a new future `--at` switches it on again).
- `--agent` is a Preset (default `claude`); without `--account` each run uses the Workspace's default account at that time. `--project` with several links needs `--link <source>`.
- Run outcomes: `started`; `skipped` (the previous run is still running / waiting, or the Source is paused); `missed` (the app was closed at that time: not run later unless `--catch-up once`); `failed` (the reason says why). Only a `started` Run has a session to resume.
- The run Panes carry `RADE_SCHEDULE=<name>`; after a run starts, finished run Panes past `--keep-runs` (default 3) are closed.

## Example: Claude asks Codex for a review
```sh
rade spawn --agent codex --name rev "Review the diff of this branch against main. List real bugs with path:line, most severe first. Be brief."
rade wait rev --until done --timeout 1200    # prints: done
rade read rev --lines 150                    # the review
```
Summarize the findings for the user, fix the valid ones, then either `rade send rev "Fixed 1 and 3, please re-check" --enter` and wait again, or `rade close rev`.
