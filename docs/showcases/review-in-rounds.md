# Review in rounds

**The idea:** review an agent's work the way you'd review a colleague's PR — but in seconds, and
without the PR. Comment on lines, send them back, look only at what changed, approve.

![Review round trip: two comments, Request changes, the Agent answers, Since round 1, Approve](../images/review.gif)
<sub>[Watch as MP4](../images/review.mp4)</sub>

## 1. The Agent finishes

An Agent on Worktree `coupons` ends its turn. A Toast says `coupons finished in coupons` with a
**Review** button (also on its Inbox row, and `Alt+R` in Agent Overview).

## 2. Read and comment

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/review-light.png">
  <img alt="The Review Pane: whose work, the files rail, the diff with a comment card" src="../images/review-dark.png">
</picture>

The header tells you whose work this is and what it was asked to do. Go file by file (`N`),
mark each **Viewed** (`v`), select a line and press `c` to comment:

> Read the coupon table from config, not a hard-coded check.

## 3. Request changes

**Request changes (2)** sends both comments to the Agent as one prompt, `Review round 1`, with the
file, line and quoted code for each — and remembers the files as they are.

## 4. Only what changed

When the Agent's turn ends, the Review opens on **Since round 1**: just the files it touched
since your comments, each unread again. Every comment says `code changed` or `untouched`.
**Resolve** the ones that are done, **Follow up** on the ones that aren't.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/review-round2-light.png">
  <img alt="Round 2: Since round 1, comment states and Resolve / Follow up" src="../images/review-round2-dark.png">
</picture>

## 5. Approve

**Approve ▾ → Ask Agent to commit** (or *commit and open a PR*), or do it yourself with *Commit
myself…* / *Create PR…*. The Navigator's Worktree row now says `approved`.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/review-approve-light.png">
  <img alt="After Approve: the header shows Round 2 · approved" src="../images/review-approve-dark.png">
</picture>

## Why it works

- The diff is the Worktree **against its base**, so you see the Agent's work and nothing else.
- Rounds keep the conversation: what you asked, whether it was done, what changed after.
- Agents can join in: `rade review status` shows them open comments; `rade review request
  --summary "…"` tells you they're ready.

## What you used

[Review](../features/review.md) · [Changes and PRs](../features/changes-and-prs.md) ·
[Code navigation](../features/code-navigation.md)
