# Sidebar membership is explicit; main checkouts are pinned; the UI never blocks on readiness

Status: accepted 2026-10-05 (M26, PRD R30 rewrite, R38/R55 amendments, R80–R82). Supersedes the R30.1 0.3.1 branch rule (the ADR-0017 follow-up). Amends ADR-0023 (the UI side of the Definition of Ready) and ADR-0025 (a Verify command typed in Project settings counts as confirmed). Owner decisions in v0.7.1:docs/design/simplify-proposal.md ("Owner decisions").

Amended by ADR-0033 and ADR-0034 (1.0.0): decisions 4, 5 and 7 (readiness, the Run dialog / Running together, features in the Task flow) and the Verify clause of 6 are removed with Tasks; decisions 1–3 become the Navigator's rules and No Project becomes Scratch.

The owner found Tasks, Batches and Initiatives hard to use and the sidebar unstable: a main checkout showed under whichever Project's Named Base branch it happened to be on, so `git switch` moved rows under the cursor, and every Project repeated every Source as a header row.

## Decision

1. **A Worktree is under a Project only when its recorded `project` says so** (set when it is made from a Project or a Task, by Move to Project ▸, or by a suggestion click). No branch inference: `design.rs` drops `shows_under`'s branch rule. Branches still produce *suggestions* (`→ BEACON?`), never placement.
2. **Main checkouts are a pinned section**, one row per Source with its current branch as dimmed text. They never get a `project` and never move. Linked Worktrees without one are in **No Project**.
3. **One-time freeze** so nothing visible moves on upgrade: a linked Worktree the old rule showed under exactly one Project gets that `project` recorded, once, guarded by `state.json` `sidebar_rule: 2`. `project` already exists on a Worktree (R28.2), so the state version does not change and an older RADE still reads the file.
4. **The UI never blocks on the Definition of Ready.** It shows "Needs info" with Fix with Claude (a Brainstorm scoped to the missing items) and Start anyway (= `--force`, logged). The CLI, the socket and `batch run` keep every gate: Agents still have to write ready Tasks (ADR-0023 stands for them). An owner action in the UI is the owner's decision, which is what `--force` already meant.
5. **Running several Tasks is a dialog that writes the Batch file** (`batch.add` + `run`), and a running Batch is a group in the Tasks list; the Batch page stays for debugging. One new policy value, `review: agent-then-human`, makes "Claude reviews, then me" honest instead of approximating it.
6. **Project settings writes `project.toml` one key at a time with `toml_edit`** (comments and order kept), through `project.set`. A Verify command typed there is the owner's own text in RADE's UI, so it is stored as confirmed in the same action; `project.set` from the socket is not (an Agent can call it).
7. **M27: features in the Task flow.** An Initiative is a card in its Project's Tasks list (four steps Brief · Spec · Plan · Build, one primary action, its milestone Tasks under it); its document gates are answered from a review bar on the Markdown Viewer (the same UI-only actions as the Initiative page, ADR-0027 stands). New Initiatives merge Concept into Spec (Discovery writes one `spec.md`), so a `backend` one has two owner gates before Build; the flow is recorded per Initiative (`flow`), older ones keep their seven phases and `[initiative] gates = "full"` keeps them for new ones too. The Run dialog's presets are layered config (`[batch.presets.<name>]`) over three built-ins.

## Considered Options

- **Keep the branch rule, but sticky once seen**: still infers, and a first sight on the wrong branch sticks. Rejected.
- **Main checkouts under a Project picked once per repo**: one repo is in several Projects (globex-web is in BEACON, ATLAS and Globex Web Main), so any pick is wrong for the others. Rejected (owner Q2).
- **Drop the readiness gate everywhere**: Agents would start unready Tasks unattended. Rejected (owner Q3).
- **A standing queue Batch per Project (direction B)**: a model change for Batches and Initiatives; deferred.

## Consequences

- The m16 (R30) and m17/m21 (sidebar, Board refusal dialog) suites change their expectations; m19, m25 keep driving Batches and Initiatives through `debug.*`.
- A Worktree made by hand outside a Project lands in No Project until the owner moves it or clicks its suggestion.
- Older RADE versions reading a 0.6 `state.json` ignore `sidebar_rule` and `start_here_dismissed`, and fail to parse a Batch with `review: agent-then-human` (only a downgrade with such a Batch running is affected).
