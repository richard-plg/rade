# Spec F — `+` menu with Agent Accounts, limit forecast Toast

Mockup: `mockup.html#f-dark` / `#f-light` (popover and Toast over screen A), images `new-f-dark.png`, `new-f-light.png`. Design §10.1, §10.3 (menu `+`, forecast); R108 (accounts, `+` menu, Workspace lock), R110 (forecast Toast), R103 (Toast never covers actions). Screen A underneath is unchanged ([a-workspace.md](a-workspace.md)). Method, tokens and tolerances: [README](README.md).

Opened by the titlebar `plus-square` button (or `+` in the tab strip); the header names where the new Pane goes.

**Codex row versus the mockup** (decisions log): the mockup's `acme` (codex) row shows `wk 1%`, but design §10.3 says each menu row shows the account's Tightest limit, and screen E gives `acme` Spark 5-hour 81 %. The spec shows `Spark 5h 81%` in `wait` with `warning-diamond` (F.10).

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| F.1 | Popover width / top | 340 px; top edge at the titlebar bottom (y = 40 px) | ±1 px; ±2 px for y | `geometry new-menu → width, y` |
| F.2 | Popover anchor | horizontally contains the centre of the `plus-square` button; right edge ≤ window − 8 px | exact rule | `geometry new-menu → x, width`; `geometry tb-button-new → x, width` |
| F.3 | Popover box | 1 px `line2`, radius 8 px, `bg2`, `shadow`, padding 6 px 0 | ±1 px; exact | `geometry new-menu → border, radius, background, padding` |
| F.4 | Header | `New in Shop / shop-web / checkout-v2`, JetBrains Mono 11 px `fg3`, padding 4 14 8 px, bottom border 1 px `line`, 4 px below | ±1 px; exact text | `geometry new-menu-header → font, color, padding, border.bottom, label` |
| F.5 | Section labels | `CLAUDE CODE`, `CODEX`, `OTHER`; label A.18 style, padding 8 14 2 px | ±1 px; exact text | `geometry new-menu-section → padding, label` |
| F.6 | Item row | height 30 px, padding 0 14 px, gap 10 px, Inter 13 px `fg`; detail JetBrains Mono 11.5 px `fg3`; key hints right (A.13) | ±1 px; exact | `geometry new-menu-item → height, padding, font`; `geometry new-menu-item-detail → font, color` |
| F.7 | Account glyph (R115, ADR-0038; replaces the badge) | the agent's glyph 14 px in an 18 px column: Claude Code and Codex logos (A.29); `fg2`, `acc` on the selected row | ±1 px; exact | `geometry new-menu-glyph → width, color`; `class mark-<agent>` |
| F.8 | Default account row (`acme-work`) | selected: `acc-dim` fill, 2 px `acc` inset bar; `check` 16 px `acc` at the right; detail `5h 62% · wk 41%` | exact | `class new-menu-item nth=acme-work has selected`; `geometry new-menu-default → color`; detail label |
| F.9 | Other account row (`acme-team`) | detail `5h 8% · wk 12%`, no check | exact text | `debug.ui new_menu` → rows |
| F.10 | Codex row (`acme`) | detail = Tightest limit `Spark 5h 81%` in `wait` with `warning-diamond` Fill 11 px (≥ 80 %); default `check` | exact | `debug.ui new_menu` → row `acme` detail; `geometry new-menu-item-detail nth=acme → color` |
| F.11 | Locked row (account of another Workspace: `personal`) | opacity 0.45, detail `other workspace`, `lock-simple` 16 px at the right; not activatable | exact | `geometry new-menu-item nth=personal → opacity`; activating it → no Pane, `FORBIDDEN` not reached (insensitive) |
| F.12 | Shell row | `terminal-window`, `Shell`, keys `Ctrl ⇧ T` | exact text | `debug.ui new_menu` → rows |
| F.13 | Other rows | `users-three` `Fan-out…` detail `one prompt, several agents`; `globe-simple` `Browser` keys `Ctrl ⇧ O` | exact text | `debug.ui new_menu` → rows |
| F.14 | Row order | Shell · Claude Code (default first, then by name; locked last) · Codex · Other (Fan-out, Browser) | exact | `debug.ui new_menu` → rows in order |
| F.15 | Toast position | horizontally centred in the window; bottom edge 44 px above the window bottom (18 px above the Statusline) | ±1 px | `geometry toast → x + width / 2 = 720, y + height = 856` |
| F.16 | Toast box | padding 10 14 px, gap 12 px, 1 px `line2`, radius 8 px, `bg2`, `shadow`, Inter 12.5 px `fg` | ±1 px; exact | `geometry toast → padding, border, radius, background, font` |
| F.17 | Toast content | `warning-diamond` Fill 16 px `wait`; `acme-work` 600 + ` will hit its 5-hour limit in ~40m at this pace`; sub-line `62% now · resets in 1h 12m · acme-team is at 8%` JetBrains Mono 11 px `fg3`, 2 px below | exact text; ±0.5 px | `debug.ui chrome` → `toasts[0]`; `geometry toast-sub → font, color` |
| F.18 | Toast action and close | secondary button `Next agents use acme-team` (A.15 style); `x` 16 px `fg3` | exact text | `debug.ui chrome` → toast buttons |
| F.19 | Toast never covers actions | the Toast rectangle intersects no visible sensitive button, entry or tab other than its own (A's `Commit`, drift actions, Pane headers included) | exact rule | for each `geometry … all:true` button → no overlap with `geometry toast` |
| F.20 | Toast action effect | changes only the default for Agents opened later: after a click, the next `+` → Claude opens with `acme-team`; running Panes keep `RADE_ACCOUNT=acme-work` | exact | `debug.ui new_menu` → default row; `rade pane list --json` accounts unchanged |
| F.21 | Primary buttons | `count .suggested-action = 1` (A's `Commit`; the popover and Toast add none) | exact | `count .suggested-action = 1` |
| F.22 | Contrast | `fg`, `fg3`, `wait`, `acc` on `bg2` and on `acc-dim` over `bg2` ≥ 4.5 : 1 | ≥ threshold | `contrast.py` (both themes) |
| F.23 | Whole scene | matches the approved golden, dark and light | pixel threshold, terminal masked | `screenshot` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Menu item | `bg3` fill | default account of each Agent: `check`; keyboard selection: F.8 style | `↑↓` move, `Enter` activates; focus returns to the previous widget on `Esc` | locked account F.11 (tooltip `Declared in Workspace personal; RADE never runs an Agent under another Workspace's account (ADR-0015)`) |
| `+` titlebar button | `bg3` | menu open: `bg3`, icon `acc` | `glow` ring | — |
| Toast action button | `bg3` | — | `glow` ring | — |
| Toast close | icon `fg` | — | `glow` ring | — |

Toast rules (R103, R110): shown when a limit's forecast reaches 100 % before its reset; at most one forecast Toast per limit per window (re-shown only after that limit resets); auto-hides after 10 s unless hovered; also recorded in Inspector › Activity. For a per-model limit (Opus, Spark) the action offers a model change first, then an account change.

## Empty state

- **No accounts declared for an Agent kind:** its section shows one row with the Agent's name (`Claude Code`) and detail `workspace default` — the Workspace env's config dir (ADR-0015) — with no usage figure.
- **Account with no usage data:** detail `no data yet` in `fg3` (never `0%`).
- **No forecast reaching 100 %:** no Toast.

## 1280 px

Window 1280 × 800 px: popover keeps 340 px and the F.2 anchor rule; Toast stays centred (x centre = 640 px) and still obeys F.19 — when it would overlap a visible action it moves up in 8 px steps above that action. No change otherwise.

## Text scale 200 %

- Popover widens to fit the longest row (max window − 32 px); rows grow to text height + 10 px; details move to a second line when they do not fit; the popover scrolls vertically if taller than the window − 80 px.
- Toast wraps its text to at most 3 lines, max width window − 64 px; the action button moves under the text; F.19 still holds; no text clipped.

## Content

- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Where: `Shop / shop-web / checkout-v2` (Workspace `acme`).
- Claude Code: `acme-work` (default ✓, `5h 62% · wk 41%` — `five_hour` 62 %), `acme-team` (`5h 8% · wk 12%`), `personal` (other workspace, locked).
- Codex: `acme` (default ✓, Tightest limit Spark 5-hour 81 %).
- Other: `Fan-out…`, `Browser`; Shell first.
- Toast: `acme-work` will hit its 5-hour limit in ~40m at this pace; 62 % now, resets in 1h 12m, `acme-team` at 8 %; action `Next agents use acme-team`.
- Underneath: screen A (Projects `Shop` base `develop` and `Billing`, Sources `shop-web`, `shop-api`, `shared-ui`, `billing`, `shared-ui` in Billing at `release/2.4`, Worktree `checkout-v2`).
