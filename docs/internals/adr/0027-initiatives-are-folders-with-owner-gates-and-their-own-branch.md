# Initiatives are folders with owner gates, built milestone by milestone on their own branch

Status: accepted 2026-10-04 (M25, PRD R74–R79). Builds on ADR-0018 (Tasks are files), ADR-0019 (read-only Versions), ADR-0020 (Batches, UI-only approval, the Integration Branch), ADR-0021 (`debug.*` only in test mode), ADR-0023 (Definition of Ready), ADR-0025 (owner decisions are UI-only; RADE keeps the phase) and ADR-0026 (Browser Annotations).

Superseded by ADR-0033 (1.0.0).

The roadmap's M25 asks for the whole agentic SDLC above Tasks: an owner's notes become a concept, a design (for UI products), a spec and a plan, each approved by the owner, then a build milestone by milestone — Ready Tasks, a Batch, verify and review, an independent acceptance check — with several such efforts running at once without stepping on each other.

## Decision

1. **An Initiative is a folder** `<root>/.rade/projects/<p>/initiatives/I-<n>-<slug>/` beside `tasks/` and `batches/`: `initiative.md` (the owner's brief and a Log RADE appends to), `inputs/`, and the documents the Agents write (`concept.md`, `design/`, `spec.md`, `plan.md`, `checks/M<k>.md`). Ids under `flock` like Tasks; the folder is created exclusively. Documents are plain files the Agent edits in place; what the owner reviewed is kept as RADE's read-only Versions (`versions/<phase>/vN.md`, the Report Versions code, ADR-0019) — the history, the diff and the Approve check ("the file changed while the dialog was open") come from those.
2. **The phase is RADE's** (`state.json` `initiatives`), never a file's, like the Plan gate (ADR-0025): Concept (its first draft shown as Discovery) → Design (`kind: ui` only) → Spec → Plan → Build → Done. Each document phase reuses the Plan gate's machine (`plan::Gate`: drafting → review, round, two reminders) — the Agent's turn ending submits; nothing an Agent writes moves a phase.
3. **Every gate is the owner's, in the UI only**: Approve… / Request changes… on a document, Continue… / Request changes… / Ask again on a milestone, and line comments, are buttons of the Initiative page. There is no socket method or CLI command for any of them (`debug.initiative` clicks the page in test mode, ADR-0021). An Agent that wants to move on asks the owner. A prompt is sent before the state changes; with no Initiative Agent open, a new one is started with it.
4. **One Initiative Agent per Initiative**, in a marked Tab of the Workspace's General (`Tab.initiative`, `RADE_INITIATIVE[_DIR]`), prompted per phase. The content is the Agent's: RADE does not template documents beyond one-line prompts and two skills (`rade-initiative`, `rade-design`). Per-line comments reuse the Review Comment type and quoting (`review::compose_doc`); Mermaid is drawn by the existing Markdown Viewer (Open rendered); design feedback is a Browser Annotation (ADR-0026) on the prototype opened beside the Agent, so it is sent to that Agent by the existing routing.
5. **Build = the plan's milestones in order, one at a time**, each a Batch: the Agent writes the milestone's Ready Tasks labelled `I-<n>/M<k>` (`rade task add --label`, new), RADE writes and runs a batch file with `initiative: I-<n>` and `merge: integration` (the rest from the `[batch]` layers). That Batch's Integration Branch is `initiative/I-<n>`: made by RADE at the first merge (recorded per Source), reused with its Worktree by every later milestone's Batch, refused when it exists and RADE did not make it. It extends ADR-0020's one exception (RADE commits only to a branch it made) from one Batch to one Initiative; still never the base, never pushed by RADE.
6. **Acceptance is an independent Agent**: after the Batch, RADE starts a fresh Agent (not the builders, not the Initiative Agent) in the integration Worktree to check the milestone's exit and acceptance criteria and write `checks/M<k>.md` ending `Result: PASS|FAIL`. RADE only reads the result line; the owner decides (Continue or Request changes).
7. **Many Initiatives** share `[initiative] agents` (default 4) per Workspace: an extra scheduler observation (`Obs::Slots`) caps how many items an Initiative Batch may hold, computed from the other Initiative Batches' live items and what higher-priority ones want to start (pure `initiative::allowed`); nothing running is stopped. Conflicts are found early by `git merge-tree --write-tree` between the branches of Initiatives sharing a Source, whenever a branch moves (checked every 10 s, off the GTK thread), shown on both pages and in the Inbox.

8. **Only RADE runs an Initiative's Batch.** `batch run` refuses a batch file with `initiative:` unless RADE is running it for the current milestone (acceptance H1: an Agent could otherwise commit onto `initiative/I-<n>` before any gate). The brief's `kind` is read once when RADE first sees the Initiative (an Agent editing the brief can't skip Design); `title` and `priority` follow the file. A checks file from an earlier round is moved aside (`M<k>.round-<n>.md`) before an acceptance Agent starts, so only its own file counts; a closed acceptance Pane or a vanished Batch stops the milestone with a problem and Ask again.

## Simplifications (kept lean)

- **Discovery is the Concept phase's first draft**, not a separate gate: the Agent asks the owner in its own Terminal before writing.
- **The design-review Agent is the Initiative Agent's to spawn** (the prompt and `rade-design` say so); RADE doesn't orchestrate it.
- **Milestones run one at a time per Initiative** (parallelism is inside the Batch and across Initiatives); `Depends:` is validated (earlier milestones only) but the order is the file's.
- **The acceptance Agent and the Initiative Agent use the Project's Agent** (`[task] profile`, else `claude`), without a separate Profile key.
- **Conflict detection is a trial merge, not a trial rebase** (same answer about conflicting files, writes nothing).
- **The Initiative Agent is not continued after a restart** (unlike a Batch's Task Agents): the phase is kept and the owner prompts it or starts a new one.
- **`initiative/I-<n>` is not namespaced by Project**: two Projects whose Initiatives share a number and a Source can't both build there — the second is refused (its branch exists and RADE did not make it for that Initiative); rename or delete the branch, or renumber. Safe, rare, left for later.
- **`priority` is in the brief an Agent can write**: an Agent could raise its own Initiative's share of the cap; it can't exceed the cap or stop other Agents. Accepted.
- No Event Journal events of their own: the Log in `initiative.md` and the Batches' journal lines cover it.

## Considered Options

- **Phases in `initiative.md`'s frontmatter**: the Agent writes that file. Rejected (ADR-0025).
- **Initiatives as a kind of Task or Batch**: a Task is one Run, a Batch one list; an Initiative spans documents, gates and many Batches. A folder that uses both is simpler than stretching either.
- **A `batch/B-<n>` per milestone, merged by the owner**: the owner would merge N branches per Initiative and later milestones wouldn't build on earlier ones. One branch per Initiative, made and reused by RADE.
- **An `initiative.approve` method guarded by the caller**: any Agent could call it (ADR-0020, ADR-0025). Rejected.
- **RADE asking the acceptance Agent for JSON**: a Markdown table with a last result line is readable by the owner and the Agent alike.

## Consequences

- `state.json` gains `initiatives` and `initiative_page`; a Tab gains `initiative`; a Batch gains `initiative`; a batch file may say `initiative: I-<n>` (no version bump: all optional).
- `initiative/I-<n>` collects every milestone; merging it (or opening a PR) is the owner's.
- An Agent with shell access can still write `checks/M<k>.md` itself or edit a document after review; the gates guarantee the order (nothing builds before the plan is approved, no milestone goes on without the owner), not the Agents' honesty — the Versions show what was approved.
