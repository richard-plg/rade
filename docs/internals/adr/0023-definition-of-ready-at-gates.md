# The Definition of Ready is checked at gates, not when a Task is read

Status: accepted 2026-10-03 (M21, PRD R58–R62). Builds on ADR-0018 (Tasks are files edited by lines) and ADR-0020 (Batches run by a deterministic scheduler).

Superseded by ADR-0033 (1.0.0).

M21 gives Tasks the sections an Agent needs to work alone (Context, Constraints, Verify, Open questions; `kind`, `risk`, `depends`) and a Definition of Ready (docs/research/ai-native-sdlc.md §6). The owner's existing Task files (v1: Goal, Done when, Log) and older RADE versions must keep working.

## Decision

1. **Readiness is a pure function evaluated at gates.** `ready::readiness(task, body, ctx, gate)` runs when a Task moves to `ready` (any path: CLI, socket, `task add --status ready`, Board, Status menu, Task Detail), when `task start` starts an `inbox` / `ready` Task, and at `batch run`. Parsing, listing, showing and editing never judge a Task, so a v1 file in `inbox` shows no error; it only meets the rules when it is moved on. A re-run of a `running` / `review` Task is not checked.
2. **Errors block, warnings advise; `--force` is the escape** on `task status`, `task add` and `task start` (UI: Set ready anyway / Start anyway). There is no force on `batch run`: a Batch starts Tasks unattended, so its Tasks must be ready; its own starts pass `force` internally because `run` already checked them.
3. **Gates differ only for `depends`**: `→ ready` needs each dep to resolve and not be dropped (you plan dependent Tasks before their deps are done); `start` needs each dep `done`; `batch run` accepts a dep that is an item of the same Batch and turns it into an `after:` edge (so `depends` is the one place order is written; the existing cycle check covers it).
4. **New keys are optional scalars or flow lists; new sections are text above `## Log`.** Older RADE versions ignore them (unknown keys are skipped, the Description / Log split is at `## Log`). An unknown `kind` / `risk` parses; readiness reports it.
5. **`## Verify` is text RADE never runs.** It counts as "verification is known". A `[task] verify` command in the Config Layers (trusted layers only, R29) also counts; executing it is M22's, and only from the config, never from a Task file an Agent can write.
6. **Inline Batch items are not checked.** The Batch author writes them in the batch file and RADE makes them `ready` Tasks at `run`; requiring v2 sections inside a YAML flow item would make inline items unusable. Use Task refs when the DoR matters.
7. **Template hints are HTML comments** (`<!-- … -->`): not content for readiness and not rendered in the Task Detail, so an untouched template is "not ready" and looks empty.

## Considered Options

- **Validate at parse time / mark v1 files as errors**: would put an `error` on the owner's existing Tasks and break listing. Rejected.
- **A `ready` flag stored in the file**: drifts from the body; computed is always current. Rejected.
- **`batch run --force`**: an unattended run of unready Tasks is what the DoR exists to stop. Rejected; fix the Task or start it by hand.
- **Execute `## Verify` from the Task file**: an Agent writes that file, so it would run Agent-chosen commands with the owner's rights outside the Agent's sandbox. Rejected (M22 runs only trusted config).

## Consequences

- Existing E2E suites and Agents that created `ready` Tasks with only Goal / Done when now get a refusal listing what is missing; the skills and prompts tell Agents to write v2 bodies and retry.
- The owner's real `ready` v1 Tasks stay `ready`; starting one asks first (Start anyway) until its Verify is written or a `[task] verify` is configured.
