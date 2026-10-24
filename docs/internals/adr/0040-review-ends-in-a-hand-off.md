# Review is a loop with the Agent and ends in a hand-off

Status: accepted 2026-10-24 (M44, PRD R118, R119; owner decisions in docs/plans/M44-review-loop.md). Amends PRD R13.3 ("No commit / push / PR actions in the Review Pane"). ADR-0013 (RADE runs git / `gh` only on the owner's click, from the Changes page) stands.

## Context

In RADE the owner mostly reads what Agents wrote, says what to change, and decides when it is good. The Review Pane (R13) was built as a diff viewer: it compares against the base, takes Review Comments, and sends them once. The owner found it "missing something". A product review (2026-10-24) named five gaps:
- no context (which Agent, which task, which commits);
- no incremental reading (every round re-reads the whole diff, and nothing marks a file as read);
- comments that vanish when sent;
- no conclusion (R13.3 forbids actions, so a review ends nowhere);
- a layout where Impact pushes the diff down and the comment list sits away from the code.

## Decision

1. **Rounds.** *Request changes* sends the draft comments (R13.2's prompt) and records a **Round**: its number, its time, and a snapshot of the Worktree's files at that moment. The snapshot is `merge::snapshot`, a commit object of the working tree including untracked files. "Since round n" diffs that snapshot against the working tree, so the owner reads only what the Agent changed since. Approving records a Round too.
2. **Snapshots live in the Source's repository** as `refs/rade/review/<worktree name>/<n>` (the git worktree's name, unique in its repository; RADE's own ids are not, across the installed and dev apps), so `git gc` keeps them. The Round records the commit too, so the diff never depends on the ref. At most 20 per Worktree are kept (the oldest is dropped), and all are deleted when the Worktree is removed. `git branch` does not show them; `git for-each-ref` does.
3. **Sent comments stay.** They keep their Round and a state:
   - `draft` → `sent`;
   - after the Agent's next turn ends, `code changed` (the commented lines or their context differ from the Round's snapshot) or `untouched`. These are facts, not a judgement of whether the Agent did it right.
   - The owner then resolves a comment or follows it up.
4. **Viewed** is per file and per Worktree, keyed by a hash of that file's diff text: a file whose diff changes is unread again.
5. **The conclusion is a hand-off** (amends R13.3). The Review Pane's primary button is *Request changes* while there are drafts, else *Approve ▾*. Approve's items:
   - **send the Agent a prompt** (commit, or commit and open a PR): the Agent does the chores the way it already does in its Terminal;
   - **open an existing flow**: the Changes page with the commit message focused, the Create PR dialog, or the Fan-out Choose dialog with this candidate selected.

   The Review Pane itself still runs no git and no `gh`.
6. **Agents take part** through the CLI (`rade review status`, `rade review request`):
   - an Agent reads the open comments after a Review prompt;
   - when done, it says it is ready, with a short summary that the Review header shows; the owner gets a *Ready for review* Notification.

## Considered options

- **Keep R13.3 (no actions).** It is simple, but the review dead-ends; this is the gap the owner felt. Rejected.
- **Stage / commit inside the Review Pane.** It would be a second Changes page, with a diff against the base next to one against `HEAD` / the index. Two mental models on one screen. Rejected.
- **Snapshots as stashes or files under `$XDG_DATA_HOME`.** Stashes are a stack shared with the user and with other tools. Copying files out costs disk and loses git's diff machinery. Rejected for refs under a private namespace.
- **"Since I last opened the Review"** as the incremental mark. Opening is not a decision, and a glance would hide changes. Rejected by the owner for the Round (comment sent or approved).
- **An AI judgement of whether a comment was addressed.** That would be a second Agent reviewing the first, slow and not always right. The fact "the code there changed" plus the owner's Resolve is enough. Rejected.

## Consequences

- `state.json` gains, all additive with no migration:
  - per Worktree: `rounds`, `viewed`, `summary`;
  - per comment: `round`, `state`, `resolved`, `reply_to`.
- API.md: `review.status`, `review.request`; new fields in `review.list` / `review.send`.
- A Review prompt is headed `Review round <n>`; a follow-up quotes the earlier comment.
- Removing a Worktree also deletes its `refs/rade/review/<name>/*`.
- The `rade` skill tells Agents to use `rade review status` and `rade review request`.
