# The Plan gate and the verify-command confirmation are the owner's, in the UI only

Status: accepted 2026-10-03 (M23, PRD R67–R70); amended 2026-10-03 (M24, PRD R73: a running Task's status is RADE's). Builds on ADR-0020 (human approval is UI-only), ADR-0021 (`debug.*` only in test mode) and ADR-0023 (Definition of Ready at gates). Amends ADR-0024: its accepted risk "`project.toml` / `workspace.toml` count as the owner's files" is replaced by decision 4 below.

Superseded by ADR-0033 (1.0.0).

M23 adds two owner decisions to a Task Run: approving a plan before a risky change is made (docs/internals/research/ai-native-sdlc.md §3.2, roadmap § M23), and confirming a verify command before RADE runs it with the owner's rights. Both guard against the same actor: an Agent with shell access, which can edit the Task file, the Workspace's TOML files and run `rade` against the socket.

## Decision

1. **When a plan is required.** `risk: high` read at `task start` (so a later edit can't lift it), or a Batch with `plan: required` (`auto` = `risk: high` only, the default; frozen with the policy at run like every Batch Policy key). The start's own `plan` flag is never a socket parameter. A gate approved once is not asked again for that Task; `done` / `dropped` clears it so a later Run plans again if it must.
2. **The gate is RADE's state, not the file's.** `State.plan_gates` (`state.json`) holds `planning | review | approved`, the round and the reminders. The Task file only carries the plan text (`## Plan`) and the Log lines; an Agent that writes `status: review|done` into it does not open the gate, and `task.status` over the socket is refused while it holds — any status, since a drop clears the gate and a move back to `running` would reopen the Run without it (acceptance H1); the owner drops or moves the Task in the Task Detail. Approve plan… refuses a plan changed since its dialog showed it. While it holds, a Run root's `done` submits the plan (or reminds the Agent, twice, then submits none) and never moves the Task on: no verify, no `review`; a Batch item is `planning` / `plan-review` and the scheduler ignores the Agent's `done` there (the app reports the gate as `Obs::Plan`, like the verify's `Obs::Verify`).
3. **Approval is a UI action.** Approve plan… (the confirmation shows the plan as written now) and Request changes… (a note is required) exist in the Task Detail and the Batch page; there is no socket method or CLI command, as for Batch approval (ADR-0020 §4). Tests click them through `debug.ui task_action` / `debug.batch click` and answer the dialog with `debug.dialog`, all test-mode only (ADR-0021). Approving sends "The owner approved your plan…" plus the normal Task Agent prompt (R61); asking for changes sends the note. The prompt is sent first: no Run Tab → nothing changes.
4. **A verify command runs only once the owner confirmed that exact text** for that Project / Source, from whichever layer it comes (user `config.toml`, `workspace.toml`, `project.toml`, a trusted `.rade.toml`). `rade config trust` is not a confirmation: it is a CLI command an Agent can run. The confirmed text is kept in `State.verify_confirmed` (`<project>/<source>` → text), like `.rade.toml` trust is kept as content (ADR-0017). An unconfirmed command holds the Run (the Task stays `running`, overlay `verify-confirm`, a Batch item `verifying` holding its slot and counted in "needs you"), logs once which command from which file waits, notifies, and `task.verify` refuses; a Fan-out Judge reports it "not run". The owner confirms in the Task Detail or the Batch row (**Review command…** shows each command and the path of its file); what runs is what was shown (a change since the dialog opened is refused). Then every held verify that is now confirmed runs. Checked Sources are the Run's Task Worktrees' (from the model), so another Source of the Project with an unconfirmed command does not hold a Task that doesn't run there.

## Considered Options

- **Keep the gate in the Task file** (`plan: approved` frontmatter): the Agent writes the file. Rejected.
- **A `task.approve-plan` method guarded by a flag or the caller's Pane**: any Agent with socket access could call it, or spoof `caller`. Rejected, as for `batch.approve`.
- **Gate every Task**: one more round trip for routine work; `plan: required` per Batch covers "plan everything here".
- **Trust `project.toml` / `workspace.toml` as the owner's files** (ADR-0024's accepted risk): an Agent edits them as easily as a `.rade.toml`, and the Log line naming the file only shows the surprise after it ran. Replaced by the confirmation.
- **Confirm per file content (like `.rade.toml` trust)** instead of per command: any unrelated edit to `project.toml` (a Named Base, a prompt extra) would ask again. Rejected: the command text is what runs.
- **Hash the command**: the dialog shows the text anyway; storing the text keeps `state.json` readable. Accepted as plain text.

## Consequences

- The first verify after upgrading asks for one confirmation per Project / Source; changing the command asks again. Suites that only need a confirmed command use the test-mode `debug.ui task_verify_confirm`, which does what Confirm does.
- An Agent able to write `state.json` while RADE is stopped could add a confirmation or approve a gate; that is the same boundary as `trusted_configs` (ADR-0017): RADE's own state is not defended against a process that already runs as the owner outside RADE.
- A Task whose Agent forges `status: review` in the file outside a gate still skips the automatic verify (the R37 machine sees no `running → review` move); the gate itself holds. Open for M24 — closed by the amendment below.
- A planning Agent is told to change nothing but the Task file; RADE can't enforce that without a sandbox. The gate guarantees the order of the Run (no verify, review or merge before approval), not the Agent's hands.

## Amendment (M24, PRD R73): a running Task's status is RADE's

The forged-status gap above is closed by treating the `status:` of a Task RADE runs as RADE's, like the gate's phase:

1. RADE remembers the last status it wrote per Task (`running` at start, `review` after a passed verify, the owner's moves). While that is `running` and the Run Tab is open, any other `status:` in the file is **not a transition** — whoever wrote it (an Agent, an editor). RADE writes `running` back at the next index poll (2 s) or at once when the Run root's status changes, with a Log line, and journals nothing (the Task never left `running`). When the Run root is `done`, the normal flow runs: verify, then `review`. This also applies during a Plan gate (the file says `running` while the gate holds).
2. `task.status` (any status but `running`) from inside that Task's Run while it runs is refused. Inside = the caller's Pane or the Pane the calling process runs in — taken from the socket peer's PID and its parents' environment, so an Agent can't escape by dropping `RADE_PANE_ID` (acceptance H2) — is in the Run Tab or is a Subagent of one there. This is a refusal heuristic, not a permission (ADR-0021 still holds: a same-UID process can start its own app); the owner moves the Task in the Task Detail, or with the CLI from outside the Run. A Run RADE saw stays guarded after its Tab closes.
3. After a restart RADE does not guess: the first look at a running Task only records `running`. A file that says `review` at that moment is left alone.

Considered: refusing every socket `task.status` from any Pane (the owner also works in RADE Terminals), or reading the Event Journal for the last real status (slower, and the journal is per machine). A hand edit of a running Task's status is now undone too; the Task Detail is the way to move it.
