# Spec A — Workspace (window chrome, Navigator, Panes, Inspector, Statusline)

Mockup: `mockup.html#a-dark` / `#a-light`, images `new-a-dark.png`, `new-a-light.png`. Design §4, §5; R103 (chrome), R104 (Navigator), R107 (Changes by Project), R100 (drift card), R110 (usage cell). Method, tokens and tolerances: [README](README.md).

Window 1440 × 900 px. Columns: Navigator 268 px · center 832 px · Inspector 340 px. Rows: titlebar 40 px · body 834 px · Statusline 26 px.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| A.1 | Window size of the golden scene | 1440 × 900 px | exact | `debug.window` → width, height |
| A.2 | Titlebar height | 40 px | ±1 px | `geometry titlebar → height` |
| A.3 | Titlebar background / bottom border | `bg1` / 1 px `line` | exact | `geometry titlebar → background, border.bottom` |
| A.4 | Titlebar padding left / right; gap between items | 10 px / 8 px; 10 px | ±1 px | `geometry titlebar → padding`; x of `ws-button` and `crumbs` |
| A.5 | Workspace button height, radius, padding | 28 px, 6 px, 0 8 px 0 4 px | ±1 px | `geometry ws-button → height, radius, padding` |
| A.6 | Workspace mark size, radius, fill, letter | 20 × 20 px, 5 px, `acc` flat, `acc-fg` JetBrains Mono 600 11 px | ±1 px; colors exact | `geometry ws-mark → width, height, radius, background, color, font` |
| A.7 | Workspace name `acme` font | Inter 13 px 600, `fg` | ±0.5 px; exact | `geometry ws-name → font, color` |
| A.8 | Breadcrumb font and colors: segments / separators / last segment | Inter 12.5 px `fg2` / `fg3` / `fg` 500 | ±0.5 px; exact | `geometry crumbs → font, color`; `geometry crumb-sep`, `crumb-current` |
| A.9 | Breadcrumb branch `checkout-v2`: font, icon | JetBrains Mono 11.5 px `fg2`; `git-branch` 13 px `wt` | ±0.5 px; exact | `geometry crumb-branch → font, color`; `screenshot` |
| A.10 | Breadcrumb base tail `← develop ↑3` | JetBrains Mono 11 px `fg3` | ±0.5 px; exact | `geometry crumb-base → font, color, label` |
| A.11 | Command field size, radius, border, fill | 440 × 26 px, 6 px, 1 px `line2`, `bg0` | ±1 px; exact | `geometry command-field → width, height, radius, border, background` |
| A.12 | Command field centred in the space between breadcrumb and right buttons; placeholder | horizontal centre within ±1 px; `Search or run a command…` Inter 12.5 px `fg3` | ±1 px; exact text | `geometry command-field → x, width`; `geometry command-field → label, font, color` |
| A.13 | Key caps (`kbd`) | JetBrains Mono 500 10.5 px `fg2`, padding 1 5 px, border 1 px `line2` (bottom 2 px), radius 4 px, `bg2` | ±1 px; exact | `geometry kbd → font, padding, border, radius, background` |
| A.14 | Titlebar icon button size, radius, icon | 28 × 28 px, 6 px, 16 px `fg2` | ±1 px; exact | `geometry tb-button → width, height, radius, color` |
| A.15 | Review button: height, padding, radius, border, fill, label, count | 26 px, 0 10 px, 6 px, 1 px `line2`, `bg2`, `Review` Inter 12.5 px `fg`, `4` (files Review would open: `checkout-v2`'s, R103.1) JetBrains Mono 11 px `fg3` | ±1 px; exact | `geometry review-button → height, padding, radius, border, background, label`; `geometry review-count → label, font` |
| A.16 | Window controls group: left border, padding-left, button size | 1 px `line`, 8 px, 26 × 26 px | ±1 px | `geometry window-controls → border.left, padding.left`; `geometry window-controls tb-button → width` |
| A.17 | Navigator width, background, right border | 268 px, `bg1`, 1 px `line` | ±1 px; exact | `geometry navigator → width, background, border.right` |
| A.18 | Navigator header padding; label `PROJECTS` | 12 12 6 14 px; JetBrains Mono 600 10.5 px, uppercase, letter-spacing 0.12em, `fg3` | ±1 px; exact | `geometry nav-header → padding`; `geometry section-label → font, color` |
| A.19 | Navigator header action buttons | 22 × 22 px, `fg3` | ±1 px | `geometry nav-header tb-button → width, height, color` |
| A.20 | Tree side padding | 6 px | ±1 px | `geometry nav-tree → padding.left, padding.right` |
| A.21 | Navigator row height | 26 px | ±1 px | `geometry nav-row → height` (every row) |
| A.22 | Row radius, gap, right padding | 5 px, 6 px, 8 px | ±1 px | `geometry nav-row → radius, padding.right` |
| A.23 | Indent per level (left padding = 6 + 14 × depth) | 14 px per level: 6 / 20 / 34 / 48 px for depth 0–3 | ±1 px | `geometry nav-row nth=k → padding.left` for Shop, shop-web, checkout-v2, planner |
| A.24 | Row title font / color | Inter 13 px `fg`; Project rows 600 | ±0.5 px; exact | `geometry nav-row-title → font, color` |
| A.25 | Row meta font / color | JetBrains Mono 11 px `fg3`, right-aligned, gap 6 px | ±0.5 px; exact | `geometry nav-row-meta → font, color, x + width = row right − 8` |
| A.26 | Chevron | 12 px wide column, icon 11 px `fg3` | ±1 px; exact | `geometry nav-chevron → width, color` |
| A.27 | Row icon size / color | 15 px `fg3`; Worktree icon `wt`; Project cube `proj-1` (Shop), `proj-2` (Billing), `proj-3` (Docs) | ±1 px; exact | `geometry nav-row-icon → width, color` |
| A.28 | Selected row (`tests`, the focused Agent; its Worktree `checkout-v2` when folded, R104.6): fill, text, bar | `acc-dim`; `fg`; bar 2 × 16 px `acc`, radius 2 px, x = row x, inset 5 px top and bottom | ±1 px; exact | `class nav-row has selected`; `geometry nav-row-selbar → width, height, background` |
| A.29 | Agent glyph size and icon (R115, ADR-0038; replaces the LED on Agent rows) | 14 × 14 px, the agent's logo (`agent-<name>-symbolic`): `planner` / `dev-server` / `ui` Claude, `tests` Codex | ±0 px; exact | `geometry agent-glyph → width, height`; `debug.ui sidebar` → `glyph` |
| A.30 | Glyph colors (the LED's: a collapsed parent's LED keeps 7 × 7 px, radius 2 px) | running `run`; waiting `wait` + glow 6 px `wait`; done `ok`; error `err`; interrupted `wt`; idle `fg3` | exact | `class agent-glyph has glyph-<status>`; `geometry agent-glyph → color`; glow by `screenshot` |
| A.31 | Running glyph pulse | opacity 1 → 0.45 → 1, 1.6 s ease-in-out, infinite | exact | CSS class check `glyph-running` |
| A.32 | Waiting badge on collapsed/parent Project (`1`) | JetBrains Mono 600 10.5 px `wait` on `wait-dim`, padding 0 5 px, radius 4 px, line height 16 px | ±1 px; exact | `geometry nav-badge → font, color, background, padding, height` |
| A.33 | Drift marker on `shop-api` | meta `feat/checkout-v2` in `wait` + `warning-diamond` Fill 12 px `wait` | exact | `debug.ui navigator` row `shop-api` → `drift: true`; `geometry nav-row-meta nth=shop-api → color` |
| A.34 | Also-in marker on `shared-ui` (`also_in` not empty, PRD R98.1; not the same-checkout `shared` label) | `link-simple` 12 px `run`, tooltip `Also in Billing @ release/2.4` | exact | `geometry nav-also-in → color`; tooltip text via `debug.ui navigator` |
| A.35 | Navigator section separator | 1 px `line`, margin 8 px 12 px | ±1 px | `geometry nav-sep → height, margin` |
| A.36 | Scratch section label and row | `SCRATCH` (A.18 style), header top padding 2 px; row `~/workspaces/acme` meta `2` | ±1 px; exact text | `debug.ui navigator` → section `scratch`; `geometry nav-header nth=1 → padding.top` |
| A.37 | Navigator footer: top border, padding, gap | 1 px `line`, 8 px 10 px, 8 px | ±1 px | `geometry nav-footer → border.top, padding` |
| A.38 | Inbox pill | height 28 px, padding 0 8 px, radius 6 px, `wait-dim` fill, Inter 12 px `wait`, Agent name `fg`, key cap `Ctrl ⇧ G` at the right | ±1 px; exact | `geometry inbox-pill → height, padding, radius, background, font, color` |
| A.39 | Tab strip height, background, bottom border, left padding | 34 px, `bg1`, 1 px `line`, 4 px | ±1 px; exact | `geometry tab-strip → height, background, border.bottom, padding.left` |
| A.40 | Tab padding, gap, font, right border | 0 12 px, 8 px, Inter 12.5 px `fg2`, 1 px `line` | ±1 px; exact | `geometry tab → padding, font, color, border.right` |
| A.41 | Active tab (`agents`) | fill `bg0`, text `fg`, top bar 2 px `acc` full tab width | ±1 px; exact | `class tab has active`; `geometry tab-activebar → height, width = tab width, background` |
| A.42 | Tab count / close | `2` JetBrains Mono 11 px `fg3`; close `x` 12 px `fg3` | ±0.5 px; exact | `geometry tab-count → font, color`; `geometry tab-close → width, color` |
| A.43 | Tab glyph and type icon | the lead Agent's glyph 14 px in the Tab's status colour (A.29, A.30) for Agent tabs; `globe-simple` / `git-diff` 13 px `fg3` for Browser / Review | ±1 px; exact | `geometry tab-glyph → width, color`; `geometry tab-icon → width, color` |
| A.44 | New tab button | 34 px wide, `plus` 16 px `fg3` | ±1 px | `geometry tab-add → width, color` |
| A.45 | Pane grid padding and gap | 6 px / 6 px | ±1 px | `geometry panes → padding`; x of pane 2 − (x + width of pane 1) = 6 |
| A.46 | Pane border, radius, body | 1 px `line`, 6 px, `bg0` | ±1 px; exact | `geometry pane → border, radius, background` |
| A.47 | Focused Pane (`tests`) | class `focused`; 1 px `acc-line` border; `glow` halo `0 0 18px` (dark; none in light) | exact | `geometry pane → classes, border_color, shadow`; ring and halo as drawn: `geometry pane → drawn.edge, drawn.outside`; `screenshot` |
| A.48 | Pane header height, padding, gap, background, bottom border | 26 px, 0 8 px, 8 px, `bg1`, 1 px `line` | ±1 px; exact | `geometry pane-header → height, padding, background, border.bottom` |
| A.49 | Pane header font: base / name / status | JetBrains Mono 11.5 px `fg3` / `fg` 500 / waiting status `wait` with `hand-palm` Fill 13 px | ±0.5 px; exact | `geometry pane-header → font, color`; `geometry pane-name → color`; `geometry pane-status nth=1 → color` |
| A.50 | Pane header text (`planner · claude · opus · done · 12s ago`; `tests · codex · gpt-5 · needs input`) | name, agent, model, status in that order | exact text | `debug.ui pane` → header labels |
| A.51 | Terminal padding inside the Pane | 10 px top/bottom, 12 px left/right | ±1 px | `geometry terminal → x − pane x, y − (pane-header y + height)` |
| A.52 | Terminal background / foreground | `bg0` / `term-fg` | exact | Ghostty theme generated from tokens: `geometry terminal → background`; `screenshot` |
| A.53 | Inspector width, background, left border | 340 px, `bg1`, 1 px `line` | ±1 px; exact | `geometry inspector → width, background, border.left` |
| A.54 | Inspector tabs: height, labels | 34 px; `CHANGES 7`, `FILES`, `SEARCH`, Activity icon tab right-aligned | ±1 px; exact text | `geometry inspector-tabs → height`; `debug.ui chrome` → `inspector.labels` |
| A.55 | Inspector tab font, padding, icon | JetBrains Mono 500 11 px, uppercase, letter-spacing 0.06em, `fg3`; 0 12 px; icon 14 px | ±0.5 px; exact | `geometry inspector-tab → font, color, padding` |
| A.56 | Active Inspector tab | text `fg`; bar 2 px `acc` inset 10 px left and right, at the bottom edge | ±1 px; exact | `class inspector-tab has active`; `geometry inspector-tab-bar → height, x, width` |
| A.57 | Inspector tab count | JetBrains Mono 10 px `acc` | ±0.5 px; exact | `geometry inspector-tab-count → font, color` |
| A.58 | Changes header `PROJECT · SHOP`, totals, tools | label style A.18; padding 0 12 px 8 px; `+212` `add`, `−58` `del`, JetBrains Mono 11 px; 3 tools 22 × 22 px | ±1 px; exact | `geometry changes-header → padding`; `geometry changes-total-add → color, label`; `geometry changes-tool all → width, height` |
| A.59 | Link card | margin 0 8 px 10 px, 1 px `line`, radius 6 px, `bg2` | ±1 px; exact | `geometry changes-group → margin, border, radius, background` |
| A.60 | Card header | padding 7 10 px, bottom border 1 px `line`, Inter 12 px, title 600, meta JetBrains Mono 11 px `fg3`; chevron 12 px `fg3` | ±1 px; exact | `geometry changes-group-header → padding, border.bottom, font` |
| A.61 | File row | height 24 px, padding 0 10 px, gap 6 px, JetBrains Mono 12 px; icon 14 px; dir `fg3`, name `fg` | ±1 px; exact | `geometry file-row → height, padding, font`; `geometry file-row-dir → color` |
| A.62 | Status letter | right-most, 12 px column, 600; `M` `wait`, `A` `add`, `U` `add`, `D` `del`, `R` `run`, `!` `err` | ±1 px; exact | `geometry file-status → width, color` per letter; right edge = row right − 10 px |
| A.63 | `+/−` counts | JetBrains Mono 11 px, `add` / `del`, right edge 8 px left of the status letter | ±1 px; exact | `geometry file-stat → font, x + width` |
| A.64 | Commit area | padding 8 10 px, bottom border 1 px `line`, gap 6 px; message box min 26 px high, radius 5 px, 1 px `line2`, `bg0`, placeholder `Message (Ctrl+Enter to commit on ‘<branch>’)` 12 px `fg3` | ±1 px; exact | `geometry commit-row → padding, border.bottom`; `geometry commit-entry → height, radius, border, background` |
| A.65 | `Commit` split button | main: `acc` fill, `acc-fg` text 600, no border, 26 px high, radius 6 0 0 6; arrow part `.commit-more` 22 px wide, same fill, radius 0 6 6 0, 1 px left border `acc-fg` at 25 %; exactly one `.suggested-action` visible on screen A | exact; count exact | `geometry commit-button → background, color, font.weight`; `geometry commit-more → width, background`; `count .suggested-action = 1` |
| A.66 | Draft button (`sparkle`) | 26 px high, secondary (A.15 style), icon 14 px `fg2`, tooltip `Draft with agent` | ±1 px; exact | `geometry draft-button → height, background` |
| A.67 | Drift card | margin 0 8 px 10 px, padding 8 10 px, 1 px `wait-line`, `wait-dim`, radius 6 px, Inter 12 px `fg`, gap 8 px; icon `warning-diamond` Fill 15 px `wait` | ±1 px; exact | `geometry drift-card → margin, padding, border, background, radius, font` |
| A.68 | Drift card sub-line and actions | sub-line Inter 11.5 px `fg2`, 2 px below; actions 6 px below, gap 6 px, buttons 22 px high Inter 11.5 px: `Keep, show in Shop`, `Move to a Worktree…` (no `Back to base`: the checkout is dirty) | ±1 px; exact text | `geometry drift-sub → font, color`; `geometry drift-card btn → height, label` |
| A.69 | Clean card | header only, no rows, opacity 0.75 (1.0 when it shows an action button) | exact | `geometry changes-group nth=2 → opacity`; `debug.scm` → rows 0 |
| A.70 | Statusline height, background, top border, font | 26 px, `bg1`, 1 px `line`, JetBrains Mono 11 px `fg2` | ±1 px; exact | `geometry statusline → height, background, border.top, font, color` |
| A.71 | Statusline cell padding, gap, divider, icon | 0 10 px, 6 px, right border 1 px `line`, icon 12 px | ±1 px; exact | `geometry status-cell → padding, border.right` |
| A.72 | Workspace cell | `acc` fill, `acc-fg` text 600, `circles-four`, text `ACME` (uppercase), radius 0 | exact | `geometry status-cell-ws → background, color, font.weight, radius, label` |
| A.73 | Status counts cell | `● 1 waiting` `wait`, `● 1 running` `run`, `● 2 done` `ok` | exact | `geometry status-count nth=k → color, label` |
| A.74 | Usage cell | `gauge` + `claude 5h` + meter 36 × 5 px radius 3 px track `bg4` fill `acc` 62 % + `62% · wk 41%`, tooltip `Claude · acme-work` | ±1 px; exact | `geometry usage-meter → width, height, background`; `geometry usage-meter-fill → width = 22 ± 1`; `debug.ui chrome` → statusline cells |
| A.75 | Identity cell | `user-circle` + `richard@acme.io` | exact text | `debug.ui chrome` → statusline cells |
| A.76 | Right key-hint cell | pushed right, left border 1 px `line`, no right border: `Ctrl ⇧ P commands`, `Ctrl ⇧ G next waiting` (the forms that work in a Terminal, R17 amendment) | ±1 px; exact text | `geometry status-cell-hints → x + width = 1440, border.left` |
| A.77 | Statusline cell order | Workspace · Project · Worktree (↑3 ↓0) · counts · port `:5173` · usage · identity · hints | exact | `debug.ui chrome` → statusline cells in order |
| A.78 | Contrast of every text token used on this screen | ≥ 4.5 : 1 (`fg`, `fg2`, `fg3`, `acc`, `wait`, `run`, `ok`, `add`, `del` on `bg0`–`bg3`, `acc-dim`, `wait-dim`; `acc-fg` on `acc`) | ≥ threshold | `contrast.py` (both themes) |
| A.79 | Contrast of meaning-carrying icons / glyphs / LEDs | ≥ 3 : 1 (`wt`, `proj-1..3`, glyph and LED colors on `bg1`, `bg0`) | ≥ threshold | `contrast.py` (both themes) |
| A.80 | Whole scene | matches the approved golden, dark and light | pixel threshold, terminal + "ago" masked | `screenshot` |
| A.81 | Section row | height 22 px, padding 0 10 px; label JetBrains Mono 10.5 px 600 `fg2` uppercase; count pill `.scm-count` JetBrains Mono 10 px, `bg3`, radius 8 px, padding 0 5 px | ±1 px; exact | `geometry scm-section → height, padding, font`; `geometry scm-count → background, radius` |
| A.82 | Row actions | `.scm-act` 20 × 20 px, flat, icon 14 px `fg2`, gap 2 px; shown in place of the counts on hover / selection / focus | ±1 px; exact | `geometry scm-act → width, height` (with a row selected) |
| A.83 | Selected row | background `acc-dim`; text colors unchanged | exact | `geometry file-row (selected) → background` |
| A.84 | Action button | margin 8 10 px, 26 px high, secondary (A.15 style), Inter 12 px 600, icon 14 px | ±1 px; exact | `geometry scm-action-button → height, margin, font` |
| A.85 | Subject warning | Inter 11 px `wait`, 2 px below the message box | ±1 px; exact | `geometry commit-warning → font, color` |
| A.86 | Sync / Publish button | flat, 22 px high, icon 14 px `fg2`, label JetBrains Mono 11 px `fg2` | ±1 px; exact | `geometry scm-sync → height, font` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Titlebar icon button | `bg3` fill, icon `fg` | `bg3` fill, icon `acc` (Inspector toggle while shown) | `glow` ring | opacity 0.45, no hover |
| Workspace button | `bg3` fill | popover open: `bg3` | `glow` ring | — |
| Command field | border `line2` → `acc-line` | click opens the palette (screen D) | `glow`, border transparent | — |
| Review button / secondary buttons | `bg3` fill | — | `glow` ring | opacity 0.45 |
| Primary button (`Commit`) | adds `glow` | — | `glow` ring | opacity 0.45 (message empty), no hover |
| Navigator row | `bg3` fill | A.28 | `glow` ring inside the row radius | `dim`: title `fg2` (Docs with no Sources) |
| Tab | text `fg` | A.41 | `glow` ring | — |
| Pane | — | A.47 (`focused`) | same as selected | — |
| Inspector tab | text `fg2` | A.56 | `glow` ring | — |
| File row | `bg3` fill | opened in Review: `acc-dim` | `glow` ring | — |
| Drift action button | `bg3` fill | — | `glow` ring | `Back to base` hidden (not disabled) while dirty |
| Inbox pill | fill `wait-dim` stays, text `fg` underline none, cursor pointer | — | `glow` ring | hidden when nothing waits |
| Statusline cell (usage, identity, counts) | `bg3` fill (clickable cells only) | — | `glow` ring | — |

Transitions: 120 ms ease-out on background and color.

## Empty state

- **No Projects in the Workspace:** Navigator shows `PROJECTS` and one line `No Projects yet` (Inter 13 px `fg3`) with a secondary button `New Project…`; Scratch stays. No illustration, no warning icon.
- **Tab with no Pane / Worktree with no Agents:** center shows one mono line `No terminals here` (JetBrains Mono 12 px `fg3`) and key hints `Ctrl+Shift+T shell · + agent` (kbd A.13). Replaces `adw::StatusPage`.
- **Inspector › Changes with no changes:** `PROJECT · SHOP` header with `+0 −0`, each link group collapsed to its header with meta `clean` (A.69), one line `No changes against each link's base` `fg3`. Never a warning icon (design §1).
- **Nothing waiting:** the Inbox pill is hidden; the bell button stays.
- **No account for the focused Agent / no usage data:** the usage cell is hidden (not `0%`).

## 1280 px

Window 1280 × 800 px, same data.
- Navigator keeps 268 px. The Inspector becomes an **overlay**: 340 px wide, over the right edge of the center, `bg1`, left border 1 px `line2`, `shadow`; hidden by default, toggled by the Inspector button / `Ctrl+Shift+E` (`Ctrl+B` outside a Terminal). Center = 1280 − 268 = 1012 px.
- Command field shrinks from 440 px to a minimum of 280 px before anything else; the breadcrumb then ellipsizes the middle segments (`shop-web`) first, keeping Project and branch.
- The two Panes stay side by side (each ≥ 400 px wide).
- Statusline drops cells from the right side of the order in A.77 in this order until it fits: hints, port, identity (never Workspace, counts, usage).
- No horizontal scroll anywhere; every row in A.21–A.38 keeps its height.

## Text scale 200 %

1440 × 900 px with text-scaling-factor 2.0.
- Fonts double (13 → 26 px). Heights given in px become **minimums**: a row grows to its text's natural height + 8 px; nothing is clipped vertically (`geometry` label natural height ≤ allocation for every visible label).
- Navigator collapses to an **icon rail** 48 px wide (Project cubes, Scratch, bell; tooltips carry names); a click on a rail icon opens the full Navigator (268 px) as an overlay with `shadow`.
- Center shows **one main Pane** (the focused one); other Panes of the Agent Grid become title bars (R2.4 overflow).
- Inspector hidden; `Ctrl+Shift+E` (`Ctrl+B` outside a Terminal) opens it as an overlay (as at 1280 px).
- Statusline keeps Workspace, counts and usage; other cells drop as at 1280 px.
- No horizontal scroll; the command field shows its icon and key caps only if the placeholder does not fit.

## Content

Mockup data used on this screen (golden scene and E2E fixtures use exactly these):
- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Workspace `acme` (mark `A`), identity `richard@acme.io`.
- Projects: `Shop` (icon `proj-1`, waiting badge `1`), `Billing` (`proj-2`, running LED), `Docs` (`proj-3`, dim, no Sources).
- Shop links (base `develop`): `shop-web` `develop` → Agent `dev-server` (done, `:5173`) and Worktree `checkout-v2` (`↑3 ●7`, selected) → Agents `planner` (done, `claude`) and `tests` (waiting, `needs input`); `shop-api` drifted on `feat/checkout-v2` (3 uncommitted changes); `shared-ui` `develop`, also in Billing at `release/2.4` (each has its own checkout).
- Billing links: `billing` `main`; `shared-ui` `release/2.4` → Worktree `tokens` (`↑1`) → Agent `ui` (running, `2m 14s`).
- Scratch: `~/workspaces/acme` (2 Panes).
- Breadcrumb `Shop / shop-web / checkout-v2 ← develop ↑3`; Review `4` (the mockup's `7` is the Project total, which the Inspector's `Changes 7` tab shows; the Review button counts what Review opens, R103.1).
- Tabs: `agents 2` (active, waiting LED), `localhost:5173`, `Review`.
- Panes: `planner · claude · opus · done · 12s ago`; `tests · codex · gpt-5 · needs input` (focused).
- Changes: `PROJECT · SHOP +212 −58`; shop-web `checkout-v2 ↑3 · 4`: `M src/checkout/Form.tsx +84 −31`, `A src/checkout/Steps.tsx +66`, `M src/api/quote.ts +12 −9`, `D src/checkout/legacy.ts −18`; shop-api `feat/checkout-v2 3`: `M src/routes/quote.rs +41 −0`, `A tests/quote_v2.rs +9`, `M Cargo.toml +1`; drift card "shop-api is on feat/checkout-v2, Shop's base is develop. 3 uncommitted changes, so RADE won't switch it."; shared-ui `develop clean`.
- Statusline: `ACME`, `Shop`, `checkout-v2 ↑3 ↓0`, `1 waiting · 1 running · 2 done`, `:5173`, `claude 5h 62% · wk 41%` (account `acme-work`, `five_hour` 62 %), `richard@acme.io`.
- Accounts in the Workspace (used by F and E): `acme-work` (claude, default), `acme-team` (claude), `acme` (codex, default; Spark 5-hour 81 %).
