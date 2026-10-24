# Spec D — Command palette (`shared-ui` in two Projects)

Mockup: `mockup.html#d-dark` / `#d-light` (palette over screen A), images `new-d-dark.png`, `new-d-light.png`. Design §3.2 (Navigate), §4 (command field); R107 (palette items, Search scope), amended R14. Screen A underneath is unchanged ([a-workspace.md](a-workspace.md)). Method, tokens and tolerances: [README](README.md).

Opened by the titlebar command field (click), `Ctrl+Shift+P` from anywhere, or `Ctrl+K` while the focus is outside a Terminal (PRD R17 amendment). Query in the scene: `shared`.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| D.1 | Scrim | full window, `scrim` | exact | `geometry palette-scrim → background, width, height` |
| D.2 | Palette width / position | 720 px; centred (x = 360 px at 1440 px); top at 64 px | ±1 px | `geometry palette → width, x, y` |
| D.3 | Palette box | 1 px `line2`, radius 10 px, `bg2`, `shadow` | exact; shadow by screenshot | `geometry palette → border, radius, background` |
| D.4 | Query row | height 46 px, padding 0 14 px, gap 10 px, bottom border 1 px `line` | ±1 px; exact | `geometry palette-entry-row → height, padding, border.bottom` |
| D.5 | Query icon / text | `magnifying-glass` 18 px `acc`; Inter 14 px `fg` | ±1 px; exact | `geometry palette-entry-icon → width, color`; `geometry palette-entry → font, color` |
| D.6 | `Esc` key cap | A.13 style, right-aligned | exact | `geometry palette-esc → x + width = palette right − 14` |
| D.7 | Scope row | padding 8 12 px, gap 4 px, bottom border 1 px `line` | ±1 px; exact | `geometry palette-scopes → padding, border.bottom` |
| D.8 | Scope chips | `All`, `> Actions`, `@ Agents`, `/ Sources`, `# Files`, `$ Symbols`; hint `Tab next` right; JetBrains Mono 500 11 px `fg3`, padding 2 8 px, radius 4 px | ±1 px; exact text | `debug.palette --ui info` → `scopes`; `geometry palette-scope → font, padding, radius` |
| D.9 | Active scope (`All`) | `acc-dim` fill, `acc` text | exact | `class palette-scope nth=0 has active`; `geometry palette-scope nth=0 → background, color` |
| D.10 | Section label | `SOURCES`, `WORKTREES`, `ACTIONS`, `FILES`; label A.18 style; padding 8 14 4 px | ±1 px; exact text | `geometry palette-section → padding, font`; `debug.palette --ui info` → sections |
| D.11 | Item row | height 32 px, padding 0 14 px, gap 10 px, Inter 13 px `fg` | ±1 px; exact | `geometry palette-item → height, padding, font, color` |
| D.12 | Item icon | 16 px `fg3` in an 18 px column; Worktree `git-branch` `wt` | ±1 px; exact | `geometry palette-item-icon → width, color` |
| D.13 | Item detail | JetBrains Mono 11.5 px `fg3`, right after the title | ±0.5 px; exact | `geometry palette-item-detail → font, color` |
| D.14 | Item key hints | right-aligned, key caps A.13 style, gap 3 px | ±1 px | `geometry palette-item-keys → x + width = palette right − 14` |
| D.15 | Selected item (first) | `acc-dim` fill, 2 px `acc` bar inset at the left edge, icon `acc`; hint `Enter open terminal` | exact | `class palette-item nth=0 has selected`; `geometry palette-item nth=0 → background`; `geometry palette-item-selbar → width = 2` |
| D.16 | Two `shared-ui` items, one per link | `shared-ui · Shop · develop`; `shared-ui · Billing · release/2.4 · 1 agent` | exact text, this order | `debug.palette shared` → first two results `kind: "link"` with these `detail` |
| D.17 | Worktree item | `tokens · Billing · shared-ui ← release/2.4 · ↑1` | exact text | `debug.palette shared` → result `kind: "worktree"` |
| D.18 | Action items | `Add shared-ui to Project…` (`link-simple`, keys `Ctrl ⇧ A`), `Change base of shared-ui in Shop…` (`arrows-split`), `Fan-out on shared-ui…` (`users-three`, detail `one prompt, several agents`) | exact text | `debug.palette shared` → `kind: "action"` results |
| D.19 | File item | `shared/theme.ts · shared-ui · src/` (`file-ts`); a file detail never shows an absolute path | exact text | `debug.palette shared` → `kind: "file"` result; `detail` has no leading `/` |
| D.20 | Footer | padding 8 14 px, gap 14 px, top border 1 px `line`, Inter 11.5 px `fg3`: `↑↓ move`, `Enter run`, `Ctrl Enter in new tab`, right `9 results` | ±1 px; exact text | `geometry palette-footer → padding, border.top, font`; `debug.palette --ui info` → `status` |
| D.21 | Palette height | content height, at most 900 − 64 − 64 = 772 px; rows scroll inside | ±1 px | `geometry palette → height ≤ 772` |
| D.22 | No primary button while open | 0 visible `.suggested-action` (A's `Commit` loses the class while a modal surface is open, as C.26) | exact | `count .suggested-action = 0` |
| D.23 | Contrast | `fg`, `fg3`, `acc` text on `bg2` and on `acc-dim` over `bg2` ≥ 4.5 : 1 | ≥ threshold | `contrast.py` (both themes) |
| D.24 | Whole scene | matches the approved golden, dark and light | pixel threshold, terminal masked | `screenshot` |

## States

| Element | Hover | Selected / on | Focus (keyboard) | Disabled |
|---|---|---|---|---|
| Item row | `bg3` fill (pointer); does not move the selection | D.15 (keyboard selection, `↑↓`) | keyboard focus stays in the query entry; the selected row is the target of `Enter` | an action that cannot run here: opacity 0.45, detail says why |
| Scope chip | text `fg2` | D.9 | `Tab` / `Shift+Tab` move the active scope | — |
| Query entry | — | — | always focused while open; caret `fg` 1 px | — |
| `Esc` key cap | `bg3` | — | — | — |

Prefixes typed in the query select the scope (`>` Actions, `@` Agents, `/` Sources, `#` Files, `$` Symbols). `Enter` runs the selected item; `Ctrl+Enter` opens it in a new tab; `Esc` closes and returns focus to the widget that had it (R14).

## Empty state

- **No results:** sections disappear; one 32 px row `No results for "xyz"` in `fg3`, and the footer shows `0 results` plus the hint `Tab next scope`.
- **Empty query:** `Recent` section (recently used items, R14) then `Actions`.
- **Files still indexing:** the `FILES` section shows `Indexing files…` in `fg3`; other sections are unaffected.

## 1280 px

Window 1280 × 800 px: palette keeps 720 px (x = 280 px), top 64 px; at most 800 − 128 = 672 px tall. No change otherwise.

## Text scale 200 %

- Palette widens to min(window − 32 px, 1100 px); rows grow to text height + 12 px; details move under the title (second line, mono `fg3`) when they no longer fit on one line; key hints stay right-aligned on line 1.
- The scope row wraps to two lines rather than scroll; no horizontal scroll; no text clipped.

## Content

- Shared fixture (identical on all six screens): Workspace `acme`; Projects `Shop` (base `develop`) and `Billing`; Sources `shop-web`, `shop-api`, `shared-ui`, `billing`; `shared-ui` in Billing at `release/2.4`; Worktree `checkout-v2`; accounts `acme-work`, `acme-team` (claude) and `acme` (codex); `acme` Spark 5-hour 81 %; `acme-work` `five_hour` 62 %.
- Query `shared`, scope `All`, 9 results.
- Sources: `shared-ui` in `Shop · develop` (selected, `Enter open terminal`); `shared-ui` in `Billing · release/2.4 · 1 agent`.
- Worktrees: `tokens` in `Billing · shared-ui ← release/2.4 · ↑1`.
- Actions: `Add shared-ui to Project…` (`Ctrl ⇧ A`); `Change base of shared-ui in Shop…`; `Fan-out on shared-ui…` (`one prompt, several agents`).
- Files: `shared/theme.ts` (`shared-ui · src/`).
- Underneath: screen A (Workspace `acme`, Projects `Shop` base `develop` and `Billing`, Sources `shop-web`, `shop-api`, `shared-ui`, `billing`, Worktree `checkout-v2`, usage cell `acme-work` `five_hour` 62 %).
