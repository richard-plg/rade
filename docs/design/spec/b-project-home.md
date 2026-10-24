# Spec B — Project home (Shop)

Mockup: `mockup.html#b-dark` / `#b-light`, images `new-b-dark.png`, `new-b-light.png`. Design §3.3 (P0 Project home); R105 (Project home), R104 (Navigator), R100 (drift chip), R103 (chrome). Chrome items not listed here (titlebar, Navigator rows, Statusline) follow [a-workspace.md](a-workspace.md) row for row. Method, tokens and tolerances: [README](README.md).

Window 1440 × 900 px. Columns: Navigator 268 px · page 1172 px (no Inspector on this screen). Page content width 1172 − 2 × 28 = 1116 px.

**0.8.0 scope versus the mockup** (decisions log): the mockup's `New branch across sources…` button, the `Pull requests` KPI and the Worktree meta `PR #482 ✓` belong to "later" (design §8, PRD Non-goals); 0.8.0 shows three KPIs and no such button. The Activity row `#482 opened from perf-img` stays (Activity records PRs opened from RADE).

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| B.1 | Titlebar breadcrumb | `Shop / Home` (M43: was `Shop / Overview`), `Shop` `fg` 500, `Home` `fg2`, Inter 12.5 px | ±0.5 px; exact text | `geometry crumbs → label, font`; `geometry crumb-current → color` |
| B.2 | Titlebar right buttons | `plus-square`, Inspector toggle (not on), window controls; no Review button | exact | `count titlebar review-button = 0`; `class tb-button nth=1 has-not on` |
| B.3 | Navigator: Shop row selected, its links collapsed | Shop row A.28 style; links `shop-web`, `shop-api`, `shared-ui` at depth 1 with chevron `caret-right` | exact | `debug.ui navigator` → selected `Shop`, rows collapsed |
| B.4 | Page area width / background | 1172 px / `bg0` | ±1 px; exact | `geometry project-home → width, background` |
| B.5 | Page padding | 22 px top/bottom, 28 px left/right | ±1 px | `geometry project-home → padding` |
| B.6 | Header icon | `cube` 26 px `proj-1` | ±1 px; exact | `geometry home-icon → width, color` |
| B.7 | Header title `Shop` | Inter 20 px 600, letter-spacing −0.01em, `fg` | ±0.5 px; exact | `geometry home-title → font, color` |
| B.8 | Header sub-line | `acme · 3 sources · .rade/projects/shop/project.toml`, JetBrains Mono 12 px `fg3` | ±0.5 px; exact text | `geometry home-sub → font, color, label` |
| B.9 | Header icon–text gap / actions gap | 12 px / 8 px | ±1 px | `geometry home-header → x` deltas |
| B.10 | Header actions (right-aligned) | `Settings` (`gear-six`, secondary), `Open terminal` (`terminal-window`, primary) | exact text, order | `debug.ui home` → buttons |
| B.11 | Primary button `Open terminal` | `acc` fill, `acc-fg` 600, height 26 px, radius 6 px; exactly one `.suggested-action` on screen B | exact; count exact | `geometry home-open-terminal → background, color, height, radius`; `count .suggested-action = 1` |
| B.12 | Secondary button | height 26 px, padding 0 10 px, radius 6 px, 1 px `line2`, `bg2`, Inter 12.5 px `fg`, icon 14 px `fg2` | ±1 px; exact | `geometry home-settings → height, padding, radius, border, background` |
| B.13 | KPI row margin and gap | 18 px above, 22 px below; gap 10 px | ±1 px | `geometry home-kpis → margin`; x deltas of `kpi` |
| B.14 | KPI cards: count and width | 3 cards (`Agents`, `Worktrees`, `Uncommitted`), each (1116 − 20) / 3 = 365 px | ±1 px; exact count | `count kpi = 3`; `geometry kpi → width` |
| B.15 | KPI card box | 1 px `line`, radius 8 px, padding 10 12 px, `bg1` | ±1 px; exact | `geometry kpi → border, radius, padding, background` |
| B.16 | KPI label / value / detail | label A.18 style; value JetBrains Mono 18 px 600 `fg`, 2 px below label; detail 12 px 400 `fg3` | ±0.5 px; exact | `geometry kpi-label → font`; `geometry kpi-value → font, color`; `geometry kpi-detail → font, color` |
| B.17 | KPI texts | `3` `1 waiting · 1 running`; `2` `checkout-v2, perf-img`; `7` `files in 2 sources` | exact text | `debug.ui home` → kpis |
| B.18 | Section header `SOURCES` | label A.18 style, margin 6 px above / 8 px below; `Add source` secondary button (`plus`) right-aligned | ±1 px; exact | `geometry home-section → margin`; `debug.ui home` → section buttons |
| B.19 | Link table width | 1116 px (full content width) | ±1 px | `geometry link-table → width` |
| B.20 | Link table head | columns `SOURCE`, `BASE`, `CHECKOUT`, `HEAD`, `VS BASE`, `STATE`, (actions); JetBrains Mono 600 10.5 px, uppercase, letter-spacing 0.1em, `fg3`, padding 6 10 px, bottom border 1 px `line` | ±1 px; exact text | `geometry link-table-head → font, padding, border.bottom`; `debug.ui home` → columns |
| B.21 | Link table row height | 39 px (padding 9 10 px + content + 1 px border) | ±1 px | `geometry link-row → height` (each) |
| B.22 | Link table cell font | Inter 12.5 px `fg2`; Source name `fg` 500; Base, Checkout, HEAD, vs base JetBrains Mono 12 px | ±0.5 px; exact | `geometry link-cell nth=k → font, color` |
| B.23 | Source cell icon | `git-fork` 16 px `fg3` | ±1 px; exact | `geometry link-row-icon → width, color` |
| B.24 | Drifted HEAD cell (`shop-api`) | `feat/checkout-v2` in `wait` | exact | `geometry link-cell-head nth=1 → color` |
| B.25 | Checkout cell of a Project checkout | `project checkout` + path `.worktrees-dev/.projects/shop/shared-ui` in `fg3` (the Devel build that E2E runs; `.worktrees/…` in the Installed build and the mockup, PRD R98.1) | exact | `geometry link-cell-checkout nth=2 → label`; path span color |
| B.26 | Chip box | JetBrains Mono 11 px, padding 1 6 px, radius 4 px, 1 px border, gap 4 px, icon 12 px | ±1 px; exact | `geometry chip → font, padding, radius, border` |
| B.27 | Chip `clean` | `check` icon, text and icon `ok`, border `ok-line`, no fill | exact | `class chip nth=0 has chip-ok`; `geometry chip nth=0 → color, border` |
| B.28 | Chip `off base · 3 changes` | `warning-diamond` Fill, text `wait`, border `wait-line`, fill `wait-dim` | exact | `class chip nth=1 has chip-warn`; `geometry chip nth=1 → color, border, background` |
| B.29 | Chip `also in · Billing @ release/2.4` (from `also_in`, PRD R98.1) | `link-simple`, text `run`, border `run-line`, no fill | exact | `class chip nth=2 has chip-info`; `geometry chip nth=2 → color, border` |
| B.30 | Row action icons | `terminal-window`, `dots-three`; 15 px `fg3`, padding 0 4 px | ±1 px; exact | `geometry link-row-action → width, color` |
| B.31 | Lower grid | two columns 1.25 fr : 1 fr, gap 22 px, 22 px below the table: 608 px and 486 px | ±1 px | `geometry home-worktrees → width`; `geometry home-activity → width, x` |
| B.32 | Card box (Worktrees, Recent activity) | 1 px `line`, radius 8 px, `bg1`, rows without radius | ±1 px; exact | `geometry home-card → border, radius, background` |
| B.33 | Card row height and left padding | 30 px, 12 px | ±1 px | `geometry home-card-row → height, padding.left` |
| B.34 | Worktree row | `git-branch` 15 px `wt`, name `fg`, `shop-web ← develop` JetBrains Mono 11 px `fg3`, meta right (`↑3 · 4 changed · ` LED waiting `2 agents`) | exact | `debug.ui home` → worktrees; `geometry home-card-row nth=0 → label` |
| B.35 | Activity row | LED or icon, actor `fg`, text `fg3`, time right JetBrains Mono 11 px `fg3` (`now`, `1m`, `2h`) | exact | `debug.ui home` → activity rows |
| B.36 | Section labels `WORKTREES`, `RECENT ACTIVITY` | label A.18 style, margin 6 px above / 8 px below | ±1 px | `geometry home-section nth=1,2 → margin` |
| B.37 | Statusline | `ACME`, `Shop`, `● 1 waiting ● 1 running`, `richard@acme.io`, hints `Ctrl ⇧ P commands`, `Alt 1 Shop` | exact text | `debug.ui chrome` → statusline cells |
| B.38 | Contrast of every text token on this screen | ≥ 4.5 : 1 (incl. chip texts on their fills over `bg0`) | ≥ threshold | `contrast.py` (both themes) |
| B.39 | Whole scene | matches the approved golden, dark and light | pixel threshold, times masked | `screenshot` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Header buttons (Settings, Open terminal) | secondary: `bg3`; primary: adds `glow` | — | `glow` ring | opacity 0.45 (Open terminal while the Project has no link) |
| Link table row | all cells `bg1` | opened in the Navigator: none (the table has no selection) | `glow` ring around the row | — |
| Row action icons | icon `fg` | menu open: `bg3` | `glow` ring | opacity 0.45 |
| Chips | none (not interactive); `also in` chip tooltip lists the other Projects | — | — | — |
| Card rows (Worktree, Activity) | `bg3` fill; click opens the Worktree / the Agent | — | `glow` ring | — |
| `Add source` | `bg3` | — | `glow` ring | — |
| Navigator Shop row | as A | A.28 | as A | — |

Drift actions for `shop-api` are in the row's `dots-three` menu: `Keep`, `Move to a Worktree…`, and `Back to base` only when the checkout is clean (R100).

## Empty state

- **Project with no links:** KPI cards show `0`; the table is replaced by one line `No sources in Shop.` (Inter 12.5 px `fg3`) and the `Add source` button stays; `Open terminal` is disabled.
- **No Worktrees:** the Worktrees card shows one 30 px row `No Worktrees · New Worktree…` in `fg3` (the link is a button).
- **No activity:** one row `No activity since RADE started` `fg3` (Activity is in memory, design §3.3).
- **No changes:** the `Uncommitted` KPI shows `0` and detail `clean`.

## 1280 px

Window 1280 × 800 px: page width 1012 px, content 956 px.
- KPI cards shrink equally (each (956 − 20) / 3 = 312 px).
- Link table keeps all columns; the Checkout column ellipsizes the path (`.worktrees-dev/…/shared-ui`) first, then the State chip text; Source, Base, HEAD never ellipsize.
- The lower grid stays two columns (≈ 520 / 414 px).
- No horizontal scroll.

## Text scale 200 %

- Navigator becomes the 48 px icon rail (A, Text scale); page width 1392 px.
- KPI cards wrap to one per line (full width) when a card is narrower than its value + detail at 2×.
- Link table: rows grow to text height + 18 px; the `VS BASE` and `CHECKOUT` columns move into a second line of each row (mono, `fg3`), so the table never scrolls sideways.
- Lower grid becomes one column (Worktrees, then Recent activity).
- No text clipped (`geometry` natural height ≤ allocation for every label).

## Content

- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Workspace `acme`; Project `Shop`, file `.rade/projects/shop/project.toml`, 3 Sources.
- Links (all base `develop`): `shop-web` main checkout, HEAD `develop`, `↓2`, clean; `shop-api` main checkout, HEAD `feat/checkout-v2`, `↑4 ↓0`, off base · 3 changes; `shared-ui` project checkout `.worktrees-dev/.projects/shop/shared-ui` (Devel build), HEAD `develop`, `=`, also in · Billing @ `release/2.4`.
- KPIs: Agents 3 (1 waiting · 1 running); Worktrees 2 (`checkout-v2`, `perf-img`); Uncommitted 7 files in 2 sources.
- Worktrees: `checkout-v2` `shop-web ← develop` `↑3 · 4 changed · 2 agents` (waiting LED); `perf-img` `shop-web ← develop` `clean`.
- Recent activity: `tests asked which fixtures` (waiting, now); `planner finished · 24 tests passed` (done, 1m); `#482 opened from perf-img` (`git-pull-request` `ok`, 2h).
- Navigator: Shop (badge 1, selected), Billing (running LED), Docs (dim); Scratch `~/workspaces/acme` 2.
- Statusline: `ACME · Shop · 1 waiting · 1 running · richard@acme.io`, hints `Ctrl ⇧ P commands`, `Alt 1 Shop`.
- Other mockup data referenced: Billing's `shared-ui` at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team`, `acme` (codex) are not shown here.
