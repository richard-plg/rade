# Batches are run by a deterministic scheduler in the app; RADE commits only to a Batch's own Integration Branch

Status: accepted 2026-10-02 (M19, PRD R48–R54). Narrowly extends ADR-0013 (RADE runs git only on the owner's click) for one case: the squash commit of an approved Task onto a branch the Batch made. Builds on ADR-0018 (Tasks are files) and ADR-0017 (layered config).

Superseded by ADR-0033 (1.0.0).

The owner wants to pick N ready Tasks, press Run and be called only when needed (design doc §5.4): Task Agents in parallel up to a limit, each result reviewed, approved work gathered on one branch for one review and one PR. A Batch can last hours; the Orchestrator Agent's context fills up, Agents crash, RADE restarts. If the plan and the bookkeeping live in an Agent's head, all of that is lost; if an Agent can approve, merge or push through RADE, the owner's confirmation (ADR-0013) is gone.

## Decision

1. **The Agent decides, RADE keeps the books.** The Orchestrator (an Agent or the owner) writes the batch file — which Tasks, `after:` order, Profiles, its reasoning under `## Plan` — and runs it. Everything that must be exact and survive a crash is RADE's: slots, starts, waiting, reviews, merges, the item states, `## Progress`.
2. **The scheduler is a pure function** `rade_core::batch::step(batch, observations) → (batch, actions)`, GTK-free and unit-tested; the app calls it every second and on each event, **saves the new state first**, then carries out the actions (start a Task, spawn a Reviewer, prompt, merge, set a Task done, notify, log). A crash between save and action is redone on restart from the saved state (`starting`, `reviewing`, `merging` are redone; an existing Task Worktree or Run Tab is reused).
3. **The policy is frozen at run** into `state.json`. Later edits to the frontmatter are reported (`policy_edited`), never applied: an Agent can't loosen a running Batch by editing a file.
4. **Human review is UI-only.** Approve, Request changes, Reject and Retry merge are buttons on the Batch page. No socket method or CLI command approves; an Agent that wants approval has to ask the owner. Tests drive the buttons through the hidden `debug.batch` (ADR-0006).
5. **`merge` is `never` or `integration`.** With `integration`, each approved item is squash-committed (`T-<n> <title>`, the Source's own git identity) onto `batch/B-<n>`, a local branch the Batch creates from the Project's Named Base in its own Worktree, one per Source it touches. This is the one commit RADE makes without a click (the extension of ADR-0013): it is on a branch nobody else uses, never the base or an existing branch (`run` refuses if `batch/B-<n>` exists), never the Source's checkout. Bringing it into the base is the owner's merge or PR. Merges run one at a time; a conflict stops the item (`conflict`) for the owner to fix in that Worktree and retry.
6. **`push` is `never` or `ask`.** `ask` only adds a Push… button that opens ADR-0013's existing Push confirmation for the integration Worktree. RADE never pushes on its own and never opens a PR.
7. **Pause and Cancel never kill Agents.** Pause starts nothing new; Cancel skips what has not finished; running Agents and their Worktrees stay for the owner.
8. **Restore continues.** After a restart, each `interrupted` Task Agent of a running Batch gets one "RADE restarted while you were working on T-n…" prompt once its screen settles (a paused Batch: on Resume).
9. **`agent-judge` reuses the Fan-out Judge**: a read-only Reviewer with the `[fanout] judge` (or `reviewer:` Profile's) Agent and the Judge's read-only command line, in the item's Run Tab, prompted with the Task file and the Run's diff; RADE reads the last `VERDICT: approve|changes` line on its screen. `changes` sends the notes back up to `max_rounds`; no verdict falls back to the owner.

## Considered Options

- **The Orchestrator Agent schedules** (calls `task start` / `task wait` itself): no new app code, but nothing survives the Agent's context or a crash, and limits depend on its discipline. Kept as possible (the commands exist), but a Batch is run by RADE.
- **`merge: per-task-pr` and `push: auto`** (design doc §5.4): publish work without the owner seeing it. Left out until a few Batches have run as intended; `ask` keeps the confirmation.
- **Merge into the base branch** after approval: changes the owner's branch without a click. Rejected: an Integration Branch is reviewable and disposable.
- **A `batch.approve` method guarded by a flag**: any Agent with socket access could call it. Rejected.

## Consequences

- An approved Task's work lands on `batch/B-<n>` with no click; the owner reviews that branch (Review Pane, Changes) and merges or pushes it by hand.
- A global merge lock serialises merges across Batches (simple; per-Batch locks if Batches merge in parallel a lot).
- `state.json` holds `batches` (optional, no version bump); a Batch file without a run is only a draft.
