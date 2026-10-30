# Spec E — Agent usage page

Mockup: `mockup.html#e-dark` / `#e-light`, images `new-e-dark.png`, `new-e-light.png`. Design §10.3; R110 (Usage UI), R109 (data), R108 (accounts). Chrome items not listed here (titlebar, Navigator, Statusline) follow [a-workspace.md](a-workspace.md). Method, tokens and tolerances: [README](README.md).

Window 1440 × 900 px. Columns: Navigator 268 px · page 1172 px (no Inspector). Page padding 18 / 24 px → content width 1124 px: account list 668 px · gap 16 px · detail 440 px.

Opened from the Statusline usage cell, the titlebar `gauge` button, or palette `> usage`. The golden scene uses the filter `All workspaces` (as the mockup) with Detailed limits enabled for `acme-work` (design §10.5 A, opt-in).

**Order versus the mockup** (decisions log): design §10.3 sorts the account table by Tightest limit, highest first, so an account near its limit is always on top; the mockup lists rows unsorted. The spec follows the design (E.18); accounts with no data go last.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| E.1 | Titlebar | breadcrumb `Agent usage` (`fg` 500); `gauge` button on (`bg3`, icon `acc`); no Review button | exact | `geometry crumbs → label`; `class tb-button-usage has on` |
| E.2 | Page background / padding | `bg0`; 18 px top/bottom, 24 px left/right | ±1 px; exact | `geometry usage-page → background, padding` |
| E.3 | Header icon / title / sub-line | `gauge` 24 px `acc`; `Agent usage` Inter 20 px 600; `7 accounts · limits from each CLI · tokens from local session logs` JetBrains Mono 12 px `fg3` | ±0.5 px; exact text | `geometry usage-title → font`; `geometry usage-sub → font, color, label` |
| E.4 | Period control | three segments `Today`, `7 days`, `30 days`; height 26 px, 1 px `line2`, radius 6 px, JetBrains Mono 11.5 px `fg2`, segment padding 0 10 px, dividers 1 px `line` | ±1 px; exact text | `geometry usage-period → height, border, radius, font`; `geometry usage-period-seg → padding` |
| E.5 | Active period (`7 days`) | `acc-dim` fill, `acc` text | exact | `class usage-period-seg nth=1 has active`; colors |
| E.6 | Workspace filter / Add account | secondary buttons (A.15 style): `circles-four` `All workspaces` `caret-down` 11 px; `plus` `Add account` | exact text | `debug.ui usage` → header buttons |
| E.6b | Close (M51) | `x` flat icon button at the header's right end, tooltip `Close (Esc)`; `Esc` closes the page too | exact | `debug.ui usage` → `header.close`; `geometry usage-close` |
| E.7 | No primary button | `count .suggested-action = 0` on screen E | exact | `count .suggested-action = 0` |
| E.8 | List–detail grid | list 668 px, gap 16 px, detail 440 px, 16 px below the header, tops aligned | ±1 px | `geometry usage-accounts → width, y`; `geometry usage-detail → width, x, y` |
| E.9 | Account table card | 1 px `line`, radius 8 px, `bg1` | exact | `geometry usage-accounts-card → border, radius, background` |
| E.10 | Table head | `ACCOUNT`, `PLAN`, `TIGHTEST LIMIT`, `TOKENS 7D` (right-aligned), `UPDATED`; style B.20 | exact text | `debug.ui usage` → columns; `geometry usage-table-head → font` |
| E.11 | Table row height / cell padding | 39 px / 8 10 px; bottom border 1 px `line` | ±1 px | `geometry usage-row → height`; `geometry usage-cell → padding` |
| E.12 | Agent badge (small) | 18 × 18 px, radius 4 px, JetBrains Mono 600 10 px; Claude `C` `agent-claude` on `agent-claude-bg`; Codex `X` `agent-codex` on `bg4` | ±1 px; exact | `geometry agent-badge → width, height, radius, font, color, background` |
| E.13 | Account name / Workspace chip | name Inter 12.5 px `fg` 500; chip JetBrains Mono 10.5 px, padding 0 5 px, radius 4 px, 1 px `line2`, `fg2`, margin-left 4 px; default account chip ends in `✓` | ±0.5 px; exact | `geometry usage-account-name → font, color`; `geometry usage-ws-chip → font, padding, label` |
| E.14 | Tightest limit cell | meter 56 × 5 px, radius 3 px, track `bg4`, fill `acc`; value JetBrains Mono 600 12 px `fg`, min width 34 px; limit name Inter 12 px `fg2`; gap 8 px | ±1 px; exact | `geometry usage-meter → width, height, background`; `geometry usage-tightest-value → font, color` |
| E.15 | Hot limit (≥ 80 %) | value and meter fill `wait`, value preceded by `warning-diamond` Fill 11 px `wait` (color always with icon + number) | exact; threshold exact at 80 % | `class usage-tightest nth=k has hot`; `geometry usage-tightest-value → color` for `acme` (81 %) and `personal` (88 %) |
| E.16 | Meter fill width | proportional: 62 % → 35 px of 56 px | ±1 px | `geometry usage-meter-fill nth=acme-work → width` |
| E.17 | Tokens / Updated cells | JetBrains Mono 12 px `fg2`; tokens right-aligned (`12.4M`); updated relative (`40s`) | ±0.5 px; exact | `geometry usage-tokens → font, x + width` |
| E.18 | Row order | by Tightest limit descending: `personal` (claude) 88 % · `acme` (codex) 81 % · `acme-work` 62 % · `acme-biz` 27 % · `acme-team` 12 % · `personal` (codex) 4 % · `oss` no data | exact order | `debug.ui usage` → rows |
| E.19 | Selected row (`acme-work`) | every cell `acc-dim`; 2 px `acc` inset bar on the first cell | exact | `class usage-row nth=2 has selected`; `geometry usage-row nth=2 → background` |
| E.20 | Row with no data (`oss`) | opacity 0.6; limit cell `no data yet` `fg3`; tokens `0`; updated `—` | exact | `geometry usage-row nth=6 → opacity, label` |
| E.21 | `NEXT RESETS` section | label A.18 style, 16 px above, 8 px below; right note `quota coming back` JetBrains Mono 11 px `fg3` | ±1 px; exact text | `geometry usage-resets-header → margin`; label text |
| E.22 | Reset row | height 32 px, padding 0 12 px, columns 86 · 18 · 90 · flexible · auto px, gap 10 px, Inter 12 px `fg2`, bottom border 1 px `line` (none on the last) | ±1 px; exact | `geometry usage-reset-row → height, padding, font` |
| E.23 | Reset row parts | time JetBrains Mono 12 px (`acc` for the soonest, else `fg2`); account `fg` 500; forecast JetBrains Mono 11 px `fg3` right | exact | `geometry usage-reset-time nth=0 → color`; `geometry usage-reset-forecast → font, color` |
| E.24 | Reset rows for the scene | `in 1h 12m` acme-work `5-hour · now 62%` `forecast: hits 100% in ~40m`; `in 2h 30m` acme `Spark 5-hour · now 81%` `GPT-5.3-Codex-Spark only`; `Sat 21:00` personal `weekly · now 88%` `forecast: 97% at reset`; `Mon 09:00` acme-work `weekly ×4 · max 58% Opus` | exact text, order by time | `debug.ui usage` → resets |
| E.25 | Detail card | padding 14 16 10 px, 1 px `line`, radius 8 px, `bg1` | ±1 px; exact | `geometry usage-detail → padding, border, radius, background` |
| E.26 | Detail header | badge 24 × 24 px radius 6 px JetBrains Mono 600 12 px; name Inter 13 px 600; sub `Claude Code · Max 20x · ~/.claude-work` JetBrains Mono 11 px `fg3`; `dots-three` secondary button right | ±1 px; exact text | `geometry usage-detail-badge → width, radius`; `geometry usage-detail-sub → font, color, label` |
| E.27 | Limit group labels | `SESSION`, `WEEKLY`, `EXTRA USAGE`, `TOKENS PER DAY`; label A.18 style, margin 12 px above, 4 px below | ±1 px; exact text | `geometry usage-limit-group → margin, label` |
| E.28 | Limit row | height 24 px, columns 88 · flexible · 40 · 92 px, gap 10 px; name Inter 12 px `fg2`; value JetBrains Mono 600 12 px `fg` right; reset JetBrains Mono 11 px `fg3` | ±1 px; exact | `geometry usage-limit-row → height, font`; `geometry usage-limit-value → font, color` |
| E.29 | Limit bar | 6 px high, radius 3 px, track `bg4`, fill `acc`; ≥ 80 % fill `wait` | ±1 px; exact | `geometry usage-limit-bar → height, radius, background`; fill color |
| E.30 | Limit rows for `acme-work` | Session `5-hour 62% 1h 12m`; Weekly `All models 41%`, `Opus 58%`, `Sonnet 22%`, `OAuth apps 3%` (`Mon 09:00`); Extra usage `Spend limit $12 of $50 /mo` | exact text | `debug.ui usage` → detail limits |
| E.31 | Spend limit shown in $ | value `$12`, reset column `of $50 /mo`; no other $ amount anywhere on the page | exact | `debug.ui usage` → detail; no `$` in other rows |
| E.32 | Tokens per day chart | 7 bars, height 110 px (top padding 18 px), gap 8 px, bar radius 4 4 0 0 px; past days `acc-bar`, today `acc`; day letters JetBrains Mono 10.5 px `fg3`; max and today values JetBrains Mono 10.5 px 500 `fg2` (`3.1M`, `1.5M`); right note `cache reads excluded` | ±1 px; exact | `geometry usage-chart → height, padding.top`; `geometry usage-chart-bar nth=6 → background`; `debug.ui usage` → chart values |
| E.33 | Detail footer | top border 1 px `line`, 10 px above, padding-top 8 px, gap 12 px, JetBrains Mono 11 px `fg3`: `Shop 9.1M`, `Billing 2.3M`, `Scratch 1.0M`, right `opus 78% · sonnet 22%` | ±1 px; exact text | `geometry usage-detail-footer → border.top, padding.top, font, label` |
| E.34 | Statusline | `ACME`, usage cell `claude 5h 62% · wk 41%` (meter A.74), counts, `richard@acme.io`, hint `Ctrl ⇧ P commands` | exact text | `debug.ui chrome` → statusline cells |
| E.35 | Contrast | all texts ≥ 4.5 : 1 on `bg0`, `bg1`, `acc-dim` over `bg1`; badge letters on their fills; meters ≥ 3 : 1 against `bg4` | ≥ threshold | `contrast.py` (both themes) |
| E.36 | Whole scene | matches the approved golden, dark and light | pixel threshold, "ago" texts and reset times masked | `screenshot` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Account row | all cells `bg3` | E.19; the detail card shows that account | `glow` ring; `↑↓` move the selection | locked account of another Workspace (when filter = All): row shows `lock-simple` before the chip; still selectable (read-only view) |
| Period segment | text `fg` | E.5 | `glow` ring; `←→` change it | — |
| Workspace filter / Add account / `dots-three` | `bg3` | menu open: `bg3` | `glow` ring | — |
| Reset row | `bg3`; click selects that account | — | `glow` ring | — |
| Chart bar | bar `acc`; tooltip `<day> <tokens>` | today: `acc` | — | — |
| Limit row | none | — | — | Detailed limits off: Opus / Sonnet / OAuth apps rows show `—` and the note `Detailed limits off · Enable…` (design §10.5 A) |

## Empty state

- **No accounts in the filter:** the table card holds one line `No Agent Accounts in acme.` (Inter 12.5 px `fg3`) and `Add account` stays the way forward; Next resets and the detail card are hidden.
- **Account with no data:** E.20; its detail shows `no data yet` and how data arrives (`claude: run a session with the RADE statusline` / `codex: run a session`).
- **Format changed:** limit values show `no usage data (format changed?)` in `fg3`, never a stale or guessed number (design §10.2).
- **Unsupported Agent (OpenCode, Antigravity):** `no usage data` in the limit cell.

## 1280 px

Window 1280 × 800 px: page content 964 px. The detail card moves **below** the list (full width) when the list would be narrower than 560 px: 964 − 16 − 440 = 508 < 560, so at 1280 px the layout is list (964 px), then Next resets, then detail. The table keeps all columns; the Plan column ellipsizes first.

## Text scale 200 %

- Navigator becomes the 48 px icon rail (A); one column: table, Next resets, detail.
- Table rows grow to text height + 16 px; the Plan and Updated cells move to a second line of each row (`fg3`) so the table never scrolls sideways.
- Chart keeps 7 bars; its height becomes 160 px; day letters stay visible; no text clipped.

## Content

- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Workspace `acme` (shown), filter `All workspaces`, period `7 days`.
- Accounts: `acme-work` (claude, acme ✓, Max 20x, 62 % 5-hour, 12.4M, 40s); `acme-team` (claude, acme, Team, 12 % weekly, 1.1M, 2h); `acme` (codex, acme ✓, Pro Lite, 81 % Spark 5-hour, 0.9M, 5m); `acme-biz` (codex, acme, Business, 27 % weekly, 2.6M, 1h); `personal` (claude, personal ✓, Pro, 88 % weekly, 6.8M, 1d); `personal` (codex, personal ✓, Plus, 4 % weekly, 0.2M, 3d); `oss` (codex, personal, —, no data yet, 0, —). The two `personal` accounts are allowed because names are unique per Workspace and agent (PRD R108.1); rows show the agent, and `rade account usage personal` needs `--agent` (else `AMBIGUOUS`).
- Selected `acme-work`: `~/.claude-work`, `five_hour` 62 % (resets 1h 12m), weekly all models 41 %, Opus 58 %, Sonnet 22 %, OAuth apps 3 % (Mon 09:00), spend limit $12 of $50 /mo; tokens per day F S S M T W T, max 3.1M, today 1.5M; by Project Shop 9.1M, Billing 2.3M, Scratch 1.0M; by model opus 78 %, sonnet 22 %.
- Statusline usage cell: `claude 5h 62% · wk 41%` (focused Agent's account `acme-work`).
- Navigator: Projects `Shop` (badge 1) and `Billing` (running) collapsed, `Docs`; Scratch `~/workspaces/acme`.
