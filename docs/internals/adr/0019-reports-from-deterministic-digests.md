# Reports are drafted from deterministic Digests by a no-tools command; versions are immutable; RADE never posts

Status: accepted 2026-10-02 (M18, PRD R41–R47). Builds on ADR-0013 (Drafts: an Agent writes text, the human confirms), ADR-0017 (Projects and layered config) and ADR-0018 (Tasks are files).

Superseded by ADR-0033 (1.0.0).

The owner writes a daily and a weekly status report for a boss and a team, in English, with the sections Done / In progress / Blockers / Next, and pastes it into Slack or DingTalk (design doc §6, §11). The facts are scattered: commits in several Sources, PRs on GitHub, Task Logs, which Agents ran on what. Asking a model "what did I do this week?" over raw logs is slow, costly and makes things up.

## Decision

1. **A Digest is built from facts, with no model call.** For one local day of one Workspace: Task `## Log` lines of the day and the Event Journal's `task.status` changes; the user's non-merge commits on any ref of every Source (author = `git config user.email` read in that Source, so `includeIf` identities apply; deduplicated by hash); PRs from the journal plus best-effort `gh pr list --author @me --search updated:<day>` under the Workspace env (10 s; on failure the Digest says `PRs: unavailable`); Agent runs per Task or Tab. One section per Project plus "Outside Projects". Commits are matched on their local commit date, not with `git log --since/--until` (which stops at the first commit with an older date).
2. **Past Digests are files, written once:** `<root>/.rade/digests/YYYY-MM-DD.md`. A later `digest` reads the file, so the owner's hand edits flow into every Report; `--regen` rebuilds it. Today's Digest is always computed and never stored (it is still changing).
3. **The Event Journal is append-only JSON lines per day**, `<root>/.rade/events/YYYY-MM-DD.jsonl`, written only by the app: `agent.start` (a new Pane of an Agent Preset), `agent.end` (an Agent leaving `running` for `done`/`error`/`exited`/`interrupted`, with the seconds it ran), `task.status` (`by: owner|auto`) and `pr.create` (from Changes). Every model update compares Pane snapshots, so all status paths log through one place. One line is one `write` under 4 KiB on an `O_APPEND` file, so concurrent writers never interleave. `events/.gitignore` is `*`: the journal is per machine, a shared `.rade` repository would otherwise conflict on it. No retention yet.
4. **Drafting reuses the Draft runner with no tools.** `[report] draft_command` (layered: Workspace and Project may set it), else `[scm] draft_command` (default `claude -p --tools "" --no-session-persistence`); 300 s; run in the Workspace root with the Workspace env, so company content goes through the company account. The prompt holds the style text, the range, each day's Digest (cut at 200 KB, and the prompt says so), the open Tasks and the reply rule (English Markdown, no preamble). A no-tools command cannot load a skill, so RADE pastes the style in: the Project's `skills/report-style/SKILL.md`, else the Workspace's, else built-in.
5. **Report Versions are immutable.** `reports/<id>/vN.md` is created with `create_new` under `flock(reports/<id>/.lock)` and made read-only (0444); `versions.jsonl` gets `{v, created, by: draft|revise|save, from?, instructions?, command?, range}`. Revise drafts a new Version from one Version plus instructions; a hand edit is saved as a new Version (`rade report save`). A failed or empty draft writes nothing. Ids: `2026-10-02`, `2026-W40` (ISO week), `2026-09-15..2026-09-30` (at most 92 days); a Project's Reports live under `projects/<p>/reports/`, the Workspace's under `reports/`, and the scope is always explicit.
6. **Export is a pure AST walk** (comrak): Slack mrkdwn, and for DingTalk the documented robot subset (headings, bold, italic, links, images, quotes, flat lists; tables, code and strikethrough converted; docs/internals/research/dingtalk-markdown.md). `--json` gives each webhook's body.
7. **RADE never posts.** There is no webhook or token in RADE. Posting is the `rade-report` skill's job, done by an Agent with the owner's own MCP tool or webhook variable, only when the owner asks in that conversation and after showing the exact text and target.
8. **The Reports page is a page, not a Pane** (like the Tasks page, ADR-0018): a named child of the main content stack, `State.reports_page` optional (no `state.json` v3). The Version diff is `git diff --no-index` shown as text: the Review Pane needs a Worktree.

## Considered Options

- **A model summarizes Agent session logs into the Digest**: richer, but slow, costly, not testable and can invent work. Skipped; add a summary step when Digests feel thin.
- **Reports as one mutable file with git history**: needs a repository per Workspace and a commit per change; revise could lose the owner's edit. Rejected for numbered read-only Versions.
- **Webhooks in RADE** (`[report] slack_webhook`): a secret in a config file and a button that posts to the boss by mistake. Rejected.
- **Re-using the Review Pane for the Version diff**: it compares a Worktree with its base. Rejected for plain text.

## Consequences

- Reports cost one model call each and are reproducible from files; a Digest can be corrected by hand before drafting.
- `rade report new|revise` block their socket call up to 300 s (like `pane.wait`); the page runs them off the GTK thread with Cancel.
- The DingTalk rendering is unverified until a real paste (pending owner); the subset is conservative.
