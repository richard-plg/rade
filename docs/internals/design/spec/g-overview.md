# Spec G — Agent Overview (16 Agents in 5 Workspaces)

Mockup: [`../overview-mode/mockup.html#a`](../overview-mode/mockup.html) / `#a-light`, images `../overview-mode/overview-a.png`, `overview-a-light.png`; Preview `#p`, `../overview-mode/overview-preview.png`. PRD R117, ADR-0039. Method, tokens and tolerances: [README](README.md).

Opened by `Ctrl+Shift+Space`, the titlebar button, the palette's `Agent Overview`, the switcher's `!` or a Statusline count. Scene: the mockup's 16 Agents (`rade_core::overview` test fixture `mockup_scene`).

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| G.1 | Scrim | full window, `scrim` over the window content | exact | `geometry overview → width, height`; `geometry overview-scrim → background` |
| G.2 | Content padding | 28 px top, 40 px left / right | ±1 px | `geometry overview-content → padding` |
| G.3 | Title | `Agents`, Inter 600 15 px `fg`; count `16 in 5 Workspaces` JetBrains Mono 11.5 px `fg3` | ±0.5 px; exact text | `debug.ui overview` → `title`, `count`; `geometry overview-title → font` |
| G.4 | Filter field | 360 px × 32 px, `bg0`, 1 px `acc-line`, radius 6 px, placeholder `Filter by name, Project, branch, status…` | ±1 px; exact | `geometry overview-filter → width, height, background, border, radius` |
| G.5 | Group toggle | `by status` / `by Workspace`, active `acc-dim` fill + `acc` text, JetBrains Mono 11.5 px | exact | `geometry overview-group → …`; `debug.ui overview` → `group` |
| G.6 | Count chips | `2 need input`, `1 failed`, `1 interrupted`, `2 finished, unseen`, `6 running`, `4 quiet` with LEDs; JetBrains Mono 11.5 px `fg2`, 1 px `line2`, radius 4 px, `bg1` | exact text | `debug.ui overview` → `counts` |
| G.7 | Workspace chips | one per Workspace in switcher order: color square 8 px (radius 2), `<name> <n>`, key cap `⌃⌥<n>` | exact text | `debug.ui overview` → `workspaces` |
| G.8 | Band label | uppercase Inter 600 10.5 px `fg3`, letter spacing 0.08 em; count JetBrains Mono 11 px `fg2`; 8 px below | ±0.5 px; exact | `geometry overview-band-label → font` |
| G.9 | Grid | cards ≥ 300 px: 4 columns at 1440 px, 6 at 2000 px (≤ 8), gap 10 px; bands 18 px apart | ±1 px | `geometry overview-card nth=0..3 → x` |
| G.10 | Card | `bg2`, 1 px `line`, radius 6 px, padding 9 11 px, gap 6 px | ±1 px; exact | `geometry overview-card → background, border, radius, padding` |
| G.11 | Selected card | 1 px `acc-line` border + `glow`; foot shows `↵ jump` in `acc` | exact | `class overview-card nth=0 has selected` |
| G.12 | Card line 1 | the agent's glyph 14 px in its status colour (R115; a Fan-out card: LED 7 px), name Inter 600 13 px, model JetBrains Mono 11 px `fg3`, status right in its status token (`wait`, `err`, `run`, `ok`, `wt`) | ±1 px; exact | `geometry overview-card-status → color` |
| G.13 | Card line 2 | Workspace square 7 px in the Workspace color, name `fg`, `Project / link / Worktree` JetBrains Mono 11 px `fg2` | exact | `debug.ui overview` → card `where` |
| G.14 | Tail | `bg0`, 1 px `line`, radius 4 px, padding 5 7 px, JetBrains Mono 10.5 px / 1.45 `term-fg`, 3 lines | ±1 px | `geometry overview-card-tail → …`; `debug.ui overview` → card `tail` |
| G.15 | Foot | JetBrains Mono 11 px `fg3`; Notification text `fg2` | exact | `geometry overview-card-foot → font` |
| G.16 | Quiet row | one line, padding 7 10 px, glyph, name, where, status `fg3`; folded only past the room left, ending in a `+n more ▾` tile (1 px dashed `line2`, radius 6 px) | ±1 px | `geometry overview-card.compact → padding` |
| G.17 | Key hints bar | 40 px, `bg1`, top 1 px `line`, JetBrains Mono 11 px `fg3` | ±1 px; exact | `geometry overview-hints → height, background, border.top` |
| G.18 | Motion | in 200 ms, jump 220 ms, Esc 140 ms; 0 with `gtk-enable-animations` off | exact | `debug.ui overview` → `durations` |
| G.19 | Contrast | `fg`, `fg2`, `fg3`, status tokens on `bg2`; `term-fg` on `bg0` ≥ 4.5 : 1 | ≥ threshold | `contrast.py` (both themes) |
| G.20 | Preview frame | `bg2`, 1 px `line2`, radius 8 px, `0 20px 60px @shadow`; width min(600 px, 46 % of the window); header padding 10 12 8 px, foot 8 12 px, both split off by 1 px `line` | ±1 px; exact | `debug.ui overview` → `preview.rect`; `geometry overview-preview → background, border, radius` |
| G.21 | Preview header | glyph, name Inter 600 13.5 px, model, status right in its status token; line 2 as G.13 plus `↳ <Parent>` right | exact | `geometry overview-preview → …` |
| G.22 | Preview body | `bg0`, padding 10 12 px, JetBrains Mono 11.5 px / 1.5 `term-fg`, ≤ 24 lines, ellipsized, scrolls under the pointer | ±0.5 px | `debug.ui overview` → `preview.lines`, `preview.text` |
| G.23 | Preview place | right of its card + 12 px, top-aligned; else left; else below / above its row; else beside, narrower (≥ 300 px); else the larger of below / above, shortened; inside the body; never over its card | ±1 px | `debug.ui overview` → `preview.side`, `preview.rect`, `preview.card` |
| G.24 | Preview foot | last Notification `fg2`, git facts, account `fg3`, `↵ jump · Alt+P hide` `acc` right, JetBrains Mono 11 px | exact | `geometry overview-preview → …` |

## States

| Element | Hover | Selected | Focus (keyboard) |
|---|---|---|---|
| Card | `bg3` fill; does not move the selection | G.11 | keyboard stays in the filter field; arrows move the selection |
| Count / Workspace chip | text `fg` | active filter: `acc-dim` fill, `acc` text, `acc-line` border | — |

## Empty state

- No Agent: one mono line `No Agents running · Ctrl+Shift+P > Spawn Agent` in `fg3`, centered under the header; no icon.
- No match: `No Agent matches "<query>" · Esc clears`.

## 1280 px

3 columns; the Workspace chips wrap under the count chips.

## Text scale 200 %

2 columns; tails keep 3 lines; the hint bar wraps to two lines rather than scroll; no horizontal scroll, no clipped text.
