# AI-native SDLC: practices, RADE gaps, Task format

Researched 2026-10-03, after v0.4.0 (M20). Starting point: Anthropic's *AI-Native SDLC Playbook*. Practices are taken from vendor docs, engineering blogs, specs and repositories (Anthropic, OpenAI, GitHub, Kiro, Cognition/Devin, the AGENTS.md site), read with page fetches. Each claim links to its source. Anything I could not confirm from a primary source is marked **UNVERIFIED**. I did not consult Cursor, Factory or Amp docs (nothing below relies on them).

---

## 1. Summary

- **The bottleneck has moved.** The playbook's thesis is that building is no longer the constraint; the human-speed steps around it are (planning, review, deploy). Each stage ends by committing an artifact that the next stage reads: `intent.md` → `spec.md` → `plan.md` → diff + tests → PR → merge ([playbook][pb]).
- **Every vendor uses the same task shape:** *Goal, Context, Constraints, Done when* (Codex, [oai-bp]). Copilot uses *problem, acceptance criteria, which files* ([gh-bp]). Claude Code adds *files and interfaces involved, out of scope, an end-to-end verification step* ([cc-bp]). RADE Tasks have only Goal and Done when.
- **Verification is the biggest lever.** "Give Claude a check it can run". "Have Claude show evidence rather than asserting success" ([cc-bp]). The playbook wants a single command that exits non-zero, a quantified target, and pasted output ([pb]). RADE gives the Task Agent no verification command, and the Reviewer gets no evidence.
- **Plan before code for anything non-trivial.** The plan names the files that change, the order, the risks and the proof. A human corrects it while that is still a document edit ([pb], [cc-bp], [oai-bp]). RADE has no plan step or gate in a Task Run or Batch.
- **Separation of duties in review.** A fresh-context reviewer checks the diff against the spec, and "the agent that wrote code cannot approve it" ([pb], [cc-bp]). RADE already does this (Batch `agent-judge` plus human approval that only works in the UI, ADR-0020). It is ahead of most tools here.
- **Deterministic gates go behind advisory text.** Skills and CLAUDE.md are advisory; hooks and code enforce ([pb], [cc-bp], [hooks]). RADE's scheduler is this pattern already. What it lacks is a deterministic "verify passed" fact.
- **Compounding knowledge.** "When Claude makes a mistake twice, the correction goes into CLAUDE.md", and review findings feed back into it ([pb]). RADE collects Reviewer notes and Log lines but never closes this loop.
- **Measure the flow.** Recommended measures: first-pass success rate, rework cycles per change, time from plan approval to merge, review time, concurrent sessions per engineer while review quality holds ([pb]). RADE's Event Journal already has the raw events (`task.status`, `agent.*`, `pr.create`) but no flow metrics.
- **The Task format is close but thin.** Keep it, and add optional body sections (`## Context`, `## Constraints`, `## Verify`, `## Plan`) plus three scalar or flow-list keys (`kind`, `risk`, `depends`). RADE already parses and edits that shape. Add a Definition of Ready check in rade-core, used by `task status ready`, `task start` and `batch run`.
- **Use the existing pieces.** Almost every recommended change extends Tasks, Batches, Profiles, layered config, the Event Journal and the skills. Nothing needs a new subsystem.

---

## 2. What the playbook says

*The AI-Native SDLC Playbook*, Louis Claxton, 2026-08-21 ([pb]). It has six stages. Each stage has "plays" with prerequisites, steps, governance and leading/lagging metrics.

| Stage | Practice | Artifact / mechanism |
|---|---|---|
| 1 Plan | The person with the idea brainstorms it with Claude until it is concrete, then commits it. The product owner reviews. | `intent.md`: problem, proposed outcome, affected users/systems, constraints, open questions, author + status. |
| 2 Design | Requirements and design are written in one session, with organization **skills** (security, brand, UX, compliance) applied as constraints. Concerns are flagged for named policy owners. A human makes the acceptance decision. | `spec.md`, produced by a slash command or automatically when `intent.md` is merged. |
| 3 Build: plan mode | Claude plans read-only. The engineer interrogates the plan ("what could break? riskiest step? rejected options?") and iterates "until someone unfamiliar with the conversation could implement from the plan alone". If the implementation departs from the plan, `plan.md` is updated in the same commit. | `plan.md`: files that change, order of work, risks, proof (tests). |
| 3 Build: auto mode | No per-edit prompts once the plan is approved, for routine, well-tested, small-blast-radius work. The human reviews artifacts afterwards instead of each action. | Needs mature guardrails. |
| 3 Build: CLAUDE.md | Keep it under one page: commands with success indicators, conventions, architecture, common mistakes. "When Claude makes a mistake twice, the correction goes into CLAUDE.md." | Checked into git, shared by the team. |
| 3 Build: skills | Use skills for institutional knowledge that must be applied consistently. The policy owner signs off on changes. Skills are advisory; a hook or review pass makes them deterministic. | `.claude/skills/<name>/SKILL.md` |
| 3 Build: hooks | Fast, file-scoped build hooks (protected paths, formatter, no credentials). Heavy checks run at commit/PR. Approval prompts belong at deploy, not at build. | `.claude/settings.json` |
| 3 Build: parallel | One worktree per independent task. Start with 2–3 sessions; "ceiling is how many streams one person reviews properly". Recurring jobs become subagents, e.g. a read-only `verifier` that runs the app and reports without fixing. | `.claude/agents/*.md` |
| 4 Test: feedback loop | One command per check (exits non-zero on failure), listed in CLAUDE.md with its healthy output. Quantified target. Bugs: write the failing test first, commit it, fix without editing the test. A hook blocks test edits during fix tasks. "Run all three before reporting done; paste output." | A "Verifying your work" block in CLAUDE.md. |
| 4 Test: evals in CI | 20–50 real tasks with accepted outcomes, run whenever CLAUDE.md, skills or hooks change. Every incident becomes an eval. | Eval suite plus a pass-rate threshold. |
| 5 Deploy: AI review | Claude reviews against `REVIEW.md` (passes: bugs / security / compliance with spec + plan; "Important" defined; at most 5 nits; exclusions). The writing agent cannot approve. Branch protection keeps a human owner. | `REVIEW.md` |
| 5 Deploy: gates | Hooks as approval gates. Autonomy tiered by environment. Rollback is the most rehearsed path. | Hooks, managed settings |
| 6 Maintain | Deterministic anomaly detection (rolling baseline, σ tiers: log / diagnose read-only / propose). The diagnosis comes back in as a new `intent.md`. Scheduled scans. | `bands.yaml` |

Principles worth quoting ([pb]): "A stage ends by committing an artifact the next stage can read." "Humans remain accountable for every decision that requires judgment." "The chain of commits is the audit trail."

---

## 3. Best practices by area

### 3.1 Spec-driven development
- **Spec Kit** runs `/speckit.constitution → specify → clarify → plan → tasks → analyze → implement` ([sk]). The spec states *what/why* (user stories, Given/When/Then acceptance scenarios, edge cases, `FR-###` requirements, `SC-###` measurable success criteria, assumptions) and marks unknowns as `[NEEDS CLARIFICATION]`. It is not complete while any marker remains ([sk], [sk-spec]). A task line is `- [ ] T001 [P] [US1] Description with file path`, where `[P]` means "different files, no dependencies". Tests are written first and must fail. There are checkpoints between stories ([sk-tasks]).
- **Kiro** writes three files per spec: `requirements.md` (user stories + **EARS** acceptance criteria, `WHEN <event> THE SYSTEM SHALL <behavior>`), `design.md` (architecture, sequence diagrams), `tasks.md` (checkbox tasks with `_Requirements: 1.1_` back-references and real-time status) ([kiro], [kiro-fs]). Kiro runs independent tasks concurrently in "waves" after dependency analysis ([kiro]).
- **Claude Code**: for larger features, "have Claude interview you" with `AskUserQuestion`, write `SPEC.md`, then implement in a fresh session. "The most useful specs are self-contained: they name the files and interfaces involved, state what is out of scope, and end with an end-to-end verification step" ([cc-bp]).
- **Anthropic long-running harness**: an initializer agent writes a feature list as **JSON with a `passes` flag**, because "the model is less likely to inappropriately change or overwrite JSON files compared to Markdown". The agent works "on only one feature at a time", commits, and writes a progress file ([harness]).

### 3.2 Task / issue format that agents implement well
- Codex: **Goal, Context** (files, docs, examples, errors), **Constraints** (standards, architecture, safety), **Done when** (tests pass, behavior changes) ([oai-bp]).
- Copilot coding agent: a clear problem description, "complete acceptance criteria on what a good solution looks like (for example, should there be unit tests?)", "directions about which files need to be changed" ([gh-bp]). Keep ambiguous, security-sensitive or broad cross-repo refactors for humans ([gh-bp]).
- Devin: be specific, give completion criteria with concrete thresholds, say how to test ("Run `npm run dev` and verify the new page renders"), name reference files ("Use authTemplate.rs as a reference"), and "split tasks into verifiable sub-tasks, and start one Devin session for each" ([devin]).
- Claude Code: scope (file, scenario, test preferences), point to sources, reference existing patterns, describe the symptom + likely location + what "fixed" looks like ([cc-bp]).

### 3.3 Verification loops
- "Without a check it can run, 'looks done' is the only signal". Gate the stop in a prompt, with `/goal`, with a **Stop hook** that blocks the end of the turn until the check passes, or with a **verification subagent** in a fresh context ([cc-bp], [hooks]).
- Ask for evidence: test output, commands and their results, screenshots ([cc-bp]). Paste the output ([pb]).
- Agents mark features done without checking end to end. Browser automation "dramatically improved outcomes" ([harness]). Devin tests in its own desktop/browser ([devin]).
- Fix the root cause, don't suppress the error. For bugs, write the failing test first and don't let the fix edit the test ([cc-bp], [pb]).
- Adversarial review: tell the reviewer to "flag only gaps that affect correctness or the stated requirements", or it will invent findings and push the work toward over-engineering ([cc-bp]).

### 3.4 Context engineering
- CLAUDE.md / AGENTS.md: short. For each line ask "Would removing this cause Claude to make mistakes?" Put build/test commands, non-default conventions and gotchas in it, not file-by-file descriptions. Put sometimes-relevant knowledge in **skills** ([cc-bp]). AGENTS.md is "a README for agents". The closest file wins and the user's prompt overrides everything ([agents-md]). Codex concatenates files root → cwd up to 32 KiB ([oai-agents]). "A short, accurate AGENTS.md is more useful than a long file full of vague rules" ([oai-bp]).
- Load context just in time by reference (paths, links) instead of pasting it. Use compaction, structured notes outside the context window (NOTES.md / progress files), and subagents that return 1–2k-token summaries ([ctx]).
- `/clear` between tasks. After two failed corrections, start over with a better prompt ([cc-bp]).

### 3.5 Multi-agent orchestration
- Patterns: prompt chaining, routing, parallelization (sectioning or voting), orchestrator–workers, evaluator–optimizer. Use workflows (fixed code paths) when steps are predictable. Agents need "stopping conditions (such as a maximum number of iterations)" and checkpoints for human feedback ([bea]).
- Writer/Reviewer sessions. Fan-out with `claude -p` over a task list: "test on a few files, then run on all of them" ([cc-bp]).
- Agent teams: shared task list with dependencies and file-locked claiming. Start with 3–5 teammates and 5–6 tasks each. Each teammate owns different files to avoid conflicts. `TaskCompleted` / `TeammateIdle` hooks can refuse completion (exit 2) as quality gates. Teammates can plan first and send a plan approval request ([teams], [hooks]).

### 3.6 Human review gates
- The agent that wrote the code cannot approve it. Branch protection keeps a human owner. Approval gates are hooks that explain why they block ([pb]).
- Review artifacts after longer autonomous runs instead of approving each action ([pb], [cc-bp] auto mode).

### 3.7 Observability and metrics
- Leading measures: time idea → intent → spec, plan approval → merged PR, first-pass CI success, time to first review, share of review comments resolved without a human. Lagging measures: rework cycles, requirements rework after build starts, change failure rate, defects that escaped, "how often merged diff matches committed plan.md", concurrent sessions per engineer while review quality holds ([pb]).
- Evals: a live suite of real tasks that gates changes to agent configuration (CLAUDE.md, skills, hooks, model) ([pb]).

---

## 4. Gap analysis: RADE vs practices

| Practice | RADE today | Gap | Impact |
|---|---|---|---|
| Captured intent → spec (intent/spec.md, Spec Kit, Kiro) | Task file (`## Goal`, `## Done when`), Brainstorm with Claude asks one question at a time and proposes a split (R35.4, R57, ADR-0022, `skills/rade-task`). | No context / constraints / non-goals sections. No clarification markers. Nothing distinguishes a one-line Task from a feature that needs a spec. | High: under-specified Tasks drive rework and Reviewer `changes` rounds. |
| Definition of Ready | `status: ready` is a label. `batch run` only checks the status string (`batch.rs` `check`, R48.2). The skill says to use `ready` "only when Goal and Done when are clear". | Nothing checks that the content exists. | High: Batches start Tasks with empty specs. |
| Context pointers (files, patterns, docs) | Task Agent prompt = file path + Worktrees (`crates/rade-app/src/tasks.rs` `prompt`). CLAUDE.md / AGENTS.md come from the repo. | No convention for "look at X, follow pattern Y". | Medium |
| Plan before code, human-approved | RADE's own process: "Start in plan mode" (CLAUDE.md). For user Tasks: none. A Profile could pass `--permission-mode plan` (R36 `args`) but nothing waits on a plan. | No plan section, no plan gate in Task Run or Batch. | High for risky Tasks. |
| Single verify command + evidence | RADE itself has `docs/internals/checks/M<n>.md` with commands and results. Sources have `setup` (R12.2, R29) but no `verify`/`test` key. The Task Agent prompt says "summarize what changed and stop". | No verify command, no evidence recorded in the Task, no gate. | **Highest**: [cc-bp] calls this the difference between watching and walking away. |
| Deterministic stop gate (Stop hook / TaskCompleted) | `rade setup` installs status hooks (`rade status --hook`). The scheduler is deterministic (ADR-0020). | `running → review` happens when the turn ends, whether or not checks passed. | High (Batch quality). |
| Fresh-context reviewer, separation of duties | `agent-judge` Reviewer, read-only, Task file + diff, `VERDICT:` line, `max_rounds`. Human approval only in the UI (R50, ADR-0020). Fan-out Judge (R15). | The Reviewer gets no verify output and no review rubric (REVIEW.md-style passes, "only correctness/requirement gaps"). Not asked to tick each Done-when item. | Medium. A good base. |
| Compounding knowledge (mistake twice → CLAUDE.md) | Reviewer notes reach the Task Agent (`changes_prompt`). Log lines. Reports (R41–R47). | Notes are never aggregated or proposed as CLAUDE.md/skill edits. | Medium, compounds over time. |
| Parallel worktrees, 2–3 streams, file ownership | Task Runs = Worktrees per Source (R35). Batch `parallel`, `after:` (R48–R49). Fan-out (R15). | No "touches files" hint to choose `after:` or `[P]`. The Orchestrator guesses (`skills/rade-batch`). | Low–Medium |
| Task dependencies | Only Batch `after:` (inside one Batch). | No `depends:` on the Task itself, so Brainstorm splits lose ordering. | Medium |
| Human gates for publishing | ADR-0013 (git/gh only on click), ADR-0020 (Integration Branch only, `push: ask`). | None. RADE is stricter than the playbook. | n/a |
| Flow metrics | Event Journal (`agent.start/end`, `task.status` with `by: owner|auto`, `pr.create`; `crates/rade-core/src/journal.rs`). Digest/Reports (R41–R43). | No first-pass rate, review rounds, cycle time, rework. | Medium: tells the owner whether parallelism is paying off. |
| Evals for agent configuration | E2E suites for RADE itself. None for Profiles/skills. | No replay of past Tasks against a new Profile/model/skill. | Low now (single owner). Later. |
| Short, curated agent instructions | CLAUDE.md is long (Commands + Code map ≈ 100 lines). Skills are on-demand. | RADE's own CLAUDE.md breaks "keep it under one page" ([pb]) / "would removing this cause mistakes?" ([cc-bp]). | Low–Medium (RADE's own dev, not the product). |

---

## 5. Prioritized improvement directions

Ordered by impact per effort. Sizes: S ≤ 2 days, M ≤ 1 week, L > 1 week.

1. **Task template v2 + Definition of Ready check** (S–M)
   *What:* Optional body sections `## Context`, `## Constraints`, `## Verify`, `## Plan`, `## Open questions`, and keys `kind`, `risk`, `depends` (§6). A pure `task::readiness(&Task, body) -> Vec<Problem>` in rade-core. `rade task status T ready` refuses with the list (or `--force`). `task start` and the Task Detail show the problems. `batch run` validation includes them (R48.2 already lists every problem at once).
   *Why:* every vendor's task shape has context/constraints/done-when ([oai-bp], [gh-bp], [devin]). Spec Kit refuses specs with open `[NEEDS CLARIFICATION]` ([sk]).
   *Builds on:* `task.rs` parse/`set_field_raw`, `task_detail.rs` (body split, New Task template), `batch.rs` `check`, rade-task skill, Brainstorm prompt.

2. **Verify command and evidence in the Log** (M)
   *What:* A layered key `[task] verify = "cargo test --workspace"` (Project / Source `.rade.toml` under the existing trust rule, R29.4). Optionally per Task through `## Verify` (text for the agent, not executed). When the Run root reaches `done`, RADE runs the trusted `verify` in each Run Worktree (like `setup`, R12.2). It appends `verify passed|failed (exit n, <cmd>)` to the Log, saves the output to a file next to the diff, and moves to `review` only on success. On failure it prompts the Task Agent with the tail of the output (bounded rounds, like `max_rounds`). The Task Agent prompt also tells the agent to run it and paste the result.
   *Why:* "Give Claude a check it can run", evidence over assertion, Stop-hook gating ([cc-bp], [hooks]). One command that exits non-zero ([pb]). Agents mark features done without end-to-end checks ([harness]).
   *Builds on:* `setup` execution, `shell_env::bounded_output`, R37 auto status, Batch `step` (a new `verifying` item state), `changes_prompt`.

3. **Reviewer rubric: per-criterion verdict + verify evidence + REVIEW.md** (S)
   *What:* `review_prompt` adds the verify result and asks for each `## Done when` item as met/unmet with a path:line. It adds "flag only correctness or requirement gaps; scope creep is a gap". A layered `[review] guide = "REVIEW.md"` file (Source/Project) is appended when present. The Fan-out Judge uses the same rubric.
   *Why:* REVIEW.md passes, caps and "Important" definition ([pb]). The reviewer over-reports unless scoped ([cc-bp]).
   *Builds on:* `batch::review_prompt`, `parse_verdict`, `fanout.rs` Judge.

4. **Plan gate for risky Tasks** (M)
   *What:* When `risk: high` (or a Batch policy `plan: human|agent|none`), the Task Agent's first turn is "write `## Plan` (files that change, order, risks, proof) in the Task file and stop". RADE moves to a `planned` item state (Batch) or shows "Plan ready" (Task Detail). The owner approves with a button (UI-only, like R50.3), which sends "implement the plan; update `## Plan` if you deviate". Without a Batch: the Task Detail's Approve plan button.
   *Why:* plan mode + `plan.md` + approval before code ([pb], [cc-bp]). Teammate plan approval ([teams]).
   *Builds on:* Batch `step` (one more state between `starting` and running), `awaiting-human` UI, `task_detail` sections, Profiles `args` (e.g. `--permission-mode plan`).

5. **`depends:` on Tasks, used by Batches and Brainstorm** (S)
   *What:* A flow list `depends: [T-3]`. `batch add --task` derives `after:` from it when both are in the Batch, and validation fails if a dependency is neither `done` nor in the Batch. Brainstorm writes it when it splits. The Board/List shows "blocked by".
   *Why:* Kiro waves, Spec Kit `[P]`, agent-team task dependencies ([kiro], [sk-tasks], [teams]).
   *Builds on:* `list_of`, `batch.rs` `after:` cycle check, `task_view.rs` filters.

6. **Flow metrics in Digest/Reports** (S–M)
   *What:* From the Event Journal + Task Logs: first-pass rate (Tasks approved with 0 `changes` rounds / verify failures), review rounds per Task, time ready → review → done, verify failure rate, Batch items failed vs merged. Add them as a "Flow" section to the Digest (`rade digest --flow`) and as `rade task stats --since 7d`.
   *Why:* the playbook's leading/lagging indicators ([pb]).
   *Builds on:* `journal.rs` (add `verify.*` / `review.verdict` events), `digest.rs` deterministic render (ADR-0019).

7. **Lessons loop: Reviewer notes → proposed CLAUDE.md/skill edits** (S)
   *What:* `rade task lessons [--since 30d]` collects `changes` notes, `verify failed` lines and human Request-changes notes. It hands them to the no-tools draft command (like Reports) to propose a short diff to the Source's CLAUDE.md/AGENTS.md. The owner applies it by hand (ADR-0013 spirit: agents draft, humans commit).
   *Why:* "mistake twice → CLAUDE.md". Review findings feed back ([pb]). Prune and test CLAUDE.md ([cc-bp]).
   *Builds on:* `report.rs` drafting, `[report] draft_command`, Event Journal.

8. **Context pointers in the Task Agent prompt** (S)
   *What:* The prompt tells the agent to read `## Context` and `## Constraints` first, follow listed reference files, and run `## Verify` before stopping. A layered `[task] prompt_extra` (short text) for Project conventions, such as "follow docs/adr". No content is inlined (ADR-0018 §4 stays).
   *Why:* just-in-time context by reference ([ctx]). Point to patterns ([cc-bp], [devin]).
   *Builds on:* `tasks.rs` `prompt`, layers allow-list (`task` table already layered, R36).

Later / maybe: **replay evals** (L): re-run N done Tasks from their base commit through a Fan-out with a new Profile/model/skill and compare verify pass rate. This is the playbook's "continuous evals" ([pb]) built from Fan-out + verify. Worth it only after 2 and 6 exist.

**Candidate milestones**
- **M21 — Ready Tasks**: directions 1, 5, 8 (format, DoR, depends, prompt). Mostly rade-core + skill + Task Detail.
- **M22 — Verified Runs**: directions 2, 3 (verify + evidence + reviewer rubric), Batch `verifying` state.
- **M23 — Plan gate + Flow**: directions 4, 6, 7.

---

## 6. Task format evaluation

### Is the current format enough?
For small, well-understood work, yes. Goal + checkbox Done when matches the core of Codex's *Goal / Done when* and Copilot's *problem + acceptance criteria* ([oai-bp], [gh-bp]). The Task file is live and passed by path, which already follows just-in-time context ([ctx]). Line edits and `flock` ids are better engineered than most tools' task stores.

What it lacks, compared with Kiro / Spec Kit / Copilot / Codex / Devin:

| Missing | Who asks for it | Needed? |
|---|---|---|
| Context: files, patterns, docs to read | Codex Context, Copilot "which files", Devin reference files, Claude "files and interfaces involved" | **Yes**: `## Context` |
| Constraints and non-goals / out of scope | Codex Constraints, Claude "state what is out of scope", Spec Kit Assumptions | **Yes**: `## Constraints` (with a "Not in scope" list) |
| Acceptance criteria as checkable items | Kiro EARS, Spec Kit Given/When/Then + SC-###, Copilot | Already there (`- [ ]` under Done when). Tighten the guidance: observable, one behavior per line. EARS/GWT optional. |
| Verification commands + expected result | Claude "end-to-end verification step", Devin "how to test", playbook "paste output" | **Yes**: `## Verify` (text) + trusted `[task] verify` (executed) |
| Test plan (which tests to add) | Copilot "should there be unit tests?", Spec Kit tests-first | Fold into Done when / Verify. No separate section. |
| Dependencies | Kiro waves, Spec Kit `[P]`, agent teams | **Yes**: `depends:` key |
| Risk / rollback | Playbook plan.md risks, tiered autonomy | `risk:` key (drives the plan gate). Rollback is implicit: the Worktree/branch is disposable (ADR-0020). |
| Plan | Playbook plan.md, Claude plan mode | `## Plan`, written by the agent (optional, required when `risk: high`) |
| Open questions / clarification markers | Spec Kit `[NEEDS CLARIFICATION]`, intent.md open questions | `## Open questions`. DoR fails while it has unchecked items. |
| Task kind | Kiro feature vs bugfix spec, playbook "failing test first for bugs" | `kind:`, so `bug` adds "reproduce as a failing test first" to the prompt |

**Unnecessary for agents** (keep for humans, no change): `estimate`, `due`, `labels`, `priority`. Agents ignore them and they cost nothing. Don't add `assignee`, story points or EARS-mandatory syntax: single owner, and EARS adds ceremony that the checkbox list already covers.

### Parser compatibility
`task.rs` `parse` reads known keys by name. Unknown keys are ignored, and `list_of` accepts flow lists. `set_field_raw` inserts a missing key before the closing `---` and collapses block lists to one flow line. So the new keys are scalars or flow lists that old RADE versions simply ignore. The body is free Markdown. Description and Log are split at `## Log` (`task_detail.rs`), so new sections must sit **above `## Log`**, which stays last.

### Recommended Task template

```markdown
---
id: T-12
title: "OTP login"
status: inbox            # inbox | ready | running | review | done | dropped
kind: feature            # feature | bug | chore | spike   (new, optional; bug → failing test first)
risk: low                # low | medium | high             (new, optional; high → plan gate)
depends: []              # Task refs that must be done first (new, optional, flow list)
priority: p1
due: 2026-10-20
sources: [backend, app]
base: dev
profile: deep
branch: task/T-12-otp-login
links: [https://jira/BEACON-341]
labels: [auth]
estimate: 2d
created: 2026-10-02T09:10
updated: 2026-10-02T11:05
---
## Goal
What changes for whom, and why. 2–5 lines.

## Context
- Read: backend/src/auth/session.rs (token refresh), docs/internals/adr/0007-….md
- Follow the pattern in: backend/src/auth/email_login.rs
- Docs: https://…/sms-provider-api

## Constraints
- No new dependencies; reuse the existing SMS client.
- Not in scope: rate limiting (T-14), UI copy.

## Done when
- [ ] POST /otp/send returns 202 and sends one SMS (unit test with the fake client)
- [ ] A code older than 5 min is rejected with 401 (test)
- [ ] Existing auth tests still pass

## Verify
- `cargo test -p backend auth::` → all pass
- Manual/E2E: `rade browser open 5173`, log in with the OTP, screenshot

## Open questions
- [ ] (none: ready only when every item here is checked or removed)

## Plan
(Task Agent writes this when asked: files that change, order, risks, proof)

## Log
- 2026-10-02 09:10 created
```

Only `## Goal` and `## Done when` are required, as today. Old Tasks stay valid.

### Definition of Ready (pure `task::readiness`, enforced by `rade task status … ready`, `rade task start` and `batch run`; the Brainstorm prompt and the rade-task skill tell the agent to meet it)
1. `## Goal` is non-empty.
2. `## Done when` has at least one `- [ ]` item. No item is vague (warn on items without a verb/observable, e.g. only "works", "clean", "better": a heuristic, warning not error).
3. Verification is known: a `## Verify` section **or** a trusted `[task] verify` for the Task's Sources.
4. `## Open questions` has no unchecked item and the body has no `NEEDS CLARIFICATION`.
5. `sources:` and `base:` resolve (already checked at start, R35). `depends:` refs resolve, and each one is `done` or in the same Batch.
6. `risk: high` → a plan gate applies (not a blocker for `ready`, but `start` uses the plan-first prompt).
7. Size: at most ~7 Done-when items and one deliverable. Otherwise suggest a split (warning). This follows "split tasks into verifiable sub-tasks" ([devin]) and "self-contained units that produce a clear deliverable" ([teams]).

Errors (1, 2-empty, 3, 4, 5) block `ready` unless `--force`. Warnings (2-vague, 7) are shown in the Task Detail and `task show`.

---

## 7. Sources

- [pb] Anthropic, *The AI-Native SDLC Playbook* (2026-08-21): https://claude.com/blog/the-ai-native-sdlc-playbook
- [cc-bp] Claude Code docs, *Best practices*: https://code.claude.com/docs/en/best-practices
- [hooks] Claude Code docs, *Hooks reference*: https://code.claude.com/docs/en/hooks
- [teams] Claude Code docs, *Agent teams*: https://code.claude.com/docs/en/agent-teams
- [ctx] Anthropic Engineering, *Effective context engineering for AI agents*: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- [bea] Anthropic Engineering, *Building effective agents*: https://www.anthropic.com/engineering/building-effective-agents
- [harness] Anthropic Engineering, *Effective harnesses for long-running agents*: https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents
- [oai-bp] OpenAI Codex, *Best practices* (developers.openai.com/codex/learn/best-practices redirects here): https://learn.chatgpt.com/guides/best-practices
- [oai-agents] OpenAI Codex, *AGENTS.md* (redirected from developers.openai.com/codex/guides/agents-md): https://learn.chatgpt.com/docs/agent-configuration/agents-md
- [agents-md] AGENTS.md open format: https://agents.md/
- [gh-bp] GitHub Docs, *Best practices for Copilot coding agent*: https://docs.github.com/en/copilot/tutorials/coding-agent/get-the-best-results
- [sk] GitHub Spec Kit, *Specification-Driven Development*: https://github.com/github/spec-kit/blob/main/spec-driven.md
- [sk-spec] Spec Kit spec template: https://github.com/github/spec-kit/blob/main/templates/spec-template.md
- [sk-tasks] Spec Kit tasks template: https://github.com/github/spec-kit/blob/main/templates/tasks-template.md
- [kiro] Kiro docs, *Specs*: https://kiro.dev/docs/specs/
- [kiro-fs] Kiro docs, *Feature specs* (EARS): https://kiro.dev/docs/specs/feature-specs/
- [devin] Devin docs, *Instructing Devin effectively*: https://docs.devin.ai/essential-guidelines/instructing-devin-effectively

UNVERIFIED / not covered: Cursor, Factory and Amp task-writing guidance (not consulted). The Codex pages were read after a 308 redirect to learn.chatgpt.com, which I take to be OpenAI's own docs host (the redirect came from developers.openai.com). The Kiro wave-execution detail comes from the Kiro specs overview page; I did not test it.

[pb]: https://claude.com/blog/the-ai-native-sdlc-playbook
[cc-bp]: https://code.claude.com/docs/en/best-practices
[hooks]: https://code.claude.com/docs/en/hooks
[teams]: https://code.claude.com/docs/en/agent-teams
[ctx]: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
[bea]: https://www.anthropic.com/engineering/building-effective-agents
[harness]: https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents
[oai-bp]: https://learn.chatgpt.com/guides/best-practices
[oai-agents]: https://learn.chatgpt.com/docs/agent-configuration/agents-md
[agents-md]: https://agents.md/
[gh-bp]: https://docs.github.com/en/copilot/tutorials/coding-agent/get-the-best-results
[sk]: https://github.com/github/spec-kit/blob/main/spec-driven.md
[sk-spec]: https://github.com/github/spec-kit/blob/main/templates/spec-template.md
[sk-tasks]: https://github.com/github/spec-kit/blob/main/templates/tasks-template.md
[kiro]: https://kiro.dev/docs/specs/
[kiro-fs]: https://kiro.dev/docs/specs/feature-specs/
[devin]: https://docs.devin.ai/essential-guidelines/instructing-devin-effectively
