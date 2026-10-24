# Spec C — New Project dialog (one base per link, checkout plan)

Mockup: `mockup.html#c-dark` / `#c-light` (dialog over screen A), images `new-c-dark.png`, `new-c-light.png`. Design §2 rules 1–2, §3.3 P0; R106 (New Project / Add Source / Change base / Project settings), R98 (checkout plan). Screen A underneath is unchanged ([a-workspace.md](a-workspace.md)). Method, tokens and tolerances: [README](README.md).

The same dialog body (link table) is used by **Add Source**, **Change base** and **Project settings**; only the title, the primary label and which rows are editable differ (see States).

**Checkout plan text versus the mockup** (decisions log): the mockup's `shop-api` row reads `main checkout · on feat/checkout-v2`, but ADR-0034 decision 3 gives a link with `checkout = "auto"` a Project checkout when the main checkout is not on its base. The spec follows the ADR: that row reads `new project checkout · main checkout is on feat/checkout-v2`. A row whose Source + base already has a checkout says `shared with <Project>`.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| C.1 | Scrim over the window | full window, `scrim` | exact | `geometry dialog-scrim → width, height, background` |
| C.2 | Dialog width / position | 680 px; centred horizontally (x = 380 px at 1440 px); top at 92 px | ±1 px | `geometry new-project-dialog → width, x, y` |
| C.3 | Dialog box | 1 px `line2`, radius 10 px, `bg2`, `shadow` | exact; shadow by screenshot | `geometry new-project-dialog → border, radius, background`; `screenshot` |
| C.4 | Header padding, gap, bottom border | 14 18 px, 10 px, 1 px `line` | ±1 px; exact | `geometry dialog-header → padding, border.bottom` |
| C.5 | Header icon / title / context | `cube` 20 px `acc`; `New Project` Inter 15 px 600 `fg`; `in acme` JetBrains Mono 11.5 px `fg3` | ±0.5 px; exact text | `geometry dialog-title → font, color, label`; `geometry dialog-context → font, color, label` |
| C.6 | Close button | `x` 16 px `fg3`, right-aligned in the header | ±1 px; exact | `geometry dialog-close → color, x + width = dialog right − 18` |
| C.7 | Body padding | 16 18 px | ±1 px | `geometry dialog-body → padding` |
| C.8 | Field label | Inter 12 px `fg2`, 6 px above its control; fields 14 px apart | ±1 px; exact | `geometry field-label → font, color`; y deltas |
| C.9 | Name input | height 30 px, radius 6 px, 1 px `line2`, `bg0`, padding 0 10 px, Inter 13 px `fg` | ±1 px; exact | `geometry project-name-entry → height, radius, border, background, padding, font` |
| C.10 | Name input focused on open | keyboard focus in the Name input; `glow`, border transparent | exact | `debug.window` → focus widget `project-name-entry`; glow by `screenshot` |
| C.11 | Links field label row | `Sources and base branch` left; hint `Each source gets one base. A source can be in many Projects.` right in `fg3` | exact text | `geometry field-label nth=1 → label`; `geometry field-hint → color` |
| C.12 | Link table box | 1 px `line`, radius 6 px, rows separated by 1 px `line` (none after the last) | ±1 px; exact | `geometry link-editor → border, radius`; `geometry link-editor-row → border.bottom` |
| C.13 | Link row grid | columns 22 px · 150 px · flexible · 190 px · 24 px, gap 8 px, padding 8 10 px, Inter 12.5 px | ±1 px | `geometry link-editor-row → padding`; `geometry link-editor-cell nth=k → width` |
| C.14 | Link row height (one-line plan) | 43 px (8 + 26 + 8 + 1 border) | ±1 px | `geometry link-editor-row nth=0 → height` |
| C.15 | Link row height (two-line plan, `shared-ui`) | 50 px; the plan wraps, never ellipsizes | ±2 px | `geometry link-editor-row nth=2 → height`; plan label fully visible |
| C.16 | Include check box | `check-square` Fill 17 px `acc` (checked); unchecked `square` 17 px `fg3` | ±1 px; exact | `geometry link-check → width, color` |
| C.17 | Source name | JetBrains Mono 12.5 px `fg` | ±0.5 px; exact | `geometry link-source → font, color` |
| C.18 | Checkout plan text | Inter 11.5 px `fg3`; kind (`main checkout` / `new project checkout`) `fg2` 500; a drifted HEAD (`on feat/checkout-v2`) in `wait` | ±0.5 px; exact | `geometry link-plan → font, color`; `geometry link-plan-kind → color`; drift span color |
| C.19 | Plan texts for the scene | `shop-web`: `main checkout · on develop · shared with Shop`; `shop-api`: `new project checkout · main checkout is on feat/checkout-v2`; `shared-ui`: `new project checkout · Shop uses develop, Billing release/2.4` | exact text | `debug.ui dialog` → link rows `plan` (equals `project.plan` for the same input) |
| C.20 | Base select | 190 × 26 px, radius 5 px, 1 px `line2`, `bg0`, padding 0 8 px, JetBrains Mono 12 px `fg`, `caret-down` 12 px `fg3` right | ±1 px; exact | `geometry link-base → width, height, radius, border, background, font` |
| C.21 | Remove-row button | `x` 16 px `fg3` in the 24 px column | ±1 px; exact | `geometry link-remove → color` |
| C.22 | `Add source…` row | `plus` 15 px, text `fg3`, hint `from acme, a folder, or owner/repo` (`owner/repo` mono), height 33 px | ±1 px; exact text | `geometry link-add-row → height, color, label` |
| C.23 | Footer | padding 12 18 px, top border 1 px `line`, `bg1`, gap 8 px | ±1 px; exact | `geometry dialog-footer → padding, border.top, background` |
| C.24 | Footer note | `info` icon + `RADE never switches a dirty checkout. It makes a Worktree when a base is taken.`, Inter 11.5 px `fg3`, left-aligned, may wrap | exact text | `geometry dialog-note → font, color, label` |
| C.25 | Footer buttons | `Cancel` secondary (A.15 style) then `Create Project` primary, right-aligned | exact order | `debug.ui dialog` → buttons |
| C.26 | Primary button | `acc` / `acc-fg` 600, 26 px, radius 6 px; exactly one `.suggested-action` visible while the dialog is open | exact; count exact | `geometry dialog-primary → background, color, height`; `count .suggested-action = 1` (screen A's `Commit` is insensitive under the scrim and loses the class while a modal is open) |
| C.27 | Contrast | all texts ≥ 4.5 : 1 on `bg2` / `bg0` / `bg1`; check box ≥ 3 : 1 | ≥ threshold | `contrast.py` (both themes) |
| C.28 | Whole scene | matches the approved golden, dark and light | pixel threshold, terminal masked | `screenshot` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Name input | border `acc-line` | — | `glow`, border transparent (C.10) | — |
| Check box | icon `fg` (unchecked) | checked: C.16 | `glow` ring | Source already in this Project (Add Source): checked, opacity 0.45 |
| Base select | border `acc-line` | open: a list of the Source's local and remote branches, current base marked `check` | `glow` ring | opacity 0.45 when the row is unchecked |
| Remove-row `x` | icon `fg` | — | `glow` ring | hidden on the last remaining row in Project settings |
| `Add source…` row | `bg3` fill | — | `glow` ring | — |
| Cancel | `bg3` | — | `glow` ring | — |
| Create Project | adds `glow` | — | `glow` ring | opacity 0.45 while Name is empty or names an existing Project (the error shows under Name, Inter 11.5 px `err`) |

Variants of the same body: **Add Source** — title `Add Source to Shop`, existing links shown read-only, primary `Add`; **Change base** — one row, base select focused, plan updated live, primary `Change base`; **Project settings** — title `Shop settings`, every link editable, primary `Save`. The plan text (C.18) updates within 200 ms of any change and always comes from `project.plan` (nothing is written before the primary button).

## Empty state

- **Workspace with no Sources:** the link table shows only the `Add source…` row; the hint reads `No sources in acme yet: add a folder or owner/repo`. `Create Project` is enabled once Name is set (a Project may start with no links, like `rade project add <name>`).
- **Plan cannot be resolved** (e.g. `checkout = "main"` held at another base): the row's plan shows the refusal in `err` (`main checkout is used by Shop at develop`) and `Create Project` is disabled until it is fixed.

## 1280 px

Window 1280 × 800 px: the dialog keeps 680 px (x = 300 px), top 92 px. The flexible plan column shrinks by nothing (the dialog width is fixed); no change.

## Text scale 200 %

- The dialog widens to min(window − 32 px, 1000 px) and the link row becomes two lines: line 1 = check box, Source, base select, remove; line 2 = the plan text (full width, wraps).
- The body scrolls vertically if needed; header and footer stay visible; no horizontal scroll; no text clipped.

## Content

- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Workspace `acme`; dialog title `New Project`, Name `Checkout revamp` (being typed).
- Rows: `shop-web` base `develop` (main checkout, on `develop`, shared with Shop); `shop-api` base `develop` (main checkout on `feat/checkout-v2` → new project checkout); `shared-ui` base `release/2.5` (new project checkout; Shop uses `develop`, Billing `release/2.4`); `Add source…` (from acme, a folder, or `owner/repo`).
- Underneath: screen A (Projects `Shop` base `develop` and `Billing`, Sources `shop-web`, `shop-api`, `shared-ui`, `billing`, Worktree `checkout-v2`; statusline `claude 5h 62% · wk 41%` of `acme-work`).
