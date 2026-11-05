# 0053 — RADE reads PR and CI state with `gh` on its own; writes stay a click

Status: accepted (2026-11-05). Amends ADR-0013 ("RADE runs git / gh only on the owner's click") for **reads** only.

## Context
The owner chose (2026-11-04, `PLAN-m54.md`) to show a Worktree's pull request and its checks (`#482 ✓ / ✗ / ●`) in the Navigator, Project home and the Changes card, and to hand a failing run's logs to the Worktree's Agent. ADR-0013 lets RADE run `gh` only on a click, so a status that refreshes by itself needs a decision. Without it the owner opens GitHub, finds the run, copies the log and pastes it into the Agent, every round.

## Decision
- **Reads run on their own.** For Worktrees whose branch has an upstream or a recorded PR, RADE runs `gh pr list --head <branch> --json …` (and, for a failing PR, `gh run view --log-failed --job <id>`) with the Workspace's env, through `rade_core::proc` (20 s / 30 s limits), off the UI thread, at most one call per Workspace in flight. Every 60 s for the shown Workspace while the window is active (20 s while checks are pending), every 10 min otherwise, at once after a Push / Create PR from RADE or a `HEAD` change. A timeout or a missing / logged-out `gh` stops that Workspace's calls for 10 min. `[scm] pr_status = false` turns all of it off.
- **Writes stay a click.** Create PR, and the cross-link `gh pr edit` of a Branch set's PRs (ADR-0054), run only from the owner's click in a dialog. No socket or CLI method writes to GitHub; `rade pr status|wait|failures` only read.
- **CI failures go to the Agent as text.** The prompt (failed jobs' log tails, cut and stated) is offered on a click (`[scm] ci_failures = "ask"`, the default). The opt-in `"send"` delivers it without a click, only to the Worktree's one Agent once it is `waiting` or `done`, once per head commit and at most 3 times in a row per PR without a passing run; then it asks. RADE itself still writes no git and no GitHub state: the Agent commits and pushes in its Terminal, as before.
- **GitHub Actions only** for logs; other checks are named with their URL.

## Considered Options
- **Refresh only on a click / after a push from RADE**: no background network, but the point (seeing CI turn red while an Agent works elsewhere) is lost. Rejected by the owner.
- **A GitHub webhook / App**: needs a public endpoint or a token RADE manages. Rejected: `gh` and its own login, as ADR-0013.
- **Auto-send by default**: an Agent could loop on a flaky test, spending tokens. Rejected: opt-in, capped at 3.

## Consequences
- RADE makes GitHub API calls through the owner's `gh` login without a click: about 1 per Worktree per minute while watched (gh's REST/GraphQL rate limits are far above this; the backoff covers errors).
- A Worktree's PR number is kept in `state.json` (`pr`), so it shows after a relaunch before the first read.
