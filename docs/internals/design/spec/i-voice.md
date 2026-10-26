# Spec I — Voice Composer

Mockup: [`../voice/mockup.html`](../voice/mockup.html) `#a` / `#c` / `#e` (add `-light`). R124 (Voice prompts), ADR-0046, `PLAN-voice.md` §5.1–§5.2 (the composer, mic button, REC cell). Screen underneath: screen A's window chrome with an Agent Pane `planner` focused. Method, tokens and tolerances: [README](README.md).

Scenes (1440 × 900, scale 1, dark + light, the mockup's data: Shop / shop-web / checkout-v2, Agent Pane `planner` (claude · opus) on the left 1.35 : 1 of `tests`): **I-a** listening (first hold, 6 s), **I-c** ready, **I-e** revised (2 changed bullets, one `[?]`). Scene data is written by `debug.voice scene <a|c|e>` (§6), not by real audio.

## Measurements

| ID | Element | Value (px/pt/token) | Tolerance | How measured |
|---|---|---|---|---|
| I.1 | Composer box | `bg2`, 1 px `line2`, radius 8 px, `shadow` | exact | `geometry voice-composer → background, border, radius` |
| I.2 | Composer place | left/right 10 px inside the Pane body, bottom 10 px, width ≤ 760 px, centred | ±1 px | `geometry voice-composer → x, y, width` vs `geometry pane-body nth=0` |
| I.3 | Listening ring | border `acc-line`; glow screenshot | exact; golden | `geometry voice-composer → border` (I-a); screenshot |
| I.4 | Header | 34 px high, padding 0 6 0 12 px, bottom 1 px `line`, gap 8 px | ±1 px | `geometry voice-header → height, padding, border.bottom` |
| I.5 | State dot | 8 × 8 px, radius 4 px; `err` (I-a), `acc` (I-c) | ±1 px; exact | `geometry voice-dot → width, height, background` |
| I.6 | State label | Inter 12 px 600 `fg`; `Listening` (I-a), `Ready` (I-c, I-e) | ±0.5 px; exact text | `geometry voice-state → font, color, label` |
| I.7 | Time | JetBrains Mono 11 px `fg3`; `0:06` (I-a), `0:11 · 3.2 s` (I-c), `revision 2 · 1.9 s` (I-e) | exact | `geometry voice-time → font, color, label` |
| I.8 | Meter | 110 × 16 px, visible only in I-a | ±1 px; exact | `geometry voice-meter → width, height, visible` |
| I.9 | Target | JetBrains Mono 11 px `fg3`; label `→ planner · checkout-v2` | exact | `geometry voice-target → font, color, label` |
| I.10 | Close | 22 × 22 px, icon 13 px `fg3` | ±1 px | `geometry voice-close → width, height` |
| I.11 | Heard text | Inter italic 12.5 px `fg2`, padding 8 12 px; quote icon 14 px `fg3` | ±0.5 px; exact | `geometry voice-heard → padding, font, color` |
| I.12 | Heard folded | I-c: one line (height ≤ 20 px), `show` toggle JetBrains Mono 11 px `fg3` | ±1 px | `geometry voice-heard → height`; `geometry voice-heard-toggle → font, label` |
| I.13 | Draft box | `bg0`, 1 px `line` (`acc-line` when focused, I-c), radius 6 px, padding 9 11 px, margin 0 12 px | ±1 px; exact | `geometry voice-draft → background, border, radius, padding, margin` |
| I.14 | Draft text | Inter 13.5 px `fg`, line height 1.55 | ±0.5 px; exact | `geometry voice-draft → font, color` |
| I.15 | Code spans | JetBrains Mono 12.5 px on `bg3`; backticks not drawn | exact; golden | `debug.voice info` → `draft_tags` has `code` ranges; screenshot |
| I.16 | Changed marks | I-e: `acc-dim` behind exactly the two changed bullets' words | exact | `debug.voice info` → `changed` ranges = 2; screenshot |
| I.17 | Unsure mark | I-e: `[?] "agent tests"` JetBrains Mono 12 px 500 `wait` on `wait-dim` | exact | `debug.voice info` → `unsure` = 1; screenshot |
| I.18 | Placeholder | I-a: `The prompt appears here when you let go of F8.` `fg3` | exact text | `geometry voice-placeholder → color, label` |
| I.19 | Activity line | JetBrains Mono 11 px `fg3`, padding 7 12 0 px, min height 24 px; I-a text `Reading the session and checkout-v2 while you speak` | ±1 px; exact | `geometry voice-activity → font, color, padding, height, label` |
| I.20 | Footer | padding 8 8 8 12 px, top 1 px `line`, Inter 11.5 px `fg3`; hints I-c `Hold F8 to revise`, `Esc discard` | ±1 px; exact | `geometry voice-footer → padding, border.top, font`; `debug.voice info` → `hints` |
| I.21 | Send | `acc` fill, `acc-fg` 600 12 px, 26 px high, radius 6 px; the only `.suggested-action` on screen | exact | `geometry voice-send → background, color, height, radius`; `count .suggested-action = 1` |
| I.22 | Paste only | secondary (A.15), 26 px high | exact | `geometry voice-paste → height, background, border` |
| I.23 | Disabled buttons | I-a: Send and Paste only insensitive (opacity 0.45) | exact | `geometry voice-send → opacity` |
| I.24 | Mic button | 22 × 22 px; I-a `recording`: icon `err`, background `rec-dim`; I-c `open`: `acc` on `acc-dim` | ±1 px; exact | `geometry voice-button nth=0 → width, color, background, classes` |
| I.25 | REC cell | I-a: `REC 0:06` JetBrains Mono 11 px `err`, icon 13 px | exact | `geometry status-voice → font, color, label` |
| I.26 | Contrast | `fg2` on `bg2`, `fg3` on `bg0`/`bg2`, `wait` on `wait-dim`/`bg0`, `acc-fg` on `acc`, `err` on `bg1` ≥ 4.5 : 1 | ≥ | `contrast.py` |
| I.27 | 1280 px / Text scale 200 % | no clipped label, no horizontal scroll in the composer | exact | `debug.ui clipped`, `hscroll` (m40 helpers) |
