# Design spec sheets (RADE 0.8.0)

Status: written 2026-10-23 (M33, D4). Binding for M35 (R102, R103), M37 (R104–R107), M38 (R108), M39 (R110), M40 (R111).
Sources: [mockup.html](../redesign-2026-10-23/mockup.html) (all numbers below are read from its CSS), `new-<a..f>-{dark,light}.png`, redesign-2026-10-23.md §4–§6 and §10.3, redesign-build-plan.md §3, [ADR-0036](../../adr/0036-terminal-native-visual-system.md).

The mockup is the picture; these sheets are what tests check. One file per mockup screen:

| Screen | File | Mockup hash | What it shows |
|---|---|---|---|
| A | [a-workspace.md](a-workspace.md) | `#a-dark` / `#a-light` | Window chrome, Navigator, flat tabs, two Agent Panes, Inspector › Changes by Project with Base drift, Statusline |
| B | [b-project-home.md](b-project-home.md) | `#b-…` | Project home of Shop: KPIs, link table, Worktrees, Recent activity |
| C | [c-new-project.md](c-new-project.md) | `#c-…` | New Project dialog with one base per link and the checkout plan, over A |
| D | [d-palette.md](d-palette.md) | `#d-…` | Command palette, query `shared`, `shared-ui` in two Projects, over A |
| E | [e-usage.md](e-usage.md) | `#e-…` | Agent usage page: account table, Next resets, account detail |
| F | [f-new-agent.md](f-new-agent.md) | `#f-…` | `+` menu with Agent Accounts, limit forecast Toast, over A |

## Rules for every sheet

- **IDs are stable.** `A.1`, `A.2`, … (letter = screen). A row is never renumbered or reused; a dropped row keeps its ID with the value `— (removed <date>)`. PRD ACs cite `spec A.4` or "every row of the Measurements table of `docs/design/spec/a-workspace.md`".
- **Columns.** `ID | Element | Value (px/pt/token) | Tolerance | How measured`. Every Value is a number with a unit (`px` = GTK CSS px at scale 1; `ms`; `%`; `em`) or a token name from the tables below. Every row has all five cells.
- **Window.** Golden scenes run at **1440 × 900 px, scale 1**, with the mockup's data (each sheet's `## Content`). `## 1280 px` = the same scene at 1280 × 800. `## Text scale 200 %` = 1440 × 900 with `org.gnome.desktop.interface text-scaling-factor 2.0` (GTK `gtk-xft-dpi` doubled).
- **Themes.** Every token row holds in both themes; the hex comes from the dark or light table. A screen is checked in both.
- **Where the sheet and the mockup differ**, the sheet wins and says why (mostly GTK limits and WCAG AA; see Token notes and `redesign-decisions.md`).

## Measurement method

| Method (as written in "How measured") | What it is | Built in |
|---|---|---|
| `geometry <widget> → <field>` | Hidden socket method `debug.ui {"op":"geometry","widget":"<widget>","nth"?:n}` (test mode only, ADR-0021). `<widget>` is the CSS name or class given in the sheet (e.g. `nav-row`, `titlebar`); `nth` picks the n-th match in document order (0-based, default 0). Returns `{"x","y","width","height"}` (window coordinates, allocation), `"margin"`, `"padding"`, `"border"` (each `{top,right,bottom,left}` px), `"radius"` (px), `"font": {"family","size","weight","letter_spacing","features"}`, `"color"`, `"background"` (computed: what GTK draws for the widget's own box, `#rrggbbaa`), `"drawn"` (the window's pixels: `background` where no child covers the widget, `edge` / `outside` per side), `"opacity"`, `"classes"`, `"visible"`, `"label"` (text of a label child, if any). Colors compare to the token's hex. | M35 (R102) |
| `class <widget> has <c>` / `count <selector> = n` | The same op's `classes` field; `count` = number of visible matches of the selector in the window (`{"op":"geometry","widget":…,"all":true}` returns a list). | M35 |
| `contrast.py <ink> on <bg>` | `docs/design/contrast.py` reading `crates/rade-app/src/tokens.toml` (both themes), compositing translucent tokens over their base. Text ≥ 4.5 : 1, icons / LEDs / borders that carry meaning ≥ 3 : 1. | M35 |
| `screenshot` | Golden image of the scene (`rade debug screenshot`), light + dark, compared by pixel to the approved golden with a small threshold; terminal contents and clock / "ago" texts are masked. First approval: a reviewer agent compares GTK and mockup side by side in `target/design-diff/index.html` (build plan §3.5). Used only where geometry cannot see the thing (glow, gradient-free fills, icon shape). | M35 |
| `debug.ui <op>` | An existing or milestone-specific `debug.ui` op that returns model-level facts (row titles, meta texts, order). Named per row. | per milestone |

Default tolerances: sizes and positions **±1 px**; font size **±0.5 px**, family and weight **exact**; colors **exact token** (computed `#rrggbbaa` equals the token, ±1 per channel for rounding); durations **exact**; counts and texts **exact**.

## Tokens

`crates/rade-app/src/tokens.toml` (M35) is the only place a color is written (ADR-0036 §3). Names below are the token names; the mockup's CSS variable is given where it differs. Translucent tokens are written as 8-digit hex (`#rrggbbaa`) because GTK CSS has no `color-mix()` (ADR-0036 Spike); the mockup's `color-mix(in srgb, X N%, transparent)` becomes a precomputed `-line` token.

**AA column.** Some mockup values fail WCAG AA (4.5 : 1 for text) on the backgrounds they are used on — e.g. dark `fg3` `#5e6773` on `bg3` = 2.95 : 1, light `acc` `#0b8f7b` under white text = 4.02 : 1. ADR-0036 §3 requires every text pair to pass, so `tokens.toml` uses the **Spec** value: the same hue, lightness moved the minimum needed to reach 4.5 : 1 against every background in that token's "On" list (3 : 1 for icon-only tokens). Where Mockup = Spec the mockup already passes. The mockup's `tokens.css`, generated from `tokens.toml` (build plan §3.1), follows.

### Dark (default)

| Token | Mockup value | Spec value | Used for | On (contrast set) |
|---|---|---|---|---|
| `bg0` | `#07090b` | `#07090b` | terminal, Pane body, page body (Project home, Usage), inputs, command field | — |
| `bg1` | `#0c0e11` | `#0c0e11` | chrome: titlebar, Navigator, tab strip, Pane header, Inspector, Statusline, cards | — |
| `bg2` | `#12151a` | `#12151a` | raised: buttons, groups, dialog, palette, popover, Toast, kbd | — |
| `bg3` | `#191d23` | `#191d23` | hover | — |
| `bg4` | `#21262d` | `#21262d` | meter / bar track, Codex badge | — |
| `line` | `rgba(255,255,255,.07)` | `#ffffff12` | hairlines, borders of panes, groups, cards | — |
| `line2` | `rgba(255,255,255,.12)` | `#ffffff1f` | stronger borders: buttons, inputs, kbd, dialogs, chips | — |
| `fg` | `#e6e9ed` | `#e6e9ed` | primary text | bg0–bg3 (≥ 13.9) |
| `fg2` | `#9aa3ae` | `#9aa3ae` | secondary text | bg0–bg3 (≥ 6.6) |
| `fg3` | `#5e6773` | `#828c99` | metadata, labels, placeholders, inactive icons | bg0–bg3, `acc-dim` over bg1 |
| `acc` (`--acc`) | `#4fe3c8` | `#4fe3c8` | focus, selection bar, active tab bar, primary button, LED-free accents | bg0–bg3 (≥ 10.6) |
| `acc-fg` | `#04110e` | `#04110e` | text on `acc` | `acc` (12.05) |
| `acc-dim` | `rgba(79,227,200,.12)` | `#4fe3c81f` | selected row / palette item / scope chip fill | — |
| `acc-line` | `rgba(79,227,200,.45)` | `#4fe3c873` | focus ring (inside `glow`) | — |
| `acc-bar` | `color-mix(acc 55%)` | `#4fe3c88c` | token bars of past days | — |
| `wait` | `#f5b84b` | `#f5b84b` | waiting LED, `needs input`, drift text, ≥ 80 % limits | bg0–bg3, `wait-dim` |
| `wait-dim` | `rgba(245,184,75,.13)` | `#f5b84b21` | waiting badge, Inbox pill, drift card, warn chip fill | — |
| `wait-line` | `color-mix(wait 40–45%)` | `#f5b84b73` | drift card / warn chip border | — |
| `run` (`--run`) | `#6cb6ff` | `#6cb6ff` | running LED, also-in marker, info chip | bg0–bg3 |
| `run-line` | `color-mix(run 45%)` | `#6cb6ff73` | info chip border | — |
| `ok` (`--ok`) | `#7bd88f` | `#7bd88f` | done LED, ok chip, PR checks | bg0–bg3 |
| `ok-line` | `color-mix(ok 45%)` | `#7bd88f73` | ok chip border | — |
| `err` (`--err`) | `#ff6b6b` | `#ff6b6b` | error LED | bg0–bg3 |
| `wt` (`--wt`) | `#c4a7ff` | `#c4a7ff` | Worktree icon | bg0–bg3 (icon) |
| `add` | `#7bd88f` | `#7bd88f` | `+n`, status `A` | bg1, bg2 |
| `del` | `#ff7b7b` | `#ff7b7b` | `−n`, status `D` | bg1, bg2 |
| `proj-1` | `var(--acc)` | `#4fe3c8` | first Project's cube icon (Shop) | bg1, bg3 (icon) |
| `proj-2` | `#e58fff` | `#e58fff` | second Project's icon (Billing) | bg1, bg3 (icon) |
| `proj-3` | `#ffb86b` | `#ffb86b` | third Project's icon (Docs) | bg1, bg3 (icon) |
| `agent-claude` | `#e08a6a` | `#e08a6a` | Claude badge letter | `agent-claude-bg` over bg2 (5.6) |
| `agent-claude-bg` | `rgba(217,119,87,.16)` | `#d9775729` | Claude badge fill | — |
| `agent-codex` | `var(--fg)` on `--bg4` | `#e6e9ed` | Codex badge letter (fill `bg4`) | bg4 |
| `term-fg` | `#cfd6de` | `#cfd6de` | Ghostty foreground (terminal theme) | bg0 |
| `term-0` | — | `#191d23` | Ghostty palette 0 (black), from the status tokens | — (background-like) |
| `term-1` | — | `#ff6b6b` | Ghostty palette 1 (red), from the status tokens | bg0 |
| `term-2` | — | `#7bd88f` | Ghostty palette 2 (green), from the status tokens | bg0 |
| `term-3` | — | `#f5b84b` | Ghostty palette 3 (yellow), from the status tokens | bg0 |
| `term-4` | — | `#6cb6ff` | Ghostty palette 4 (blue), from the status tokens | bg0 |
| `term-5` | — | `#c4a7ff` | Ghostty palette 5 (magenta), from the status tokens | bg0 |
| `term-6` | — | `#4fe3c8` | Ghostty palette 6 (cyan), from the status tokens | bg0 |
| `term-7` | — | `#cfd6de` | Ghostty palette 7 (white), from the status tokens | bg0 |
| `term-8` | — | `#828c99` | Ghostty palette 8 (bright black), from the status tokens | bg0 |
| `term-9` | — | `#ff8f8f` | Ghostty palette 9 (bright red), from the status tokens | bg0 |
| `term-10` | — | `#9be6a9` | Ghostty palette 10 (bright green), from the status tokens | bg0 |
| `term-11` | — | `#ffd27a` | Ghostty palette 11 (bright yellow), from the status tokens | bg0 |
| `term-12` | — | `#93c9ff` | Ghostty palette 12 (bright blue), from the status tokens | bg0 |
| `term-13` | — | `#e58fff` | Ghostty palette 13 (bright magenta), from the status tokens | bg0 |
| `term-14` | — | `#8af0dd` | Ghostty palette 14 (bright cyan), from the status tokens | bg0 |
| `term-15` | — | `#e6e9ed` | Ghostty palette 15 (bright white), from the status tokens | bg0 |
| `term-sel` | — | `#19403a` | Ghostty selection background (`acc` over `bg0`) | under `term-fg` |
| `syn-comment` | — | `#8b949e` | code token: comments (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-keyword` | — | `#ff7b72` | code token: keywords, storage (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-string` | — | `#a5d6ff` | code token: strings, regexps (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-constant` | — | `#79c0ff` | code token: constants, numbers, builtins, attributes (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-function` | — | `#d2a8ff` | code token: function names (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-type` | — | `#ffa657` | code token: types, entity names (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-tag` | — | `#7ee787` | code token: tags, inserted (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-invalid` | — | `#ffa198` | code token: errors, invalid (GitHub Dark Default, MIT; ADR-0037) | bg0, bg1 |
| `scrim` | `rgba(0,0,0,.5)` | `#00000080` | behind dialog / palette | — |
| `shadow` | `0 20px 60px rgba(0,0,0,.55)` | `0 20px 60px #0000008c` | dialog, palette, popover, Toast only | — |
| `glow` | `0 0 0 1px acc-line, 0 0 18px rgba(79,227,200,.10)` | `0 0 0 1px #4fe3c873, 0 0 18px #4fe3c81a` | focused Pane, focused input, keyboard focus ring | — |

### Light

| Token | Mockup value | Spec value | Used for | On (contrast set) |
|---|---|---|---|---|
| `bg0` | `#fcfcfb` | `#fcfcfb` | as dark | — |
| `bg1` | `#f3f4f5` | `#f3f4f5` | as dark | — |
| `bg2` | `#ffffff` | `#ffffff` | as dark | — |
| `bg3` | `#eceef0` | `#eceef0` | as dark | — |
| `bg4` | `#e2e5e8` | `#e2e5e8` | as dark | — |
| `line` | `rgba(16,22,30,.08)` | `#10161e14` | as dark | — |
| `line2` | `rgba(16,22,30,.14)` | `#10161e24` | as dark | — |
| `fg` | `#14181d` | `#14181d` | as dark | bg0–bg3 (≥ 15.3) |
| `fg2` | `#525c67` | `#525c67` | as dark | bg0–bg3 (≥ 5.8) |
| `fg3` | `#89919b` | `#616872` | as dark | bg0–bg3, `acc-dim` over bg1 |
| `acc` | `#0b8f7b` | `#097565` | as dark; text in `acc` and white on `acc` | bg0–bg3, `acc-dim` over bg1/bg2, under `acc-fg` |
| `acc-fg` | `#ffffff` | `#ffffff` | text on `acc` | `acc` (≥ 4.5) |
| `acc-dim` | `rgba(11,143,123,.10)` | `#0b8f7b1a` | as dark | — |
| `acc-line` | `rgba(11,143,123,.5)` | `#0b8f7b80` | as dark | — |
| `acc-bar` | `color-mix(acc 55%)` | `#0b8f7b8c` | as dark | — |
| `wait` | `#a8690b` | `#925c0a` | as dark | bg0–bg3, `wait-dim` over bg1/bg2 |
| `wait-dim` | `rgba(196,128,20,.12)` | `#c480141f` | as dark | — |
| `wait-line` | `color-mix(wait 40–45%)` | `#a8690b73` | as dark | — |
| `run` | `#2a6fc0` | `#2868b5` | as dark | bg0–bg3 |
| `run-line` | `color-mix(run 45%)` | `#2a6fc073` | as dark | — |
| `ok` | `#25844a` | `#217642` | as dark | bg0–bg3 |
| `ok-line` | `color-mix(ok 45%)` | `#25844a73` | as dark | — |
| `err` | `#cf3d3d` | `#c23030` | as dark | bg0–bg3 |
| `wt` | `#7a55d6` | `#744dd4` | as dark | bg0–bg3 |
| `add` | `#25844a` | `#217642` | as dark | bg1, bg2 |
| `del` | `#cf3d3d` | `#c23030` | as dark | bg1, bg2 |
| `proj-1` | `var(--acc)` | `#097565` | as dark | bg1, bg3 (icon) |
| `proj-2` | `#e58fff` | `#d036ff` | as dark | bg1, bg3 (icon, 3 : 1) |
| `proj-3` | `#ffb86b` | `#cc6a00` | as dark | bg1, bg3 (icon, 3 : 1) |
| `agent-claude` | `#e08a6a` | `#ad4924` | as dark | `agent-claude-bg` over bg2 |
| `agent-claude-bg` | `rgba(217,119,87,.16)` | `#d9775729` | as dark | — |
| `agent-codex` | `var(--fg)` on `--bg4` | `#14181d` | as dark | bg4 |
| `term-fg` | `#2a3038` | `#2a3038` | as dark | bg0 |
| `term-0` | — | `#14181d` | Ghostty palette 0 (black), from the light status tokens | bg0 |
| `term-1` | — | `#c23030` | Ghostty palette 1 (red), from the light status tokens | bg0 |
| `term-2` | — | `#217642` | Ghostty palette 2 (green), from the light status tokens | bg0 |
| `term-3` | — | `#925c0a` | Ghostty palette 3 (yellow), from the light status tokens | bg0 |
| `term-4` | — | `#2868b5` | Ghostty palette 4 (blue), from the light status tokens | bg0 |
| `term-5` | — | `#744dd4` | Ghostty palette 5 (magenta), from the light status tokens | bg0 |
| `term-6` | — | `#097565` | Ghostty palette 6 (cyan), from the light status tokens | bg0 |
| `term-7` | — | `#e2e5e8` | Ghostty palette 7 (white), from the light status tokens | — (background-like, as in Adwaita) |
| `term-8` | — | `#616872` | Ghostty palette 8 (bright black), from the light status tokens | bg0 |
| `term-9` | — | `#b02a2a` | Ghostty palette 9 (bright red), from the light status tokens | bg0 |
| `term-10` | — | `#1d6a3b` | Ghostty palette 10 (bright green), from the light status tokens | bg0 |
| `term-11` | — | `#7f5009` | Ghostty palette 11 (bright yellow), from the light status tokens | bg0 |
| `term-12` | — | `#235c9f` | Ghostty palette 12 (bright blue), from the light status tokens | bg0 |
| `term-13` | — | `#9030b0` | Ghostty palette 13 (bright magenta), from the light status tokens | bg0 |
| `term-14` | — | `#08685a` | Ghostty palette 14 (bright cyan), from the light status tokens | bg0 |
| `term-15` | — | `#f3f4f5` | Ghostty palette 15 (bright white), from the light status tokens | — (background-like, as in Adwaita) |
| `term-sel` | — | `#cbe1dd` | Ghostty selection background (`acc` over `bg0`) | under `term-fg` |
| `syn-comment` | `#6e7781` | `#636c76` | code token: comments (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-keyword` | — | `#cf222e` | code token: keywords, storage (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-string` | — | `#0a3069` | code token: strings, regexps (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-constant` | — | `#0550ae` | code token: constants, numbers, builtins, attributes (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-function` | — | `#8250df` | code token: function names (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-type` | — | `#953800` | code token: types, entity names (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-tag` | — | `#116329` | code token: tags, inserted (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `syn-invalid` | — | `#82071e` | code token: errors, invalid (GitHub Light Default, MIT; ADR-0037) | bg0, bg1 |
| `scrim` | `rgba(20,26,34,.28)` | `#141a2247` | as dark | — |
| `shadow` | `0 20px 60px rgba(20,30,40,.18)` | `0 20px 60px #141e282e` | as dark | — |
| `glow` | `0 0 0 1px acc-line` | `0 0 0 1px #0b8f7b80` | as dark (no outer glow in light) | — |

Token notes:
- Project icon colors cycle `proj-1`, `proj-2`, `proj-3` by the Project's order in the Workspace (4th = `proj-1` again). They are icon-only.
- The mockup's Workspace mark uses a gradient (`acc` → `#3aa0ff`); ADR-0036 §1 forbids decorative gradients, so the mark is flat `acc` with `acc-fg` text.
- The bars of the "Tokens per day" chart use `acc-bar`; today's bar and the hovered bar use `acc`.

## Type scale

Inter (UI) and JetBrains Mono (metadata), both bundled (ADR-0036 §4). Mono always has `font-feature-settings: "tnum"`. Body line height 1.4.

| Name | Family | Size | Weight | Other | Used for |
|---|---|---|---|---|---|
| `t-page` | Inter | 20 px | 600 | letter-spacing −0.01em | Project home / Usage page title |
| `t-kpi` | JetBrains Mono | 18 px | 600 | small part 12 px / 400 / `fg3` | KPI value |
| `t-dialog` | Inter | 15 px | 600 | — | dialog title |
| `t-pal` | Inter | 14 px | 400 | — | palette query |
| `t-body` | Inter | 13 px | 400 | line-height 1.4 | Navigator rows, palette items, inputs, window default |
| `t-ui` | Inter | 12.5 px | 400 / 500 | — | breadcrumb, tabs, buttons, tables, Toast |
| `t-small` | Inter | 12 px | 400 / 600 | — | group headers, field labels, drift card, Inbox pill, limit rows |
| `t-hint` | Inter | 11.5 px | 400 | — | dialog notes, checkout plan, drift sub-line |
| `m-term` | Maple Mono (bundled; mockup JetBrains Mono 12.5 px, ADR-0037) | 14 px (Settings) | 500 | line height 1.15 × the font's | terminal (Ghostty config, not CSS) |
| `m-file` | JetBrains Mono | 12 px | 400 | — | file rows, link table mono cells, base select |
| `m-meta` | JetBrains Mono | 11.5 px | 400 / 500 | — | Pane header, branch in breadcrumb, palette detail |
| `m-xs` | JetBrains Mono | 11 px | 400 / 500 | — | row meta, Statusline, Inspector tabs (uppercase, 0.06em), chips |
| `m-label` | JetBrains Mono | 10.5 px | 600 | uppercase, letter-spacing 0.12em (table heads 0.1em), `fg3` | section labels, table heads, kbd (500), badges |
| `m-count` | JetBrains Mono | 10 px | 400 | `acc` | Inspector tab count |

## Spacing, radii, motion

- **Spacing scale (px):** 2, 4, 6, 8, 10, 12, 14, 16, 18, 22, 28. Every margin, padding and gap in the sheets is one of these, except the 26 / 28 / 30 / 32 px control heights, the 14 px Navigator indent and the 1 px hairlines.
- **Control heights (px):** 22 (small icon button, drift action), 24 (file row, limit row), 26 (Navigator row, command field, button, Pane header, Statusline, select), 28 (titlebar button, Workspace button, Inbox pill), 30 (input, popover item, Project home card row), 32 (palette item, Next resets row), 34 (tab strip, Inspector tabs), 40 (titlebar), 46 (palette input).
- **Radii (px):** 2 (LED, selection bar), 3 (meter, bar), 4 (kbd, chip, badge, scope chip, Codex/Claude small badge), 5 (Navigator row, commit input, base select, Workspace mark), 6 (buttons, command field, Pane, group, input, seg control, agent badge), 8 (KPI, card, popover, Toast), 10 (dialog, palette). No 99 px pills anywhere.
- **Shadows:** only dialog, palette, popover and Toast (`shadow`); focus uses `glow`. Chrome has none.
- **Motion:** 120 ms ease-out for hover / selection / reveal; no sliding panels. Running LED pulse: opacity 1 → 0.45 → 1 over 1.6 s ease-in-out, infinite.

## Icons

Phosphor Regular (MIT), Fill or Bold where marked, renamed `ph-<name>[-fill|-bold]-symbolic.svg` in the GResource `assets/icons/` (ADR-0036 §4, ADR-0038). Default size 16 px; the sheets give other sizes. A unit test checks every name used in code exists in the GResource.

| Concept | Icon | Concept | Icon |
|---|---|---|---|
| Workspace | `circles-four` | Workspace switch caret | `caret-up-down` |
| Project | `cube` (color `proj-n`) | Source / link | `git-fork` |
| Worktree | `git-branch` (color `wt`) | Terminal / Scratch | `terminal-window` |
| Browser | `globe-simple` | Review / Changes | `git-diff` |
| Files | `tree-structure` | Search / command field | `magnifying-glass` |
| Activity | `pulse` | Base drift | `warning-diamond` (Fill, `wait`) |
| Also-in link | `link-simple` (`run`) | Needs input | `hand-palm` (Fill, `wait`) |
| Done | `check-circle` (`ok`) | Fan-out | `users-three` |
| Change base | `arrows-split` | Draft commit | `sparkle` |
| PR | `git-pull-request` | Identity | `user-circle` |
| Port | `plug` | Usage | `gauge` |
| New (`+` menu) | `plus-square` | Split | `square-split-horizontal` |
| Inspector toggle | `sidebar-simple` (mirrored) | Window controls | `minus`, `corners-out`, `x` |
| Navigator filter / add | `funnel-simple`, `plus` | Tree chevrons | `caret-down`, `caret-right` |
| Notifications | `bell-simple` | Agent Grid / more | `squares-four`, `dots-three` |
| Settings | `gear-six` | Check / default | `check` |
| Checked box | `check-square` (Fill, `acc`) | Info note | `info` |
| Locked account | `lock-simple` | File (TS) | `file-ts` (other types: `file-code`, `file`) |
| Other Agent (R115) | `robot` (Bold) | System monitor CPU / memory (R116) | `cpu`, `memory` |
| System monitor GPU / network | `graphics-card`, `arrow-down`, `arrow-up` | | |

An Agent shows its glyph (14 px) coloured by its status (R115, ADR-0038): the agent's own single-colour logo from Lobe Icons (MIT) — `agent-claude`, `agent-codex`, `agent-antigravity`, `agent-opencode` (`-symbolic.svg` in `assets/icons/`) — else `robot`; the status LED (CSS, 7 × 7 px square, radius 2 px) stays where a status stands alone: Statusline counts, a collapsed parent, Activity and Project home rows.
