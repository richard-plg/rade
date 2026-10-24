# Spec H — Review Loop (a Worktree in round 2)

No mockup (owner decision, 2026-10-24): this sheet and docs/plans/M44-review-loop.md §3 are the design, and golden H is approved by the owner on screenshots. PRD R118, R119, ADR-0040. Method, tokens and tolerances: [README](README.md).

Scene: Worktree `feat` of `api` (base `main`), a fake `claude` Agent spawned with the prompt `add OTP login`, 3 commits, 6 changed files (one `Cargo.lock`), Round 1 sent with 2 comments (one `code changed`, one `untouched`), 1 draft, 2 of 5 files viewed; Review in its own Tab at 1440 × 900, Inspector hidden.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| H.1 | Header | two lines, `bg1`, bottom 1 px `line`; line 1 height 30 px, line 2 height 30 px, padding 0 8 0 12 px | ±1 px | `geometry review-head → background, border.bottom`; `geometry review-head1 / review-head2 → height` |
| H.2 | Branch | `git-branch` 13 px `fg3`, `feat` Inter 600 12.5 px `fg`, `← main` JetBrains Mono 11 px `fg3` | ±0.5 px; exact text | `debug.review info` → `head.branch`, `head.base` |
| H.3 | Agent | the agent's glyph 14 px in its status colour (R115), name Inter 12.5 px `fg`; `+n` JetBrains Mono 11 px `fg3` | exact | `debug.review info` → `head.agent`, `head.glyph` |
| H.4 | Task | Inter 12.5 px `fg2`, in quotes, one line ellipsized at the end, takes the remaining width | exact text | `debug.review info` → `head.task` |
| H.5 | Commits | `3 commits ▾` JetBrains Mono 11 px `fg3`, a flat button; the popover lists `sha subject age` | exact text | `debug.review info` → `head.commits` |
| H.6 | Round chip | `Round 2 · <state>` (`changes requested` `wait`, `approved` `ok`, `changed after approval` `wt`), JetBrains Mono 11 px, 1 px border in the state's `-line` token (`wt` at 45 %), radius 4 px, padding 1 6 px; under line 1 the Agent's summary, Inter 12.5 px `fg2`, at most 3 lines | exact | `debug.review info` → `head.round`, `head.summary` |
| H.7 | Scope control | two linked toggles `All changes 6` / `Since round 1 2`, height 24 px, radius 6 px, 1 px `line2`; on: `acc-dim` fill, `acc` text; off: `fg2` | ±1 px; exact | `geometry review-scope → height, radius`; `debug.review info` → `scope` |
| H.8 | Viewed meter | `2/5 viewed` JetBrains Mono 11 px `fg2`, then a 64 × 4 px bar, radius 2 px, track `bg4`, fill `acc` | ±1 px | `geometry review-progress → width, height`; `debug.review info` → `viewed` |
| H.9 | Header buttons | whitespace (`text-aa` / ws) toggle, Impact toggle (on: `acc-dim` + `acc`), refresh, close: 24 px, icons 14 px `fg3` | ±1 px | `geometry review-ws / review-impact-toggle → width` |
| H.10 | Files rail | 240 px wide (Paned, user can drag), `bg1`, right 1 px `line`; filter field 26 px high, `bg0`, 1 px `line2`, radius 6 px, margin 8 px | ±1 px | `geometry review-files → width, background`; `geometry review-filter → height` |
| H.11 | Folder row | `caret` 10 px + folder name Inter 12 px `fg3`, indent 12 px per level, height 24 px | ±1 px | `geometry review-folder → height` |
| H.12 | File row | height 26 px; status letter JetBrains Mono 11 px (M `wait`, A `add`, D `del`, R `run`, U `fg3`); name Inter 12.5 px `fg`; viewed → name `fg3` and a `check` 12 px `ok`, unread → a 5 px `acc` dot; comment count JetBrains Mono 10.5 px on `bg3` radius 8 px; `+n −n` 11 px `add` / `del` | ±1 px; exact | `debug.review info` → `files[]` (`status`, `viewed`, `comments`); `geometry review-file nth=0 → height` |
| H.13 | Generated group | last, label `Lock & generated  1`, folded, its files' names `fg3` | exact text | `debug.review info` → `generated` |
| H.14 | File header (diff) | height 30 px, `bg2`, top 1 px `line2`; Viewed checkbox right with label `Viewed` Inter 12 px `fg2` | ±1 px | `geometry review-file-head → height, background` |
| H.15 | Comment card | under its last line, left inset at the code column (after the numbers), right 12 px; `bg2`, 1 px `line2`, radius 6 px, padding 8 10 px; head `You · round 1` Inter 600 11.5 px `fg2` + state chip; text Inter 13 px `fg` | ±1 px | `geometry review-card → background, border, radius, padding` |
| H.16 | Card states | draft: 1 px `acc-line` border, chip `draft` `acc`; sent: chip `sent` `fg3`; `code changed` `run` (`run-line` border); `untouched` `wait` (`wait-line`); resolved: one line (no text), the card at 0.55 opacity, chip `resolved` `ok`, Reopen | exact | `debug.review info` → `card_states[]` (`state`, `resolved`); spec_h.py checks the draft |
| H.17 | Composer | a card with a 3-line entry (`bg0`, 1 px `acc-line`, radius 4 px, JetBrains Mono 12.5 px), hint `Enter add · Shift+Enter newline · Esc cancel` 11 px `fg3` | ±1 px | `geometry review-composer → background, border` |
| H.18 | Impact rail | right of the diff, 320 px, `bg1`, left 1 px `line`; overlay below 1100 px Pane width | ±1 px | `geometry review-impact → x, width`; `debug.review info` → `impact_mode` |
| H.19 | Footer | 40 px, `bg1`, top 1 px `line`; hints JetBrains Mono 11 px `fg3`; `2 drafts` JetBrains Mono 11 px `fg2`; `Discard…` flat button `fg2`; one primary button (`.suggested-action`) | ±1 px; exact | `geometry review-footer → height, background`; `count review-footer .suggested-action = 1` |
| H.20 | Approve menu | a `suggested-action` MenuButton `Approve ▾` (only without drafts; with drafts `Request changes (n)`), popover above it with R119.4's items in order, Inter 13 px, rows 28 px; items that do not apply are left out, not greyed | exact text | `debug.review info` → `primary`, `approve_items` |
| H.21 | Contrast | `fg`, `fg2`, `fg3`, state tokens on `bg1` / `bg2` ≥ 4.5 : 1 (`fg3` ≥ 3 : 1 for hints) | ≥ threshold | `contrast.py` (both themes) |
| H.22 | 1280 px / 200 % | no horizontal scroll; at 200 % the files rail hides and the task line drops before anything clips | — | `debug.ui hscroll`, `clipped` |
