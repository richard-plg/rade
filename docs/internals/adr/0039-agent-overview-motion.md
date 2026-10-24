# Agent Overview: a full-window view with ~200 ms motion

Status: accepted 2026-10-24 (M43, PRD R117). Amends ADR-0036 §6 ("120 ms ease-out, no sliding panels") for Agent Overview only. Owner decisions in docs/internals/design/overview-mode-proposal.md (Q1–Q6, answered 2026-10-24).

The owner runs Agents in several Workspaces at once, and only one Workspace is shown (R23.4). Finding the Agent that needs them means switching Workspace and scanning its Navigator. They asked for an app-switcher-like view "with a nice macOS-style animation" that lists every Agent, most urgent first, and jumps to one.

## Decision

1. **One full-window view, Agent Overview** (R117), over the whole window, opened by `Ctrl+Shift+Space` (free in Ghostty's, GNOME's and IBus's defaults, checked 2026-10-24). It lists Agents only, in four bands by status, and replaces nothing: the Inbox stays the event history, the palette keeps its one-Workspace scope, `Ctrl+Shift+G` still jumps to the newest unread.
2. **Name.** "Overview" alone meant Project home's breadcrumb (`Shop / Overview`); that breadcrumb becomes `Shop / Home`, so the word means one thing.
3. **Motion beyond ADR-0036 §6, for this view only:** in 200 ms (the window behind shrinks to 94 % under the scrim; cards fade, rise 6 px, 15 ms apart for the first 12), a jump 220 ms (the layer fades while the window behind scales back to full size; owner's choice 2026-10-24, see below), `Esc` 140 ms. Ease-out (decelerate) curves, no springs or bounce, nothing slides in from an edge. Every other surface keeps 120 ms.
4. **Reduce Animation wins.** With `gtk-enable-animations` off (GNOME's Accessibility › Reduce Animation) nothing moves: libadwaita's animations skip to their end value, the view simply appears and disappears.
5. **Budget.** Each animation frame's own work ≤ 16.7 ms (p95) with 40 Agents; whatever misses it (first the blur, then the window shrink) is dropped, not slowed down.

## Spike (M43)

Measured 2026-10-24 in headless Weston (Mesa llvmpipe, i.e. GL on the CPU), 1440 × 900, 40 Agents, `debug.ui perf` (`overview` = key → first frame with the cards; `overview:anim` = each frame's own work, layout → paint end, during the 400 ms after opening):

| Variant | First frame p50 / p95 | Frame work p50 / p95 |
|---|---|---|
| Shrink (CSS `transform: scale(.94)` on the window content) + fade + card rise | 49 / 161 ms | 19 / 43 ms |
| Same without the shrink | 47 / 63 ms | 19 / 41 ms |
| No motion at all | 50 / 180 ms | 12 / 18 ms |
| + CSS `filter: blur(6px)` and `opacity` on the window behind (dropped) | — | washed-out gray backdrop; no gain worth the cost |

- **The window behind can be shrunk without a paintable:** GTK 4.22 CSS `transform` (with a `transition`) on the window's content box scales the Ghostty GL areas with it; no snapshot code is needed. It costs nothing measurable here.
- **Blur is dropped:** `filter: blur()` plus `opacity` turned the backdrop gray instead of dark; a plain `alpha(@bg0, 0.93)` layer reads like the mockup.
- **RADE's own work is under 1 ms** (facts 0.1–0.3 ms, tails 0.04 ms, building 40 cards 0.6–0.7 ms). The first frame and each frame are GTK layout and painting of the full window on llvmpipe; even with no motion a frame's p95 is 18 ms there. So §5's budget cannot be judged in headless CI on a CPU renderer: it needs the real desktop (GPU), like R111-f. **Open: the owner decides whether R117.8 is checked live only** (see docs/internals/checks/M43.md).
- **Jump:** a jump fades the layer out over 220 ms while the window behind scales back to full size. The owner kept this (2026-10-24) instead of the first draft's "the chosen card grows to the main area", which is dropped.
- **Preview (R117.9, 2026-10-24):** a hovered or selected card's Preview fades in over 120 ms with a 4 px slide from its card (ADR-0036's own timing, not this ADR's longer ones); nothing moves under Reduce Animation.

## Considered Options

- **Keep 120 ms opacity + scale only.** Within ADR-0036, but the owner asked for the macOS feel; the zoom into the chosen card is what tells the eye where it went. Rejected by the owner (2026-10-24).
- **Spring animations (`AdwSpringAnimation`).** Overshoot reads as playful, at odds with the terminal-native look. Rejected.
- **Live terminal thumbnails on the cards.** A hidden Workspace's Ghostty surfaces are not drawn; rendering each off-screen costs GPU time and still goes stale. The last screen lines (text) answer "what is it asking?" better. Rejected.

## Consequences

- Golden screenshots stay static: the motion is checked by durations reported through `debug.ui overview`, not by pixels.
- A new `debug.ui perf overview` budget joins R111.3's.
- A test asserts the Reduce Animation path (R117-i).
